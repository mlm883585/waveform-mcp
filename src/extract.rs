//! Signal value extraction and multi-bit reconstruction.
//!
//! Provides two modes:
//! 1. **Single signal extraction**: Extract all value changes of a signal in a time range.
//! 2. **Multi-bit signal reconstruction**: Reconstruct a composite signal from individual bit signals,
//!    generalizing the algorithm from `docs/reference/parse_vcd.py`.

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::{
    format_biguint_value, format_signal_value, format_time, is_signal_high, signal_value_to_biguint,
};
use crate::hierarchy::resolve_signal_with_width;
use crate::summary::downsample_signal_changes;
use num_bigint::BigUint;
use num_traits::Zero;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use wellen::simple::Waveform;

/// Mapping of a single bit position to a signal path.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BitMappingEntry {
    /// Bit position (0 = LSB).
    pub bit_position: u32,
    /// Signal path in the waveform hierarchy.
    pub signal_path: String,
}

/// Request parameters for signal value extraction.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractRequest {
    /// Waveform ID (from open_waveform).
    pub waveform_id: String,
    /// Mode 1: Single signal path to extract.
    pub signal_path: Option<String>,
    /// Mode 2: Bit-to-signal mapping for multi-bit reconstruction.
    /// Each entry maps a bit position to a signal path.
    #[serde(default)]
    pub bit_mapping: Vec<BitMappingEntry>,
    /// Start time index (inclusive). Default: 0.
    /// If start_time_ps is also provided, start_time_ps takes precedence.
    pub start_time_index: Option<usize>,
    /// End time index (inclusive). Default: last time index.
    /// If end_time_ps is also provided, end_time_ps takes precedence.
    pub end_time_index: Option<usize>,
    /// Start time in picoseconds (inclusive). Takes precedence over start_time_index.
    /// Converted to the nearest time index internally.
    pub start_time_ps: Option<u64>,
    /// End time in picoseconds (inclusive). Takes precedence over end_time_index.
    /// Converted to the nearest time index internally.
    pub end_time_ps: Option<u64>,
    /// Value output format: "hex" (default), "binary", "decimal".
    pub value_format: Option<String>,
    /// Maximum number of sample points to return. None = unlimited.
    pub downsample: Option<usize>,
}

/// A single sampled point from signal extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedPoint {
    /// Time index in the waveform.
    pub time_index: u64,
    /// Formatted value string (e.g., "16'h0A3F").
    pub value: String,
}

/// Result of signal value extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResult {
    /// Signal identifier (path or "reconstructed[WIDTH:0]").
    pub signal_name: String,
    /// Signal width in bits.
    pub width: u32,
    /// Total number of changes in the time range (before downsampling).
    pub total_changes: usize,
    /// Number of points returned (after downsampling).
    pub sample_count: usize,
    /// Extracted data points.
    pub points: Vec<ExtractedPoint>,
    /// Timescale string.
    pub timescale: String,
}

