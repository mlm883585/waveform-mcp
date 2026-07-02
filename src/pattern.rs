//! Data pattern analysis for waveform signals.
//!
//! Provides value distribution histograms, change frequency statistics,
//! and idle/active cycle analysis. Useful for debugging data anomalies,
//! detecting stuck-at faults, and quantifying bus activity.

use crate::error::{WaveAnalyzerError, WaveResult};
use num_bigint::BigUint;
use num_traits::Zero;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wellen::simple::Waveform;

use crate::formatting::{ReportWriter, signal_value_to_biguint_lenient};
use crate::hierarchy::{get_signal_width, resolve_signal_var_refs};
use crate::protocol::{MeasurementStats, compute_stats};
use crate::report_writeln;

// === Data Structures ===

/// A single bin in a value distribution histogram.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistogramBin {
    /// The signal value at this bin (formatted string, e.g. "8'h0A").
    pub value: String,
    /// Raw numeric value (BigUint truncated to u64 for wide signals; 0 if too large).
    pub numeric_value: u64,
    /// Number of time indices where this value was observed.
    pub count: usize,
    /// Fraction of total observed time (0.0 to 1.0).
    pub fraction: f64,
}

/// Value distribution histogram for a signal.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ValueDistribution {
    /// Signal path.
    pub signal_path: String,
    /// Signal width in bits.
    pub width: u32,
    /// Total time indices observed.
    pub total_time_indices: usize,
    /// Distinct values observed.
    pub distinct_values: usize,
    /// Histogram bins, sorted by count descending.
    pub bins: Vec<HistogramBin>,
    /// Most common value (mode).
    pub mode_value: String,
    /// Least common value (anti-mode).
    pub anti_mode_value: String,
}

/// Change frequency statistics for a signal.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChangeFrequency {
    /// Signal path.
    pub signal_path: String,
    /// Total number of value changes in the time range.
    pub change_count: usize,
    /// Total time indices in the range.
    pub total_time_indices: usize,
    /// Change rate (changes per time index).
    pub change_rate: f64,
    /// Statistics of time gaps between consecutive changes (in time indices).
    pub gap_stats: MeasurementStats,
    /// Longest stable period (time indices between two consecutive changes).
    pub longest_stable_period: u64,
    /// Shortest gap between two consecutive changes.
    pub shortest_gap: u64,
}

/// Idle/active cycle analysis for a signal.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IdleActiveStats {
    /// Signal path.
    pub signal_path: String,
    /// Threshold value used to define "idle" state.
    pub threshold: String,
    /// Number of active-to-idle transitions.
    pub active_to_idle_count: usize,
    /// Number of idle-to-active transitions.
    pub idle_to_active_count: usize,
    /// Statistics of active period durations (in time indices).
    pub active_duration_stats: MeasurementStats,
    /// Statistics of idle period durations (in time indices).
    pub idle_duration_stats: MeasurementStats,
    /// Total time indices in active state.
    pub total_active_time_indices: usize,
    /// Total time indices in idle state.
    pub total_idle_time_indices: usize,
    /// Active fraction (0.0 to 1.0).
    pub active_fraction: f64,
}

/// Combined result of pattern analysis for one or more signals.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PatternAnalysisResult {
    /// Value distributions for each analyzed signal.
    pub value_distributions: Vec<ValueDistribution>,
    /// Change frequency statistics for each analyzed signal.
    pub change_frequencies: Vec<ChangeFrequency>,
    /// Idle/active statistics for each analyzed signal.
    pub idle_active_stats: Vec<IdleActiveStats>,
    /// Change frequency ranking (signal paths sorted by change_rate descending).
    pub change_frequency_ranking: Vec<String>,
}

// === Core Functions ===

