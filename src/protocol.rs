//! Protocol analysis and signal measurement.
//!
//! Provides three analysis capabilities:
//! 1. **Handshake detection**: Detect valid/ready handshake transactions and report
//!    each handshake with timing information (latency between valid and ready).
//! 2. **Clock/pulse measurement**: Measure clock properties (period, frequency, duty cycle)
//!    or pulse widths on arbitrary signals.
//! 3. **Interval measurement**: Measure time intervals between two condition events,
//!    enabling nanosecond-level cross-signal timing analysis.

use num_bigint::BigUint;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use wellen::simple::Waveform;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::{ReportWriter, format_signal_value, is_signal_high};
use crate::hierarchy::{find_signal_by_path, find_var_by_path, resolve_signal_with_width};
use crate::report_write;
use crate::report_writeln;
use crate::time_map::compute_time_ps_from_table;

// === Handshake Analysis ===

/// A single detected handshake transaction.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HandshakeEvent {
    /// Time index when valid signal went high.
    pub valid_time_index: u64,
    /// Time index when ready signal went high (completing handshake).
    pub ready_time_index: u64,
    /// Latency from valid assertion to transfer in time index units.
    pub latency_time_indices: u64,
    /// Latency formatted with appropriate physical time unit (e.g., "10.000ns").
    pub latency_formatted: String,
    /// Latency in seconds (physical time).
    #[serde(default)]
    pub latency_sec: f64,
    /// Data value at transfer time (if data_signal was specified), else None.
    pub data_value: Option<String>,
}

/// Summary statistics across all detected handshakes.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HandshakeSummary {
    /// Total number of handshake transactions detected (including zero-delay).
    pub total_handshakes: usize,
    /// Number of zero-delay events (valid and ready asserted at same time index).
    #[serde(default)]
    pub zero_delay_count: usize,
    /// Average latency in seconds (physical time), excluding zero-delay.
    pub avg_latency_sec: f64,
    /// Minimum latency in seconds, excluding zero-delay.
    pub min_latency_sec: f64,
    /// Maximum latency in seconds, excluding zero-delay.
    pub max_latency_sec: f64,
    /// Average latency in time indices, excluding zero-delay.
    pub avg_latency: f64,
    /// Minimum latency (in time indices), excluding zero-delay.
    pub min_latency: u64,
    /// Maximum latency (in time indices), excluding zero-delay.
    pub max_latency: u64,
    /// First handshake time index.
    pub first_handshake_time: u64,
    /// Last handshake time index.
    pub last_handshake_time: u64,
}

/// Full handshake analysis report.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HandshakeReport {
    pub valid_signal: String,
    pub ready_signal: String,
    pub data_signal: Option<String>,
    pub summary: HandshakeSummary,
    /// Individual events (limited by report_mode and limit parameter).
    pub events: Vec<HandshakeEvent>,
    /// Warning message if the valid signal appears continuously asserted,
    /// indicating the detected events may not represent true handshake transactions.
    #[serde(default)]
    pub warning: Option<String>,
    /// Report mode: "summary" or "detail".
    #[serde(default)]
    pub report_mode: String,
}

// === Signal Measurement ===

/// Statistics for a set of measurements.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MeasurementStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub stddev: f64,
}

/// Clock measurement result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ClockMeasurement {
    pub signal_path: String,
    pub edge_type: String,
    /// Period statistics in seconds.
    pub period: MeasurementStats,
    /// Frequency in Hz (derived from period and timescale).
    pub frequency_hz: Option<f64>,
    /// Duty cycle percentage (0-100).
    pub duty_cycle_pct: Option<f64>,
    /// Jitter (standard deviation of period in seconds).
    pub jitter: f64,
}

/// Pulse measurement result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PulseMeasurement {
    pub signal_path: String,
    /// Positive pulse width statistics (in seconds).
    pub high_pulses: MeasurementStats,
    /// Negative pulse width statistics (in seconds).
    pub low_pulses: MeasurementStats,
    /// Total number of positive pulses.
    pub high_pulse_count: usize,
    /// Total number of negative pulses.
    pub low_pulse_count: usize,
}

/// A single interval event — the time between a from-condition match and the next to-condition match.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntervalEvent {
    /// Time index where from-condition was satisfied.
    pub from_time_index: usize,
    /// Time index where to-condition was satisfied.
    pub to_time_index: usize,
    /// Interval duration in seconds.
    pub interval_sec: f64,
    /// Interval duration formatted with appropriate unit (e.g., "4330.000ns").
    pub interval_formatted: String,
}

/// Interval measurement result — time between two condition events.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct IntervalMeasurement {
    /// From condition expression.
    pub from_condition: String,
    /// To condition expression.
    pub to_condition: String,
    /// Statistics of all measured intervals (in seconds).
    pub interval_stats: MeasurementStats,
    /// Expected interval in seconds (if provided by user).
    pub expected_sec: Option<f64>,
    /// Deviation from expected value as percentage (if expected provided).
    pub deviation_pct: Option<f64>,
    /// Individual interval events (limited by max_events).
    pub events: Vec<IntervalEvent>,
}

/// Compute basic statistics from a list of f64 values.
pub fn compute_stats(values: &[f64]) -> MeasurementStats {
    if values.is_empty() {
        return MeasurementStats {
            count: 0,
            min: 0.0,
            max: 0.0,
            avg: 0.0,
            stddev: 0.0,
        };
    }
    let count = values.len();
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = values.iter().sum();
    let avg = sum / count as f64;
    let variance: f64 = values.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / count as f64;
    MeasurementStats {
        count,
        min,
        max,
        avg,
        stddev: variance.sqrt(),
    }
}

/// Resolve the logical level of a signal at a time index.
fn signal_level_at_time(
    waveform: &Waveform,
    signal_ref: wellen::SignalRef,
    time_idx: usize,
) -> Option<bool> {
    let signal = waveform.get_signal(signal_ref)?;
    let time_table_idx: wellen::TimeTableIdx = time_idx.try_into().ok()?;
    let offset = signal.get_offset(time_table_idx)?;
    Some(is_signal_high(&signal.get_value_at(&offset, 0)))
}