/// Extract signal values in the specified time range.
///
/// Supports two modes:
/// - Single signal: extract all value changes of one signal.
/// - Multi-bit reconstruction: combine individual bit signals into a composite value.
///
/// Time range can be specified by time index (`start_time_index`/`end_time_index`)
/// or by picosecond value (`start_time_ps`/`end_time_ps`). The ps values take
/// precedence and are converted to the nearest time index internally.
pub fn extract_signal_values(
    waveform: &mut Waveform,
    request: &ExtractRequest,
) -> WaveResult<ExtractResult> {
    let time_table_len = waveform.time_table().len();

    if time_table_len == 0 {
        return Err(WaveAnalyzerError::Other(
            "Waveform has no time entries".into(),
        ));
    }
    if request.downsample == Some(0) {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "downsample must be greater than 0".into(),
        });
    }

    // Resolve time range: ps values take precedence over index values
    let start_idx = if let Some(start_ps) = request.start_time_ps {
        crate::time_map::find_time_index_by_value(waveform, start_ps).map_err(|e| {
            WaveAnalyzerError::Other(format!("Cannot resolve start_time_ps {}: {}", start_ps, e))
        })?
    } else {
        request.start_time_index.unwrap_or(0)
    };

    let end_idx = if let Some(end_ps) = request.end_time_ps {
        crate::time_map::find_time_index_by_value(waveform, end_ps).map_err(|e| {
            WaveAnalyzerError::Other(format!("Cannot resolve end_time_ps {}: {}", end_ps, e))
        })?
    } else {
        request
            .end_time_index
            .unwrap_or(time_table_len.saturating_sub(1))
    };

    let value_format = request.value_format.as_deref().unwrap_or("hex");

    let timescale = waveform
        .hierarchy()
        .timescale()
        .map(|ts| crate::formatting::format_timescale(&ts))
        .unwrap_or_else(|| "1ns".to_string());

    if request.bit_mapping.is_empty() {
        // Mode 1: Single signal extraction
        extract_single_signal(
            waveform,
            request,
            start_idx,
            end_idx,
            value_format,
            &timescale,
        )
    } else {
        // Mode 2: Multi-bit signal reconstruction
        extract_reconstructed_signal(
            waveform,
            request,
            start_idx,
            end_idx,
            value_format,
            &timescale,
        )
    }
}