/// Analyze data patterns for one or more signals.
///
/// Computes value distribution histogram, change frequency statistics,
/// and idle/active cycle analysis.
pub fn analyze_signal_patterns(
    waveform: &mut Waveform,
    signal_paths: &[String],
    start_idx: usize,
    end_idx: usize,
    max_bins: Option<usize>,
    idle_threshold: Option<String>,
) -> WaveResult<PatternAnalysisResult> {
    let max_bins = max_bins.unwrap_or(50);
    let idle_threshold_biguint =
        parse_threshold(&idle_threshold.unwrap_or_else(|| "0".to_string()))?;

    let hierarchy = waveform.hierarchy();
    let time_table_len = waveform.time_table().len();

    let effective_end = if end_idx == 0 || end_idx >= time_table_len {
        time_table_len.saturating_sub(1)
    } else {
        end_idx
    };

    // Collect and load all signals
    // For each signal path, resolve to either a single bus VarRef or multiple
    // bit-slice VarRefs, then load the corresponding SignalRefs.
    struct SignalEntry {
        path: String,
        width: u32,
        // Packed bus: single (SignalRef, None). Bit-slice: multiple (SignalRef, bit_position).
        components: Vec<(wellen::SignalRef, Option<u32>)>,
    }

    let mut entries: Vec<SignalEntry> = Vec::new();
    for path in signal_paths {
        let var_refs = resolve_signal_var_refs(hierarchy, path)
            .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: path.clone() })?;
        let width = get_signal_width(hierarchy, path);

        // Build component list: each VarRef maps to a (SignalRef, bit_position)
        let mut components: Vec<(wellen::SignalRef, Option<u32>)> = Vec::new();
        if var_refs.len() == 1 {
            // Single bus variable — no bit position needed
            let vr = var_refs[0];
            let sig_ref = hierarchy[vr].signal_ref();
            components.push((sig_ref, None));
        } else {
            // Multiple bit-slice variables — each has a bit position
            // Sort by MSB descending (highest bit first) to match VCD convention
            let mut sorted_refs = var_refs;
            sorted_refs.sort_by_key(|vr| hierarchy[*vr].index().map(|idx| idx.msb()).unwrap_or(0));
            sorted_refs.reverse(); // MSB first

            for vr in &sorted_refs {
                let var = &hierarchy[*vr];
                let sig_ref = var.signal_ref();
                // bit_position from VarIndex MSB (for bit-slice, msb == lsb)
                let bit_pos = var.index().map(|idx| idx.msb() as u32);
                components.push((sig_ref, bit_pos));
            }
        }

        entries.push(SignalEntry {
            path: path.clone(),
            width,
            components,
        });
    }

    // Load all unique SignalRefs
    let all_refs: Vec<wellen::SignalRef> = entries
        .iter()
        .flat_map(|e| e.components.iter().map(|(sr, _)| *sr))
        .collect();
    waveform.load_signals(&all_refs);

    let mut value_distributions = Vec::new();
    let mut change_frequencies = Vec::new();
    let mut idle_active_stats = Vec::new();

    for entry in &entries {
        // Collect all changes within range as (time_index, BigUint) pairs.
        // For a single bus signal, iterate changes on the bus SignalRef.
        // For bit-slice signals, iterate time_table and reconstruct the
        // composite value at each time point by reading all bit slices.
        let mut changes: Vec<(u64, BigUint)> = Vec::new();

        if entry.components.len() == 1 {
            // Single bus: iterate changes directly
            let (sig_ref, _) = entry.components[0];
            let signal = waveform.get_signal(sig_ref).ok_or_else(|| {
                WaveAnalyzerError::Other(format!("Failed to get signal: {}", entry.path))
            })?;
            for (time_idx, value) in signal.iter_changes() {
                let idx = time_idx as usize;
                if idx >= start_idx && idx <= effective_end {
                    let biguint_val = signal_value_to_biguint(&value);
                    changes.push((time_idx as u64, biguint_val));
                }
            }
        } else {
            // Bit-slice decomposed signal: scan the time table and reconstruct
            // composite values at each time point where ANY bit changes.
            // First, collect all change times across all bit slices.
            let mut prev_composite: Option<BigUint> = None;

            // Collect change time indices from all bit-slice signals
            let mut change_indices: Vec<usize> = Vec::new();
            for (sig_ref, _) in &entry.components {
                let signal = waveform.get_signal(*sig_ref).ok_or_else(|| {
                    WaveAnalyzerError::Other(format!(
                        "Failed to get bit-slice signal for {}",
                        entry.path
                    ))
                })?;
                for (time_idx, _) in signal.iter_changes() {
                    let idx = time_idx as usize;
                    if idx >= start_idx && idx <= effective_end {
                        change_indices.push(idx);
                    }
                }
            }
            change_indices.sort();
            change_indices.dedup();

            // At each change time, read all bit slices and compose the value
            for idx in change_indices {
                let time_table_idx: wellen::TimeTableIdx = idx.try_into().map_err(|_| {
                    WaveAnalyzerError::Other(format!("Time index {} too large", idx))
                })?;

                let mut composite = BigUint::zero();
                for (sig_ref, bit_pos) in &entry.components {
                    let signal = waveform.get_signal(*sig_ref).ok_or_else(|| {
                        WaveAnalyzerError::Other(format!(
                            "Failed to get bit-slice signal for {}",
                            entry.path
                        ))
                    })?;
                    if let Some(offset) = signal.get_offset(time_table_idx) {
                        let value = signal.get_value_at(&offset, 0);
                        let bit_val = signal_value_to_biguint(&value);
                        // bit_pos is the bit index in the composite value (from VarIndex.msb)
                        if let Some(bp) = bit_pos {
                            if !bit_val.is_zero() {
                                composite.set_bit(*bp as u64, true);
                            }
                        }
                    }
                }

                // Only record this change if the composite value actually changed
                if prev_composite.as_ref() != Some(&composite) {
                    changes.push((idx as u64, composite.clone()));
                    prev_composite = Some(composite);
                }
            }
        }

        // Compute value distribution
        let vd = compute_value_distribution(
            &changes,
            &entry.path,
            entry.width,
            effective_end - start_idx + 1,
            max_bins,
        );
        value_distributions.push(vd);

        // Compute change frequency
        let cf = compute_change_frequency(&changes, &entry.path, effective_end - start_idx + 1);
        change_frequencies.push(cf);

        // Compute idle/active stats
        let ia = compute_idle_active_stats(
            &changes,
            &entry.path,
            entry.width,
            effective_end - start_idx + 1,
            &idle_threshold_biguint,
        );
        idle_active_stats.push(ia);
    }

    // Build change frequency ranking
    let mut cf_sorted: Vec<&ChangeFrequency> = change_frequencies.iter().collect();
    cf_sorted.sort_by(|a, b| {
        b.change_rate
            .partial_cmp(&a.change_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let ranking: Vec<String> = cf_sorted
        .iter()
        .map(|cf| format!("{} (rate={:.4})", cf.signal_path, cf.change_rate))
        .collect();

    Ok(PatternAnalysisResult {
        value_distributions,
        change_frequencies,
        idle_active_stats,
        change_frequency_ranking: ranking,
    })
}

// === Internal Functions ===

fn parse_threshold(threshold_str: &str) -> WaveResult<BigUint> {
    // Use canonical Verilog literal parser from formatting module.
    // parse_verilog_literal returns BigUint (never fails), but we validate
    // the input format first to preserve the original error messages.
    let s = threshold_str.trim();
    if s.contains('\'') {
        // Validate Verilog literal format
        let parts: Vec<&str> = s.splitn(2, '\'').collect();
        if parts.len() != 2 || parts[1].is_empty() {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!("Invalid Verilog literal: {}", threshold_str),
            });
        }
        let base_char = parts[1].chars().next().unwrap_or('d');
        if !matches!(base_char, 'b' | 'B' | 'h' | 'H' | 'd' | 'D') {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!("Unsupported Verilog literal base: '{}'", base_char),
            });
        }
    } else if s.parse::<u64>().is_err() && !s.starts_with("0x") {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Invalid threshold '{}': not a number or Verilog literal",
                threshold_str
            ),
        });
    }
    Ok(crate::formatting::parse_verilog_literal(threshold_str))
}