fn has_high_event_at_time(changes: &[(u32, wellen::SignalValue)], time_idx: usize) -> bool {
    changes
        .iter()
        .any(|(change_idx, value)| *change_idx as usize == time_idx && is_signal_high(value))
}

/// Return true iff the most recent change in `changes` at or before
/// `time_idx` indicates the signal is currently high. Used by the handshake
/// state machine to detect AXI-style bursts where valid stays high across
/// multiple ready pulses.
///
/// The change stream is `(time_index, value)` pairs in time order. If no
/// change exists at or before `time_idx` the signal value is the recorded
/// initial value, which we conservatively treat as "low" (returning false).
fn is_signal_high_at_or_before(changes: &[(u32, wellen::SignalValue)], time_idx: u32) -> bool {
    // Walk from the end (latest change ≤ time_idx) using the fact that
    // changes is sorted by time. Linear scan is fine here because the
    // change stream is bounded by the number of recorded edges, typically
    // O(10²–10⁴) for a handshake analysis window.
    let mut last_high: Option<bool> = None;
    for (change_t, value) in changes {
        if *change_t > time_idx {
            break;
        }
        last_high = Some(is_signal_high(value));
    }
    matches!(last_high, Some(true))
}

/// Read a multi-bit signal value at a specific time index and format it.
fn read_data_value_at_time(
    waveform: &Waveform,
    data_signal_ref: wellen::SignalRef,
    time_idx: usize,
    time_table: &[wellen::Time],
    _width: u32,
) -> Option<String> {
    if time_idx >= time_table.len() {
        return None;
    }
    let signal = waveform.get_signal(data_signal_ref)?;
    let time_table_idx: wellen::TimeTableIdx = time_idx.try_into().ok()?;
    if let Some(offset) = signal.get_offset(time_table_idx) {
        let value = signal.get_value_at(&offset, 0);
        return Some(format_signal_value(value));
    }
    // Fallback: try to parse from iter_changes context
    None
}

// === Handshake Analysis ===

/// Analyze valid/ready handshake transactions in a waveform.
///
/// Detects handshake events where:
/// 1. Valid signal goes high (assertion)
/// 2. Ready signal goes high while valid is still high (transfer)
///
/// Stale handshakes (valid goes low before ready goes high) are not reported.
#[allow(clippy::too_many_arguments)]
pub fn analyze_handshake(
    waveform: &mut Waveform,
    valid_path: &str,
    ready_path: &str,
    data_path: Option<&str>,
    start_idx: usize,
    end_idx: usize,
    limit: Option<isize>,
    report_mode: &str,
    _filter_zero_delay: bool,
) -> WaveResult<HandshakeReport> {
    analyze_handshake_with_level_sensitive(
        waveform,
        valid_path,
        ready_path,
        data_path,
        start_idx,
        end_idx,
        limit,
        report_mode,
        _filter_zero_delay,
        false,
    )
}

