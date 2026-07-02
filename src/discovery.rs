//! Signal auto-discovery module.
//!
//! Discovers bus slices, clock signals, and reset signals from waveform hierarchy
//! by pattern matching signal names and analyzing change behavior.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use wellen::simple::Waveform;

use crate::cdc::ClockDomainInfo;
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::ReportWriter;
use crate::hierarchy::{find_signal_by_path, is_non_observable_var_type};
use crate::report_writeln;

fn signal_leaf_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn behavioral_signal_key(path: &str, changes: &[u32]) -> String {
    format!("{}::{:?}", signal_leaf_name(path), changes)
}

fn prefer_signal_path(current: &str, candidate: &str) -> bool {
    candidate.len() < current.len() || (candidate.len() == current.len() && candidate < current)
}

fn record_unique_behavioral_signal(
    ordered_paths: &mut Vec<String>,
    seen_keys: &mut std::collections::HashMap<String, usize>,
    path: &str,
    changes: &[u32],
) {
    let key = behavioral_signal_key(path, changes);
    if let Some(existing_index) = seen_keys.get(&key).copied() {
        if prefer_signal_path(&ordered_paths[existing_index], path) {
            ordered_paths[existing_index] = path.to_string();
        }
        return;
    }

    seen_keys.insert(key, ordered_paths.len());
    ordered_paths.push(path.to_string());
}

/// Information about a discovered signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalInfo {
    /// Full hierarchical path.
    pub path: String,
    /// Signal width in bits.
    pub width: u32,
    /// Bit index if this appears to be a bus slice element.
    pub bit_index: Option<u32>,
}

/// A discovered bus group (e.g., o_crc[0]..o_crc[15]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusGroup {
    /// Base name of the bus (e.g., "o_crc").
    pub name: String,
    /// Number of bits in the bus.
    pub width: u32,
    /// Individual signal info, sorted by bit index.
    pub signals: Vec<SignalInfo>,
}

/// Result of signal discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    /// Discovered bus groups.
    pub bus_groups: Vec<BusGroup>,
    /// Detected clock signals (by path).
    pub clock_signals: Vec<String>,
    /// Detected reset signals (by path).
    pub reset_signals: Vec<String>,
    /// Individual 1-bit signals that match the pattern filter (if provided).
    /// These are signals that are neither bus groups nor clock/reset signals.
    #[serde(default)]
    pub individual_signals: Vec<String>,
    /// Clock domain grouping (populated when discovery includes "all").
    pub clock_domains: Vec<ClockDomainInfo>,
}

