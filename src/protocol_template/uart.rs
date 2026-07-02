//! UART protocol template analysis.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::hierarchy::find_signal_by_path;
use crate::protocol::{ClockMeasurement, MeasurementStats, compute_stats};
use crate::protocol_template::shared::{
    collect_changes, find_next_falling_after, find_next_rising_after, read_bit_at_time_from_changes,
};

/// UART protocol analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UartAnalysisResult {
    pub baud_rate_measurement: ClockMeasurement,
    pub frame_count: usize,
    pub start_bit_width_stats: MeasurementStats,
    pub stop_bit_width_stats: MeasurementStats,
    pub parity_errors: usize,
    pub framing_errors: usize,
}

/// Internal representation of a detected UART frame.
#[allow(dead_code)]
struct UartFrame {
    start_time_index: u64,
    data_bits: Vec<bool>,
    has_parity: bool,
    is_framing_error: bool,
}

/// Estimate UART bit width (in **picoseconds**) from the TX/RX signal transitions.
///
/// BUG-fix: the previous version computed `(t - prev_t) as f64` where `t` is a
/// time-table index, not a physical time. On waveforms with non-uniform
/// `#<delay>` resolution (e.g. mixed `#1` / `#0.5` time steps), this produces
/// a count of indices that is not a real "width in seconds". The fix uses the
/// time table to map indices to `wellen::Time` (which is internally stored as
/// a `f64` picosecond value) before taking the difference. The baud-rate
/// computation downstream now treats the result as picoseconds directly.
fn estimate_uart_bit_width(changes: &[(u32, bool)], time_table: &[wellen::Time]) -> f64 {
    let mut widths: Vec<f64> = Vec::new();
    let mut prev: Option<(u32, bool)> = None;

    for &(t, is_high) in changes {
        if let Some((prev_t, prev_high)) = prev
            && !prev_high
            && is_high
        {
            // Both `prev_t` and `t` are wellen time-table indices; the time
            // table is monotonically non-decreasing, so the difference is the
            // gap in wellen's internal time units (picoseconds in practice).
            if let (Some(&prev_ps), Some(&cur_ps)) =
                (time_table.get(prev_t as usize), time_table.get(t as usize))
            {
                widths.push((cur_ps - prev_ps) as f64);
            }
        }
        prev = Some((t, is_high));
    }

    if widths.is_empty() {
        return 0.0;
    }

    widths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_idx = widths.len() / 2;
    widths[median_idx]
}

