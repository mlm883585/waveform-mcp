//! BFS root-cause tracing engine.
//!
//! This module implements the BFS-based root-cause analysis algorithm
//! that traces failures from a signal entry point back through the
//! dependency graph using clock-edge-based time backtracking.

use crate::cdc::{CdcCrossing, detect_synchronizer};
use crate::condition::{
    Condition, build_signal_cache_entry, evaluate_condition, extract_signal_names, parse_condition,
};
use crate::deps::{BoundaryKind, CheckExpr, ClockEdge, DepEdge, DepGraph, DepType, LogicType};
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::format_signal_value;
use crate::hierarchy::{find_var_by_path, resolve_signal_var_refs};
use crate::time_map::{
    ClockEdgeTable, ClockEdgeType, build_clock_edge_table, compute_time_ps_from_table,
};
use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use wellen;

// ── BFS result disk cache ──────────────────────────────────────────────────
// These functions persist BFS results to disk so they can survive across
// process boundaries (e.g., MCP server restarts or separate CLI invocations).

/// Return the directories where BFS result cache files should be stored.
///
/// Priority order: TEMP env var directory, then current working directory.
pub fn bfs_cache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(tmp) = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMP")) {
        dirs.push(
            PathBuf::from(tmp)
                .join("wave-analyzer-cli")
                .join("bfs-results"),
        );
    }
    dirs.push(
        std::env::current_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(".wave-analyzer-cli")
            .join("bfs-results"),
    );
    dirs
}

/// Produce a safe file name from a trace_id for disk caching.
fn bfs_result_cache_file_name(trace_id: &str) -> String {
    let mut safe = String::new();
    for ch in trace_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
    }
    format!("{}.json", safe)
}

/// Persist a BFS result to disk for cross-process retrieval.
pub fn persist_bfs_result(trace_id: &str, result: &BfsResult) -> WaveResult<()> {
    let json = serde_json::to_string_pretty(result).map_err(|e| WaveAnalyzerError::BfsError {
        message: format!("Failed to serialize BFS result: {}", e),
    })?;

    let mut errors = Vec::new();
    let file_name = bfs_result_cache_file_name(trace_id);
    for dir in bfs_cache_dirs() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            errors.push(format!("{} ({})", dir.display(), e));
            continue;
        }
        let path = dir.join(&file_name);
        match std::fs::write(&path, &json) {
            Ok(_) => return Ok(()),
            Err(e) => errors.push(format!("{} ({})", path.display(), e)),
        }
    }

    Err(WaveAnalyzerError::BfsError {
        message: format!(
            "Failed to persist BFS result for trace_id '{}': {}",
            trace_id,
            errors.join("; ")
        ),
    })
}

/// Load a BFS result from disk cache (for cross-process retrieval).
pub fn load_bfs_result_from_cache(trace_id: &str) -> WaveResult<BfsResult> {
    let file_name = bfs_result_cache_file_name(trace_id);
    let mut errors = Vec::new();

    for dir in bfs_cache_dirs() {
        let path = dir.join(&file_name);
        match std::fs::read_to_string(&path) {
            Ok(json) => {
                return serde_json::from_str(&json).map_err(|e| WaveAnalyzerError::FileError {
                    path: path.display().to_string(),
                    message: format!("Failed to parse cached BFS result: {}", e),
                });
            }
            Err(e) => errors.push(format!("{} ({})", path.display(), e)),
        }
    }

    Err(WaveAnalyzerError::BfsError {
        message: format!(
            "BFS result not found for trace_id '{}'. Run trace_root_cause first. [{}]",
            trace_id,
            errors.join("; ")
        ),
    })
}

/// Pre-parsed condition expressions for BFS evaluation.
struct ConditionCache {
    parsed: HashMap<String, Condition>,
    signal_caches: HashMap<String, HashMap<String, crate::condition::SignalCacheEntry>>,
}

impl ConditionCache {
    fn new() -> Self {
        ConditionCache {
            parsed: HashMap::new(),
            signal_caches: HashMap::new(),
        }
    }
}

/// Status of a BFS node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Node is suspicious, needs further investigation.
    Suspect,
    /// Edge check passed, node is consistent with upstream.
    Ok,
    /// Reached a boundary (input port, CDC, blackbox, etc.).
    Boundary,
    /// Reached a user-specified stop signal.
    Stopped,
    /// Reached maximum depth limit.
    Truncated,
    /// Discovered a cycle and truncated.
    Cyclic,
    /// Context node (no check, informational only).
    Context,
    /// High-priority root cause candidate.
    RootCauseCandidate,
    /// CDC boundary not penetrated (no synchronizer or penetration disabled).
    CdcBoundary,
    /// CDC boundary penetrated (synchronizer verified).
    CdcPenetrated,
    /// Synchronizer intermediate FF node.
    CdcSynchronizer,
    /// Signal not found in waveform — cannot evaluate, trace continues past it.
    Unresolved,
}

impl NodeStatus {
    /// Priority rank for sorting candidates. Lower = higher priority.
    pub fn priority_rank(&self) -> u8 {
        match self {
            NodeStatus::RootCauseCandidate => 0,
            NodeStatus::Suspect => 1,
            NodeStatus::CdcPenetrated => 1,
            NodeStatus::Boundary => 2,
            NodeStatus::CdcBoundary => 2,
            NodeStatus::Context => 3,
            NodeStatus::CdcSynchronizer => 3,
            NodeStatus::Unresolved => 3,
            NodeStatus::Ok => 4,
            NodeStatus::Stopped => 5,
            NodeStatus::Truncated => 6,
            NodeStatus::Cyclic => 7,
        }
    }
}

/// A single node in the BFS trace tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BfsNode {
    /// Canonical signal path from deps.yaml.
    pub signal_path: String,
    /// Resolved signal path in the waveform (after alias resolution).
    pub resolved_signal_path: String,
    /// Time index in the waveform time_table.
    pub time_index: usize,
    /// Time value in picoseconds.
    pub time_ps: u64,
    /// Depth from root node.
    pub depth: usize,
    /// Node status.
    pub status: NodeStatus,
    /// Actual signal value at this time index.
    pub actual_value: Option<String>,
    /// Optional expected hint.
    pub expected_hint: Option<String>,
    /// Edge type that led to this node.
    pub edge_type: Option<String>,
    /// Clock name for the edge.
    pub clock_name: Option<String>,
    /// Latency cycles for the edge.
    pub latency_cycles: Option<u32>,
    /// Optional note.
    pub note: Option<String>,
    /// Parent node ID (for tree construction).
    pub parent_id: Option<String>,
    /// Unique node ID.
    pub node_id: String,
    /// Source clock domain (for CDC-penetrated nodes).
    pub source_clock_domain: Option<String>,
    /// Destination clock domain (for CDC-penetrated nodes).
    pub dest_clock_domain: Option<String>,
    /// Synchronizer chain info (for CdcSynchronizer nodes).
    pub synchronizer_info: Option<String>,
}