/// Auto-discover signals from the waveform hierarchy.
///
/// Supports discovery modes:
/// - `"bus_slices"`: Find grouped bit signals like `crc[0]`..`crc[15]` or `data_0`..`data_7`.
/// - `"clocks"`: Detect clock signals by name pattern and regular edge behavior.
/// - `"groups"`: Find all bus groups + clocks + resets.
/// - `"all"`: Same as "groups".
pub fn auto_discover_signals(
    waveform: &mut Waveform,
    discovery_mode: &str,
    scope_path: Option<&str>,
    pattern: Option<&str>,
    limit: Option<isize>,
) -> WaveResult<DiscoveryResult> {
    let hierarchy = waveform.hierarchy();

    // Compile pattern regex once before the loop
    let pattern_re = if let Some(pat) = pattern {
        Some(
            Regex::new(pat).map_err(|e| WaveAnalyzerError::InvalidArgument {
                message: format!("Invalid pattern regex: {}", e),
            })?,
        )
    } else {
        None
    };

    // Collect all signal paths, filtering out non-observable variable types.
    // Maintain two lists: pattern-filtered (for bus groups) and unfiltered (for clock/reset).
    // Deduplicate by full_path (not signal_ref) so that the same physical signal
    // can appear at both TB-level and DUT-level hierarchy paths (BUG-4 fix).
    let mut all_signals: Vec<String> = Vec::new();
    let mut filtered_signals: Vec<String> = Vec::new();
    let mut seen_paths = HashSet::new();
    for var in hierarchy.iter_vars() {
        let var_type = var.var_type();
        // Skip non-observable types: parameters, supply nets, strings, events, ports
        // BUG-5/8 fix: also skip supply variables (VCD $supply0/$supply1) which are
        // constant supply nets, not meaningful design signals.
        // BUG-VarType fix: use centralized is_non_observable_var_type() which covers
        // all non-observable types in one check.
        if is_non_observable_var_type(var_type) {
            continue;
        }

        let full_path = var.full_name(hierarchy);

        // Filter by scope_path if provided
        if let Some(scope) = scope_path
            && !full_path.starts_with(scope)
        {
            continue;
        }

        // Deduplicate by path, not signal_ref, so DUT-level paths are kept
        if seen_paths.insert(full_path.clone()) {
            all_signals.push(full_path.clone());

            // Add to pattern-filtered list (for bus group discovery)
            if let Some(ref re) = pattern_re {
                if re.is_match(&full_path) {
                    filtered_signals.push(full_path);
                }
            } else {
                // No pattern: filtered list equals all_signals
                filtered_signals.push(full_path);
            }
        }
    }

    let do_bus =
        discovery_mode == "bus_slices" || discovery_mode == "groups" || discovery_mode == "all";
    let do_clocks =
        discovery_mode == "clocks" || discovery_mode == "groups" || discovery_mode == "all";
    let do_resets = discovery_mode == "groups" || discovery_mode == "all";

    let bus_groups = if do_bus {
        discover_bus_groups(&filtered_signals, hierarchy)
    } else {
        Vec::new()
    };

    let (clock_signals, reset_signals) = if do_clocks || do_resets {
        detect_clock_and_reset_signals(waveform, &all_signals, do_clocks, do_resets)
    } else {
        (Vec::new(), Vec::new())
    };

    // Collect individual 1-bit signals matching pattern
    // (not bus groups, not clock, not reset)
    // Must be after detect_clock_and_reset_signals to avoid borrow conflict
    let hierarchy = waveform.hierarchy();
    let bus_paths: HashSet<String> = bus_groups
        .iter()
        .flat_map(|bg| bg.signals.iter().map(|s| s.path.clone()))
        .collect();
    let clock_set: HashSet<&String> = clock_signals.iter().collect();
    let reset_set: HashSet<&String> = reset_signals.iter().collect();

    let individual_signals: Vec<String> = filtered_signals
        .iter()
        .filter(|path| {
            let width = hierarchy_var_width(hierarchy, path);
            width == 1
                && !bus_paths.contains(path.as_str())
                && !clock_set.contains(path)
                && !reset_set.contains(path)
        })
        .cloned()
        .collect();

    // Apply limit if specified
    let (bus_groups, clock_signals, reset_signals) = if let Some(lim) = limit
        && lim > 0
    {
        let max = lim as usize;
        let bus_groups = if bus_groups.len() > max {
            bus_groups[..max].to_vec()
        } else {
            bus_groups
        };
        let clock_signals = if clock_signals.len() > max {
            clock_signals[..max].to_vec()
        } else {
            clock_signals
        };
        let reset_signals = if reset_signals.len() > max {
            reset_signals[..max].to_vec()
        } else {
            reset_signals
        };
        (bus_groups, clock_signals, reset_signals)
    } else {
        (bus_groups, clock_signals, reset_signals)
    };

    Ok(DiscoveryResult {
        bus_groups,
        clock_signals,
        reset_signals,
        individual_signals,
        clock_domains: Vec::new(), // Populated separately via identify_clock_domains_from_waveform
    })
}