/// Map a physical time value (in wellen time units) back to the closest
/// time-table index at or before it. Used to translate bit-center positions
/// (computed from the picosecond bit-width) into indices for `tx_changes`
/// lookups.
fn physical_time_to_time_index(time_table: &[wellen::Time], ps: f64) -> Option<usize> {
    if time_table.is_empty() {
        return None;
    }
    // Binary search for the largest index where time_table[i] <= ps.
    let mut lo: usize = 0;
    let mut hi: usize = time_table.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (time_table[mid] as f64) <= ps {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 { None } else { Some(lo - 1) }
}

/// Check if a parity bit exists and is valid for the given frame.
///
/// `bit_width_ps` is the bit width in **picoseconds** (wellen time units).
/// All bit positions are converted from physical time to time indices via
/// the time table before lookup in the changes stream, so this works
/// correctly on variable-resolution waveforms.
fn check_parity_bit(
    changes: &[(u32, bool)],
    time_table: &[wellen::Time],
    start_edge: u32,
    bit_width_ps: f64,
    _data_bits: &[bool],
    end_idx: usize,
) -> bool {
    let start_edge_ps = time_table.get(start_edge as usize).copied().unwrap_or(0);
    let parity_pos_ps = start_edge_ps + ((9.0 + 0.5) * bit_width_ps) as u64;
    if let Some(parity_idx) = physical_time_to_time_index(time_table, parity_pos_ps as f64)
        && parity_idx > end_idx
    {
        return false;
    }
    let stop_no_parity_ps = start_edge_ps + ((10.0 + 0.5) * bit_width_ps) as u64;
    let stop_with_parity_ps = start_edge_ps + ((11.0 + 0.5) * bit_width_ps) as u64;
    let stop_no_parity_pos = physical_time_to_time_index(time_table, stop_no_parity_ps as f64);
    let stop_with_parity_pos = physical_time_to_time_index(time_table, stop_with_parity_ps as f64);

    let no_parity_stop =
        stop_no_parity_pos.and_then(|idx| read_bit_at_time_from_changes(changes, idx));
    let with_parity_stop =
        stop_with_parity_pos.and_then(|idx| read_bit_at_time_from_changes(changes, idx));

    if no_parity_stop == Some(true) {
        false
    } else if with_parity_stop == Some(true) && no_parity_stop != Some(true) {
        true
    } else {
        false
    }
}

/// Compute UART baud rate measurement from estimated bit width.
///
/// `bit_width_ps` is the bit width in **picoseconds** (the unit wellen uses
/// internally for the time table). The previous implementation expected
/// `bit_width` in (raw time-table indices × timescale factor × unit-seconds);
/// that combination is incorrect on variable-resolution waveforms. This
/// version takes the picosecond value directly and converts to Hz by
/// `1e12 / bit_width_ps`, independent of the recorded timescale.
fn compute_uart_baud_rate(
    signal_path: &str,
    bit_width_ps: f64,
    _ts_opt: &Option<wellen::Timescale>,
) -> ClockMeasurement {
    let period_ps = if bit_width_ps > 0.0 {
        bit_width_ps
    } else {
        0.0
    };
    let period = if period_ps > 0.0 {
        MeasurementStats {
            count: 1,
            min: period_ps,
            max: period_ps,
            avg: period_ps,
            stddev: 0.0,
        }
    } else {
        MeasurementStats {
            count: 0,
            min: 0.0,
            max: 0.0,
            avg: 0.0,
            stddev: 0.0,
        }
    };

    let frequency_hz = if period_ps > 0.0 {
        // wellen stores time as f64 picoseconds internally. Convert to Hz.
        let hz = 1.0e12 / period_ps;
        Some(hz)
    } else {
        None
    };

    ClockMeasurement {
        signal_path: signal_path.to_string(),
        edge_type: "start_bit".to_string(),
        period,
        frequency_hz,
        duty_cycle_pct: None,
        jitter: 0.0,
    }
}

pub(super) fn analyze_uart(
    waveform: &mut wellen::simple::Waveform,
    signals: &HashMap<String, String>,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<crate::protocol_template::ProtocolAnalysisResult> {
    let tx_path = signals
        .get("tx")
        .or_else(|| signals.get("rx"))
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "UART template requires 'tx' or 'rx' signal".to_string(),
        })?;

    let hierarchy = waveform.hierarchy();
    let tx_ref = find_signal_by_path(hierarchy, tx_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: tx_path.to_string(),
        }
    })?;
    let ts_opt = hierarchy.timescale();
    waveform.load_signals(&[tx_ref]);

    let tx_signal = waveform.get_signal(tx_ref).ok_or(WaveAnalyzerError::Other(
        "Failed to get UART signal after loading".to_string(),
    ))?;

    let tx_changes = collect_changes(tx_signal, start_idx, end_idx);
    // Snapshot the time table so we can map indices → physical time (ps).
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();

    let mut frames: Vec<UartFrame> = Vec::new();
    let mut start_bit_widths: Vec<f64> = Vec::new();
    let mut stop_bit_widths: Vec<f64> = Vec::new();
    let mut parity_errors = 0usize;
    let mut framing_errors = 0usize;

    let mut prev_high: Option<bool> = None;
    let mut falling_edges: Vec<u32> = Vec::new();

    for &(t, is_high) in &tx_changes {
        if prev_high == Some(true) && !is_high {
            falling_edges.push(t);
        }
        prev_high = Some(is_high);
    }

    // bit_width_ps: bit width in picoseconds (wellen internal time unit).
    let bit_width_ps = estimate_uart_bit_width(&tx_changes, &time_table);

    if bit_width_ps > 0.0 {
        for &start_edge in &falling_edges {
            // Convert physical-time bit positions back to the closest time
            // index for `read_bit_at_time_from_changes` lookups.
            let start_edge_ps = time_table.get(start_edge as usize).copied().unwrap_or(0);

            let stop_bit_start_ps = start_edge_ps + (10.0 * bit_width_ps) as u64;
            let stop_bit_with_parity_ps = start_edge_ps + (11.0 * bit_width_ps) as u64;
            let stop_bit_start_idx =
                physical_time_to_time_index(&time_table, stop_bit_start_ps as f64);
            let stop_bit_with_parity_idx =
                physical_time_to_time_index(&time_table, stop_bit_with_parity_ps as f64);

            let mut data_bits: Vec<bool> = Vec::new();
            for bit_num in 0..8 {
                let bit_center_ps =
                    start_edge_ps + ((1.0 + bit_num as f64 + 0.5) * bit_width_ps) as u64;
                if let Some(bit_center_idx) =
                    physical_time_to_time_index(&time_table, bit_center_ps as f64)
                    && bit_center_idx <= end_idx
                    && let Some(bit_val) =
                        read_bit_at_time_from_changes(&tx_changes, bit_center_idx)
                {
                    data_bits.push(bit_val);
                }
            }

            let has_parity = check_parity_bit(
                &tx_changes,
                &time_table,
                start_edge,
                bit_width_ps,
                &data_bits,
                end_idx,
            );

            let stop_pos_idx = if has_parity {
                stop_bit_with_parity_idx
            } else {
                stop_bit_start_idx
            };

            let stop_bit_high = match stop_pos_idx {
                Some(idx) if idx <= end_idx => read_bit_at_time_from_changes(&tx_changes, idx),
                _ => None,
            };

            let is_framing_error = stop_bit_high.is_none_or(|v| !v);
            if is_framing_error {
                framing_errors += 1;
            }

            if has_parity {
                let parity_pos_ps = start_edge_ps + ((9.0 + 0.5) * bit_width_ps) as u64;
                let parity_pos_idx = physical_time_to_time_index(&time_table, parity_pos_ps as f64);
                let parity_bit = match parity_pos_idx {
                    Some(idx) if idx <= end_idx => read_bit_at_time_from_changes(&tx_changes, idx),
                    _ => None,
                };
                let data_xor: bool = data_bits.iter().fold(false, |a, b| a ^ *b);
                if parity_bit.is_some_and(|p| p != data_xor) {
                    parity_errors += 1;
                }
            }

            // Width measurements stay in index-units to preserve back-compat
            // with downstream consumers (start/stop bit stats). For high-
            // precision physical-time reporting, see `baud_rate_measurement`.
            let start_end = find_next_rising_after(&tx_changes, start_edge);
            if let Some(se) = start_end {
                start_bit_widths.push((se - start_edge) as f64);
            }
            if let Some(stop_idx) = stop_pos_idx
                && stop_idx <= end_idx
                && let Some(stop_end) = find_next_falling_after(&tx_changes, stop_idx as u32)
            {
                stop_bit_widths.push((stop_end - stop_idx as u32) as f64);
            }

            frames.push(UartFrame {
                start_time_index: start_edge as u64,
                data_bits,
                has_parity,
                is_framing_error,
            });
        }
    }

    let baud_rate_measurement = compute_uart_baud_rate(tx_path, bit_width_ps, &ts_opt);

    Ok(crate::protocol_template::ProtocolAnalysisResult {
        protocol: crate::protocol_template::ProtocolTemplate::Uart,
        signal_mapping: signals.clone(),
        spi_result: None,
        uart_result: Some(UartAnalysisResult {
            baud_rate_measurement,
            frame_count: frames.len(),
            start_bit_width_stats: compute_stats(&start_bit_widths),
            stop_bit_width_stats: compute_stats(&stop_bit_widths),
            parity_errors,
            framing_errors,
        }),
        i2c_result: None,
        axi_lite_result: None,
    })
}