/// A root cause candidate from the BFS trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseCandidate {
    /// Signal path.
    pub signal_path: String,
    /// Time index.
    pub time_index: usize,
    /// Time in picoseconds.
    pub time_ps: u64,
    /// Status of the candidate node.
    pub status: NodeStatus,
    /// Reason why this node is a candidate.
    pub reason: String,
}

/// Result of a BFS root-cause trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BfsResult {
    /// Root signal path.
    pub root_signal: String,
    /// Root time index.
    pub root_time_index: usize,
    /// Root time in picoseconds.
    pub root_time_ps: u64,
    /// All nodes in the trace tree.
    pub tree: Vec<BfsNode>,
    /// Root cause candidates (sorted by priority).
    pub candidates: Vec<RootCauseCandidate>,
    /// Text summary.
    pub summary: String,
}

/// Options for BFS tracing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BfsOptions {
    /// Maximum depth for BFS expansion.
    pub max_depth: usize,
    /// Signals where BFS should stop.
    pub stop_signals: Vec<String>,
    /// Whether to enable automatic edge checking.
    pub enable_auto_check: bool,
    /// Simulator identifier for alias resolution.
    pub simulator: String,
    /// Whether to penetrate CDC boundaries when synchronizer detected.
    pub penetrate_cdc: bool,
    /// Maximum depth to expand within a penetrated CDC domain.
    pub cdc_max_depth: usize,
    /// Minimum synchronizer stages required for CDC penetration.
    pub cdc_min_sync_stages: u32,
}

impl Default for BfsOptions {
    fn default() -> Self {
        BfsOptions {
            max_depth: 8,
            stop_signals: Vec::new(),
            enable_auto_check: true,
            simulator: "modelsim".to_string(),
            penetrate_cdc: false,
            cdc_max_depth: 4,
            cdc_min_sync_stages: 2,
        }
    }
}