/// Extract values from a single signal in a time range.
///
/// For bus signals stored as a single multi-bit variable in VCD, this reads
/// the signal directly. For wire signals decomposed into individual 1-bit
/// variables in VCD (e.g., data[0], data[1], ..., data[7]), this merges
/// all bit-slice change points and reconstructs the composite multi-bit value
/// at each change point — fixing the "wire only rebuilds LSB" BUG.
fn extract_single_signal(
    waveform: &mut Waveform,
    request: &ExtractRequest,
    start_idx: usize,
    end_idx: usize,
    value_format: &str,
    timescale: &str,
) -> WaveResult<ExtractResult> {
    let signal_path = request
        .signal_path
        .as_ref()
        .ok_or(WaveAnalyzerError::InvalidArgument {
            message: "signal_path is required when bit_mapping is not provided".into(),
        })?;

    let hierarchy = waveform.hierarchy();
    let width = crate::hierarchy::get_signal_width(hierarchy, signal_path);

    // Check if the signal is a proper bus (single multi-bit SignalRef)
    // or a bit-slice decomposed wire (multiple 1-bit VarRefs sharing one SignalRef)
    let var_refs = crate::hierarchy::resolve_signal_var_refs(hierarchy, signal_path)
        .or_else(|| crate::hierarchy::find_var_by_path(hierarchy, signal_path).map(|vr| vec![vr]));

    let var_refs = match var_refs {
        Some(refs) => refs,
        None => {
            return Err(WaveAnalyzerError::SignalNotFound {
                path: signal_path.clone(),
            });
        }
    };

    // Single bus variable (len == 1 and width > 1): use direct SignalRef path
    if var_refs.len() == 1 && width > 1 {
        let signal_ref = hierarchy[var_refs[0]].signal_ref();
        waveform.load_signals(&[signal_ref]);

        let signal = waveform
            .get_signal(signal_ref)
            .ok_or(WaveAnalyzerError::Other(
                "Failed to get signal after loading".into(),
            ))?;

        let mut changes: Vec<(u32, wellen::SignalValue)> = Vec::new();
        for (time_idx, value) in signal.iter_changes() {
            let idx = time_idx as usize;
            if idx >= start_idx && idx <= end_idx {
                changes.push((time_idx, value));
            }
        }

        if changes
            .first()
            .is_none_or(|(time_idx, _)| *time_idx as usize > start_idx)
            && let Some(value) = sample_signal_value_at(signal, start_idx)
        {
            changes.insert(0, (start_idx as u32, value));
        }

        let total_changes = changes.len();

        let sampled: Vec<crate::summary::SamplePoint> = if let Some(max) = request.downsample {
            downsample_signal_changes(&changes, max)
        } else {
            changes
                .iter()
                .map(|(time, value)| crate::summary::SamplePoint {
                    time_index: *time as u64,
                    value: format_signal_value(*value),
                })
                .collect()
        };

        let points: Vec<ExtractedPoint> = if request.downsample.is_some() {
            sampled
                .iter()
                .map(|sp| {
                    let value = if sp.value.contains('\'') {
                        let big = crate::formatting::parse_verilog_literal(&sp.value);
                        format_biguint_value(&big, width, value_format)
                    } else if let Ok(big) = sp.value.parse::<num_bigint::BigUint>() {
                        format_biguint_value(&big, width, value_format)
                    } else {
                        sp.value.clone()
                    };
                    ExtractedPoint {
                        time_index: sp.time_index,
                        value,
                    }
                })
                .collect()
        } else {
            changes
                .iter()
                .map(|(time_idx, sv)| {
                    let big = signal_value_to_biguint(*sv, Some(width));
                    ExtractedPoint {
                        time_index: *time_idx as u64,
                        value: format_biguint_value(&big, width, value_format),
                    }
                })
                .collect()
        };

        return Ok(ExtractResult {
            signal_name: signal_path.clone(),
            width,
            total_changes,
            sample_count: points.len(),
            points,
            timescale: timescale.to_string(),
        });
    }

    // 1-bit signal (single var, width == 1): direct path
    if var_refs.len() == 1 {
        let signal_ref = hierarchy[var_refs[0]].signal_ref();
        waveform.load_signals(&[signal_ref]);

        let signal = waveform
            .get_signal(signal_ref)
            .ok_or(WaveAnalyzerError::Other(
                "Failed to get signal after loading".into(),
            ))?;

        let mut changes: Vec<(u32, wellen::SignalValue)> = Vec::new();
        for (time_idx, value) in signal.iter_changes() {
            let idx = time_idx as usize;
            if idx >= start_idx && idx <= end_idx {
                changes.push((time_idx, value));
            }
        }

        if changes
            .first()
            .is_none_or(|(time_idx, _)| *time_idx as usize > start_idx)
            && let Some(value) = sample_signal_value_at(signal, start_idx)
        {
            changes.insert(0, (start_idx as u32, value));
        }

        let total_changes = changes.len();

        let sampled: Vec<crate::summary::SamplePoint> = if let Some(max) = request.downsample {
            downsample_signal_changes(&changes, max)
        } else {
            changes
                .iter()
                .map(|(time, value)| crate::summary::SamplePoint {
                    time_index: *time as u64,
                    value: format_signal_value(*value),
                })
                .collect()
        };

        let points: Vec<ExtractedPoint> = if request.downsample.is_some() {
            sampled
                .iter()
                .map(|sp| {
                    let value = if sp.value.contains('\'') {
                        let big = crate::formatting::parse_verilog_literal(&sp.value);
                        format_biguint_value(&big, width, value_format)
                    } else if let Ok(big) = sp.value.parse::<num_bigint::BigUint>() {
                        format_biguint_value(&big, width, value_format)
                    } else {
                        sp.value.clone()
                    };
                    ExtractedPoint {
                        time_index: sp.time_index,
                        value,
                    }
                })
                .collect()
        } else {
            changes
                .iter()
                .map(|(time_idx, sv)| {
                    let big = signal_value_to_biguint(*sv, Some(width));
                    ExtractedPoint {
                        time_index: *time_idx as u64,
                        value: format_biguint_value(&big, width, value_format),
                    }
                })
                .collect()
        };

        return Ok(ExtractResult {
            signal_name: signal_path.clone(),
            width,
            total_changes,
            sample_count: points.len(),
            points,
            timescale: timescale.to_string(),
        });
    }

    // Bit-slice decomposed multi-bit wire: reconstruct from individual bits
    // This fixes the BUG where extract_signal_values for wire signals only
    // returned LSB values (8'h00/8'h01) because find_signal_by_path returned
    // only a single 1-bit SignalRef from the decomposed set.
    extract_bit_slice_decomposed(
        waveform,
        signal_path,
        &var_refs,
        width,
        start_idx,
        end_idx,
        value_format,
        timescale,
        request.downsample,
    )
}

