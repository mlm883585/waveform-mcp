//! FSM state extraction from waveform signals.
//!
//! Infers FSM state encoding by value clustering on a state register signal,
//! then discovers state transitions by tracking consecutive state changes.
//! Supports clock-aligned observation for synchronous FSMs.
//! Outputs state transition graph in DOT format.

use crate::error::{WaveAnalyzerError, WaveResult};
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wellen::simple::Waveform;

use crate::formatting::{
    ReportWriter, format_biguint_verilog, is_signal_high, signal_value_to_biguint_lenient,
};
use crate::hierarchy::{find_signal_by_path, find_var_by_path};
use crate::protocol::{MeasurementStats, compute_stats};
use crate::report_writeln;

// === Data Structures ===

/// A discovered FSM state.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsmState {
    /// State encoding value (formatted, e.g. "4'h3").
    pub value: String,
    /// Raw numeric value (u64).
    pub numeric_value: u64,
    /// State name (auto-generated or from user mapping).
    pub name: String,
    /// Number of time indices where this state was observed.
    pub occurrence_count: usize,
    /// Fraction of total time spent in this state.
    pub fraction: f64,
}

/// A discovered FSM state transition edge.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsmTransition {
    /// Source state name.
    pub from_state: String,
    /// Source state value (formatted).
    pub from_value: String,
    /// Destination state name.
    pub to_state: String,
    /// Destination state value (formatted).
    pub to_value: String,
    /// Number of times this transition was observed.
    pub count: usize,
    /// Statistics of time spent in source state before this transition.
    pub duration_stats: MeasurementStats,
    /// Whether this transition was observed at a clock edge.
    pub at_clock_edge: bool,
}

/// FSM extraction result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FsmExtractionResult {
    /// State register signal path.
    pub signal_path: String,
    /// Signal width in bits.
    pub width: u32,
    /// Clock signal used for edge alignment.
    pub clock_signal: Option<String>,
    /// Discovered states.
    pub states: Vec<FsmState>,
    /// Discovered transitions (sorted by count descending).
    pub transitions: Vec<FsmTransition>,
    /// Total distinct states.
    pub state_count: usize,
    /// Total distinct transitions.
    pub transition_count: usize,
    /// State transition graph in DOT format.
    pub dot_graph: String,
    /// Self-loop transitions.
    pub self_loops: Vec<FsmTransition>,
}

// === Core Functions ===

/// Extract FSM state encoding and transition graph from a state register signal.
///
/// Without clock alignment: tracks raw value changes.
/// With clock alignment (recommended): reads state at each clock edge.
pub fn extract_fsm(
    waveform: &mut Waveform,
    signal_path: &str,
    clock_path: Option<&str>,
    edge_type: &str,
    start_idx: usize,
    end_idx: usize,
    state_name_map: Option<&HashMap<String, String>>,
) -> WaveResult<FsmExtractionResult> {
    let hierarchy = waveform.hierarchy();
    let time_table_len = waveform.time_table().len();
    let effective_end = if end_idx == 0 || end_idx >= time_table_len {
        time_table_len.saturating_sub(1)
    } else {
        end_idx
    };

    // Load state signal
    let state_var_ref = find_var_by_path(hierarchy, signal_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.to_string(),
        }
    })?;
    let width = hierarchy[state_var_ref].length().unwrap_or(1);
    let state_signal_ref = find_signal_by_path(hierarchy, signal_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.to_string(),
        }
    })?;

    let mut refs_to_load = vec![state_signal_ref];

    // Load clock signal if provided
    let clock_signal_ref = if let Some(clk_path) = clock_path {
        let clk_ref = find_signal_by_path(hierarchy, clk_path).ok_or_else(|| {
            WaveAnalyzerError::SignalNotFound {
                path: clk_path.to_string(),
            }
        })?;
        refs_to_load.push(clk_ref);
        Some(clk_ref)
    } else {
        None
    };

    waveform.load_signals(&refs_to_load);

    // Collect state observations
    let observations: Vec<(u64, BigUint)> = if let Some(clk_ref) = clock_signal_ref {
        // Clock-aligned: read state at each clock edge
        collect_clock_aligned_states(
            waveform,
            state_signal_ref,
            clk_ref,
            edge_type,
            start_idx,
            effective_end,
        )?
    } else {
        // Raw: track all state changes
        collect_raw_state_changes(waveform, state_signal_ref, start_idx, effective_end)
    };

    if observations.is_empty() {
        return Err(WaveAnalyzerError::Other(format!(
            "No state observations found for signal {} in range [{}, {}]",
            signal_path, start_idx, effective_end
        )));
    }

    // Build state map: value -> FsmState
    let mut state_counts: HashMap<Vec<u8>, usize> = HashMap::new();
    for (_, value) in &observations {
        let key = value.to_bytes_le();
        *state_counts.entry(key).or_insert(0) += 1;
    }

    let total_observations = observations.len();
    let states = build_fsm_states(&state_counts, width, total_observations, state_name_map);

    // Build value -> name lookup
    let value_to_name: HashMap<Vec<u8>, String> = states
        .iter()
        .map(|s| (BigUint::from(s.numeric_value).to_bytes_le(), s.name.clone()))
        .collect();

    // Build transitions
    let (transitions, self_loops) =
        build_fsm_transitions(&observations, &value_to_name, width, clock_path.is_some());

    let state_count = states.len();
    let transition_count = transitions.len() + self_loops.len();

    // Generate DOT graph
    let dot_graph = generate_dot_graph(&states, &transitions, signal_path);

    Ok(FsmExtractionResult {
        signal_path: signal_path.to_string(),
        width,
        clock_signal: clock_path.map(|s| s.to_string()),
        states,
        transitions,
        state_count,
        transition_count,
        dot_graph,
        self_loops,
    })
}