/// Perform BFS root-cause tracing from a given signal and time index.
///
/// # Arguments
/// * `waveform` - The loaded waveform
/// * `dep_graph` - The loaded dependency graph
/// * `signal_path` - Canonical entry signal path
/// * `time_index` - Failure time index in the waveform
/// * `options` - BFS tracing options
pub fn trace_root_cause(
    waveform: &mut wellen::simple::Waveform,
    dep_graph: &DepGraph,
    signal_path: &str,
    time_index: usize,
    options: &BfsOptions,
) -> WaveResult<BfsResult> {
    let time_table: Vec<u64> = waveform.time_table().to_vec();
    let timescale = waveform.hierarchy().timescale();

    // Pre-load all signals from dep_graph to avoid repeated load_signals calls
    let all_signals = collect_all_signals_from_dep_graph(dep_graph, &options.simulator);
    preload_signals(waveform, &all_signals)?;

    // Build condition cache for condition_expression edges
    let mut condition_cache = ConditionCache::new();
    build_condition_cache(
        dep_graph,
        waveform,
        &options.simulator,
        &mut condition_cache,
    )?;

    // Pre-load additional signals from condition expressions
    let cond_signals = collect_condition_signals(&condition_cache, dep_graph, &options.simulator);
    if !cond_signals.is_empty() {
        preload_signals(waveform, &cond_signals)?;
    }

    // Build root node
    let root_time_ps = crate::time_map::compute_time_ps_from_table_checked(
        &time_table,
        time_index,
        timescale.as_ref(),
    )?;
    let canonical_path = resolve_canonical_signal_path(dep_graph, signal_path, &options.simulator);
    let resolved_path = resolve_signal_path(dep_graph, &canonical_path, &options.simulator);

    // Read root signal value
    let root_value = read_signal_value_cached(waveform, &resolved_path, time_index)?;

    let root_node = BfsNode {
        signal_path: canonical_path.clone(),
        resolved_signal_path: resolved_path,
        time_index,
        time_ps: root_time_ps,
        depth: 0,
        status: NodeStatus::Suspect,
        actual_value: Some(root_value),
        expected_hint: None,
        edge_type: None,
        clock_name: None,
        latency_cycles: None,
        note: None,
        parent_id: None,
        node_id: "n0".to_string(),
        source_clock_domain: None,
        dest_clock_domain: None,
        synchronizer_info: None,
    };

    // BFS traversal
    let mut queue: VecDeque<BfsNode> = VecDeque::new();
    let mut visited: HashSet<(String, usize)> = HashSet::new();
    let mut tree: Vec<BfsNode> = Vec::new();
    let mut node_counter = 1;
    let mut clock_edge_cache: HashMap<String, ClockEdgeTable> = HashMap::new();

    // Pre-register root in visited to prevent re-enqueue
    visited.insert((root_node.signal_path.clone(), root_node.time_index));
    queue.push_back(root_node);

    while !queue.is_empty() {
        let mut node = queue.pop_front().unwrap();

        // Read node value if not already set (signal already pre-loaded)
        if node.actual_value.is_none() {
            node.actual_value = Some(read_signal_value_cached(
                waveform,
                &node.resolved_signal_path,
                node.time_index,
            )?);
        }

        // Check stop signals
        if options.stop_signals.contains(&node.signal_path) {
            node.status = NodeStatus::Stopped;
            tree.push(node);
            continue;
        }

        // Check depth limit
        if node.depth >= options.max_depth {
            node.status = NodeStatus::Truncated;
            tree.push(node);
            continue;
        }

        // Get fan-in dependencies
        let deps = dep_graph.fan_in(&node.signal_path);
        let deps = match deps {
            Some(d) if !d.is_empty() => d,
            _ => {
                node.status = NodeStatus::Boundary;
                tree.push(node);
                continue;
            }
        };
        let mut children: Vec<BfsNode> = Vec::new();

        for dep_edge in deps {
            let child_resolved_path =
                resolve_signal_path(dep_graph, &dep_edge.signal, &options.simulator);

            // Try to resolve time and read value; if either fails, create
            // an Unresolved node and continue BFS from other edges.
            let child_data = (|| -> WaveResult<(usize, u64, String)> {
                let ti = resolve_dep_time_index(
                    waveform,
                    dep_graph,
                    &node,
                    dep_edge,
                    &mut clock_edge_cache,
                    options,
                )?;
                let time_ps = compute_time_ps_from_table(&time_table, ti, timescale.as_ref());
                let value = read_signal_value_cached(waveform, &child_resolved_path, ti)?;
                Ok((ti, time_ps, value))
            })();

            let (child_time_index, child_time_ps, child_value) = match child_data {
                Ok((ti, tps, val)) => (ti, tps, val),
                Err(_reason) => {
                    // Signal or clock not found — mark as Unresolved and continue
                    let unresolved_note = if child_resolved_path != dep_edge.signal {
                        format!(
                            "signal '{}' resolved to '{}' not found in waveform",
                            dep_edge.signal, child_resolved_path
                        )
                    } else {
                        format!(
                            "signal '{}' not found in waveform (no alias mapping)",
                            dep_edge.signal
                        )
                    };
                    let unresolved_node = BfsNode {
                        signal_path: dep_edge.signal.clone(),
                        resolved_signal_path: child_resolved_path,
                        time_index: node.time_index,
                        time_ps: node.time_ps,
                        depth: node.depth + 1,
                        status: NodeStatus::Unresolved,
                        actual_value: None,
                        expected_hint: Some(unresolved_note),
                        edge_type: Some(format_dep_type(&dep_edge.dep_type)),
                        clock_name: dep_edge.clock.clone(),
                        latency_cycles: Some(dep_edge.latency_cycles.unwrap_or(0)),
                        note: None,
                        parent_id: Some(node.node_id.clone()),
                        node_id: format!("n{}", node_counter),
                        source_clock_domain: dep_edge.cdc_from_clock.clone(),
                        dest_clock_domain: dep_edge.cdc_to_clock.clone(),
                        synchronizer_info: None,
                    };
                    node_counter += 1;
                    children.push(unresolved_node);
                    continue;
                }
            };

            // Evaluate edge status, then override with stop/depth/boundary checks
            let (eval_status, expected_hint) = if options.enable_auto_check {
                evaluate_edge_status(
                    &node,
                    &child_value,
                    dep_edge,
                    &child_resolved_path,
                    dep_graph,
                    options,
                    waveform,
                    &condition_cache,
                    child_time_index,
                )
            } else {
                (NodeStatus::Context, None)
            };
            let mut child_status = eval_status;

            // Check if child is a stop signal (overrides any other status)
            if options.stop_signals.contains(&dep_edge.signal) {
                child_status = NodeStatus::Stopped;
            }

            // Check if child depth exceeds max_depth
            if node.depth + 1 >= options.max_depth && child_status != NodeStatus::Stopped {
                child_status = NodeStatus::Truncated;
            }

            if should_collapse_self_history(&node, dep_edge, child_time_index, &child_status) {
                child_status = NodeStatus::Cyclic;
            }

            // Check if child has no fan-in (annotate as leaf, don't override status)
            let is_leaf = if !options.stop_signals.contains(&dep_edge.signal) {
                match dep_graph.fan_in(&dep_edge.signal) {
                    Some(d) => d.is_empty(),
                    None => true,
                }
            } else {
                false
            };

            let is_cdc_penetrated = child_status == NodeStatus::CdcPenetrated;

            let child_node = BfsNode {
                signal_path: dep_edge.signal.clone(),
                resolved_signal_path: child_resolved_path,
                time_index: child_time_index,
                time_ps: child_time_ps,
                depth: node.depth + 1,
                status: child_status,
                actual_value: Some(child_value),
                expected_hint,
                edge_type: Some(format_dep_type(&dep_edge.dep_type)),
                clock_name: dep_edge.clock.clone(),
                latency_cycles: Some(dep_edge.latency_cycles.unwrap_or(0)),
                note: if is_leaf {
                    Some("leaf: no fan-in".to_string())
                } else {
                    None
                },
                parent_id: Some(node.node_id.clone()),
                node_id: format!("n{}", node_counter),
                source_clock_domain: dep_edge.cdc_from_clock.clone(),
                dest_clock_domain: dep_edge.cdc_to_clock.clone(),
                synchronizer_info: None,
            };
            node_counter += 1;

            children.push(child_node);

            // If CDC penetrated, try to expand synchronizer chain
            if is_cdc_penetrated {
                let crossing = CdcCrossing {
                    signal_path: dep_edge.signal.clone(),
                    from_domain: dep_edge.cdc_from_clock.clone().unwrap_or_default(),
                    to_domain: dep_edge.cdc_to_clock.clone().unwrap_or_default(),
                    source: "bfs_trace".to_string(),
                    has_synchronizer: false,
                    synchronizer: None,
                    description: dep_edge.description.clone(),
                };
                let sync_result =
                    detect_synchronizer(waveform, &crossing, dep_graph, &options.simulator);
                if let Ok(Some(sync_info)) = sync_result {
                    let stages = sync_info.stages;
                    let dest_clock = dep_edge.cdc_to_clock.clone().unwrap_or_default();
                    let dest_edge_type = dep_edge.edge.clone().unwrap_or(ClockEdge::Posedge);
                    let cdt_edge_type = match dest_edge_type {
                        ClockEdge::Negedge => ClockEdgeType::Negedge,
                        ClockEdge::Posedge => ClockEdgeType::Posedge,
                    };
                    let cdt = if !dest_clock.is_empty() {
                        let dest_path =
                            resolve_signal_path(dep_graph, &dest_clock, &options.simulator);
                        Some(build_clock_edge_table(waveform, &dest_path, cdt_edge_type).ok())
                    } else {
                        None
                    };

                    // Create CdcSynchronizer nodes for intermediate FFs
                    // The final node (child) gets synchronizer_info annotation
                    for (stage_idx, sync_signal) in
                        sync_info.intermediate_signals.iter().enumerate()
                    {
                        let sync_time_index = if let Some(Some(ref edge_table)) = cdt {
                            // Step back from child_time_index by (stages - stage_idx) clock edges
                            edge_table.step_back(child_time_index, stages - stage_idx as u32 - 1)
                        } else {
                            child_time_index
                        };
                        let sync_resolved =
                            resolve_signal_path(dep_graph, sync_signal, &options.simulator);
                        let sync_value =
                            read_signal_value_cached(waveform, &sync_resolved, sync_time_index)?;
                        let sync_time_ps = compute_time_ps_from_table(
                            &time_table,
                            sync_time_index,
                            timescale.as_ref(),
                        );

                        let sync_node = BfsNode {
                            signal_path: sync_signal.clone(),
                            resolved_signal_path: sync_resolved,
                            time_index: sync_time_index,
                            time_ps: sync_time_ps,
                            depth: node.depth + 1 + stage_idx + 1,
                            status: NodeStatus::CdcSynchronizer,
                            actual_value: Some(sync_value),
                            expected_hint: None,
                            edge_type: Some("cdc_synchronizer".to_string()),
                            clock_name: Some(dest_clock.clone()),
                            latency_cycles: Some(stage_idx as u32),
                            note: None,
                            parent_id: Some(node.node_id.clone()),
                            node_id: format!("n{}", node_counter),
                            source_clock_domain: dep_edge.cdc_from_clock.clone(),
                            dest_clock_domain: dep_edge.cdc_to_clock.clone(),
                            synchronizer_info: Some(format!(
                                "{}-FF synchronizer stage {} of {}",
                                sync_info.kind,
                                stage_idx + 1,
                                stages
                            )),
                        };
                        node_counter += 1;
                        children.push(sync_node);
                    }

                    // Annotate the penetrated child with synchronizer info
                    if let Some(last_child) = children.last_mut()
                        && last_child.status == NodeStatus::CdcPenetrated
                    {
                        last_child.synchronizer_info =
                            Some(format!("{}-stage {} synchronizer", stages, sync_info.kind));
                    }
                }
            }
        }

        // Summarize node status based on children
        node.status = summarize_node_status(&node, &children);
        tree.push(node);

        // Record non-expandable children immediately; expandable children are
        // recorded when they are actually processed from the queue so each
        // logical node appears only once in the result tree.
        for child in children {
            let child_key = (child.signal_path.clone(), child.time_index);
            if visited.contains(&child_key) {
                let mut cyclic_child = child.clone();
                cyclic_child.status = NodeStatus::Cyclic;
                tree.push(cyclic_child);
                continue;
            }
            visited.insert(child_key);
            if should_expand(&child.status) {
                let has_fan_in = dep_graph
                    .fan_in(&child.signal_path)
                    .is_some_and(|d| !d.is_empty());
                if has_fan_in {
                    queue.push_back(child);
                    continue;
                }
            }
            tree.push(child);
        }
    }

    // Build candidate list
    let candidates = build_candidate_list(&tree);

    // Build summary
    let summary = build_summary(&tree, &candidates);

    Ok(BfsResult {
        root_signal: signal_path.to_string(),
        root_time_index: time_index,
        root_time_ps,
        tree,
        candidates,
        summary,
    })
}