fn sample_signal_value_at(
    signal: &wellen::Signal,
    time_idx: usize,
) -> Option<wellen::SignalValue<'_>> {
    // Try exact match first
    let time_table_idx: wellen::TimeTableIdx = time_idx.try_into().ok()?;
    if let Some(offset) = signal.get_offset(time_table_idx) {
        return Some(signal.get_value_at(&offset, 0));
    }
    // Fallback: search backwards for the nearest prior offset.
    // This handles the case where the signal has no explicit entry at this
    // exact time table index but does hold a value from a prior change point.
    for prev_idx in (0..time_idx).rev() {
        let prev_tt_idx: wellen::TimeTableIdx = prev_idx.try_into().ok()?;
        if let Some(offset) = signal.get_offset(prev_tt_idx) {
            return Some(signal.get_value_at(&offset, 0));
        }
    }
    None
}

/// Extract values for a bit-slice decomposed multi-bit wire signal.
///
/// When a multi-bit wire signal is stored as individual 1-bit variables
/// in VCD (e.g., `data[0]`, `data[1]`, ..., `data[7]`), this function
/// merges change points from all bit signals, reconstructs the composite
/// multi-bit value at each change point, and formats the result.
///
/// This fixes the BUG where `extract_single_signal` for decomposed wire
/// signals only returned LSB values because `find_signal_by_path` resolved
/// to a single 1-bit SignalRef.
fn extract_bit_slice_decomposed(
    waveform: &mut Waveform,
    signal_path: &str,
    var_refs: &[wellen::VarRef],
    width: u32,
    start_idx: usize,
    end_idx: usize,
    value_format: &str,
    timescale: &str,
    downsample: Option<usize>,
) -> WaveResult<ExtractResult> {
    let hierarchy = waveform.hierarchy();

    // Sort bit VarRefs by MSB descending for reconstruction
    let mut sorted_refs = var_refs.to_vec();
    sorted_refs.sort_by_key(|vr| hierarchy[*vr].index().map(|idx| idx.msb()).unwrap_or(0));
    sorted_refs.reverse();

    let sorted_bit_positions: Vec<(wellen::SignalRef, Option<u32>)> = sorted_refs
        .iter()
        .map(|vr| {
            let var = &hierarchy[*vr];
            (var.signal_ref(), var.index().map(|idx| idx.msb() as u32))
        })
        .collect();

    let signal_refs: Vec<wellen::SignalRef> =
        sorted_bit_positions.iter().map(|(sr, _)| *sr).collect();

    waveform.load_signals(&signal_refs);

    // Collect all change time indices from all bit signals
    let mut change_indices: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    change_indices.insert(start_idx);

    for (sig_ref, _) in &sorted_bit_positions {
        if let Some(signal) = waveform.get_signal(*sig_ref) {
            for (time_idx, _) in signal.iter_changes() {
                let idx = time_idx as usize;
                if idx >= start_idx && idx <= end_idx {
                    change_indices.insert(idx);
                }
            }
        }
    }

    // Reconstruct composite value at each change point
    let mut points: Vec<ExtractedPoint> = Vec::new();
    let mut prev_value: Option<BigUint> = None;

    for time_idx in &change_indices {
        let time_table_idx: wellen::TimeTableIdx = (*time_idx)
            .try_into()
            .map_err(|_| WaveAnalyzerError::Other(format!("Time index {} too large", time_idx)))?;

        let mut composite = BigUint::zero();
        for (sig_ref, bit_pos) in &sorted_bit_positions {
            if let Some(signal) = waveform.get_signal(*sig_ref) {
                if let Some(offset) = signal.get_offset(time_table_idx) {
                    let value = signal.get_value_at(&offset, 0);
                    if is_signal_high(&value) {
                        if let Some(bp) = bit_pos {
                            composite.set_bit(*bp as u64, true);
                        }
                    }
                }
            }
        }

        // Mask to declared width
        if width > 0 && width < 8192 {
            let mask = (BigUint::from(1u32) << width) - BigUint::from(1u32);
            composite &= mask;
        }

        // Only record points where the value changes
        if prev_value.as_ref() != Some(&composite) {
            let formatted = format_biguint_value(&composite, width, value_format);
            points.push(ExtractedPoint {
                time_index: *time_idx as u64,
                value: formatted,
            });
            prev_value = Some(composite);
        }
    }

    let total_changes = points.len();

    // Apply downsampling if requested
    if let Some(max) = downsample
        && points.len() > max
    {
        let step = points.len() / max;
        points = points
            .iter()
            .enumerate()
            .filter(|(i, _)| i % step == 0 || *i == points.len() - 1)
            .map(|(_, p)| p.clone())
            .collect();
    }

    Ok(ExtractResult {
        signal_name: signal_path.to_string(),
        width,
        total_changes,
        sample_count: points.len(),
        points,
        timescale: timescale.to_string(),
    })
}