fn signal_value_to_biguint(value: &wellen::SignalValue) -> BigUint {
    signal_value_to_biguint_lenient(*value)
}

fn biguint_to_u64_truncated(val: &BigUint) -> u64 {
    val.try_into().unwrap_or(0u64)
}

fn compute_value_distribution(
    changes: &[(u64, BigUint)],
    signal_path: &str,
    width: u32,
    total_time_indices: usize,
    max_bins: usize,
) -> ValueDistribution {
    // Build value -> count map using duration-weighted counting
    let mut value_counts: HashMap<Vec<u8>, usize> = HashMap::new();

    // Track value occupancy: for each change, the value holds from its time until the next change
    for i in 0..changes.len() {
        let (_, value) = &changes[i];
        let next_time = if i + 1 < changes.len() {
            changes[i + 1].0
        } else {
            // Last change holds until end of range; approximate with 1 unit
            total_time_indices as u64
        };
        let duration = if i == 0 && changes.len() > 1 {
            // First change: holds from start_idx to next change
            next_time.saturating_sub(changes[0].0)
        } else {
            // Subsequent changes: hold duration from this change to next
            let prev_time = if i > 0 { changes[i - 1].0 } else { 0 };
            changes[i].0.saturating_sub(prev_time)
        };
        let key = value.to_bytes_le();
        *value_counts.entry(key).or_insert(0) += duration as usize;
    }

    // If no changes at all, the signal is constant — use total time
    if changes.is_empty() && total_time_indices > 0 {
        return ValueDistribution {
            signal_path: signal_path.to_string(),
            width,
            total_time_indices,
            distinct_values: 1,
            bins: vec![HistogramBin {
                value: "unknown".to_string(),
                numeric_value: 0,
                count: total_time_indices,
                fraction: 1.0,
            }],
            mode_value: "unknown".to_string(),
            anti_mode_value: "unknown".to_string(),
        };
    }

    let total_observed: usize = value_counts.values().sum();
    let distinct = value_counts.len();

    // Sort by count descending, limit to max_bins
    let mut sorted: Vec<(Vec<u8>, usize)> = value_counts.into_iter().collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.1));

    if sorted.len() > max_bins {
        sorted.truncate(max_bins);
    }

    let bins: Vec<HistogramBin> = sorted
        .iter()
        .map(|(key, count)| {
            let biguint = BigUint::from_bytes_le(key);
            let formatted = format_biguint(&biguint, width);
            let numeric = biguint_to_u64_truncated(&biguint);
            let fraction = if total_observed > 0 {
                *count as f64 / total_observed as f64
            } else {
                0.0
            };
            HistogramBin {
                value: formatted,
                numeric_value: numeric,
                count: *count,
                fraction,
            }
        })
        .collect();

    let mode_value = bins
        .first()
        .map(|b| b.value.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let anti_mode_value = bins
        .last()
        .map(|b| b.value.clone())
        .unwrap_or_else(|| "unknown".to_string());

    ValueDistribution {
        signal_path: signal_path.to_string(),
        width,
        total_time_indices,
        distinct_values: distinct,
        bins,
        mode_value,
        anti_mode_value,
    }
}