/// Resolve the time index for a dependency edge.
fn resolve_dep_time_index(
    waveform: &mut wellen::simple::Waveform,
    dep_graph: &DepGraph,
    node: &BfsNode,
    dep_edge: &DepEdge,
    clock_edge_cache: &mut HashMap<String, ClockEdgeTable>,
    options: &BfsOptions,
) -> WaveResult<usize> {
    match dep_edge.dep_type {
        DepType::Combinational => {
            // Same observation time, no backtracking
            Ok(node.time_index)
        }
        DepType::Boundary => {
            // CDC-penetrated edges backtrack by destination domain clock
            if dep_edge.boundary_kind == Some(BoundaryKind::Cdc)
                && options.penetrate_cdc
                && dep_edge.clock.is_some()
                && dep_edge.latency_cycles.unwrap_or(0) > 0
            {
                backtrack_by_clock(
                    waveform,
                    dep_graph,
                    node.time_index,
                    dep_edge,
                    clock_edge_cache,
                    &options.simulator,
                )
            } else {
                // Regular boundary: same observation time
                Ok(node.time_index)
            }
        }
        DepType::Control => {
            if dep_edge.latency_cycles.unwrap_or(0) == 0 {
                // Control with latency=0: same observation time
                Ok(node.time_index)
            } else {
                // Control with latency>0: backtrack by clock edges
                backtrack_by_clock(
                    waveform,
                    dep_graph,
                    node.time_index,
                    dep_edge,
                    clock_edge_cache,
                    &options.simulator,
                )
            }
        }
        DepType::Sequential | DepType::Memory => {
            if dep_edge.latency_cycles.unwrap_or(0) == 0 {
                // Sequential/memory with latency=0: align to nearest clock edge (no backtrack)
                backtrack_by_clock(
                    waveform,
                    dep_graph,
                    node.time_index,
                    dep_edge,
                    clock_edge_cache,
                    &options.simulator,
                )
            } else {
                backtrack_by_clock(
                    waveform,
                    dep_graph,
                    node.time_index,
                    dep_edge,
                    clock_edge_cache,
                    &options.simulator,
                )
            }
        }
        DepType::Protocol => {
            if dep_edge.latency_cycles.unwrap_or(0) == 0 {
                Ok(node.time_index)
            } else {
                backtrack_by_clock(
                    waveform,
                    dep_graph,
                    node.time_index,
                    dep_edge,
                    clock_edge_cache,
                    &options.simulator,
                )
            }
        }
    }
}

/// Backtrack time by clock edges.
fn backtrack_by_clock(
    waveform: &mut wellen::simple::Waveform,
    dep_graph: &DepGraph,
    from_time_index: usize,
    dep_edge: &DepEdge,
    clock_edge_cache: &mut HashMap<String, ClockEdgeTable>,
    simulator: &str,
) -> WaveResult<usize> {
    let clock_name = dep_edge
        .clock
        .as_ref()
        .ok_or_else(|| WaveAnalyzerError::BfsError {
            message: format!(
                "TIME_MAPPING_ERROR: Sequential/memory edge for '{}' has no clock reference",
                dep_edge.signal
            ),
        })?;

    let edge_type = match dep_edge.edge.as_ref() {
        Some(crate::deps::ClockEdge::Posedge) => ClockEdgeType::Posedge,
        Some(crate::deps::ClockEdge::Negedge) => ClockEdgeType::Negedge,
        None => ClockEdgeType::Posedge, // Default to posedge
    };

    // Resolve clock path from aliases
    let clock_path = dep_graph.resolve_clock(clock_name, simulator)?;

    // Get or build clock edge table
    let cache_key = format!(
        "{}_{}",
        clock_path,
        match edge_type {
            ClockEdgeType::Posedge => "posedge",
            ClockEdgeType::Negedge => "negedge",
        }
    );
    if !clock_edge_cache.contains_key(&cache_key) {
        let edge_table = build_clock_edge_table(waveform, &clock_path, edge_type)?;
        clock_edge_cache.insert(cache_key.clone(), edge_table);
    }

    let edge_table = clock_edge_cache.get(&cache_key).unwrap();
    let latency = dep_edge.latency_cycles.unwrap_or(0);

    Ok(edge_table.step_back(from_time_index, latency))
}

