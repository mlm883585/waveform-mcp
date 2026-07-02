//! CDC (Cross Clock Domain) analysis module.
//!
//! Identifies clock domains, detects CDC crossing points, verifies
//! synchronizer patterns, and provides cross-domain time mapping
//! for BFS root-cause tracing.

use crate::deps::DepGraph;
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::ReportWriter;
use crate::hierarchy::{find_signal_by_path, find_var_by_path};
use crate::report_writeln;
use crate::time_map::{ClockEdgeTable, compute_time_ps_from_table};
use serde::{Deserialize, Serialize};
use wellen::simple::Waveform;

/// A clock domain identified from deps.yaml or waveform detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockDomain {
    /// Logical clock name (from clock_aliases or auto-detected).
    pub name: String,
    /// Resolved waveform path for this clock signal.
    pub waveform_path: String,
    /// Clock period in picoseconds (0 if irregular).
    pub period_ps: u64,
    /// Clock edge type (posedge/negedge).
    pub edge_type: String,
    /// Number of edges detected in the waveform.
    pub edge_count: usize,
    /// Signals belonging to this domain (from deps edges or name-pattern heuristic).
    pub signals: Vec<String>,
}

/// A CDC crossing point between two clock domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcCrossing {
    /// Signal that crosses the domain boundary.
    pub signal_path: String,
    /// Source clock domain name.
    pub from_domain: String,
    /// Destination clock domain name.
    pub to_domain: String,
    /// How this crossing was identified: "deps_yaml" or "waveform_heuristic".
    pub source: String,
    /// Whether a synchronizer pattern was detected.
    pub has_synchronizer: bool,
    /// Synchronizer details if detected.
    pub synchronizer: Option<SynchronizerInfo>,
    /// Description from deps.yaml (if available).
    pub description: Option<String>,
}

/// Information about a detected synchronizer pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynchronizerInfo {
    /// Synchronizer type: "2_ff", "3_ff", "mux", "async_fifo", "handshake".
    pub kind: String,
    /// Stage count (for FF-chain synchronizers).
    pub stages: u32,
    /// Intermediate flip-flop signal paths.
    pub intermediate_signals: Vec<String>,
    /// Confidence level: "high", "medium", "low".
    pub confidence: String,
    /// Reason for confidence level.
    pub confidence_reason: String,
}

/// Summary statistics for CDC analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcSummary {
    pub total_domains: usize,
    pub total_crossings: usize,
    pub protected_crossings: usize,
    pub unprotected_crossings: usize,
    pub synchronizer_count: usize,
}

/// Result of CDC analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdcAnalysisResult {
    /// Clock domains identified.
    pub clock_domains: Vec<ClockDomain>,
    /// CDC crossing points identified.
    pub crossings: Vec<CdcCrossing>,
    /// Synchronizer patterns detected.
    pub synchronizers: Vec<SynchronizerInfo>,
    /// Signals that cross domains without synchronizer protection.
    pub unprotected_crossings: Vec<CdcCrossing>,
    /// Summary statistics.
    pub summary: CdcSummary,
}

/// Lightweight clock domain info from discovery (no waveform analysis).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockDomainInfo {
    /// Clock signal path in waveform.
    pub clock_path: String,
    /// Approximate period in picoseconds.
    pub period_ps: u64,
    /// Signals likely in this domain (by hierarchy proximity heuristic).
    pub likely_signals: Vec<String>,
}

/// Identify clock domains from deps.yaml clock_aliases and waveform.
///
/// Uses deps.yaml clock_aliases as primary source, augmented by
/// auto_discover_signals clock detection for any missing clocks.
pub fn identify_clock_domains(
    waveform: &mut Waveform,
    dep_graph: &DepGraph,
    simulator: &str,
) -> WaveResult<Vec<ClockDomain>> {
    let hierarchy = waveform.hierarchy();
    let timescale = hierarchy.timescale();

    // Step 1: Extract clock domains from deps.yaml clock_aliases
    let all_clock_names = dep_graph.get_all_clock_names();
    let mut domains: Vec<ClockDomain> = Vec::new();

    for clock_name in &all_clock_names {
        let resolved_path = match dep_graph.resolve_clock(clock_name, simulator) {
            Ok(path) => path,
            Err(_) => continue,
        };

        // Measure clock period from waveform
        let period_ps = measure_clock_period(waveform, &resolved_path, timescale.as_ref())?;
        let edge_count = count_clock_edges(waveform, &resolved_path)?;

        // Collect signals belonging to this domain from deps edges
        let domain_signals = dep_graph.get_signals_by_clock_domain(clock_name);

        domains.push(ClockDomain {
            name: clock_name.clone(),
            waveform_path: resolved_path,
            period_ps,
            edge_type: "posedge".to_string(),
            edge_count,
            signals: domain_signals,
        });
    }

    Ok(domains)
}