// === Internal Functions ===

fn collect_clock_aligned_states(
    waveform: &Waveform,
    state_signal_ref: wellen::SignalRef,
    clock_signal_ref: wellen::SignalRef,
    edge_type: &str,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<Vec<(u64, BigUint)>> {
    if edge_type != "posedge" && edge_type != "negedge" {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Invalid edge_type: '{}'. Must be 'posedge' or 'negedge'",
                edge_type
            ),
        });
    }

    let clock_signal = waveform
        .get_signal(clock_signal_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Failed to get clock signal".to_string(),
        ))?;
    let state_signal = waveform
        .get_signal(state_signal_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Failed to get state signal".to_string(),
        ))?;

    // Detect clock edges
    let mut prev_high: Option<bool> = None;
    let mut clock_edges: Vec<u64> = Vec::new();

    for (time_idx, value) in clock_signal.iter_changes() {
        let idx = time_idx as usize;
        if idx < start_idx || idx > end_idx {
            continue;
        }

        let is_high = signal_value_is_bool(&value);
        let is_target_edge = match edge_type {
            "posedge" => prev_high == Some(false) && is_high,
            "negedge" => prev_high == Some(true) && !is_high,
            _ => false,
        };

        if is_target_edge {
            clock_edges.push(time_idx as u64);
        }
        prev_high = Some(is_high);
    }

    // Read state at each clock edge
    let mut observations: Vec<(u64, BigUint)> = Vec::new();
    for edge_time in &clock_edges {
        let time_table_idx: wellen::TimeTableIdx =
            (*edge_time as usize).try_into().ok().unwrap_or(0);
        if let Some(offset) = state_signal.get_offset(time_table_idx) {
            let value = state_signal.get_value_at(&offset, 0);
            let biguint = signal_value_to_biguint_lenient(value);
            observations.push((*edge_time, biguint));
        }
    }

    Ok(observations)
}

fn collect_raw_state_changes(
    waveform: &Waveform,
    state_signal_ref: wellen::SignalRef,
    start_idx: usize,
    end_idx: usize,
) -> Vec<(u64, BigUint)> {
    let state_signal = waveform
        .get_signal(state_signal_ref)
        .expect("Failed to get state signal");

    let mut observations: Vec<(u64, BigUint)> = Vec::new();
    for (time_idx, value) in state_signal.iter_changes() {
        let idx = time_idx as usize;
        if idx < start_idx || idx > end_idx {
            continue;
        }
        let biguint = signal_value_to_biguint_lenient(value);
        observations.push((time_idx as u64, biguint));
    }
    observations
}

fn signal_value_is_bool(value: &wellen::SignalValue) -> bool {
    is_signal_high(value)
}

fn build_fsm_states(
    state_counts: &HashMap<Vec<u8>, usize>,
    width: u32,
    total_observations: usize,
    state_name_map: Option<&HashMap<String, String>>,
) -> Vec<FsmState> {
    let mut sorted: Vec<(Vec<u8>, usize)> =
        state_counts.iter().map(|(k, &v)| (k.clone(), v)).collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.1));

    sorted
        .iter()
        .map(|(key, count)| {
            let biguint = BigUint::from_bytes_le(key);
            let numeric = biguint.to_u64().unwrap_or(0u64);
            let value_str = format_biguint_verilog(&biguint, width);
            let fraction = if total_observations > 0 {
                *count as f64 / total_observations as f64
            } else {
                0.0
            };

            let name = state_name_map
                .and_then(|map| map.get(&value_str))
                .cloned()
                .unwrap_or_else(|| format!("STATE_{}", numeric));

            FsmState {
                value: value_str,
                numeric_value: numeric,
                name,
                occurrence_count: *count,
                fraction,
            }
        })
        .collect()
}