/// Evaluate edge status based on check expression or condition expression.
fn evaluate_edge_status(
    parent: &BfsNode,
    child_value: &str,
    dep_edge: &DepEdge,
    child_resolved_path: &str,
    dep_graph: &DepGraph,
    options: &BfsOptions,
    waveform: &mut wellen::simple::Waveform,
    condition_cache: &ConditionCache,
    child_time_index: usize,
) -> (NodeStatus, Option<String>) {
    // Boundary edges: differentiate CDC from other boundary kinds
    if dep_edge.dep_type == DepType::Boundary {
        return match dep_edge.boundary_kind {
            Some(BoundaryKind::Cdc) => {
                if options.penetrate_cdc {
                    let stages = dep_edge.latency_cycles.unwrap_or(0);
                    if stages >= options.cdc_min_sync_stages {
                        (NodeStatus::CdcPenetrated, None)
                    } else {
                        (NodeStatus::CdcBoundary, None)
                    }
                } else {
                    (NodeStatus::CdcBoundary, None)
                }
            }
            _ => (NodeStatus::Boundary, None),
        };
    }

    // Clock edges often model "this node updates on clock X" rather than
    // "the node value should equal the clock value". When the upstream signal
    // is exactly the resolved reference clock, treat it as a valid timing edge.
    if dep_edge.clock.is_some()
        && is_reference_clock_edge(dep_edge, child_resolved_path, dep_graph, &options.simulator)
    {
        return (NodeStatus::Ok, None);
    }

    // Priority: condition_expression > check > Context
    if let Some(ref expr) = dep_edge.condition_expression
        && let Some(cond_ast) = condition_cache.parsed.get(expr)
        && let Some(sig_cache) = condition_cache.signal_caches.get(expr)
    {
        let child_time_index_for_cond = child_time_index;
        let result = evaluate_condition(cond_ast, waveform, sig_cache, child_time_index_for_cond);
        match result {
            Ok(value) => {
                let expected_hint = Some(format!("condition '{}' should be true", expr));
                return if value.is_zero() {
                    (NodeStatus::Suspect, expected_hint)
                } else {
                    (NodeStatus::Ok, expected_hint)
                };
            }
            Err(_) => {
                // Evaluation failed, fall through to check field
            }
        }
    }

    // Fallback: use check field
    let check = match &dep_edge.check {
        Some(expr) => expr,
        None => {
            // No check specified — try to infer logic for combinational edges.
            // When `logic_type` is provided in deps.yaml, use it for accurate
            // classification. Otherwise, fall back to value-based inference.
            if dep_edge.dep_type == DepType::Combinational {
                let parent_val = parent.actual_value.as_deref().unwrap_or("");
                let parent_big = parse_value_string_to_biguint(parent_val);
                let child_big = parse_value_string_to_biguint(child_value);

                // Only apply to 1-bit signals (simple Boolean logic)
                if parent_big <= BigUint::from(1u32) && child_big <= BigUint::from(1u32) {
                    // When logic_type is specified, use precise classification:
                    // - OR/NOR: input=1, output=0 → Suspect (contradiction);
                    //           input=0, output=1 → Context (other inputs may satisfy)
                    // - AND/NAND: input=0, output=1 → Suspect (contradiction);
                    //             input=1, output=0 → Context (other inputs may drive low)
                    // - XOR/MUX: always Context (ambiguous for single-input analysis)
                    if let Some(ref logic) = dep_edge.logic_type {
                        return match logic {
                            LogicType::Or | LogicType::Nor => {
                                // OR/NOR: any input=1 should produce output=1 (or 0 for NOR)
                                if parent_big.is_zero() && !child_big.is_zero() {
                                    // output=0 but input=1: this input should have driven output high
                                    (
                                        NodeStatus::Suspect,
                                        Some(format!(
                                            "{} logic: input=1 but output=0, contradiction",
                                            logic
                                        )),
                                    )
                                } else if !parent_big.is_zero() && child_big.is_zero() {
                                    // output=1 but input=0: other inputs satisfy, this is not contributing
                                    (
                                        NodeStatus::Context,
                                        Some(format!(
                                            "{} logic: output=1, input=0 not contributing",
                                            logic
                                        )),
                                    )
                                } else {
                                    (NodeStatus::Context, None)
                                }
                            }
                            LogicType::And | LogicType::Nand => {
                                // AND/NAND: all inputs must be 1 for output=1 (or 0 for NAND)
                                if !parent_big.is_zero() && child_big.is_zero() {
                                    // output=1 but input=0: this input breaks the AND chain
                                    (
                                        NodeStatus::Suspect,
                                        Some(format!(
                                            "{} logic: output=1 but input=0, contradiction",
                                            logic
                                        )),
                                    )
                                } else if parent_big.is_zero() && !child_big.is_zero() {
                                    // output=0 and input=1: other inputs drive output low, normal
                                    (
                                        NodeStatus::Context,
                                        Some(format!(
                                            "{} logic: output=0, input=1 not the cause",
                                            logic
                                        )),
                                    )
                                } else {
                                    (NodeStatus::Context, None)
                                }
                            }
                            LogicType::Xor => {
                                // XOR: cannot determine from single input analysis
                                if parent_big.is_zero() && !child_big.is_zero() {
                                    (
                                        NodeStatus::Context,
                                        Some(
                                            "XOR logic: single-input analysis ambiguous"
                                                .to_string(),
                                        ),
                                    )
                                } else if !parent_big.is_zero() && child_big.is_zero() {
                                    (
                                        NodeStatus::Context,
                                        Some(
                                            "XOR logic: other inputs may determine output"
                                                .to_string(),
                                        ),
                                    )
                                } else {
                                    (NodeStatus::Context, None)
                                }
                            }
                            LogicType::Mux => {
                                // MUX: this input may not be the selected one
                                (
                                    NodeStatus::Context,
                                    Some("MUX logic: check selector signal".to_string()),
                                )
                            }
                        };
                    }

                    // No logic_type — use ambiguous inference from values alone.
                    if parent_big.is_zero() && !child_big.is_zero() {
                        // Output=0 but this input=1: OR logic contradiction
                        return (
                            NodeStatus::Suspect,
                            Some("unexpected: input=1 but output=0 (implies OR logic)".to_string()),
                        );
                    } else if !parent_big.is_zero() && child_big.is_zero() {
                        // Output=1 but this input=0: ambiguous logic.
                        // For OR logic: other inputs may satisfy output → this not contributing
                        // For AND logic: this is a contradiction (all inputs must be 1 for output=1)
                        // Mark as Context since we cannot determine logic type from one edge alone.
                        return (
                            NodeStatus::Context,
                            Some("output=1, input=0: ambiguous logic (may be OR/AND; check other inputs; specify logic_type for accurate classification)".to_string()),
                        );
                    }
                }
            }
            return (NodeStatus::Context, None);
        }
    };

    // Control edges: Equal/NotEqual compare parent vs child values which is
    // semantically wrong for control conditions. Return Context instead.
    if dep_edge.dep_type == DepType::Control {
        match check {
            CheckExpr::Equal | CheckExpr::NotEqual => {
                return (NodeStatus::Context, None);
            }
            // GreaterThanZero and EqualZero only check child value — semantically correct
            CheckExpr::GreaterThanZero | CheckExpr::EqualZero => {}
        }
    }

    let parent_value_str = parent.actual_value.as_deref().unwrap_or("");
    let parent_bigint = parse_value_string_to_biguint(parent_value_str);
    let child_bigint = parse_value_string_to_biguint(child_value);

    match check {
        CheckExpr::Equal => {
            let expected_hint = Some(format!("expected {}", parent_value_str));
            if parent_bigint == child_bigint {
                (NodeStatus::Ok, expected_hint)
            } else {
                (NodeStatus::Suspect, expected_hint)
            }
        }
        CheckExpr::NotEqual => {
            let expected_hint = Some(format!("should differ from {}", parent_value_str));
            if parent_bigint != child_bigint {
                (NodeStatus::Ok, expected_hint)
            } else {
                (NodeStatus::Suspect, expected_hint)
            }
        }
        CheckExpr::GreaterThanZero => {
            let expected_hint = Some("should be > 0".to_string());
            if child_bigint > BigUint::from(0u32) {
                (NodeStatus::Ok, expected_hint)
            } else {
                (NodeStatus::Suspect, expected_hint)
            }
        }
        CheckExpr::EqualZero => {
            let expected_hint = Some("should be 0".to_string());
            if child_bigint == BigUint::from(0u32) {
                (NodeStatus::Ok, expected_hint)
            } else {
                (NodeStatus::Suspect, expected_hint)
            }
        }
    }
}

fn is_reference_clock_edge(
    dep_edge: &DepEdge,
    child_resolved_path: &str,
    dep_graph: &DepGraph,
    simulator: &str,
) -> bool {
    let Some(clock_name) = dep_edge.clock.as_deref() else {
        return false;
    };

    dep_graph
        .resolve_clock(clock_name, simulator)
        .is_ok_and(|clock_path| clock_path == child_resolved_path)
}