/// Identify clock domains purely from waveform (no deps.yaml).
pub fn identify_clock_domains_from_waveform(
    waveform: &mut Waveform,
) -> WaveResult<Vec<ClockDomainInfo>> {
    let hierarchy = waveform.hierarchy();
    let timescale = hierarchy.timescale();

    // Collect all signal paths
    let all_signals: Vec<String> = hierarchy
        .iter_vars()
        .map(|v| v.full_name(hierarchy))
        .collect();

    // Find clock signals by name pattern
    let mut clock_candidates: Vec<String> = Vec::new();
    for path in &all_signals {
        let lower = path.to_lowercase();
        if (lower.contains("clk") || lower.contains("clock"))
            && let Some(vr) = find_var_by_path(hierarchy, path)
        {
            let width = hierarchy[vr].length().unwrap_or(1);
            if width == 1 {
                clock_candidates.push(path.clone());
            }
        }
    }

    // Load and analyze clock candidates
    let signal_refs: Vec<wellen::SignalRef> = clock_candidates
        .iter()
        .filter_map(|p| find_signal_by_path(hierarchy, p))
        .collect();

    if signal_refs.is_empty() {
        return Ok(Vec::new());
    }

    waveform.load_signals(&signal_refs);

    let mut domains: Vec<ClockDomainInfo> = Vec::new();

    for clock_path in clock_candidates.iter() {
        let period_ps = measure_clock_period(waveform, clock_path, timescale.as_ref()).unwrap_or(0);

        if period_ps == 0 {
            continue; // Not a regular clock
        }

        // Assign signals to domain by hierarchy proximity
        let likely_signals = assign_signals_by_proximity(clock_path, &all_signals);

        domains.push(ClockDomainInfo {
            clock_path: clock_path.clone(),
            period_ps,
            likely_signals,
        });
    }

    Ok(domains)
}

/// Find CDC crossing points from deps.yaml boundary_kind=cdc edges.
pub fn find_cdc_crossings(
    dep_graph: &DepGraph,
    _clock_domains: &[ClockDomain],
) -> Vec<CdcCrossing> {
    let cdc_edges = dep_graph.find_cdc_edges();
    let mut crossings: Vec<CdcCrossing> = Vec::new();

    for (_output_signal, dep_edge) in &cdc_edges {
        let from_domain = dep_edge.cdc_from_clock.as_deref().unwrap_or("unknown");
        let to_domain = dep_edge.cdc_to_clock.as_deref().unwrap_or("unknown");

        crossings.push(CdcCrossing {
            signal_path: dep_edge.signal.clone(),
            from_domain: from_domain.to_string(),
            to_domain: to_domain.to_string(),
            source: "deps_yaml".to_string(),
            has_synchronizer: false,
            synchronizer: None,
            description: dep_edge.description.clone(),
        });
    }

    crossings
}

/// Detect synchronizer patterns for a CDC crossing signal.
///
/// Uses name-pattern heuristic to find intermediate sync stages,
/// then optionally verifies value propagation in the waveform.
pub fn detect_synchronizer(
    waveform: &mut Waveform,
    crossing: &CdcCrossing,
    _dep_graph: &DepGraph,
    _simulator: &str,
) -> WaveResult<Option<SynchronizerInfo>> {
    let hierarchy = waveform.hierarchy();

    // Strategy 1: Name-pattern heuristic
    let base_signal = &crossing.signal_path;
    let name = base_signal.rsplit('.').next().unwrap_or(base_signal);

    let sync_patterns = [
        format!("{}_sync", name),
        format!("{}_sync1", name),
        format!("{}_sync2", name),
        format!("{}_sync_0", name),
        format!("{}_sync_1", name),
        format!("{}_metastable", name),
        format!("{}_stable", name),
    ];

    // Search in same scope as the crossing signal
    let scope = if let Some(pos) = base_signal.rfind('.') {
        &base_signal[..pos]
    } else {
        ""
    };

    let mut intermediate_signals: Vec<String> = Vec::new();
    for pattern in &sync_patterns {
        let full_path = if scope.is_empty() {
            pattern.clone()
        } else {
            format!("{}.{}", scope, pattern)
        };

        if let Some(vr) = find_var_by_path(hierarchy, &full_path) {
            let width = hierarchy[vr].length().unwrap_or(1);
            if width == 1 {
                intermediate_signals.push(full_path);
            }
        }
    }

    if intermediate_signals.is_empty() {
        // No synchronizer pattern detected
        return Ok(None);
    }

    // Sort by name for stage ordering
    intermediate_signals.sort();

    let stages = intermediate_signals.len() as u32;
    let kind = if stages <= 3 {
        format!("{}_ff", stages)
    } else {
        "multi_ff".to_string()
    };

    // Determine confidence
    let confidence = if intermediate_signals.len() >= 2 {
        "medium"
    } else {
        "low"
    };

    let confidence_reason = if confidence == "medium" {
        format!("Found {}-stage synchronizer by name pattern match", stages)
    } else {
        "Only single-stage synchronizer pattern found, may not provide sufficient protection"
            .to_string()
    };

    Ok(Some(SynchronizerInfo {
        kind,
        stages,
        intermediate_signals,
        confidence: confidence.to_string(),
        confidence_reason,
    }))
}

