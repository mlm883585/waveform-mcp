//! Time mapping utilities for waveform analysis.
//!
//! This module provides time value to time index mapping,
//! clock edge table construction, and cycle-based time backtracking.

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::signal_value_to_biguint_strict;
use crate::hierarchy::find_var_by_path;
use num_bigint::BigUint;
use wellen;

/// Clock edge type for edge table construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockEdgeType {
    Posedge,
    Negedge,
}

/// A single clock edge entry in the edge table.
#[derive(Debug, Clone)]
pub struct ClockEdgeEntry {
    /// Time index in the waveform time_table.
    pub time_index: usize,
    /// Raw time value from the waveform.
    pub time_value: u64,
}

/// A clock edge table used for BFS time backtracking.
#[derive(Debug, Clone)]
pub struct ClockEdgeTable {
    /// Logical clock name from deps.yaml.
    pub clock_name: String,
    /// Resolved waveform path.
    pub resolved_path: String,
    /// Edge type (posedge/negedge).
    pub edge_type: ClockEdgeType,
    /// Ordered list of clock edges.
    pub edges: Vec<ClockEdgeEntry>,
}

/// Convert a time value with unit to picoseconds.
///
/// Returns an error for unrecognized time units instead of silently defaulting.
pub fn time_value_to_ps(time_value: f64, time_unit: &str) -> WaveResult<u64> {
    let factor = match time_unit {
        "ps" => 1.0,
        "ns" => 1000.0,
        "us" => 1_000_000.0,
        "ms" => 1_000_000_000.0,
        "s" => 1_000_000_000_000.0,
        _ => {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!(
                    "Unknown time unit '{}'. Expected one of: ps, ns, us, ms, s",
                    time_unit
                ),
            });
        }
    };
    let ps_f64 = time_value * factor;
    if ps_f64 < 0.0 {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "time_value must be non-negative".to_string(),
        });
    }
    Ok(ps_f64.round() as u64)
}