fn compute_change_frequency(
    changes: &[(u64, BigUint)],
    signal_path: &str,
    total_time_indices: usize,
) -> ChangeFrequency {
    let change_count = changes.len().saturating_sub(1); // First entry is initial value, not a change

    if changes.len() < 2 {
        return ChangeFrequency {
            signal_path: signal_path.to_string(),
            change_count: 0,
            total_time_indices,
            change_rate: 0.0,
            gap_stats: MeasurementStats {
                count: 0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
                stddev: 0.0,
            },
            longest_stable_period: total_time_indices as u64,
            shortest_gap: 0,
        };
    }

    // Compute gap pairs between consecutive changes
    let gap_pairs: Vec<f64> = (1..changes.len())
        .map(|i| (changes[i].0 - changes[i - 1].0) as f64)
        .collect();

    let gap_stats = compute_stats(&gap_pairs);
    let longest_stable = gap_pairs.iter().copied().fold(0.0f64, f64::max) as u64;
    let shortest = gap_pairs.iter().copied().fold(f64::INFINITY, f64::min) as u64;

    let change_rate = if total_time_indices > 0 {
        change_count as f64 / total_time_indices as f64
    } else {
        0.0
    };

    ChangeFrequency {
        signal_path: signal_path.to_string(),
        change_count,
        total_time_indices,
        change_rate,
        gap_stats,
        longest_stable_period: longest_stable,
        shortest_gap: shortest,
    }
}