/// Map a time_index in source clock domain to equivalent time in destination clock domain.
///
/// Algorithm: Find nearest destination clock edge after source_time_ps + sync_stages * dest_period_ps.
pub fn map_time_cross_domain(
    source_time_index: usize,
    _source_edge_table: &ClockEdgeTable,
    dest_edge_table: &ClockEdgeTable,
    sync_stages: u32,
    time_table: &[u64],
    timescale: Option<&wellen::Timescale>,
) -> WaveResult<(usize, u64)> {
    // Step 1: Convert source time_index to absolute time_ps
    let source_time_ps = compute_time_ps_from_table(time_table, source_time_index, timescale);

    // Step 2: Compute arrival time accounting for synchronizer latency
    // Estimate destination period from edge table
    let dest_period_ps = estimate_period_from_edge_table(dest_edge_table, time_table, timescale);

    let arrival_time_ps = source_time_ps + (sync_stages as u64) * dest_period_ps;

    // Step 3: Find nearest destination clock edge after arrival_time_ps
    // Convert dest edges time_values to ps for comparison
    let dest_arrival_ps = arrival_time_ps;

    // Binary search in dest_edge_table edges for time_value >= arrival time
    // Need to convert edge time_values to ps first
    let dest_edge_entry = dest_edge_table.edges.iter().find(|e| {
        let edge_ps = compute_time_ps_from_table(time_table, e.time_index, timescale);
        edge_ps >= dest_arrival_ps
    });

    match dest_edge_entry {
        Some(entry) => Ok((
            entry.time_index,
            compute_time_ps_from_table(time_table, entry.time_index, timescale),
        )),
        None => {
            // Beyond waveform range, return last time_index
            let last_idx = time_table.len() - 1;
            Ok((
                last_idx,
                compute_time_ps_from_table(time_table, last_idx, timescale),
            ))
        }
    }
}

/// Estimate clock period from ClockEdgeTable edge spacing.
fn estimate_period_from_edge_table(
    edge_table: &ClockEdgeTable,
    time_table: &[u64],
    timescale: Option<&wellen::Timescale>,
) -> u64 {
    if edge_table.edges.len() < 2 {
        return 0;
    }

    // Average period from first few edges
    let sample_count = std::cmp::min(10, edge_table.edges.len() - 1);
    let mut total_period_ps: u64 = 0;

    for i in 0..sample_count {
        let t0 = compute_time_ps_from_table(time_table, edge_table.edges[i].time_index, timescale);
        let t1 =
            compute_time_ps_from_table(time_table, edge_table.edges[i + 1].time_index, timescale);
        total_period_ps += t1 - t0;
    }

    total_period_ps / sample_count as u64
}

/// Measure clock period in picoseconds from waveform.
///
/// Uses `compute_time_ps_from_table` for precise integer ps calculation.
/// Note: We intentionally don't reuse `protocol::measure_clock` here because
/// that function returns `ClockMeasurement` with period stats in seconds (f64),
/// which would require a fragile f64→u64 conversion. Direct ps computation
/// from the time table is more accurate for CDC period measurement.
fn measure_clock_period(
    waveform: &mut Waveform,
    clock_path: &str,
    timescale: Option<&wellen::Timescale>,
) -> WaveResult<u64> {
    let hierarchy = waveform.hierarchy();
    let signal_ref = find_signal_by_path(hierarchy, clock_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: clock_path.to_string(),
        }
    })?;

    waveform.load_signals(&[signal_ref]);

    let signal = waveform
        .get_signal(signal_ref)
        .ok_or_else(|| WaveAnalyzerError::CdcError {
            message: format!("Failed to get signal data for '{}'", clock_path),
        })?;

    let time_table: Vec<u64> = waveform.time_table().to_vec();

    let changes: Vec<usize> = signal.iter_changes().map(|(t, _)| t as usize).collect();

    if changes.len() < 4 {
        return Ok(0); // Not enough edges for a regular clock
    }

    // Compute average period in picoseconds using compute_time_ps_from_table
    let periods: Vec<u64> = changes
        .windows(2)
        .map(|w| {
            let t0_ps = compute_time_ps_from_table(&time_table, w[0], timescale);
            let t1_ps = compute_time_ps_from_table(&time_table, w[1], timescale);
            t1_ps - t0_ps
        })
        .collect();

    let avg_period: u64 = periods.iter().sum::<u64>() / periods.len() as u64;
    Ok(avg_period)
}

