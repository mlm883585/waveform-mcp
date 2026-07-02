//! Sequence pattern detection in waveforms.
//!
//! Finds occurrences of a specified condition sequence with optional timing constraints.

use num_traits::Zero;
use serde::{Deserialize, Serialize};
use wellen::simple::Waveform;

use crate::condition::{Condition, build_signal_cache_entry, evaluate_condition, parse_condition};
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::{ReportWriter, format_time};
use crate::report_writeln;

/// A detected sequence occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceOccurrence {
    /// Time index where the sequence started.
    pub start_time_index: u64,
    /// Formatted start time string.
    pub start_time: String,
    /// Time index where the sequence ended.
    pub end_time_index: u64,
    /// Formatted end time string.
    pub end_time: String,
    /// Timing gap between each step (in time indices).
    pub step_gaps: Vec<u64>,
    /// Whether all gaps are within max_gap_cycles.
    pub timing_ok: bool,
}

/// Result of sequence detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceResult {
    /// The original step conditions.
    pub conditions: Vec<String>,
    /// Max gap cycles constraint (if any).
    pub max_gap_cycles: Option<usize>,
    /// Total occurrences found.
    pub occurrence_count: usize,
    /// Individual occurrences.
    pub occurrences: Vec<SequenceOccurrence>,
}

/// Detect a sequence of conditions in a waveform.
///
/// Each step's condition is parsed and evaluated at every time index.
/// Occurrences are found using a greedy sliding window: from each step-0 match,
/// find the earliest subsequent step matches within max_gap_cycles constraint.
pub fn detect_sequence(
    waveform: &mut Waveform,
    conditions: &[String],
    max_gap_cycles: Option<usize>,
    start_idx: usize,
    end_idx: usize,
    limit: Option<isize>,
) -> WaveResult<SequenceResult> {
    if conditions.is_empty() {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "At least one condition must be provided".to_string(),
        });
    }

    let time_table_len = waveform.time_table().len();
    let start = start_idx.min(time_table_len.saturating_sub(1));
    let end = end_idx.min(time_table_len.saturating_sub(1));
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale = waveform.hierarchy().timescale();

    // Parse all conditions
    let condition_asts: Vec<Condition> = conditions
        .iter()
        .map(|c| parse_condition(c))
        .collect::<Result<Vec<_>, _>>()?;

    // Extract all unique signal names from all conditions
    let mut all_signal_names: Vec<String> = Vec::new();
    for ast in &condition_asts {
        extract_signal_names(ast, &mut all_signal_names);
    }

    // Build signal cache and load signals
    let hierarchy = waveform.hierarchy();
    let mut signal_cache = std::collections::HashMap::new();
    let mut all_signal_refs = Vec::new();

    for signal_name in &all_signal_names {
        let entry = build_signal_cache_entry(hierarchy, signal_name)?;
        all_signal_refs.extend_from_slice(&entry.signal_refs);
        signal_cache.insert(signal_name.clone(), entry);
    }

    waveform.load_signals(&all_signal_refs);

    // For each condition, find all time indices where it's satisfied
    let mut step_matches: Vec<Vec<usize>> = Vec::new();

    for ast in &condition_asts {
        let mut matches = Vec::new();
        for time_idx in start..=end {
            if time_idx >= time_table_len {
                break;
            }
            let result =
                evaluate_condition(ast, waveform, &signal_cache, time_idx).map_err(|e| {
                    WaveAnalyzerError::Other(format!("Error evaluating condition: {}", e))
                })?;
            if !result.is_zero() {
                matches.push(time_idx);
            }
        }
        step_matches.push(matches);
    }

    // Greedy sliding window: find occurrences (non-overlapping)
    let mut occurrences: Vec<SequenceOccurrence> = Vec::new();
    let mut consumed_until: usize = 0; // Prevent overlapping matches

    // For each step-0 match, try to find subsequent steps
    for &step0_time in &step_matches[0] {
        // Skip if this start point falls within a previous occurrence
        if step0_time < consumed_until {
            continue;
        }

        let mut current_time = step0_time;
        let mut step_times = vec![step0_time];
        let mut gaps = Vec::new();
        let mut timing_ok = true;

        for step_match in step_matches.iter().skip(1) {
            // Find earliest match for this step that is >= current_time
            // and within max_gap_cycles, and not consumed by a prior occurrence
            let found = step_match.iter().find(|&&t| {
                t >= current_time
                    && t >= consumed_until
                    && max_gap_cycles.is_none_or(|max| t - current_time <= max)
            });

            match found {
                Some(&t) => {
                    gaps.push((t - current_time) as u64);
                    current_time = t;
                    step_times.push(t);
                }
                None => {
                    timing_ok = false;
                    break;
                }
            }
        }

        if step_times.len() == conditions.len() {
            let start_time = format_time(time_table[step_times[0]], timescale.as_ref());
            let end_time = format_time(time_table[*step_times.last().unwrap()], timescale.as_ref());

            // Deduplicate: advance consumed_until past the contiguous block
            // where the last step's condition remains true. This prevents
            // trivial duplicate occurrences where the last condition stays
            // true across consecutive time indices (e.g., poff==1 at 105,
            // 106, 107 would produce 3 identical "v>0xFA0 -> poff==1"
            // occurrences shifted by 1 time index each).
            let endpoint = *step_times.last().unwrap();
            let last_step_matches = &step_matches[conditions.len() - 1];
            let max_gap = max_gap_cycles.unwrap_or(1);

            // Find the end of the contiguous block from the endpoint
            let ep_pos = last_step_matches
                .iter()
                .position(|&t| t >= endpoint)
                .unwrap_or(0);
            let mut contiguous_end = endpoint;
            for i in (ep_pos + 1)..last_step_matches.len() {
                if last_step_matches[i] - contiguous_end <= max_gap {
                    contiguous_end = last_step_matches[i];
                } else {
                    break;
                }
            }
            consumed_until = contiguous_end + 1;

            occurrences.push(SequenceOccurrence {
                start_time_index: step_times[0] as u64,
                start_time,
                end_time_index: *step_times.last().unwrap() as u64,
                end_time,
                step_gaps: gaps,
                timing_ok,
            });

            // Apply limit
            if let Some(lim) = limit
                && lim > 0
                && occurrences.len() >= lim as usize
            {
                break;
            }
        }
    }

    Ok(SequenceResult {
        conditions: conditions.to_vec(),
        max_gap_cycles,
        occurrence_count: occurrences.len(),
        occurrences,
    })
}