fn compute_idle_active_stats(
    changes: &[(u64, BigUint)],
    signal_path: &str,
    width: u32,
    total_time_indices: usize,
    idle_threshold: &BigUint,
) -> IdleActiveStats {
    let threshold_str = format_biguint(idle_threshold, width);

    if changes.is_empty() {
        // Assume constant idle state
        return IdleActiveStats {
            signal_path: signal_path.to_string(),
            threshold: threshold_str,
            active_to_idle_count: 0,
            idle_to_active_count: 0,
            active_duration_stats: MeasurementStats {
                count: 0,
                min: 0.0,
                max: 0.0,
                avg: 0.0,
                stddev: 0.0,
            },
            idle_duration_stats: MeasurementStats {
                count: 1,
                min: total_time_indices as f64,
                max: total_time_indices as f64,
                avg: total_time_indices as f64,
                stddev: 0.0,
            },
            total_active_time_indices: 0,
            total_idle_time_indices: total_time_indices,
            active_fraction: 0.0,
        };
    }

    // Determine if each value is idle (== threshold) or active (!= threshold)
    let mut active_to_idle = 0usize;
    let mut idle_to_active = 0usize;
    let mut active_durations: Vec<f64> = Vec::new();
    let mut idle_durations: Vec<f64> = Vec::new();
    let mut total_active = 0usize;
    let mut total_idle = 0usize;

    // Track current state and duration
    let mut prev_is_idle = changes[0].1 == *idle_threshold;
    let mut current_state_start = changes[0].0;

    for (i, (time, value)) in changes.iter().enumerate() {
        let is_idle = *value == *idle_threshold;
        let _duration = if i + 1 < changes.len() {
            (changes[i + 1].0 - *time) as usize
        } else {
            // Last value holds to end of range
            total_time_indices.saturating_sub(*time as usize)
        };

        if i > 0 && is_idle != prev_is_idle {
            // State transition
            if is_idle {
                active_to_idle += 1;
            } else {
                idle_to_active += 1;
            }

            // Record duration of previous state
            let prev_duration = (*time - current_state_start) as f64;
            if prev_is_idle {
                idle_durations.push(prev_duration);
                total_idle += prev_duration as usize;
            } else {
                active_durations.push(prev_duration);
                total_active += prev_duration as usize;
            }

            current_state_start = *time;
        }

        prev_is_idle = is_idle;
    }

    // Handle last segment duration
    let last_duration = if !changes.is_empty() {
        (total_time_indices as u64).saturating_sub(current_state_start) as f64
    } else {
        total_time_indices as f64
    };
    if prev_is_idle {
        idle_durations.push(last_duration);
        total_idle += last_duration as usize;
    } else {
        active_durations.push(last_duration);
        total_active += last_duration as usize;
    }

    let total = total_active + total_idle;
    let active_fraction = if total > 0 {
        total_active as f64 / total as f64
    } else {
        0.0
    };

    IdleActiveStats {
        signal_path: signal_path.to_string(),
        threshold: threshold_str,
        active_to_idle_count: active_to_idle,
        idle_to_active_count: idle_to_active,
        active_duration_stats: compute_stats(&active_durations),
        idle_duration_stats: compute_stats(&idle_durations),
        total_active_time_indices: total_active,
        total_idle_time_indices: total_idle,
        active_fraction,
    }
}