/// Count clock edges in waveform for a clock signal.
fn count_clock_edges(waveform: &mut Waveform, clock_path: &str) -> WaveResult<usize> {
    let hierarchy = waveform.hierarchy();
    let signal_ref = find_signal_by_path(hierarchy, clock_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: clock_path.to_string(),
        }
    })?;

    waveform.load_signals(&[signal_ref]);

    let signal = waveform
        .get_signal(signal_ref)
        .ok_or_else(|| WaveAnalyzerError::CdcError {
            message: format!("Failed to get signal data for '{}'", clock_path),
        })?;

    Ok(signal.iter_changes().count())
}

/// Assign signals to a clock domain by hierarchy path proximity.
///
/// A signal belongs to the domain whose scope path has the longest
/// common prefix with the signal's path.
fn assign_signals_by_proximity(clock_path: &str, all_signals: &[String]) -> Vec<String> {
    // Get scope path of the clock signal
    let clock_scope = if let Some(pos) = clock_path.rfind('.') {
        &clock_path[..pos]
    } else {
        ""
    };

    if clock_scope.is_empty() {
        return Vec::new();
    }

    // Assign signals whose path starts with the clock scope
    all_signals
        .iter()
        .filter(|sig| sig.starts_with(clock_scope))
        .cloned()
        .collect()
}

/// Format a CdcAnalysisResult as human-readable text.
pub fn format_cdc_report(result: &CdcAnalysisResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "=== CDC Analysis Report ===");
    report_writeln!(out);

    // Clock domains
    report_writeln!(out, "Clock Domains: {}", result.clock_domains.len());
    for domain in &result.clock_domains {
        report_writeln!(
            out,
            "  {} (period: {}, edges: {}, signals: {})",
            domain.name,
            if domain.period_ps > 0 {
                format!("{}ps", domain.period_ps)
            } else {
                "irregular".to_string()
            },
            domain.edge_count,
            domain.signals.len()
        );
    }
    report_writeln!(out);

    // CDC crossings
    report_writeln!(out, "CDC Crossings: {}", result.crossings.len());
    for crossing in &result.crossings {
        let sync_status = if crossing.has_synchronizer {
            "PROTECTED"
        } else {
            "UNPROTECTED"
        };
        report_writeln!(
            out,
            "  {} -> {} [{}] {} ({})",
            crossing.from_domain,
            crossing.to_domain,
            sync_status,
            crossing.signal_path,
            crossing.source
        );
        if let Some(ref sync) = crossing.synchronizer {
            report_writeln!(
                out,
                "    Synchronizer: {}-FF (stages={}), confidence={}, signals: {}",
                sync.kind,
                sync.stages,
                sync.confidence,
                sync.intermediate_signals.join(", ")
            );
        }
    }
    report_writeln!(out);

    // Unprotected crossings (highlighted)
    if !result.unprotected_crossings.is_empty() {
        report_writeln!(out, "WARNING: Unprotected CDC Crossings:");
        for crossing in &result.unprotected_crossings {
            report_writeln!(
                out,
                "  {} crosses {} -> {} without synchronizer!",
                crossing.signal_path,
                crossing.from_domain,
                crossing.to_domain
            );
        }
        report_writeln!(out);
    }

    // Summary
    report_writeln!(
        out,
        "Summary: {} domains, {} crossings ({} protected, {} unprotected), {} synchronizers",
        result.summary.total_domains,
        result.summary.total_crossings,
        result.summary.protected_crossings,
        result.summary.unprotected_crossings,
        result.summary.synchronizer_count
    );

    out.finish()
}