/// Reconstruct a multi-bit signal from individual bit signals.
///
/// This generalizes the algorithm from parse_vcd.py:
/// 1. Load all bit signals from the waveform.
/// 2. Collect all change points from each bit signal (iter_changes semantics).
/// 3. Merge all change points and sort them.
/// 4. At each change point, read all bit values and combine into a composite integer.
/// 5. Only emit points where the composite value changes.
///
/// This is O(total_changes × m) instead of O(n × m) where n is time points and m is bits.
fn extract_reconstructed_signal(
    waveform: &mut Waveform,
    request: &ExtractRequest,
    start_idx: usize,
    end_idx: usize,
    value_format: &str,
    timescale: &str,
) -> WaveResult<ExtractResult> {
    let bit_mapping = &request.bit_mapping;
    if bit_mapping.is_empty() {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "bit_mapping cannot be empty".into(),
        });
    }

    // Get timescale and time_table before any mutable borrow
    let ts_opt = waveform.hierarchy().timescale();
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();

    // Validate signals and determine width
    let max_bit = bit_mapping
        .iter()
        .map(|e| e.bit_position)
        .max()
        .unwrap_or(0);
    let width = max_bit + 1;

    let mut signal_refs: Vec<wellen::SignalRef> = Vec::new();
    let mut bit_positions: Vec<u32> = Vec::new();
    let mut signal_widths: Vec<u32> = Vec::new(); // Track width per entry for multi-bit handling

    {
        let hierarchy = waveform.hierarchy();
        for entry in bit_mapping {
            let (signal_ref, signal_width) =
                resolve_signal_with_width(hierarchy, &entry.signal_path).map_err(|e| {
                    WaveAnalyzerError::Other(format!(
                        "Signal not found for bit {}: {}",
                        entry.bit_position, e
                    ))
                })?;

            // Allow multi-bit signals in bit_mapping — we extract the specific bit
            // from the multi-bit value rather than requiring 1-bit only
            signal_refs.push(signal_ref);
            bit_positions.push(entry.bit_position);
            signal_widths.push(signal_width);
        }
    }

    waveform.load_signals(&signal_refs);

    // Build a list of (SignalRef, bit_position, signal_width) tuples
    let loaded_signals: Vec<(wellen::SignalRef, u32, u32)> = signal_refs
        .iter()
        .zip(bit_positions.iter())
        .zip(signal_widths.iter())
        .map(|((sr, bp), w)| (*sr, *bp, *w))
        .collect();

    // Collect all change points from each bit signal (iter_changes semantics)
    // This is the key optimization: instead of iterating through all time indices,
    // we only iterate through time indices where at least one bit changes.
    let mut all_change_points: std::collections::BTreeSet<usize> =
        std::collections::BTreeSet::new();
    all_change_points.insert(start_idx); // Always include start point

    for (sig_ref, _, _) in &loaded_signals {
        let signal = waveform
            .get_signal(*sig_ref)
            .ok_or(WaveAnalyzerError::Other(
                "Signal not found after loading".into(),
            ))?;

        for (time_idx, _) in signal.iter_changes() {
            let idx = time_idx as usize;
            if idx >= start_idx && idx <= end_idx {
                all_change_points.insert(idx);
            }
        }
    }

    // Iterate through change points and reconstruct composite value at each point
    let mut points: Vec<ExtractedPoint> = Vec::new();
    let mut prev_value: Option<BigUint> = None;

    for time_idx in all_change_points {
        if time_idx >= time_table.len() {
            break;
        }

        let time_table_idx: wellen::TimeTableIdx = time_idx
            .try_into()
            .map_err(|_| WaveAnalyzerError::Other(format!("Time index {} too large", time_idx)))?;

        // Read all bit values at this change point
        let mut composite = BigUint::zero();

        for (sig_ref, bit_pos, sig_width) in &loaded_signals {
            let signal = waveform
                .get_signal(*sig_ref)
                .ok_or(WaveAnalyzerError::Other(
                    "Signal not found after loading".into(),
                ))?;

            if let Some(offset) = signal.get_offset(time_table_idx) {
                // For both single-bit and multi-bit signals, use element 0
                // (wellen stores packed bus values as one element)
                let value = signal.get_value_at(&offset, 0);
                if *sig_width == 1 {
                    // Single-bit signal: use is_signal_high helper
                    let bit_set = is_signal_high(&value);
                    if bit_set {
                        composite.set_bit(*bit_pos as u64, true);
                    }
                } else {
                    // Multi-bit signal: extract specific bit from the packed value
                    let value_big = signal_value_to_biguint(value, None);
                    // For bit-mapping, we need to extract the bit at the requested position
                    // The bit_pos in bit-mapping refers to the output composite bit position.
                    // We need to find which bit of the source signal to extract.
                    // Since we resolved signal[bitN] to the full bus, extract bit N from the bus value.
                    // The mapping "0=signal[0],1=signal[1],2=signal[2]" means:
                    //   output bit 0 = signal bit 0, output bit 1 = signal bit 1, etc.
                    // For a 3-bit bus stored as value_big, bit 0 is LSB.
                    // We need to find which source bit index corresponds to this bit_pos.
                    // The BitMappingEntry stores the bit position as the output position,
                    // but we also need the source bit index.
                    // Since entry.signal_path ends with [bitN], the source bit is bitN.
                    // However, we currently only have bit_pos in loaded_signals.
                    // For now, use the simple approach: bit_pos maps directly to source bit
                    // when the mapping is "0=signal[0],1=signal[1],2=signal[2]"
                    if value_big.bit(*bit_pos as u64) {
                        composite.set_bit(*bit_pos as u64, true);
                    }
                }
            }
        }

        // Only record points where the value changes
        if prev_value.as_ref() != Some(&composite) {
            let formatted = format_biguint_value(&composite, width, value_format);
            let time_value = time_table[time_idx];
            let formatted_time = format_time(time_value, ts_opt.as_ref());

            points.push(ExtractedPoint {
                time_index: time_idx as u64,
                value: format!("{} ({})", formatted, formatted_time),
            });
            prev_value = Some(composite);
        }
    }

    let total_changes = points.len();

    // Apply downsampling if requested
    if let Some(max) = request.downsample
        && points.len() > max
    {
        let step = points.len() / max;
        points = points
            .iter()
            .enumerate()
            .filter(|(i, _)| i % step == 0 || *i == points.len() - 1)
            .map(|(_, p)| p.clone())
            .collect();
    }

    // Build signal name from bit mapping
    let signal_name = format!("reconstructed[{}:0]", max_bit);

    Ok(ExtractResult {
        signal_name,
        width,
        total_changes,
        sample_count: points.len(),
        points,
        timescale: timescale.to_string(),
    })
}