fn build_fsm_transitions(
    observations: &[(u64, BigUint)],
    value_to_name: &HashMap<Vec<u8>, String>,
    width: u32,
    at_clock_edge: bool,
) -> (Vec<FsmTransition>, Vec<FsmTransition>) {
    // Collect (from_key, to_key) pairs and durations
    let mut transition_counts: HashMap<(Vec<u8>, Vec<u8>), usize> = HashMap::new();
    let mut transition_durations: HashMap<(Vec<u8>, Vec<u8>), Vec<f64>> = HashMap::new();

    for i in 1..observations.len() {
        let from_key = observations[i - 1].1.to_bytes_le();
        let to_key = observations[i].1.to_bytes_le();
        let duration = (observations[i].0 - observations[i - 1].0) as f64;

        *transition_counts
            .entry((from_key.clone(), to_key.clone()))
            .or_insert(0) += 1;
        transition_durations
            .entry((from_key.clone(), to_key.clone()))
            .or_default()
            .push(duration);
    }

    let mut transitions: Vec<FsmTransition> = Vec::new();
    let mut self_loops: Vec<FsmTransition> = Vec::new();

    for ((from_key, to_key), count) in transition_counts {
        let from_biguint = BigUint::from_bytes_le(&from_key);
        let to_biguint = BigUint::from_bytes_le(&to_key);
        let from_str = format_biguint_verilog(&from_biguint, width);
        let to_str = format_biguint_verilog(&to_biguint, width);
        let from_name = value_to_name
            .get(&from_key)
            .cloned()
            .unwrap_or_else(|| format!("STATE_{}", from_biguint.to_u64().unwrap_or(0u64)));
        let to_name = value_to_name
            .get(&to_key)
            .cloned()
            .unwrap_or_else(|| format!("STATE_{}", to_biguint.to_u64().unwrap_or(0u64)));

        let durations = transition_durations
            .get(&(from_key.clone(), to_key.clone()))
            .map(|d| compute_stats(d))
            .unwrap_or(MeasurementStats {
                count: 0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
                stddev: 0.0,
            });

        let trans = FsmTransition {
            from_state: from_name,
            from_value: from_str,
            to_state: to_name,
            to_value: to_str,
            count,
            duration_stats: durations,
            at_clock_edge,
        };

        if from_key == to_key {
            self_loops.push(trans);
        } else {
            transitions.push(trans);
        }
    }

    // Sort by count descending
    transitions.sort_by_key(|t| std::cmp::Reverse(t.count));
    self_loops.sort_by_key(|s| std::cmp::Reverse(s.count));

    (transitions, self_loops)
}

fn generate_dot_graph(
    states: &[FsmState],
    transitions: &[FsmTransition],
    signal_path: &str,
) -> String {
    let mut dot = ReportWriter::new();
    report_writeln!(dot, "digraph FSM_{} {{", sanitize_dot_name(signal_path));
    report_writeln!(dot, "  rankdir=LR;");
    report_writeln!(dot, "  node [shape=circle];");

    for state in states {
        let label = format!("{}\\n{}", state.name, state.value);
        report_writeln!(
            dot,
            "  {} [label=\"{}\"];",
            sanitize_dot_name(&state.name),
            label
        );
    }

    for trans in transitions {
        let label = format!("{}\\navg={:.1}", trans.count, trans.duration_stats.avg);
        report_writeln!(
            dot,
            "  {} -> {} [label=\"{}\"];",
            sanitize_dot_name(&trans.from_state),
            sanitize_dot_name(&trans.to_state),
            label
        );
    }

    report_writeln!(dot, "}}");
    dot.finish()
}

fn sanitize_dot_name(name: &str) -> String {
    name.replace(['.', '-', ' '], "_")
}

// === Report Formatting ===

/// Format an FSM extraction result as human-readable text.
pub fn format_fsm_report(result: &FsmExtractionResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(
        out,
        "=== FSM Extraction: {} (width={}) ===",
        result.signal_path,
        result.width
    );
    if let Some(ref clk) = result.clock_signal {
        report_writeln!(out, "Clock-aligned: {}", clk);
    } else {
        report_writeln!(out, "Mode: raw state changes (no clock alignment)");
    }
    report_writeln!(
        out,
        "States: {}, Transitions: {}",
        result.state_count,
        result.transition_count
    );

    report_writeln!(out, "\n--- States ---");
    for state in &result.states {
        report_writeln!(
            out,
            "  {} ({}) : occurrences={}, fraction={:.4}",
            state.name,
            state.value,
            state.occurrence_count,
            state.fraction
        );
    }

    report_writeln!(out, "\n--- Transitions ---");
    for trans in &result.transitions {
        report_writeln!(
            out,
            "  {} -> {} : count={}, avg_duration={:.2}",
            trans.from_state,
            trans.to_state,
            trans.count,
            trans.duration_stats.avg
        );
    }

    if !result.self_loops.is_empty() {
        report_writeln!(out, "\n--- Self-loops ---");
        for trans in &result.self_loops {
            report_writeln!(
                out,
                "  {} -> {} : count={}, avg_duration={:.2}",
                trans.from_state,
                trans.to_state,
                trans.count,
                trans.duration_stats.avg
            );
        }
    }

    report_writeln!(out, "\n--- DOT Graph ---");
    report_writeln!(out, "{}", result.dot_graph);

    out.finish()
}