/// Extract signal names from a condition AST.
fn extract_signal_names(condition: &Condition, names: &mut Vec<String>) {
    match condition {
        Condition::And(left, right)
        | Condition::Or(left, right)
        | Condition::BitwiseAnd(left, right)
        | Condition::BitwiseOr(left, right)
        | Condition::BitwiseXor(left, right)
        | Condition::Eq(left, right)
        | Condition::Neq(left, right)
        | Condition::Lt(left, right)
        | Condition::Le(left, right)
        | Condition::Gt(left, right)
        | Condition::Ge(left, right)
        | Condition::Add(left, right)
        | Condition::Sub(left, right) => {
            extract_signal_names(left, names);
            extract_signal_names(right, names);
        }
        Condition::Not(expr)
        | Condition::BitwiseNot(expr)
        | Condition::Past(expr)
        | Condition::PastN(expr, _) => {
            extract_signal_names(expr, names);
        }
        Condition::Signal(path)
        | Condition::BitExtract(path, _, _)
        | Condition::Rose(path)
        | Condition::Fell(path)
        | Condition::Stable(path) => {
            if !names.contains(path) {
                names.push(path.clone());
            }
        }
        Condition::Literal(_) => {}
    }
}

/// Format a sequence result as human-readable text.
pub fn format_sequence_report(result: &SequenceResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "=== Sequence Detection Report ===");
    report_writeln!(out, "Conditions: {}", result.conditions.join(" -> "));
    if let Some(max_gap) = result.max_gap_cycles {
        report_writeln!(out, "Max gap between steps: {} time indices", max_gap);
    }
    report_writeln!(out, "Occurrences found: {}", result.occurrence_count);

    if !result.occurrences.is_empty() {
        report_writeln!(out, "{}", "-".repeat(70));
        for (i, occ) in result.occurrences.iter().enumerate() {
            let gaps_str = occ
                .step_gaps
                .iter()
                .map(|g| g.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            report_writeln!(
                out,
                "Occurrence #{}: {} (idx {}) -> {} (idx {}), gaps: [{}], timing: {}",
                i + 1,
                occ.start_time,
                occ.start_time_index,
                occ.end_time,
                occ.end_time_index,
                gaps_str,
                if occ.timing_ok { "OK" } else { "EXCEEDED" }
            );
        }
    } else {
        report_writeln!(out, "No occurrences detected.");
    }

    out.finish()
}