/// Analyze valid/ready handshakes with an optional level-sensitive transfer mode.
///
/// When `level_sensitive` is true, every time index where both valid and ready
/// are high is counted as one transfer. This is intended for enable-style
/// protocols where a level-held enable represents one data beat per cycle.
#[allow(clippy::too_many_arguments)]
pub fn analyze_handshake_with_level_sensitive(
    waveform: &mut Waveform,
    valid_path: &str,
    ready_path: &str,
    data_path: Option<&str>,
    start_idx: usize,
    end_idx: usize,
    limit: Option<isize>,
    report_mode: &str,
    _filter_zero_delay: bool,
    level_sensitive: bool,
) -> WaveResult<HandshakeReport> {
    let hierarchy = waveform.hierarchy();
    let time_table_len = waveform.time_table().len();
    if time_table_len == 0 {
        return Err(WaveAnalyzerError::ProtocolError {
            message: "Waveform has no time entries".into(),
        });
    }
    if start_idx > end_idx {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "start_time_index must be less than or equal to end_time_index".into(),
        });
    }
    let end_idx = end_idx.min(time_table_len - 1);

    // Validate 1-bit width for valid/ready
    let valid_var = find_var_by_path(hierarchy, valid_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: valid_path.into(),
        }
    })?;
    let ready_var = find_var_by_path(hierarchy, ready_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: ready_path.into(),
        }
    })?;

    let valid_width = hierarchy[valid_var].length().unwrap_or(1);
    let ready_width = hierarchy[ready_var].length().unwrap_or(1);

    if valid_width != 1 {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Valid signal '{}' is {} bits wide, expected 1 bit",
                valid_path, valid_width
            ),
        });
    }
    if ready_width != 1 {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Ready signal '{}' is {} bits wide, expected 1 bit",
                ready_path, ready_width
            ),
        });
    }

    let valid_signal_ref = find_signal_by_path(hierarchy, valid_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: valid_path.into(),
        }
    })?;
    let ready_signal_ref = find_signal_by_path(hierarchy, ready_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: ready_path.into(),
        }
    })?;

    let data_signal_info = match data_path {
        Some(dp) => {
            let sig_ref = find_signal_by_path(hierarchy, dp)
                .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: dp.into() })?;
            let var = find_var_by_path(hierarchy, dp)
                .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: dp.into() })?;
            let w = hierarchy[var].length().unwrap_or(1);
            Some((sig_ref, w))
        }
        None => None,
    };

    // Load signals
    let mut refs_to_load = vec![valid_signal_ref, ready_signal_ref];
    if let Some((dsr, _)) = data_signal_info {
        refs_to_load.push(dsr);
    }
    waveform.load_signals(&refs_to_load);

    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let ts_opt = waveform.hierarchy().timescale();

    // Collect changes for valid and ready within the time range
    let valid_signal =
        waveform
            .get_signal(valid_signal_ref)
            .ok_or_else(|| WaveAnalyzerError::ProtocolError {
                message: "Failed to get valid signal after loading".into(),
            })?;
    let ready_signal =
        waveform
            .get_signal(ready_signal_ref)
            .ok_or_else(|| WaveAnalyzerError::ProtocolError {
                message: "Failed to get ready signal after loading".into(),
            })?;

    let mut valid_changes: Vec<(u32, wellen::SignalValue)> = Vec::new();
    for (time_idx, value) in valid_signal.iter_changes() {
        let idx = time_idx as usize;
        if idx >= start_idx && idx <= end_idx {
            valid_changes.push((time_idx, value));
        }
    }

    let mut ready_changes: Vec<(u32, wellen::SignalValue)> = Vec::new();
    for (time_idx, value) in ready_signal.iter_changes() {
        let idx = time_idx as usize;
        if idx >= start_idx && idx <= end_idx {
            ready_changes.push((time_idx, value));
        }
    }

    // Merge sorted change streams
    // Each entry: (time_idx, is_valid: bool, value_is_high: bool)
    let mut merged: Vec<(u32, bool, bool)> = Vec::new();
    for (t, v) in &valid_changes {
        merged.push((*t, true, is_signal_high(v)));
    }
    for (t, v) in &ready_changes {
        merged.push((*t, false, is_signal_high(v)));
    }
    merged.sort_by_key(|&(t, _, _)| t);

    // State machine scan
    #[derive(PartialEq)]
    enum HsState {
        Idle,
        ValidAsserted,
    }

    let initial_valid_high = signal_level_at_time(waveform, valid_signal_ref, start_idx)
        .ok_or_else(|| WaveAnalyzerError::ProtocolError {
            message: "Failed to read valid signal at start_time_index".into(),
        })?;
    let initial_ready_high = signal_level_at_time(waveform, ready_signal_ref, start_idx)
        .ok_or_else(|| WaveAnalyzerError::ProtocolError {
            message: "Failed to read ready signal at start_time_index".into(),
        })?;

    if level_sensitive {
        let mut events: Vec<HandshakeEvent> = Vec::new();
        for idx in start_idx..=end_idx {
            let valid_high =
                signal_level_at_time(waveform, valid_signal_ref, idx).ok_or_else(|| {
                    WaveAnalyzerError::ProtocolError {
                        message: format!("Failed to read valid signal at time index {}", idx),
                    }
                })?;
            let ready_high =
                signal_level_at_time(waveform, ready_signal_ref, idx).ok_or_else(|| {
                    WaveAnalyzerError::ProtocolError {
                        message: format!("Failed to read ready signal at time index {}", idx),
                    }
                })?;

            if valid_high && ready_high {
                let data_value = if let Some((dsr, dw)) = data_signal_info {
                    read_data_value_at_time(waveform, dsr, idx, &time_table, dw)
                } else {
                    None
                };
                events.push(HandshakeEvent {
                    valid_time_index: idx as u64,
                    ready_time_index: idx as u64,
                    latency_time_indices: 0,
                    latency_formatted: "0 (level-sensitive mode)".to_string(),
                    latency_sec: 0.0,
                    data_value,
                });
            }

            if let Some(lim) = limit
                && lim > 0
                && events.len() >= lim as usize
            {
                break;
            }
        }

        let summary = compute_handshake_summary(&events);
        return Ok(HandshakeReport {
            valid_signal: valid_path.to_string(),
            ready_signal: ready_path.to_string(),
            data_signal: data_path.map(String::from),
            summary,
            events,
            warning: Some(
                "Level-sensitive mode: each sampled time index with valid=1 and ready=1 is counted as one transfer."
                    .to_string(),
            ),
            report_mode: report_mode.to_string(),
        });
    }

    let mut state = if initial_valid_high {
        HsState::ValidAsserted
    } else {
        HsState::Idle
    };
    let mut valid_high_time: Option<u32> = initial_valid_high.then_some(start_idx as u32);
    let mut events: Vec<HandshakeEvent> = Vec::new();

    if initial_valid_high
        && initial_ready_high
        && !has_high_event_at_time(&ready_changes, start_idx)
    {
        let data_value = if let Some((dsr, dw)) = data_signal_info {
            read_data_value_at_time(waveform, dsr, start_idx, &time_table, dw)
        } else {
            None
        };
        events.push(HandshakeEvent {
            valid_time_index: start_idx as u64,
            ready_time_index: start_idx as u64,
            latency_time_indices: 0,
            latency_formatted: "0 (zero-delay)".to_string(),
            latency_sec: 0.0,
            data_value,
        });
        state = HsState::Idle;
        valid_high_time = None;
    }

    for (time_idx, is_valid, is_high) in &merged {
        if *is_valid {
            if *is_high && state == HsState::Idle {
                valid_high_time = Some(*time_idx);
                state = HsState::ValidAsserted;
            } else if !*is_high {
                // Valid deasserted before handshake completed
                state = HsState::Idle;
                valid_high_time = None;
            }
        } else {
            // Ready signal
            if *is_high && state == HsState::ValidAsserted {
                let vht = valid_high_time.unwrap_or(*time_idx);
                let latency = time_idx.saturating_sub(vht);

                // Compute real latency in physical time using time_table
                let vht_ps = crate::time_map::compute_time_ps_from_table(
                    &time_table,
                    vht as usize,
                    ts_opt.as_ref(),
                );
                let ready_ps = crate::time_map::compute_time_ps_from_table(
                    &time_table,
                    *time_idx as usize,
                    ts_opt.as_ref(),
                );
                let latency_sec = (ready_ps - vht_ps) as f64 / 1e12; // ps → seconds

                let latency_formatted = format_interval_duration(latency_sec);

                let data_value = if let Some((dsr, dw)) = data_signal_info {
                    read_data_value_at_time(waveform, dsr, *time_idx as usize, &time_table, dw)
                } else {
                    None
                };

                events.push(HandshakeEvent {
                    valid_time_index: vht as u64,
                    ready_time_index: *time_idx as u64,
                    latency_time_indices: latency as u64,
                    latency_formatted,
                    latency_sec,
                    data_value,
                });

                // BUG-fix (AXI burst detection): in AXI, valid may stay high
                // across multiple ready pulses (a burst). The previous
                // implementation unconditionally reset to Idle, detecting
                // only the first beat. Now we check whether valid is still
                // high at this ready time — i.e. whether the most recent
                // valid-edge change before or at this time is a rising edge
                // (high). If so, we re-arm the handshake without resetting
                // the valid_high_time anchor; the next ready pulse will
                // record the next beat.
                let valid_still_high = is_signal_high_at_or_before(&valid_changes, *time_idx);
                if valid_still_high {
                    // Keep the original valid_high_time (vht) so the latency
                    // is measured from the same valid assertion, but stay in
                    // ValidAsserted so the next ready pulse is captured.
                    state = HsState::ValidAsserted;
                } else {
                    state = HsState::Idle;
                    valid_high_time = None;
                }
            }
        }
    }

    // Instead of removing zero-delay events, we keep them all in the events
    // list. The summary computation (compute_handshake_summary) now:
    // - reports total_handshakes including zero-delay events
    // - reports zero_delay_count separately
    // - computes avg/min/max latency excluding zero-delay events
    // This avoids the extreme behavior where filter_zero_delay removed ALL
    // events (90→0).

    // Compute summary (before potential strobe fallback reclassifies events)
    let _pre_strobe_summary = compute_handshake_summary(&events);

    // BUG-25 fix: if no handshake events detected and valid signal has
    // transitions (i.e., it's a strobe/enable-style protocol, not AXI-style),
    // fall back to "strobe mode" where each valid posedge marks a transfer event.
    // This handles designs where valid and ready are both continuously asserted
    // (simple enable/strobe, not a proper handshake pair).
    // Collect valid posedge data before potential mutable borrow below.
    let valid_posedge_times: Vec<usize> = valid_changes
        .iter()
        .filter(|(t, v)| {
            let idx = *t as usize;
            is_signal_high(v) && idx >= start_idx && idx <= end_idx
        })
        .map(|(t, _)| *t as usize)
        .collect();

    let events = if events.is_empty() && (valid_posedge_times.len() >= 2 || initial_valid_high) {
        // Strobe mode fallback: treat each valid posedge as a transfer event,
        // using the valid signal itself as the "ready" (transfer happens
        // when valid=1, no separate ready needed).
        // Only activate when valid shows repeated assertion pattern (≥2 posedges)
        // or starts high — single isolated pulse is not a strobe protocol.
        let mut strobe_events: Vec<HandshakeEvent> = Vec::new();

        // Add initial state if valid starts high
        if initial_valid_high {
            let data_value = data_signal_info.and_then(|(dsr, dw)| {
                read_data_value_at_time(waveform, dsr, start_idx, &time_table, dw)
            });
            strobe_events.push(HandshakeEvent {
                valid_time_index: start_idx as u64,
                ready_time_index: start_idx as u64,
                latency_time_indices: 0,
                latency_formatted: "0 (strobe mode)".to_string(),
                latency_sec: 0.0,
                data_value,
            });
        }

        for idx in valid_posedge_times {
            // Skip if already covered by initial state
            if initial_valid_high && idx == start_idx {
                continue;
            }

            let data_value = data_signal_info
                .and_then(|(dsr, dw)| read_data_value_at_time(waveform, dsr, idx, &time_table, dw));

            strobe_events.push(HandshakeEvent {
                valid_time_index: idx as u64,
                ready_time_index: idx as u64,
                latency_time_indices: 0,
                latency_formatted: "0 (strobe mode)".to_string(),
                latency_sec: 0.0,
                data_value,
            });

            if let Some(lim) = limit
                && lim > 0
                && strobe_events.len() >= lim as usize
            {
                break;
            }
        }

        if strobe_events.is_empty() {
            events // Return original empty events if no valid posedge found either
        } else {
            strobe_events
        }
    } else {
        events
    };

    let mut strobe_warning = None;
    // Check if we used strobe mode
    if !events.is_empty()
        && events
            .iter()
            .all(|e| e.latency_formatted.contains("strobe"))
    {
        strobe_warning = Some(
            "No handshake transactions detected (valid/ready never showed AXI-style behavior). \
            Falling back to strobe mode: each valid assertion is treated as a transfer event. \
            This may indicate the signals are enables/strobes rather than handshake valid/ready pairs.".to_string()
        );
    }

    let summary = compute_handshake_summary(&events);

    let mut reported_events = events;
    if let Some(lim) = limit
        && lim > 0
        && reported_events.len() > lim as usize
    {
        reported_events.truncate(lim as usize);
    }

    // Handshake quality check: detect continuous-assertion patterns
    // If many handshake events are detected with zero or near-zero latency,
    // this likely indicates an enable/pulse signal, not a true handshake valid.
    let warning = if reported_events.len() > 10 {
        // Compute the fraction of events with zero latency (both signals asserted simultaneously)
        let zero_latency_count = reported_events
            .iter()
            .filter(|e| e.latency_time_indices == 0)
            .count();
        let zero_latency_pct = zero_latency_count as f64 / reported_events.len() as f64 * 100.0;

        // If >80% of events have zero latency (valid and ready asserted at same time),
        // these are likely not true handshakes
        if zero_latency_pct > 80.0 {
            Some(format!(
                "Warning: {:.1}% of detected {} events have zero latency (valid and ready asserted simultaneously). \
                These may not represent true handshake transactions. \
                The signals may be enables/pulses rather than valid/ready handshake pairs.",
                zero_latency_pct,
                reported_events.len()
            ))
        } else {
            None
        }
    } else {
        None
    };

    Ok(HandshakeReport {
        valid_signal: valid_path.to_string(),
        ready_signal: ready_path.to_string(),
        data_signal: data_path.map(String::from),
        summary,
        events: reported_events,
        warning: strobe_warning.or(warning),
        report_mode: report_mode.to_string(),
    })
}