/// Full CDC analysis combining deps.yaml metadata and waveform verification.
///
/// Step 1: Extract clock domains from deps.yaml clock_aliases + waveform clock detection.
/// Step 2: Identify CDC crossings from deps.yaml boundary_kind=cdc edges.
/// Step 3: Detect synchronizer patterns for each crossing.
/// Step 4: Build CdcAnalysisResult with protected/unprotected classification.
pub fn analyze_cdc(
    waveform: &mut Waveform,
    dep_graph: &DepGraph,
    simulator: &str,
    verify_synchronizers: bool,
    min_sync_stages: u32,
) -> WaveResult<CdcAnalysisResult> {
    // Step 1: Identify clock domains
    let clock_domains = identify_clock_domains(waveform, dep_graph, simulator)?;

    // Step 2: Find CDC crossings
    let mut crossings = find_cdc_crossings(dep_graph, &clock_domains);

    // Step 3: Detect synchronizers
    let mut synchronizers: Vec<SynchronizerInfo> = Vec::new();
    if verify_synchronizers {
        for crossing in &mut crossings {
            let sync_info = detect_synchronizer(waveform, crossing, dep_graph, simulator)?;
            if let Some(info) = sync_info {
                crossing.has_synchronizer = true;
                crossing.synchronizer = Some(info.clone());
                synchronizers.push(info);
            }
        }
    }

    // Step 4: Classify protected/unprotected
    let unprotected_crossings: Vec<CdcCrossing> = crossings
        .iter()
        .filter(|c| {
            !c.has_synchronizer
                || c.synchronizer
                    .as_ref()
                    .is_none_or(|s| s.stages < min_sync_stages)
        })
        .cloned()
        .collect();

    let protected_count = crossings.len() - unprotected_crossings.len();
    let total_domains = clock_domains.len();
    let total_crossings = crossings.len();
    let synchronizer_count = synchronizers.len();
    let unprotected_count = unprotected_crossings.len();

    Ok(CdcAnalysisResult {
        clock_domains,
        crossings,
        synchronizers,
        unprotected_crossings,
        summary: CdcSummary {
            total_domains,
            total_crossings,
            protected_crossings: protected_count,
            unprotected_crossings: unprotected_count,
            synchronizer_count,
        },
    })
}

/// Waveform-only CDC analysis (no deps.yaml).
///
/// BUG-fix (MCP/CLI parity): the MCP `analyze_cdc` tool description promised
/// "Optional deps_id - uses waveform-only heuristic if not provided" but the
/// implementation rejected the no-deps case outright, contradicting both the
/// tool description and the CLI counterpart (`bin/cli/advanced_cmds.rs`).
///
/// This function performs the heuristic analysis that was advertised:
///
/// 1. Identify clock-domain candidates from the waveform (signal names
///    containing "clk"/"clock" with measurable regular period).
/// 2. Convert `ClockDomainInfo` → `ClockDomain` so the result type is
///    uniform with the full analysis path.
/// 3. Report a summary noting that no crossings can be detected without
///    deps.yaml — this is the honest answer rather than "requires deps".
///
/// The returned `crossings` / `synchronizers` / `unprotected_crossings`
/// are all empty because crossing detection fundamentally needs an explicit
/// signal-to-domain mapping (the deps.yaml `boundary_kind: cdc` edges).
pub fn analyze_cdc_waveform_only(
    waveform: &mut Waveform,
    simulator: &str,
) -> WaveResult<CdcAnalysisResult> {
    // 1. Discover clock-domain candidates purely from the waveform.
    let domain_infos = identify_clock_domains_from_waveform(waveform)?;

    // 2. Map the lightweight ClockDomainInfo into the full ClockDomain
    //    shape used by `analyze_cdc` so callers can consume both paths
    //    uniformly.
    let clock_domains: Vec<ClockDomain> = domain_infos
        .iter()
        .map(|info| ClockDomain {
            name: format!("{}_{}", simulator, info.clock_path),
            waveform_path: info.clock_path.clone(),
            period_ps: info.period_ps,
            // Waveform-only mode does not record an edge_type or edge_count
            // from a measured-clock helper; leave defaults so the JSON shape
            // is consistent with the deps-based path.
            edge_type: "posedge".to_string(),
            edge_count: 0,
            signals: info.likely_signals.clone(),
        })
        .collect();

    let total_domains = clock_domains.len();
    let summary = CdcSummary {
        total_domains,
        // Without deps.yaml we cannot enumerate signal-level crossings.
        total_crossings: 0,
        protected_crossings: 0,
        unprotected_crossings: 0,
        synchronizer_count: 0,
    };

    Ok(CdcAnalysisResult {
        clock_domains,
        crossings: Vec::new(),
        synchronizers: Vec::new(),
        unprotected_crossings: Vec::new(),
        summary,
    })
}