/// Summarize a node's status based on its children.
fn summarize_node_status(node: &BfsNode, children: &[BfsNode]) -> NodeStatus {
    // If all key children are Ok and this node is still Suspect,
    // it may be a RootCauseCandidate
    let has_ok_children = children.iter().any(|c| c.status == NodeStatus::Ok);
    let has_suspect_children = children.iter().any(|c| c.status == NodeStatus::Suspect);
    let has_cyclic_children = children
        .iter()
        .any(|c| matches!(c.status, NodeStatus::Cyclic | NodeStatus::Truncated));
    let has_boundary_children = children
        .iter()
        .any(|c| matches!(c.status, NodeStatus::Boundary | NodeStatus::CdcBoundary));

    if has_ok_children
        && !has_suspect_children
        && !has_cyclic_children
        && node.status == NodeStatus::Suspect
    {
        NodeStatus::RootCauseCandidate
    } else if has_boundary_children {
        NodeStatus::Boundary
    } else if has_suspect_children || has_cyclic_children {
        NodeStatus::Suspect
    } else if has_ok_children {
        NodeStatus::Ok
    } else {
        node.status.clone()
    }
}

fn should_collapse_self_history(
    node: &BfsNode,
    dep_edge: &DepEdge,
    child_time_index: usize,
    child_status: &NodeStatus,
) -> bool {
    if node.parent_id.is_none() {
        return false;
    }

    if dep_edge.signal != node.signal_path || child_time_index >= node.time_index {
        return false;
    }

    if !matches!(dep_edge.dep_type, DepType::Sequential | DepType::Memory) {
        return false;
    }

    if dep_edge.latency_cycles.unwrap_or(0) == 0 {
        return false;
    }

    matches!(
        child_status,
        NodeStatus::Suspect | NodeStatus::RootCauseCandidate
    )
}

/// Determine if a child node should be further expanded.
fn should_expand(status: &NodeStatus) -> bool {
    matches!(
        status,
        NodeStatus::Suspect | NodeStatus::RootCauseCandidate | NodeStatus::CdcPenetrated
    )
}

/// Build the candidate list from the trace tree.
fn build_candidate_list(tree: &[BfsNode]) -> Vec<RootCauseCandidate> {
    // RootCauseCandidate nodes are top candidates
    let mut candidates: Vec<RootCauseCandidate> = tree
        .iter()
        .filter(|node| node.status == NodeStatus::RootCauseCandidate)
        .map(|node| RootCauseCandidate {
            signal_path: node.signal_path.clone(),
            time_index: node.time_index,
            time_ps: node.time_ps,
            status: node.status.clone(),
            reason: "All fan-in inputs are correct, but this node remains suspect".to_string(),
        })
        .collect();

    // Also include Suspect nodes that are control signals (likely high-value)
    for node in tree {
        if node.status == NodeStatus::Suspect && node.edge_type.as_deref() == Some("control") {
            candidates.push(RootCauseCandidate {
                signal_path: node.signal_path.clone(),
                time_index: node.time_index,
                time_ps: node.time_ps,
                status: NodeStatus::Suspect,
                reason: "Control signal is suspect, worth checking first".to_string(),
            });
        }
    }

    // Sort by priority: RootCauseCandidate > Suspect, then by time_index (earlier = higher priority)
    candidates.sort_by(|a, b| match (&a.status, &b.status) {
        (NodeStatus::RootCauseCandidate, NodeStatus::Suspect) => std::cmp::Ordering::Less,
        (NodeStatus::Suspect, NodeStatus::RootCauseCandidate) => std::cmp::Ordering::Greater,
        _ => a.time_index.cmp(&b.time_index),
    });

    candidates
}

/// Build a text summary of the trace.
fn build_summary(tree: &[BfsNode], candidates: &[RootCauseCandidate]) -> String {
    let mut lines = Vec::new();

    if let Some(root) = tree.first() {
        lines.push(format!(
            "Root: {} @ time_index={} ({})",
            root.signal_path,
            root.time_index,
            root.actual_value.as_deref().unwrap_or("N/A")
        ));
    }

    for node in tree.iter().skip(1) {
        let prefix = if node.depth == 1 {
            "├─"
        } else {
            "│  └─"
        };
        lines.push(format!(
            "{} {} @ {} = {} [{}]",
            prefix,
            node.signal_path,
            node.time_index,
            node.actual_value.as_deref().unwrap_or("N/A"),
            format_node_status(&node.status)
        ));
    }

    if !candidates.is_empty() {
        lines.push(String::new());
        lines.push("Top Candidates:".to_string());
        for (i, candidate) in candidates.iter().enumerate().take(3) {
            lines.push(format!(
                "{}. {} @ time_index={} ({})",
                i + 1,
                candidate.signal_path,
                candidate.time_index,
                candidate.reason
            ));
        }
    }

    lines.join("\n")
}

/// Helper: resolve signal path through aliases, with fuzzy leaf-name fallback.
fn resolve_signal_path(dep_graph: &DepGraph, canonical: &str, simulator: &str) -> String {
    dep_graph
        .resolve_signal_fuzzy(canonical, simulator)
        .unwrap_or(canonical.to_string())
}

fn resolve_canonical_signal_path(dep_graph: &DepGraph, path: &str, simulator: &str) -> String {
    dep_graph
        .canonicalize_signal_fuzzy(path, simulator)
        .unwrap_or_else(|| path.to_string())
}

/// Helper: format dep type as string.
fn format_dep_type(dep_type: &DepType) -> String {
    match dep_type {
        DepType::Combinational => "combinational".to_string(),
        DepType::Sequential => "sequential".to_string(),
        DepType::Memory => "memory".to_string(),
        DepType::Control => "control".to_string(),
        DepType::Protocol => "protocol".to_string(),
        DepType::Boundary => "boundary".to_string(),
    }
}

/// Helper: format node status as string.
fn format_node_status(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Suspect => "Suspect",
        NodeStatus::Ok => "Ok",
        NodeStatus::Boundary => "Boundary",
        NodeStatus::Stopped => "Stopped",
        NodeStatus::Truncated => "Truncated",
        NodeStatus::Cyclic => "Cyclic",
        NodeStatus::Context => "Context",
        NodeStatus::RootCauseCandidate => "RootCauseCandidate",
        NodeStatus::CdcBoundary => "CdcBoundary",
        NodeStatus::CdcPenetrated => "CdcPenetrated",
        NodeStatus::CdcSynchronizer => "CdcSynchronizer",
        NodeStatus::Unresolved => "Unresolved",
    }
}