/// Compute summary statistics from handshake events.
/// Latency statistics (avg/min/max) exclude zero-delay events to give meaningful
/// timing information. total_handshakes and zero_delay_count include all events.
pub fn compute_handshake_summary(events: &[HandshakeEvent]) -> HandshakeSummary {
    if events.is_empty() {
        return HandshakeSummary {
            total_handshakes: 0,
            zero_delay_count: 0,
            avg_latency_sec: 0.0,
            min_latency_sec: 0.0,
            max_latency_sec: 0.0,
            avg_latency: 0.0,
            min_latency: 0,
            max_latency: 0,
            first_handshake_time: 0,
            last_handshake_time: 0,
        };
    }

    let total = events.len();
    let zero_delay_count = events
        .iter()
        .filter(|e| e.latency_time_indices == 0)
        .count();

    // Compute latency stats only from non-zero-delay events
    let nonzero_events: Vec<&HandshakeEvent> = events
        .iter()
        .filter(|e| e.latency_time_indices > 0)
        .collect();

    let avg_latency = if nonzero_events.is_empty() {
        0.0
    } else {
        let sum_latency: u64 = nonzero_events.iter().map(|e| e.latency_time_indices).sum();
        sum_latency as f64 / nonzero_events.len() as f64
    };
    let min_latency = nonzero_events
        .iter()
        .map(|e| e.latency_time_indices)
        .min()
        .unwrap_or(0);
    let max_latency = nonzero_events
        .iter()
        .map(|e| e.latency_time_indices)
        .max()
        .unwrap_or(0);

    let avg_latency_sec = if nonzero_events.is_empty() {
        0.0
    } else {
        nonzero_events.iter().map(|e| e.latency_sec).sum::<f64>() / nonzero_events.len() as f64
    };
    let min_latency_sec = nonzero_events
        .iter()
        .map(|e| e.latency_sec)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let max_latency_sec = nonzero_events
        .iter()
        .map(|e| e.latency_sec)
        .reduce(f64::max)
        .unwrap_or(0.0);

    let first = events.first().unwrap().valid_time_index;
    let last = events.last().unwrap().ready_time_index;

    HandshakeSummary {
        total_handshakes: total,
        zero_delay_count,
        avg_latency_sec,
        min_latency_sec,
        max_latency_sec,
        avg_latency,
        min_latency,
        max_latency,
        first_handshake_time: first,
        last_handshake_time: last,
    }
}