/// Format a BigUint as a Verilog-style literal.
fn format_biguint(val: &BigUint, width: u32) -> String {
    crate::formatting::format_biguint_verilog(val, width)
}

// === Report Formatting ===

/// Format a pattern analysis result as human-readable text.
pub fn format_pattern_report(result: &PatternAnalysisResult) -> String {
    let mut out = ReportWriter::new();

    // Value distributions
    report_writeln!(out, "=== Value Distribution ===");
    for vd in &result.value_distributions {
        report_writeln!(out, "\nSignal: {} (width={})", vd.signal_path, vd.width);
        report_writeln!(out, "  Total time indices: {}", vd.total_time_indices);
        report_writeln!(out, "  Distinct values: {}", vd.distinct_values);
        report_writeln!(out, "  Mode (most common): {}", vd.mode_value);
        report_writeln!(out, "  Anti-mode (least common): {}", vd.anti_mode_value);
        report_writeln!(out, "  Histogram (top {} bins):", vd.bins.len());
        for bin in &vd.bins {
            report_writeln!(
                out,
                "    {} : count={}, fraction={:.4}",
                bin.value,
                bin.count,
                bin.fraction
            );
        }
    }

    // Change frequencies
    report_writeln!(out, "\n=== Change Frequency ===");
    for cf in &result.change_frequencies {
        report_writeln!(out, "\nSignal: {}", cf.signal_path);
        report_writeln!(out, "  Changes: {}", cf.change_count);
        report_writeln!(out, "  Change rate: {:.6} per time index", cf.change_rate);
        report_writeln!(
            out,
            "  Gap stats: count={}, min={}, max={}, avg={:.2}, stddev={:.2}",
            cf.gap_stats.count,
            cf.gap_stats.min,
            cf.gap_stats.max,
            cf.gap_stats.avg,
            cf.gap_stats.stddev
        );
        report_writeln!(out, "  Longest stable period: {}", cf.longest_stable_period);
        report_writeln!(out, "  Shortest gap: {}", cf.shortest_gap);
    }

    // Idle/active stats
    report_writeln!(out, "\n=== Idle/Active Analysis ===");
    for ia in &result.idle_active_stats {
        report_writeln!(
            out,
            "\nSignal: {} (idle threshold={})",
            ia.signal_path,
            ia.threshold
        );
        report_writeln!(
            out,
            "  Idle->Active transitions: {}",
            ia.idle_to_active_count
        );
        report_writeln!(
            out,
            "  Active->Idle transitions: {}",
            ia.active_to_idle_count
        );
        report_writeln!(
            out,
            "  Active fraction: {:.4} ({}/{})",
            ia.active_fraction,
            ia.total_active_time_indices,
            ia.total_active_time_indices + ia.total_idle_time_indices
        );
        report_writeln!(
            out,
            "  Active duration: avg={:.2}, min={}, max={}",
            ia.active_duration_stats.avg,
            ia.active_duration_stats.min,
            ia.active_duration_stats.max
        );
        report_writeln!(
            out,
            "  Idle duration: avg={:.2}, min={}, max={}",
            ia.idle_duration_stats.avg,
            ia.idle_duration_stats.min,
            ia.idle_duration_stats.max
        );
    }

    // Ranking
    if !result.change_frequency_ranking.is_empty() {
        report_writeln!(out, "\n=== Change Frequency Ranking ===");
        for (i, entry) in result.change_frequency_ranking.iter().enumerate() {
            report_writeln!(out, "  {}. {}", i + 1, entry);
        }
    }

    out.finish()
}