/// Parse a Verilog-style value string (like "8'h5A" or "1'b0") to BigUint.
fn parse_value_string_to_biguint(value_str: &str) -> BigUint {
    // Try to parse Verilog-style format: N'bXXX, N'dXXX, N'hXXX
    if let Some(pos) = value_str.find('\'') {
        let _width_str = &value_str[..pos];
        let rest = &value_str[pos + 1..];

        if let Some(val_str) = rest.strip_prefix('b') {
            // Binary
            let clean = val_str.replace('_', "");
            let mut result = BigUint::from(0u32);
            for c in clean.chars() {
                result <<= 1u32;
                if c == '1' {
                    result |= BigUint::from(1u32);
                }
            }
            return result;
        } else if let Some(val_str) = rest.strip_prefix('d') {
            // Decimal
            let clean = val_str.replace('_', "");
            return clean
                .parse::<u64>()
                .map(BigUint::from)
                .unwrap_or(BigUint::from(0u32));
        } else if let Some(val_str) = rest.strip_prefix('h') {
            // Hex
            let clean = val_str.replace('_', "");
            return u64::from_str_radix(&clean, 16)
                .map(BigUint::from)
                .unwrap_or(BigUint::from(0u32));
        }
    }

    // Try to parse as plain decimal
    value_str
        .parse::<u64>()
        .map(BigUint::from)
        .unwrap_or(BigUint::from(0u32))
}

/// Collect all resolved waveform signal paths from the dependency graph.
fn collect_all_signals_from_dep_graph(dep_graph: &DepGraph, simulator: &str) -> Vec<String> {
    let mut signals = Vec::new();

    // Collect from all fan-in edges
    for output in dep_graph.output_signals() {
        signals.push(resolve_signal_path(dep_graph, &output, simulator));
        if let Some(edges) = dep_graph.fan_in(&output) {
            for edge in edges {
                signals.push(resolve_signal_path(dep_graph, &edge.signal, simulator));
            }
        }
    }

    // Deduplicate
    signals.sort();
    signals.dedup();
    signals
}

/// Build condition cache: parse condition expressions and resolve signal VarRefs.
fn build_condition_cache(
    dep_graph: &DepGraph,
    waveform: &mut wellen::simple::Waveform,
    simulator: &str,
    cache: &mut ConditionCache,
) -> WaveResult<()> {
    for output in dep_graph.output_signals() {
        if let Some(edges) = dep_graph.fan_in(&output) {
            for edge in edges {
                if let Some(ref expr) = edge.condition_expression {
                    if cache.parsed.contains_key(expr) {
                        continue;
                    }
                    let cond = parse_condition(expr).map_err(|e| {
                        WaveAnalyzerError::ConditionParseError {
                            message: format!(
                                "Failed to parse condition_expression '{}': {}",
                                expr, e
                            ),
                        }
                    })?;
                    cache.parsed.insert(expr.clone(), cond.clone());

                    let signal_names = extract_signal_names(&cond);
                    let hierarchy = waveform.hierarchy();
                    let mut sig_cache = HashMap::new();
                    for canonical in &signal_names {
                        let resolved = dep_graph
                            .resolve_signal(canonical, simulator)
                            .unwrap_or_else(|| canonical.clone());
                        if let Ok(entry) = build_signal_cache_entry(hierarchy, &resolved) {
                            sig_cache.insert(canonical.clone(), entry);
                        }
                    }
                    cache.signal_caches.insert(expr.clone(), sig_cache);
                }
            }
        }
    }
    Ok(())
}

/// Collect resolved signal paths from condition expression caches for pre-loading.
fn collect_condition_signals(
    cache: &ConditionCache,
    dep_graph: &DepGraph,
    simulator: &str,
) -> Vec<String> {
    let mut signals = Vec::new();
    for sig_cache in cache.signal_caches.values() {
        for (canonical, _entry) in sig_cache {
            let resolved = dep_graph
                .resolve_signal(canonical, simulator)
                .unwrap_or_else(|| canonical.clone());
            if !signals.contains(&resolved) {
                signals.push(resolved);
            }
        }
    }
    // Also try to load any signals referenced in parsed conditions that didn't get VarRefs
    // (they may exist in waveform but weren't found by canonical path)
    for (expr, cond) in &cache.parsed {
        let names = extract_signal_names(cond);
        for name in &names {
            if !cache
                .signal_caches
                .get(expr)
                .is_some_and(|sc| sc.contains_key(name))
            {
                let resolved = dep_graph
                    .resolve_signal(name, simulator)
                    .unwrap_or_else(|| name.clone());
                if !signals.contains(&resolved) {
                    signals.push(resolved);
                }
            }
        }
    }
    signals
}

/// Pre-load multiple signals into waveform to avoid repeated load_signals calls.
fn preload_signals(
    waveform: &mut wellen::simple::Waveform,
    signal_paths: &[String],
) -> WaveResult<()> {
    let hierarchy = waveform.hierarchy();
    let mut signal_refs = Vec::new();

    for path in signal_paths {
        if let Some(var_refs) = resolve_signal_var_refs(hierarchy, path) {
            for var_ref in var_refs {
                let signal_ref = hierarchy[var_ref].signal_ref();
                signal_refs.push(signal_ref);
            }
        }
    }

    if !signal_refs.is_empty() {
        waveform.load_signals(&signal_refs);
    }

    Ok(())
}

/// Read signal value without loading (assumes signal is already loaded).
fn read_signal_value_cached(
    waveform: &wellen::simple::Waveform,
    resolved_path: &str,
    time_index: usize,
) -> WaveResult<String> {
    let var_ref = find_var_by_path(waveform.hierarchy(), resolved_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: resolved_path.to_string(),
        }
    })?;
    let signal_ref = waveform.hierarchy()[var_ref].signal_ref();

    let signal =
        waveform
            .get_signal(signal_ref)
            .ok_or_else(|| WaveAnalyzerError::SignalNotFound {
                path: resolved_path.to_string(),
            })?;

    let time_table_idx: wellen::TimeTableIdx =
        time_index
            .try_into()
            .map_err(|_| WaveAnalyzerError::BfsError {
                message: format!("Time index {} too large", time_index),
            })?;

    let offset = signal
        .get_offset(time_table_idx)
        .ok_or_else(|| WaveAnalyzerError::BfsError {
            message: format!(
                "No data for signal '{}' at time index {}",
                resolved_path, time_index
            ),
        })?;

    let signal_value = signal.get_value_at(&offset, 0);
    Ok(format_signal_value(signal_value))
}

/// Result of a batch BFS trace across multiple assertion failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchBfsResult {
    /// Individual trace results with their assertion context.
    pub traces: Vec<BatchTraceEntry>,
    /// Aggregated root cause candidates from all traces, sorted by frequency and priority.
    pub aggregated_candidates: Vec<AggregatedCandidate>,
    /// Summary text.
    pub summary: String,
}

/// A single trace entry in a batch result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTraceEntry {
    /// Event name that triggered this trace.
    pub event_name: String,
    /// Entry signal used for the trace.
    pub entry_signal: String,
    /// Failure time in picoseconds.
    pub fail_time_ps: u64,
    /// Time index in the waveform.
    pub fail_time_index: usize,
    /// The BFS result for this trace.
    pub result: BfsResult,
    /// Error if trace failed for this event.
    pub error: Option<String>,
}

/// An aggregated candidate appearing in multiple traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedCandidate {
    /// Signal path.
    pub signal_path: String,
    /// Number of traces where this signal appeared as a candidate.
    pub occurrence_count: usize,
    /// Time in picoseconds (from the first occurrence).
    pub time_ps: u64,
    /// Status from the most significant occurrence.
    pub status: NodeStatus,
    /// Reasons from each trace.
    pub reasons: Vec<String>,
}