/// Format a handshake report as human-readable text.
pub fn format_handshake_report(report: &HandshakeReport) -> String {
    let mut out = ReportWriter::with_capacity(512);

    report_writeln!(out, "=== Handshake Analysis ===");
    report_writeln!(out, "Valid signal: {}", report.valid_signal);
    report_writeln!(out, "Ready signal: {}", report.ready_signal);
    if let Some(ref ds) = report.data_signal {
        report_writeln!(out, "Data signal: {}", ds);
    }
    if let Some(ref warning) = report.warning {
        report_writeln!(out);
        report_writeln!(out, "⚠ {}", warning);
    }
    report_writeln!(out);

    let s = &report.summary;
    report_writeln!(out, "Total handshakes: {}", s.total_handshakes);
    if s.zero_delay_count > 0 {
        report_writeln!(
            out,
            "Zero-delay (simultaneous): {} ({:.1}% of total)",
            s.zero_delay_count,
            s.zero_delay_count as f64 / s.total_handshakes as f64 * 100.0
        );
    }
    if s.total_handshakes > 0 {
        if s.zero_delay_count < s.total_handshakes {
            report_writeln!(
                out,
                "Average latency: {}",
                format_interval_duration(s.avg_latency_sec)
            );
            report_writeln!(
                out,
                "Min latency: {}",
                format_interval_duration(s.min_latency_sec)
            );
            report_writeln!(
                out,
                "Max latency: {}",
                format_interval_duration(s.max_latency_sec)
            );
        }
        report_writeln!(
            out,
            "First handshake at: time index {}",
            s.first_handshake_time
        );
        report_writeln!(
            out,
            "Last handshake at: time index {}",
            s.last_handshake_time
        );
    }

    // BUG-8 fix: skip events detail in summary mode
    if report.report_mode != "summary" && !report.events.is_empty() {
        report_writeln!(out);
        report_writeln!(out, "Events ({} shown):", report.events.len());
        for (i, e) in report.events.iter().enumerate() {
            report_write!(
                out,
                "  #{}: valid@{} -> ready@{} (latency: {})",
                i + 1,
                e.valid_time_index,
                e.ready_time_index,
                e.latency_formatted
            );
            if e.latency_time_indices == 0 {
                report_write!(out, " [zero-delay]");
            }
            if let Some(ref dv) = e.data_value {
                report_write!(out, ", data={}", dv);
            }
            report_writeln!(out);
        }
    }

    out.finish()
}

// === Clock Measurement ===