/// Discover bus groups from signal paths.
///
/// Matches patterns like:
/// - `name[N]` (bracket notation): `crc[0]`, `data[7:0]`
/// - `name_N` (underscore notation): `crc_0`, `data_15`
///
/// Internal implementation signals (counter, debounce, CRC, etc.) are
/// filtered out to reduce noise. Only external/port-level bus groups
/// and design-level buses are reported.
fn discover_bus_groups(signals: &[String], hierarchy: &wellen::Hierarchy) -> Vec<BusGroup> {
    // Regex for bracket notation: name[N]
    let bracket_re = Regex::new(r"^(.+?)\[(\d+)\]$").unwrap();
    // Regex for underscore notation: name_N
    let underscore_re = Regex::new(r"^(.+?)_(\d+)$").unwrap();

    // Check if a signal path is an internal implementation signal
    // Only obvious loop variables are filtered. Deeper DUT-level wires are
    // still useful when debugging waveform failures and must be discoverable.
    let is_internal_signal = |path: &str, leaf_name: &str| -> bool {
        let _ = path;
        let obvious_internal = ["i", "j", "k", "n", "m"];
        let leaf_lower = leaf_name.to_lowercase();
        for pattern in &obvious_internal {
            if leaf_lower == *pattern {
                return true;
            }
        }
        false
    };

    // Map: base_name -> Vec<(bit_index, full_path, width)>
    let mut groups: std::collections::HashMap<String, Vec<(u32, String, u32)>> =
        std::collections::HashMap::new();
    let mut vector_groups: Vec<BusGroup> = Vec::new();

    for path in signals {
        let name = path.rsplit('.').next().unwrap_or(path);

        // Skip internal implementation signals
        if is_internal_signal(path, name) {
            continue;
        }

        if let Some(caps) = bracket_re.captures(name) {
            let base = caps.get(1).unwrap().as_str().to_string();
            let bit: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            // Also filter by base name (e.g. counter[N] → base = "counter")
            if is_internal_signal(path, &base) {
                continue;
            }
            let width = hierarchy_var_width(hierarchy, path);
            groups
                .entry(base)
                .or_default()
                .push((bit, path.clone(), width));
        } else if let Some(caps) = underscore_re.captures(name) {
            let base = caps.get(1).unwrap().as_str().to_string();
            let bit: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            if is_internal_signal(path, &base) {
                continue;
            }
            let width = hierarchy_var_width(hierarchy, path);
            groups
                .entry(base)
                .or_default()
                .push((bit, path.clone(), width));
        } else {
            // Single multi-bit signal — also filter internal names
            if is_internal_signal(path, name) {
                continue;
            }
            let width = hierarchy_var_width(hierarchy, path);
            if width > 1 {
                vector_groups.push(BusGroup {
                    name: name.to_string(),
                    width,
                    signals: vec![SignalInfo {
                        path: path.clone(),
                        width,
                        bit_index: None,
                    }],
                });
            }
        }
    }

    // Convert to BusGroup, filtering out single-bit groups
    let mut result: Vec<BusGroup> = vector_groups;
    for (name, mut entries) in groups {
        if entries.len() < 2 {
            continue; // Skip single-bit "buses"
        }
        entries.sort_by_key(|e| e.0);
        let max_bit = entries.last().map(|e| e.0).unwrap_or(0);
        let width = max_bit + 1;

        let signal_infos: Vec<SignalInfo> = entries
            .iter()
            .map(|(bit, path, w)| SignalInfo {
                path: path.clone(),
                width: *w,
                bit_index: Some(*bit),
            })
            .collect();

        result.push(BusGroup {
            name,
            width,
            signals: signal_infos,
        });
    }

    // Sort by name for stable output
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Get signal width from hierarchy.
fn hierarchy_var_width(hierarchy: &wellen::Hierarchy, path: &str) -> u32 {
    let parts: Vec<&str> = path.split('.').collect();
    let var_ref = if parts.len() > 1 {
        hierarchy.lookup_var(&parts[..parts.len() - 1], parts[parts.len() - 1])
    } else {
        hierarchy.lookup_var(&[], path)
    };
    match var_ref {
        Some(vr) => hierarchy[vr].length().unwrap_or(1),
        None => 1,
    }
}

/// Detect clock and reset signals by name pattern and behavior analysis.
fn detect_clock_and_reset_signals(
    waveform: &mut Waveform,
    signals: &[String],
    detect_clocks: bool,
    detect_resets: bool,
) -> (Vec<String>, Vec<String>) {
    let mut clock_signals = Vec::new();
    let mut reset_signals = Vec::new();
    let mut seen_clock_keys = std::collections::HashMap::new();
    let mut seen_reset_keys = std::collections::HashMap::new();

    // Get time table for accurate period calculation (use actual time values, not indices)
    let time_table: Vec<u64> = waveform.time_table().to_vec();

    // Load signals for analysis
    let hierarchy = waveform.hierarchy();
    let mut signal_refs: Vec<wellen::SignalRef> = Vec::new();
    let mut signal_paths: Vec<String> = Vec::new();
    let mut seen_candidate_ids = HashSet::new();

    // BUG-28 fix: track whether each candidate is a clk or rst name match
    // so we can use it in the behavioral classification loop below.
    let mut signal_is_clk: Vec<bool> = Vec::new();
    let mut signal_is_rst: Vec<bool> = Vec::new();

    for path in signals {
        let lower = path.to_lowercase();
        let is_clk = detect_clocks
            && (lower.contains("clk")
                || lower.contains("clock")
                || lower.contains("osc")
                || lower.contains("pll")
                || lower.contains("mclk")
                || lower.contains("sclk")
                || lower.contains("hclk"));
        let is_rst = detect_resets
            && (lower.contains("rst")
                || lower.contains("reset")
                || lower.contains("arst")
                || lower.contains("srst")
                || lower.contains("init")
                || lower.contains("clr")
                || lower.contains("clear")
                || lower.contains("en_reset")
                || lower.contains("nrst"));

        if !is_clk && !is_rst {
            continue;
        }

        // Only analyze 1-bit signals
        let width = hierarchy_var_width(hierarchy, path);
        if width != 1 {
            continue;
        }

        if let Some(sr) = find_signal_by_path(hierarchy, path) {
            if !seen_candidate_ids.insert(sr.index()) {
                continue;
            }
            signal_refs.push(sr);
            signal_paths.push(path.clone());
            signal_is_clk.push(is_clk);
            signal_is_rst.push(is_rst);
        }
    }

    if signal_refs.is_empty() {
        return (clock_signals, reset_signals);
    }

    waveform.load_signals(&signal_refs);

    for (i, path) in signal_paths.iter().enumerate() {
        let is_clk = signal_is_clk[i];
        let is_rst = signal_is_rst[i];
        let signal = match waveform.get_signal(signal_refs[i]) {
            Some(s) => s,
            None => continue,
        };

        let changes: Vec<u32> = signal.iter_changes().map(|(t, _)| t).collect();

        if changes.len() < 2 {
            // Too few changes to be a clock, but might be a reset
            // Signals named rst/reset with 0-1 changes are almost certainly resets
            if is_rst {
                record_unique_behavioral_signal(
                    &mut reset_signals,
                    &mut seen_reset_keys,
                    path,
                    &changes,
                );
            } else if !changes.is_empty() {
                // clk-named signal with only 1 transition: unlikely to be clock,
                // could be an enable/strobe — skip classification
            }
            continue;
        }

        // Check for regular clock behavior: compute periods using actual time values
        // from the time table (not time index differences, which can be non-uniform)
        let periods: Vec<u64> = changes
            .windows(2)
            .map(|w| {
                let t0 = time_table[w[0] as usize];
                let t1 = time_table[w[1] as usize];
                t1 - t0
            })
            .collect();

        if periods.is_empty() {
            continue;
        }

        let avg_period: f64 = periods.iter().map(|&p| p as f64).sum::<f64>() / periods.len() as f64;

        if avg_period < 1.0 {
            continue;
        }

        let variance: f64 = periods
            .iter()
            .map(|&p| {
                let diff = p as f64 - avg_period;
                diff * diff
            })
            .sum::<f64>()
            / periods.len() as f64;

        let stddev = variance.sqrt();
        let cv = stddev / avg_period; // coefficient of variation

        // BUG-28 fix: classify signals based on name + behavior together
        // Require at least 3 periods for reliable clock classification.
        // Signals with only 1-2 transitions produce a single period with zero
        // variance (CV=0), which would falsely classify resets as clocks.
        if cv < 0.1 && periods.len() >= 3 && is_clk {
            // Confirmed clock behavior: regular period with low jitter
            record_unique_behavioral_signal(
                &mut clock_signals,
                &mut seen_clock_keys,
                path,
                &changes,
            );
        } else if is_rst {
            // Named as reset but doesn't show clock behavior — classify as reset.
            // Reset signals can have any number of transitions (assert, deassert,
            // re-assert during error recovery, etc.). Only exclude them if they
            // show clear clock behavior (≥3 periods, low CV).
            record_unique_behavioral_signal(
                &mut reset_signals,
                &mut seen_reset_keys,
                path,
                &changes,
            );
        } else if is_clk && cv < 0.1 && periods.len() >= 3 {
            // clk-named signal with clear clock behavior (already handled above)
            // This branch is redundant but kept for clarity.
        } else if changes.len() <= 10 {
            // Neither clk nor rst name, few transitions: ambiguous signal.
            // Could be an enable/strobe/control — not classified as clock or reset.
        }
    }

    (clock_signals, reset_signals)
}

/// Format a discovery result as human-readable text.
pub fn format_discovery_report(result: &DiscoveryResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "=== Signal Discovery Report ===");
    report_writeln!(out);

    // Bus groups
    report_writeln!(out, "Bus Groups: {}", result.bus_groups.len());
    for group in &result.bus_groups {
        let paths: Vec<&str> = group.signals.iter().map(|s| s.path.as_str()).collect();
        report_writeln!(
            out,
            "  {}[{}:0] ({} bits): {}",
            group.name,
            group.width - 1,
            group.width,
            paths.join(", ")
        );
    }

    report_writeln!(out);

    // Clock signals
    report_writeln!(out, "Clock Signals: {}", result.clock_signals.len());
    for clk in &result.clock_signals {
        report_writeln!(out, "  {}", clk);
    }

    report_writeln!(out);

    // Reset signals
    report_writeln!(out, "Reset Signals: {}", result.reset_signals.len());
    for rst in &result.reset_signals {
        report_writeln!(out, "  {}", rst);
    }

    // Individual 1-bit signals (matching pattern)
    if !result.individual_signals.is_empty() {
        report_writeln!(out);
        report_writeln!(
            out,
            "Individual Signals: {}",
            result.individual_signals.len()
        );
        for sig in &result.individual_signals {
            report_writeln!(out, "  {}", sig);
        }
    }

    out.finish()
}