/// Aggregate root-cause candidates across multiple BFS results.
///
/// Candidates sharing the same `signal_path` are merged: occurrence count
/// increments, the earliest time is kept, the highest-priority status wins,
/// and reasons are accumulated.  Results are sorted by
/// `(occurrence_count desc, priority asc, time_ps asc)`.
pub fn aggregate_candidates_from_results(
    candidates: &[RootCauseCandidate],
) -> Vec<AggregatedCandidate> {
    let mut candidate_map: HashMap<String, AggregatedCandidate> = HashMap::new();

    for candidate in candidates {
        let entry = candidate_map
            .entry(candidate.signal_path.clone())
            .or_insert_with(|| AggregatedCandidate {
                signal_path: candidate.signal_path.clone(),
                occurrence_count: 0,
                time_ps: candidate.time_ps,
                status: candidate.status.clone(),
                reasons: Vec::new(),
            });
        entry.occurrence_count += 1;
        entry.reasons.push(candidate.reason.clone());
        // Upgrade status priority: RootCauseCandidate > Suspect > Boundary > others
        if candidate.status.priority_rank() < entry.status.priority_rank() {
            entry.status = candidate.status.clone();
        }
        if candidate.time_ps < entry.time_ps {
            entry.time_ps = candidate.time_ps;
        }
    }

    let mut aggregated: Vec<AggregatedCandidate> = candidate_map.into_values().collect();
    aggregated.sort_by(|a, b| {
        b.occurrence_count
            .cmp(&a.occurrence_count)
            .then(a.status.priority_rank().cmp(&b.status.priority_rank()))
            .then(a.time_ps.cmp(&b.time_ps))
    });
    aggregated
}

/// Perform batch BFS root-cause tracing for multiple assertion failure events.
///
/// For each assertion event, resolves the entry signal (via spec or first
/// output node in deps), runs trace_root_cause, then aggregates candidates
/// across all traces.
pub fn batch_trace_root_cause(
    waveform: &mut wellen::simple::Waveform,
    dep_graph: &DepGraph,
    events: &[crate::assertion::AssertionEvent],
    options: &BfsOptions,
    spec_lookup: Option<&crate::spec::SpecLookup>,
) -> WaveResult<BatchBfsResult> {
    let _time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let mut traces: Vec<BatchTraceEntry> = Vec::new();
    let mut all_candidates: Vec<RootCauseCandidate> = Vec::new();

    for event in events {
        // Find entry signal for this assertion
        let entry_signal = resolve_entry_signal(event, dep_graph, spec_lookup);

        let entry_signal = match entry_signal {
            Some(sig) => sig,
            None => {
                traces.push(BatchTraceEntry {
                    event_name: event.assertion_name.clone(),
                    entry_signal: String::new(),
                    fail_time_ps: event.time_ps,
                    fail_time_index: 0,
                    result: BfsResult {
                        root_signal: String::new(),
                        root_time_index: 0,
                        root_time_ps: 0,
                        tree: Vec::new(),
                        candidates: Vec::new(),
                        summary: format!(
                            "No entry signal found for event '{}'",
                            event.assertion_name
                        ),
                    },
                    error: Some(format!(
                        "No entry signal found for event '{}'",
                        event.assertion_name
                    )),
                });
                continue;
            }
        };

        // Convert event time_ps to time_index
        let time_index = crate::time_map::find_time_index_by_value(waveform, event.time_ps)
            .map_err(|e| WaveAnalyzerError::BfsError {
                message: format!(
                    "Time conversion error for event at {} ps: {}",
                    event.time_ps, e
                ),
            })?;

        // Run BFS for this entry
        let result = trace_root_cause(waveform, dep_graph, &entry_signal, time_index, options);

        match result {
            Ok(bfs_result) => {
                all_candidates.extend(bfs_result.candidates.clone());

                traces.push(BatchTraceEntry {
                    event_name: event.assertion_name.clone(),
                    entry_signal: entry_signal.clone(),
                    fail_time_ps: event.time_ps,
                    fail_time_index: time_index,
                    result: bfs_result,
                    error: None,
                });
            }
            Err(e) => {
                traces.push(BatchTraceEntry {
                    event_name: event.assertion_name.clone(),
                    entry_signal: entry_signal.clone(),
                    fail_time_ps: event.time_ps,
                    fail_time_index: time_index,
                    result: BfsResult {
                        root_signal: entry_signal.clone(),
                        root_time_index: time_index,
                        root_time_ps: event.time_ps,
                        tree: Vec::new(),
                        candidates: Vec::new(),
                        summary: format!("BFS failed: {}", e),
                    },
                    error: Some(e.to_string()),
                });
            }
        }
    }

    let aggregated_candidates = aggregate_candidates_from_results(&all_candidates);

    // Build summary
    let success_count = traces.iter().filter(|t| t.error.is_none()).count();
    let fail_count = traces.len() - success_count;
    let summary = format!(
        "Batch BFS: {} events traced, {} succeeded, {} failed. {} unique root cause candidates identified.",
        traces.len(),
        success_count,
        fail_count,
        aggregated_candidates.len()
    );

    Ok(BatchBfsResult {
        traces,
        aggregated_candidates,
        summary,
    })
}

/// Resolve entry signal for an assertion event.
fn resolve_entry_signal(
    event: &crate::assertion::AssertionEvent,
    dep_graph: &DepGraph,
    spec_lookup: Option<&crate::spec::SpecLookup>,
) -> Option<String> {
    // Try spec lookup first (if available)
    if let Some(spec) = spec_lookup {
        // Try assertion name mapping
        let observe_signals = spec.find_entry_signals_by_assertion(&event.assertion_name);
        if !observe_signals.is_empty() {
            for sig in &observe_signals {
                if dep_graph.has_signal(sig) {
                    return Some(sig.clone());
                }
                let resolved = dep_graph.resolve_signal(sig, "modelsim");
                if let Some(ref r) = resolved
                    && dep_graph.has_signal(r)
                {
                    return Some(r.clone());
                }
            }
            // No match in deps, use first observe signal as-is
            return Some(observe_signals[0].clone());
        }

        // Try debug entry points
        let debug_entry_points = spec.find_debug_entry_points();
        for entry in &debug_entry_points {
            if dep_graph.has_signal(&entry.signal) {
                return Some(entry.signal.clone());
            }
            let resolved = dep_graph.resolve_signal(&entry.signal, "modelsim");
            if let Some(ref r) = resolved
                && dep_graph.has_signal(r)
            {
                return Some(r.clone());
            }
        }
        if !debug_entry_points.is_empty() {
            return Some(debug_entry_points[0].signal.clone());
        }
    }

    // Fallback: use first output signal in deps graph within the assertion's scope
    let output_signals = dep_graph.output_signals();
    for sig in &output_signals {
        // Match scope prefix from assertion event
        if sig.starts_with(&event.scope_path) {
            return Some(sig.clone());
        }
    }

    // Last resort: use first output signal
    if !output_signals.is_empty() {
        Some(output_signals[0].clone())
    } else {
        None
    }
}