/// Measure clock properties from a 1-bit clock signal.
pub fn measure_clock(
    waveform: &mut Waveform,
    signal_path: &str,
    edge_type: &str,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<ClockMeasurement> {
    if edge_type != "posedge" && edge_type != "negedge" {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Invalid edge_type: '{}'. Must be 'posedge' or 'negedge'",
                edge_type
            ),
        });
    }

    let hierarchy = waveform.hierarchy();
    let (signal_ref, width) = resolve_signal_with_width(hierarchy, signal_path).map_err(|_| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.into(),
        }
    })?;
    if width != 1 {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Signal '{}' is {} bits wide, expected 1 bit for clock measurement",
                signal_path, width
            ),
        });
    }

    waveform.load_signals(&[signal_ref]);
    let signal =
        waveform
            .get_signal(signal_ref)
            .ok_or_else(|| WaveAnalyzerError::ProtocolError {
                message: "Failed to get signal after loading".into(),
            })?;

    // Collect changes and detect edges (single pass: collect both target edges and all edges)
    let mut prev_value: Option<bool> = None;
    let mut edges: Vec<u32> = Vec::new();
    let mut pos_edges: Vec<u32> = Vec::new();
    let mut neg_edges: Vec<u32> = Vec::new();
    const MAX_EDGES: usize = 100_000; // Safety limit for large files

    for (time_idx, value) in signal.iter_changes() {
        let idx = time_idx as usize;
        if idx < start_idx || idx > end_idx {
            if idx <= end_idx {
                // Track value for initial state but don't record edges
                prev_value = Some(is_signal_high(&value));
            }
            continue;
        }
        let is_high = is_signal_high(&value);

        // Detect posedge and negedge
        if prev_value == Some(false) && is_high {
            pos_edges.push(time_idx);
            if edge_type == "posedge" {
                edges.push(time_idx);
            }
        } else if prev_value == Some(true) && !is_high {
            neg_edges.push(time_idx);
            if edge_type == "negedge" {
                edges.push(time_idx);
            }
        }

        prev_value = Some(is_high);

        // Safety limit: stop after collecting enough edges
        if edges.len() >= MAX_EDGES {
            break;
        }
    }

    // Compute periods (consecutive edge intervals) using actual time values
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale_ref = waveform.hierarchy().timescale();
    let mut periods: Vec<f64> = Vec::new();
    for i in 1..edges.len() {
        let t0_ps =
            compute_time_ps_from_table(&time_table, edges[i - 1] as usize, timescale_ref.as_ref());
        let t1_ps =
            compute_time_ps_from_table(&time_table, edges[i] as usize, timescale_ref.as_ref());
        periods.push((t1_ps - t0_ps) as f64 / 1e12); // ps → seconds
    }

    let period_stats = compute_stats(&periods);
    let jitter = period_stats.stddev;

    // Compute frequency: periods are already in seconds
    let frequency_hz = if period_stats.avg > 0.0 {
        Some(1.0 / period_stats.avg)
    } else {
        None
    };

    // Compute duty cycle from already-collected pos_edges and neg_edges using actual ps values
    let duty_cycle_pct = if !pos_edges.is_empty() && !neg_edges.is_empty() {
        let mut duty_cycles: Vec<f64> = Vec::new();
        for &pos in &pos_edges {
            let neg_insert_idx = neg_edges.partition_point(|&n| n <= pos);
            if neg_insert_idx < neg_edges.len() {
                let neg = neg_edges[neg_insert_idx];
                let high_time_ps =
                    compute_time_ps_from_table(&time_table, neg as usize, timescale_ref.as_ref())
                        - compute_time_ps_from_table(
                            &time_table,
                            pos as usize,
                            timescale_ref.as_ref(),
                        );
                let pos_insert_idx = pos_edges.partition_point(|&p| p <= pos);
                if pos_insert_idx < pos_edges.len() {
                    let period_ps = compute_time_ps_from_table(
                        &time_table,
                        pos_edges[pos_insert_idx] as usize,
                        timescale_ref.as_ref(),
                    ) - compute_time_ps_from_table(
                        &time_table,
                        pos as usize,
                        timescale_ref.as_ref(),
                    );
                    if period_ps > 0 {
                        duty_cycles.push(high_time_ps as f64 / period_ps as f64 * 100.0);
                    }
                }
            }
        }
        if duty_cycles.is_empty() {
            None
        } else {
            Some(duty_cycles.iter().sum::<f64>() / duty_cycles.len() as f64)
        }
    } else {
        None
    };

    Ok(ClockMeasurement {
        signal_path: signal_path.to_string(),
        edge_type: edge_type.to_string(),
        period: period_stats,
        frequency_hz,
        duty_cycle_pct,
        jitter,
    })
}

/// Format a clock measurement report as human-readable text.
pub fn format_clock_report(report: &ClockMeasurement) -> String {
    fn format_duration(seconds: f64) -> String {
        if seconds >= 1.0 {
            format!("{seconds:.6} s")
        } else if seconds >= 1e-3 {
            format!("{:.6} ms", seconds * 1e3)
        } else if seconds >= 1e-6 {
            format!("{:.6} us", seconds * 1e6)
        } else if seconds >= 1e-9 {
            format!("{:.6} ns", seconds * 1e9)
        } else if seconds >= 1e-12 {
            format!("{:.6} ps", seconds * 1e12)
        } else {
            format!("{seconds:.6e} s")
        }
    }

    let mut out = ReportWriter::with_capacity(256);

    report_writeln!(out, "=== Clock Measurement ===");
    report_writeln!(out, "Signal: {}", report.signal_path);
    report_writeln!(out, "Edge type: {}", report.edge_type);
    report_writeln!(out);

    let p = &report.period;
    if p.count == 0 {
        report_writeln!(out, "No edges detected");
    } else {
        report_writeln!(out, "Periods measured: {}", p.count);
        report_writeln!(out, "Average period: {}", format_duration(p.avg));
        report_writeln!(out, "Min period: {}", format_duration(p.min));
        report_writeln!(out, "Max period: {}", format_duration(p.max));
        report_writeln!(out, "Jitter (stddev): {}", format_duration(p.stddev));

        if let Some(freq) = report.frequency_hz {
            if freq >= 1e9 {
                report_writeln!(out, "Frequency: {:.3} GHz", freq / 1e9);
            } else if freq >= 1e6 {
                report_writeln!(out, "Frequency: {:.3} MHz", freq / 1e6);
            } else if freq >= 1e3 {
                report_writeln!(out, "Frequency: {:.3} kHz", freq / 1e3);
            } else {
                report_writeln!(out, "Frequency: {:.3} Hz", freq);
            }
        }

        if let Some(duty) = report.duty_cycle_pct {
            report_writeln!(out, "Duty cycle: {:.1}%", duty);
        }
    }

    out.finish()
}

// === Pulse Measurement ===