/// Find the time_index in a waveform that corresponds to a given time value in picoseconds.
///
/// Uses binary search on the time_table to find the nearest time_index
/// whose time value (converted to ps) is not later than the target.
///
/// # Arguments
/// * `waveform` - The loaded waveform
/// * `time_ps` - Target time in picoseconds
///
/// # Returns
/// * Exact match: the corresponding time_index
/// * No exact match: the nearest time_index not later than time_ps
/// * time_ps exceeds waveform range: the last time_index
pub fn find_time_index_by_value(
    waveform: &wellen::simple::Waveform,
    time_ps: u64,
) -> WaveResult<usize> {
    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();

    let len = time_table.len();
    if len == 0 {
        return Err(WaveAnalyzerError::Other(
            "Waveform has no time data".to_string(),
        ));
    }

    let max_time_ps = compute_time_ps_from_table(time_table, len - 1, timescale.as_ref());

    // Clamp to last index if time_ps exceeds waveform range.
    // This is more user-friendly than erroring — the user often doesn't
    // know the exact waveform duration and just wants data up to "the end".
    if time_ps >= max_time_ps {
        return Ok(len - 1);
    }

    if compute_time_ps_from_table(time_table, 0, timescale.as_ref()) > time_ps {
        return Ok(0);
    }

    // Binary search for the nearest time_index whose time_ps <= target time_ps
    let mut lo = 0;
    let mut hi = len - 1;

    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let mid_time_ps = compute_time_ps_from_table(time_table, mid, timescale.as_ref());
        if mid_time_ps <= time_ps {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    Ok(lo)
}

/// Compute time in picoseconds from a time_table entry using u128 intermediate
/// to avoid u64 overflow for large time values.
///
/// This is the unified time-to-ps conversion used by both find_time_index_by_value
/// and bfs.rs (via re-export). Keeps integer precision throughout.
///
/// # Panics / Silently returns 0 for out-of-range indices
/// Callers should validate `time_index < time_table.len()` before calling,
/// or use [`compute_time_ps_from_table_checked`] which returns `Result`.
pub fn compute_time_ps_from_table(
    time_table: &[u64],
    time_index: usize,
    timescale: Option<&wellen::Timescale>,
) -> u64 {
    if time_index >= time_table.len() {
        return 0;
    }
    compute_time_ps_from_table_inner(time_table, time_index, timescale)
}

/// Checked variant of [`compute_time_ps_from_table`] that returns an error
/// instead of silently returning 0 for out-of-range indices.
pub fn compute_time_ps_from_table_checked(
    time_table: &[u64],
    time_index: usize,
    timescale: Option<&wellen::Timescale>,
) -> WaveResult<u64> {
    if time_index >= time_table.len() {
        return Err(WaveAnalyzerError::Other(format!(
            "TIME_INDEX_OUT_OF_RANGE: time_index {} exceeds waveform range (max: {})",
            time_index,
            time_table.len().saturating_sub(1)
        )));
    }
    Ok(compute_time_ps_from_table_inner(
        time_table, time_index, timescale,
    ))
}

/// Inner implementation shared by both checked and unchecked variants.
fn compute_time_ps_from_table_inner(
    time_table: &[u64],
    time_index: usize,
    timescale: Option<&wellen::Timescale>,
) -> u64 {
    let raw_time = time_table[time_index];
    match timescale {
        Some(ts) => {
            let numerator: u128 = match ts.unit {
                wellen::TimescaleUnit::ZeptoSeconds => 1,
                wellen::TimescaleUnit::AttoSeconds => 1,
                wellen::TimescaleUnit::FemtoSeconds => 1,
                wellen::TimescaleUnit::PicoSeconds => 1,
                wellen::TimescaleUnit::NanoSeconds => 1_000,
                wellen::TimescaleUnit::MicroSeconds => 1_000_000,
                wellen::TimescaleUnit::MilliSeconds => 1_000_000_000,
                wellen::TimescaleUnit::Seconds => 1_000_000_000_000,
                _ => 1_000, // assume ns
            };
            let denominator: u128 = match ts.unit {
                wellen::TimescaleUnit::ZeptoSeconds => 1_000_000_000_000_000,
                wellen::TimescaleUnit::AttoSeconds => 1_000_000_000_000,
                wellen::TimescaleUnit::FemtoSeconds => 1_000,
                _ => 1,
            };
            // Use u128 intermediate to avoid u64 overflow for large time values.
            let ps: u128 = raw_time as u128 * ts.factor as u128 * numerator / denominator;
            ps as u64
        }
        None => raw_time * 1_000, // assume ns if no timescale
    }
}

/// Build a clock edge table from a waveform and clock signal path.
///
/// Extracts all posedge or negedge events from the clock signal
/// and builds an ordered edge table for BFS time backtracking.
///
/// # Arguments
/// * `waveform` - Mutable reference to the loaded waveform
/// * `clock_path` - Hierarchical path to the clock signal
/// * `edge_type` - Whether to collect posedge or negedge events
pub fn build_clock_edge_table(
    waveform: &mut wellen::simple::Waveform,
    clock_path: &str,
    edge_type: ClockEdgeType,
) -> WaveResult<ClockEdgeTable> {
    // Find the clock signal and extract metadata (hierarchy borrow released after this block)
    let (clock_signal_ref, width) = {
        let hierarchy = waveform.hierarchy();
        let var_ref = find_var_by_path(hierarchy, clock_path).ok_or_else(|| {
            WaveAnalyzerError::SignalNotFound {
                path: clock_path.to_string(),
            }
        })?;
        let var = &hierarchy[var_ref];
        Ok::<(wellen::SignalRef, Option<u32>), WaveAnalyzerError>((var.signal_ref(), var.length()))
    }?;
    if width != Some(1) {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "CLOCK_NOT_FOUND: Signal '{}' has width {} (expected 1-bit clock). Multi-bit signals cannot be used as clocks.",
                clock_path,
                width
                    .map(|w| w.to_string())
                    .unwrap_or("unknown".to_string())
            ),
        });
    }

    // Load the clock signal data
    waveform.load_signals(&[clock_signal_ref]);

    // Get all change events for the clock
    let time_table = waveform.time_table();
    let signal =
        waveform
            .get_signal(clock_signal_ref)
            .ok_or_else(|| WaveAnalyzerError::SignalNotFound {
                path: clock_path.to_string(),
            })?;

    // Build edge table by detecting transitions
    let mut edges = Vec::new();
    let mut prev_value: Option<BigUint> = None;

    for (time_idx, signal_value) in signal.iter_changes() {
        let current_value = signal_value_to_biguint_strict(signal_value)?;

        // Detect edge transitions
        let is_edge = match (&prev_value, edge_type) {
            (Some(prev), ClockEdgeType::Posedge) => {
                *prev == BigUint::from(0u32) && current_value == BigUint::from(1u32)
            }
            (Some(prev), ClockEdgeType::Negedge) => {
                *prev == BigUint::from(1u32) && current_value == BigUint::from(0u32)
            }
            (None, _) => false, // First event has no previous value
        };

        if is_edge {
            edges.push(ClockEdgeEntry {
                time_index: time_idx as usize,
                time_value: time_table[time_idx as usize],
            });
        }

        prev_value = Some(current_value);
    }

    Ok(ClockEdgeTable {
        clock_name: String::new(), // Will be filled by caller with logical name
        resolved_path: clock_path.to_string(),
        edge_type,
        edges,
    })
}

impl ClockEdgeTable {
    /// Step back from a given time_index by latency_cycles clock edges.
    ///
    /// # Arguments
    /// * `from_time_index` - Starting time index
    /// * `latency_cycles` - Number of clock edges to step back
    ///
    /// # Returns
    /// The time_index after stepping back, or the earliest edge if backtracking
    /// exceeds the waveform start.
    pub fn step_back(&self, from_time_index: usize, latency_cycles: u32) -> usize {
        if self.edges.is_empty() {
            return from_time_index;
        }

        // Find the nearest clock edge not later than from_time_index
        let Some(start_edge_idx) = self.find_edge_not_later_than(from_time_index) else {
            return from_time_index;
        };

        // Step back by latency_cycles
        let target_edge_idx = start_edge_idx.saturating_sub(latency_cycles as usize);

        if target_edge_idx < self.edges.len() {
            self.edges[target_edge_idx].time_index
        } else {
            self.edges[0].time_index // Clamped to earliest edge
        }
    }

    /// Find the index in the edges array for the nearest edge not later than from_time_index.
    fn find_edge_not_later_than(&self, from_time_index: usize) -> Option<usize> {
        // Binary search for the nearest edge with time_index <= from_time_index
        let mut lo = 0;
        let mut hi = self.edges.len();

        if hi == 0 {
            return None;
        }

        // Find rightmost edge with time_index <= from_time_index
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.edges[mid].time_index <= from_time_index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        // lo is the first edge > from_time_index, so lo-1 is the last edge <= from_time_index
        if lo == 0 { None } else { Some(lo - 1) }
    }
}