/// Measure pulse widths on an arbitrary signal.
///
/// Pulse widths are reported in seconds (using the waveform timescale),
/// consistent with `ClockMeasurement.period` units. This enables
/// nanosecond/picosecond-level precision for timing analysis.
pub fn measure_pulses(
    waveform: &mut Waveform,
    signal_path: &str,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<PulseMeasurement> {
    let hierarchy = waveform.hierarchy();
    let (signal_ref, _) = resolve_signal_with_width(hierarchy, signal_path).map_err(|_| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.into(),
        }
    })?;

    waveform.load_signals(&[signal_ref]);
    let signal =
        waveform
            .get_signal(signal_ref)
            .ok_or_else(|| WaveAnalyzerError::ProtocolError {
                message: "Failed to get signal after loading".into(),
            })?;

    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale_ref = waveform.hierarchy().timescale();

    let mut high_pulses: Vec<f64> = Vec::new();
    let mut low_pulses: Vec<f64> = Vec::new();
    let mut high_start: Option<u32> = None;
    let mut low_start: Option<u32> = None;
    let mut prev_value: Option<bool> = None;

    for (time_idx, value) in signal.iter_changes() {
        let idx = time_idx as usize;
        let is_high = is_signal_high(&value);

        if idx < start_idx {
            // Track state AND pulse starts before range
            match (prev_value, is_high) {
                (Some(false), true) => {
                    low_start = None;
                    high_start = Some(start_idx as u32); // Pulse spans range boundary
                }
                (Some(true), false) => {
                    high_start = None;
                    low_start = Some(start_idx as u32); // Pulse spans range boundary
                }
                (None, true) => {
                    // Signal starts high — pulse begins at range boundary
                    high_start = Some(start_idx as u32);
                }
                (None, false) => {
                    // Signal starts low — pulse begins at range boundary
                    low_start = Some(start_idx as u32);
                }
                _ => {}
            }
            prev_value = Some(is_high);
            continue;
        }
        if idx > end_idx {
            break;
        }

        match (prev_value, is_high) {
            (Some(false), true) => {
                // Rising edge: end of low pulse
                if let Some(ls) = low_start {
                    let t0_ps = compute_time_ps_from_table(
                        &time_table,
                        ls as usize,
                        timescale_ref.as_ref(),
                    );
                    let t1_ps =
                        compute_time_ps_from_table(&time_table, idx, timescale_ref.as_ref());
                    low_pulses.push((t1_ps - t0_ps) as f64 / 1e12); // ps → seconds
                }
                low_start = None;
                high_start = Some(time_idx);
            }
            (None, true) => {
                // Initial state is high — treat as rising edge from start_idx boundary
                if let Some(ls) = low_start {
                    let t0_ps = compute_time_ps_from_table(
                        &time_table,
                        ls as usize,
                        timescale_ref.as_ref(),
                    );
                    let t1_ps =
                        compute_time_ps_from_table(&time_table, idx, timescale_ref.as_ref());
                    low_pulses.push((t1_ps - t0_ps) as f64 / 1e12);
                }
                low_start = None;
                high_start = Some(start_idx as u32);
            }
            (Some(true), false) => {
                // Falling edge: end of high pulse
                if let Some(hs) = high_start {
                    let t0_ps = compute_time_ps_from_table(
                        &time_table,
                        hs as usize,
                        timescale_ref.as_ref(),
                    );
                    let t1_ps =
                        compute_time_ps_from_table(&time_table, idx, timescale_ref.as_ref());
                    high_pulses.push((t1_ps - t0_ps) as f64 / 1e12); // ps → seconds
                }
                high_start = None;
                low_start = Some(time_idx);
            }
            (None, false) => {
                // Initial state is low — treat as falling edge from start_idx boundary
                if let Some(hs) = high_start {
                    let t0_ps = compute_time_ps_from_table(
                        &time_table,
                        hs as usize,
                        timescale_ref.as_ref(),
                    );
                    let t1_ps =
                        compute_time_ps_from_table(&time_table, idx, timescale_ref.as_ref());
                    high_pulses.push((t1_ps - t0_ps) as f64 / 1e12);
                }
                high_start = None;
                low_start = Some(start_idx as u32);
            }
            _ => {}
        }
        prev_value = Some(is_high);
    }

    Ok(PulseMeasurement {
        signal_path: signal_path.to_string(),
        high_pulses: compute_stats(&high_pulses),
        low_pulses: compute_stats(&low_pulses),
        high_pulse_count: high_pulses.len(),
        low_pulse_count: low_pulses.len(),
    })
}

/// Format a pulse measurement report as human-readable text.
pub fn format_pulse_report(report: &PulseMeasurement) -> String {
    fn format_duration(seconds: f64) -> String {
        if seconds >= 1.0 {
            format!("{seconds:.6} s")
        } else if seconds >= 1e-3 {
            format!("{:.3} ms", seconds * 1e3)
        } else if seconds >= 1e-6 {
            format!("{:.3} us", seconds * 1e6)
        } else if seconds >= 1e-9 {
            format!("{:.3} ns", seconds * 1e9)
        } else if seconds >= 1e-12 {
            format!("{:.3} ps", seconds * 1e12)
        } else {
            format!("{seconds:.6e} s")
        }
    }

    let mut out = ReportWriter::with_capacity(256);

    report_writeln!(out, "=== Pulse Measurement ===");
    report_writeln!(out, "Signal: {}", report.signal_path);
    report_writeln!(out);

    report_writeln!(out, "High pulses: {}", report.high_pulse_count);
    if report.high_pulses.count > 0 {
        let hp = &report.high_pulses;
        report_writeln!(out, "  Average width: {}", format_duration(hp.avg));
        report_writeln!(out, "  Min width: {}", format_duration(hp.min));
        report_writeln!(out, "  Max width: {}", format_duration(hp.max));
        report_writeln!(out, "  Stddev: {}", format_duration(hp.stddev));
    }

    report_writeln!(out, "Low pulses: {}", report.low_pulse_count);
    if report.low_pulses.count > 0 {
        let lp = &report.low_pulses;
        report_writeln!(out, "  Average width: {}", format_duration(lp.avg));
        report_writeln!(out, "  Min width: {}", format_duration(lp.min));
        report_writeln!(out, "  Max width: {}", format_duration(lp.max));
        report_writeln!(out, "  Stddev: {}", format_duration(lp.stddev));
    }

    out.finish()
}

/// Format a BigUint value according to the requested format.
/// Delegates to the canonical `formatting::format_biguint_value`.
pub fn format_value(value: &BigUint, width: u32, format: &str) -> String {
    crate::formatting::format_biguint_value(value, width, format)
}

// === Interval Measurement ===

fn format_interval_duration(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{seconds:.6} s")
    } else if seconds >= 1e-3 {
        format!("{:.3} ms", seconds * 1e3)
    } else if seconds >= 1e-6 {
        format!("{:.3} us", seconds * 1e6)
    } else if seconds >= 1e-9 {
        format!("{:.3} ns", seconds * 1e9)
    } else if seconds >= 1e-12 {
        format!("{:.3} ps", seconds * 1e12)
    } else {
        format!("{seconds:.6e} s")
    }
}

/// Measure time intervals between two condition events in a waveform.
///
/// For each time point where `from_condition` is satisfied, finds the next time
/// point where `to_condition` is satisfied, and computes the interval duration
/// using the waveform timescale. Returns statistics across all intervals, with
/// optional comparison against an expected value.
///
/// This enables nanosecond-level cross-signal timing analysis — for example,
/// measuring the time from "bit period start" to "sampling point" to detect
/// subtle timing deviations like BAUD_HALF-1 vs BAUD_HALF.
#[allow(clippy::too_many_arguments)]
pub fn measure_intervals(
    waveform: &mut Waveform,
    from_condition: &str,
    to_condition: &str,
    start_idx: usize,
    end_idx: usize,
    expected_sec: Option<f64>,
    max_events: Option<usize>,
) -> WaveResult<IntervalMeasurement> {
    // Parse conditions
    let from_parsed = crate::condition::parse_condition(from_condition)?;
    let to_parsed = crate::condition::parse_condition(to_condition)?;

    // Extract signal paths from both conditions
    let from_signals = crate::condition::extract_signal_names(&from_parsed);
    let to_signals = crate::condition::extract_signal_names(&to_parsed);

    // Build signal cache entries for both conditions
    let hierarchy = waveform.hierarchy();
    let mut signal_cache = std::collections::HashMap::new();
    let mut all_signal_refs = Vec::new();

    // Process from-condition signals
    for signal_name in &from_signals {
        let entry =
            crate::condition::build_signal_cache_entry(hierarchy, signal_name).map_err(|_| {
                WaveAnalyzerError::SignalNotFound {
                    path: signal_name.clone(),
                }
            })?;
        all_signal_refs.extend_from_slice(&entry.signal_refs);
        signal_cache.insert(signal_name.clone(), entry);
    }

    // Process to-condition signals
    for signal_name in &to_signals {
        if !signal_cache.contains_key(signal_name) {
            let entry = crate::condition::build_signal_cache_entry(hierarchy, signal_name)
                .map_err(|_| WaveAnalyzerError::SignalNotFound {
                    path: signal_name.clone(),
                })?;
            all_signal_refs.extend_from_slice(&entry.signal_refs);
            signal_cache.insert(signal_name.clone(), entry);
        }
    }

    waveform.load_signals(&all_signal_refs);

    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale_ref = waveform.hierarchy().timescale();

    // Evaluate both conditions at each time index in range
    let time_table_len = time_table.len();
    let effective_end = end_idx.min(time_table_len.saturating_sub(1));

    let mut from_matches: Vec<usize> = Vec::new();
    let mut to_matches: Vec<usize> = Vec::new();

    // Track previous condition state for rising-edge detection
    let mut prev_from: Option<bool> = None;
    let mut prev_to: Option<bool> = None;

    for idx in start_idx..=effective_end {
        let from_val =
            crate::condition::evaluate_condition(&from_parsed, waveform, &signal_cache, idx)?;
        let to_val =
            crate::condition::evaluate_condition(&to_parsed, waveform, &signal_cache, idx)?;

        let from_is_true = from_val != BigUint::from(0u32);
        let to_is_true = to_val != BigUint::from(0u32);

        // Only record when condition transitions to true (rising edge)
        if from_is_true && prev_from != Some(true) {
            from_matches.push(idx);
        }
        if to_is_true && prev_to != Some(true) {
            to_matches.push(idx);
        }

        prev_from = Some(from_is_true);
        prev_to = Some(to_is_true);
    }

    // Pair each from-match with the next to-match after it
    let mut intervals: Vec<f64> = Vec::new();
    let mut events: Vec<IntervalEvent> = Vec::new();

    let max_events_limit = max_events.unwrap_or(100);

    for &from_idx in &from_matches {
        // Find the first to_match after from_idx
        let to_idx_opt = to_matches.iter().find(|&t| *t > from_idx);
        if let Some(to_idx) = to_idx_opt {
            let t0_ps = crate::time_map::compute_time_ps_from_table(
                &time_table,
                from_idx,
                timescale_ref.as_ref(),
            );
            let t1_ps = crate::time_map::compute_time_ps_from_table(
                &time_table,
                *to_idx,
                timescale_ref.as_ref(),
            );
            let interval_sec = (t1_ps - t0_ps) as f64 / 1e12; // ps → seconds

            intervals.push(interval_sec);

            if events.len() < max_events_limit {
                events.push(IntervalEvent {
                    from_time_index: from_idx,
                    to_time_index: *to_idx,
                    interval_sec,
                    interval_formatted: format_interval_duration(interval_sec),
                });
            }
        }
    }

    let interval_stats = compute_stats(&intervals);

    let deviation_pct = if let Some(expected) = expected_sec {
        if expected > 0.0 && interval_stats.avg > 0.0 {
            Some((interval_stats.avg - expected) / expected * 100.0)
        } else {
            None
        }
    } else {
        None
    };

    Ok(IntervalMeasurement {
        from_condition: from_condition.to_string(),
        to_condition: to_condition.to_string(),
        interval_stats,
        expected_sec,
        deviation_pct,
        events,
    })
}

/// Format an interval measurement report as human-readable text.
pub fn format_interval_report(report: &IntervalMeasurement) -> String {
    let mut out = ReportWriter::with_capacity(256);

    report_writeln!(out, "=== Interval Measurement ===");
    report_writeln!(out, "From condition: {}", report.from_condition);
    report_writeln!(out, "To condition: {}", report.to_condition);
    report_writeln!(out);

    let s = &report.interval_stats;
    if s.count == 0 {
        report_writeln!(out, "No matching intervals found");
        report_writeln!(
            out,
            "From-condition matches: check that both conditions have overlapping time ranges"
        );
    } else {
        report_writeln!(out, "Intervals measured: {}", s.count);
        report_writeln!(out, "Average interval: {}", format_interval_duration(s.avg));
        report_writeln!(out, "Min interval: {}", format_interval_duration(s.min));
        report_writeln!(out, "Max interval: {}", format_interval_duration(s.max));
        report_writeln!(out, "Stddev: {}", format_interval_duration(s.stddev));

        if let Some(expected) = report.expected_sec {
            report_writeln!(out);
            report_writeln!(out, "Expected: {}", format_interval_duration(expected));
            if let Some(dev) = report.deviation_pct {
                report_writeln!(out, "Deviation: {:.2}%", dev);
            }
        }

        if !report.events.is_empty() {
            report_writeln!(out);
            report_writeln!(out, "Events ({} shown):", report.events.len());
            for (i, e) in report.events.iter().enumerate() {
                report_writeln!(
                    out,
                    "  #{}: from@{} -> to@{} (interval: {})",
                    i + 1,
                    e.from_time_index,
                    e.to_time_index,
                    e.interval_formatted
                );
            }
        }
    }

    out.finish()
}
