//! Signal reading and querying utilities.

use crate::error::{WaveAnalyzerError, WaveResult};
use num_bigint::BigUint;
use num_traits::Zero;
use std::collections::HashSet;
use wellen;

use super::{
    formatting::format_biguint_verilog, formatting::format_signal_value, formatting::format_time,
    formatting::is_signal_high, hierarchy::classify_var_type,
    hierarchy::collect_signals_from_scope, hierarchy::find_scope_by_path,
    hierarchy::find_var_by_path, hierarchy::find_var_ref_by_path, hierarchy::get_signal_width,
    hierarchy::resolve_signal_var_refs,
};

/// List signals in a waveform hierarchy with optional filtering.
///
/// # Arguments
/// * `hierarchy` - The waveform hierarchy to search
/// * `name_pattern` - Optional regex pattern filter for signal names (e.g., ".*clk", "^TOP\\.rst")
/// * `hierarchy_prefix` - Optional hierarchy path prefix to filter signals (must match a scope)
/// * `recursive` - If true, list all signals recursively; if false, only list signals at specified level
/// * `limit` - Optional maximum number of signals to return. Use -1 for unlimited.
///
/// # Returns
/// A vector of signal paths, or an error if the regex pattern is invalid.
pub fn list_signals(
    hierarchy: &wellen::Hierarchy,
    name_pattern: Option<&str>,
    hierarchy_prefix: Option<&str>,
    recursive: bool,
    limit: Option<isize>,
) -> WaveResult<Vec<String>> {
    let mut signals = Vec::new();

    if let Some(prefix) = hierarchy_prefix {
        // Find scope by path
        if let Some(scope_ref) = find_scope_by_path(hierarchy, prefix) {
            // Collect signals from this scope (and children if recursive)
            signals = collect_signals_from_scope(hierarchy, scope_ref, recursive, name_pattern)?;
        }
        // If scope not found, return empty signals
    } else {
        // No hierarchy prefix - start from top-level scopes
        for scope_ref in hierarchy.scopes() {
            signals.extend(collect_signals_from_scope(
                hierarchy,
                scope_ref,
                recursive,
                name_pattern,
            )?);
        }
    }

    let mut seen = HashSet::new();
    signals.retain(|path| seen.insert(path.clone()));

    // Apply limit if provided and not -1 (unlimited)
    if let Some(limit) = limit
        && limit >= 0
    {
        signals.truncate(limit as usize);
    }

    Ok(signals)
}

/// Read signal values at specific time indices.
///
/// # Arguments
/// * `waveform` - The waveform to read from (must have signal loaded)
/// * `signal_ref` - The signal reference to read
/// * `time_indices` - The time indices to read values at
/// * `declared_width` - The signal's declared bit width from hierarchy (0 = use wellen's bits)
///
/// # Returns
/// A vector of formatted signal value strings, or an error if the operation fails.
pub fn read_signal_values(
    waveform: &wellen::simple::Waveform,
    signal_ref: wellen::SignalRef,
    time_indices: &[usize],
    declared_width: u32,
) -> WaveResult<Vec<String>> {
    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();

    let signal = waveform
        .get_signal(signal_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Signal not found after loading".into(),
        ))?;
    let mut error_count = 0;
    let mut results = Vec::new();

    for time_idx in time_indices {
        if *time_idx >= time_table.len() {
            results.push(format!(
                "ERROR: Time index {} out of range (max: {})",
                time_idx,
                time_table.len() - 1
            ));
            error_count += 1;
            continue;
        }

        let time_value = time_table[*time_idx];
        let formatted_time = format_time(time_value, timescale.as_ref());

        let time_table_idx: wellen::TimeTableIdx = (*time_idx).try_into().map_err(|_| {
            WaveAnalyzerError::Other(format!("Time index {} exceeds maximum value", time_idx))
        })?;

        let offset = signal
            .get_offset(time_table_idx)
            .ok_or(WaveAnalyzerError::Other(
                "No data available for this time index".into(),
            ))?;

        let signal_value = signal.get_value_at(&offset, 0);
        let width_override = if declared_width > 0 {
            Some(declared_width)
        } else {
            None
        };
        let value_str =
            crate::formatting::format_signal_value_with_width(signal_value, width_override);

        results.push(format!(
            "Time index {} ({}): {}",
            time_idx, formatted_time, value_str
        ));
    }

    // If any indices were out of bounds, return error
    if error_count > 0 {
        return Err(WaveAnalyzerError::Other(format!(
            "{} of {} requested time indices are out of range (max: {})",
            error_count,
            time_indices.len(),
            time_table.len() - 1
        )));
    }

    Ok(results)
}

pub fn read_signal_values_by_path(
    waveform: &mut wellen::simple::Waveform,
    signal_path: &str,
    time_indices: &[usize],
) -> WaveResult<Vec<String>> {
    let (var_refs, width) = {
        let hierarchy = waveform.hierarchy();
        let var_refs = resolve_signal_var_refs(hierarchy, signal_path).ok_or_else(|| {
            WaveAnalyzerError::SignalNotFound {
                path: signal_path.to_string(),
            }
        })?;
        let width = crate::hierarchy::get_signal_width(hierarchy, signal_path);
        (var_refs, width)
    };

    if var_refs.len() == 1 {
        let signal_ref = waveform.hierarchy()[var_refs[0]].signal_ref();
        waveform.load_signals(&[signal_ref]);
        return read_signal_values(waveform, signal_ref, time_indices, width);
    }

    let signal_refs: Vec<_> = {
        let hierarchy = waveform.hierarchy();
        var_refs
            .iter()
            .map(|var_ref| hierarchy[*var_ref].signal_ref())
            .collect()
    };
    waveform.load_signals(&signal_refs);

    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();
    let mut results = Vec::new();
    let mut error_count = 0;

    for time_idx in time_indices {
        if *time_idx >= time_table.len() {
            results.push(format!(
                "ERROR: Time index {} out of range (max: {})",
                time_idx,
                time_table.len() - 1
            ));
            error_count += 1;
            continue;
        }

        let mut value = BigUint::from(0u32);
        for var_ref in &var_refs {
            let hierarchy = waveform.hierarchy();
            let var = &hierarchy[*var_ref];
            let signal = waveform
                .get_signal(var.signal_ref())
                .ok_or(WaveAnalyzerError::Other(
                    "Signal not found after loading".into(),
                ))?;
            let time_table_idx: wellen::TimeTableIdx = (*time_idx).try_into().map_err(|_| {
                WaveAnalyzerError::Other(format!("Time index {} exceeds maximum value", time_idx))
            })?;
            let offset = signal
                .get_offset(time_table_idx)
                .ok_or(WaveAnalyzerError::Other(
                    "No data available for this time index".into(),
                ))?;
            let signal_value = signal.get_value_at(&offset, 0);
            let bit_is_high = crate::formatting::is_signal_high(&signal_value);
            let bit_index = var.index().map(|idx| idx.lsb()).unwrap_or(0);
            if bit_is_high && bit_index >= 0 {
                value |= BigUint::from(1u32) << (bit_index as usize);
            }
        }

        let bytes = value.to_bytes_be();
        let byte_buf = if bytes.is_empty() { vec![0] } else { bytes };
        let value_str = crate::formatting::format_signal_value_with_width(
            wellen::SignalValue::Binary(&byte_buf, width),
            Some(width),
        );
        let formatted_time = format_time(time_table[*time_idx], timescale.as_ref());
        results.push(format!(
            "Time index {} ({}): {}",
            time_idx, formatted_time, value_str
        ));
    }

    if error_count > 0 {
        return Err(WaveAnalyzerError::Other(format!(
            "{} of {} requested time indices are out of range (max: {})",
            error_count,
            time_indices.len(),
            time_table.len() - 1
        )));
    }

    Ok(results)
}

/// Get metadata about a signal.
///
/// # Arguments
/// * `hierarchy` - The waveform hierarchy
/// * `signal_path` - The hierarchical path to the signal
///
/// # Returns
/// A formatted string containing signal metadata, or an error if the signal is not found.
pub fn get_signal_metadata(hierarchy: &wellen::Hierarchy, signal_path: &str) -> WaveResult<String> {
    let var_refs = resolve_signal_var_refs(hierarchy, signal_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.to_string(),
        }
    })?;

    if var_refs.len() > 1 {
        let width = crate::hierarchy::get_signal_width(hierarchy, signal_path);
        let msb = var_refs
            .iter()
            .filter_map(|var_ref| hierarchy[*var_ref].index().map(|idx| idx.msb()))
            .max()
            .unwrap_or((width.saturating_sub(1)) as i64);
        let lsb = var_refs
            .iter()
            .filter_map(|var_ref| hierarchy[*var_ref].index().map(|idx| idx.lsb()))
            .min()
            .unwrap_or(0);
        let var_type = hierarchy[var_refs[0]].var_type();

        return Ok(format!(
            "Signal: {}\nType: {}\nWidth: {} bits\nIndex: [{}:{}]",
            signal_path,
            classify_var_type(var_type),
            width,
            msb,
            lsb
        ));
    }

    let var_ref = find_var_ref_by_path(hierarchy, signal_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: signal_path.to_string(),
        }
    })?;

    let var = &hierarchy[var_ref];

    let width_info = match var.length() {
        Some(len) => format!("{} bits", len),
        None => "variable length (string/real)".to_string(),
    };

    let index_info = match var.index() {
        Some(idx) => format!("[{}:{}]", idx.msb(), idx.lsb()),
        // BUG-1bit-index fix: 1-bit signals without explicit index implicitly
        // have [0:0]. Showing "N/A" was confusing — it implies the signal
        // has no bit range, which is not true for 1-bit signals.
        None => {
            let width = var.length().unwrap_or(1);
            if width == 1 {
                "[0:0]".to_string()
            } else {
                "N/A".to_string()
            }
        }
    };

    let type_str = classify_var_type(var.var_type());

    let info = format!(
        "Signal: {}\nType: {}\nWidth: {}\nIndex: {}",
        signal_path, type_str, width_info, index_info
    );

    Ok(info)
}

/// Find events (changes) of a signal within a time range.
///
/// # Arguments
/// * `waveform` - The waveform to read from (must have signal loaded)
/// * `signal_ref` - The signal reference to analyze
/// * `start_idx` - Starting time index (inclusive)
/// * `end_idx` - Ending time index (inclusive)
/// * `limit` - Maximum number of events to return. Use -1 for unlimited.
///
/// # Returns
/// A vector of formatted event strings, or an error if the operation fails.
pub fn find_signal_events(
    waveform: &wellen::simple::Waveform,
    signal_ref: wellen::SignalRef,
    start_idx: usize,
    end_idx: usize,
    limit: isize,
) -> WaveResult<Vec<String>> {
    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();

    let signal = waveform
        .get_signal(signal_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Signal not found after loading".into(),
        ))?;

    let mut events = Vec::new();

    for (time_idx, signal_value) in signal.iter_changes() {
        let time_idx = time_idx as usize;

        // Check if within time range
        if time_idx < start_idx || time_idx > end_idx {
            continue;
        }

        // Check limit (unless unlimited with -1)
        if limit >= 0 && events.len() >= limit as usize {
            break;
        }

        let time_value = time_table[time_idx];
        let formatted_time = format_time(time_value, timescale.as_ref());
        let value_str = format_signal_value(signal_value);

        events.push(format!(
            "Time index {} ({}): {}",
            time_idx, formatted_time, value_str
        ));
    }

    Ok(events)
}

/// Find events (changes) of a signal by path, correctly handling
/// multi-bit wire signals that are decomposed into individual 1-bit
/// variables in VCD (BUG-10 fix).
///
/// For proper bus signals (stored as a single multi-bit $var), this
/// delegates to `find_signal_events` using a single SignalRef.
///
/// For bit-slice decomposed signals (stored as individual `sig[0]`,
/// `sig[1]`, ..., `sig[N]`), this merges change times from all bit
/// signals and reconstructs the composite multi-bit value at each
/// change point, producing correct width-formatted output.
///
/// # Arguments
/// * `waveform` - The waveform (mutable, to load signals)
/// * `signal_path` - Hierarchical path to the signal
/// * `start_idx` - Starting time index (inclusive)
/// * `end_idx` - Ending time index (inclusive)
/// * `limit` - Maximum number of events to return. Use -1 for unlimited.
pub fn find_signal_events_by_path(
    waveform: &mut wellen::simple::Waveform,
    signal_path: &str,
    start_idx: usize,
    end_idx: usize,
    limit: isize,
) -> WaveResult<Vec<String>> {
    // Resolve signal and prepare data before loading (avoids borrow conflicts)
    let (var_refs, width, signal_refs, sorted_bit_positions) = {
        let hierarchy = waveform.hierarchy();
        let var_refs = resolve_signal_var_refs(hierarchy, signal_path)
            .or_else(|| find_var_by_path(hierarchy, signal_path).map(|vr| vec![vr]));

        let var_refs = match var_refs {
            Some(refs) => refs,
            None => {
                return Err(WaveAnalyzerError::SignalNotFound {
                    path: signal_path.into(),
                });
            }
        };

        let width = get_signal_width(hierarchy, signal_path);

        // Single bus variable: return early with single signal ref
        if var_refs.len() == 1 {
            let signal_ref = hierarchy[var_refs[0]].signal_ref();
            (var_refs, width, vec![signal_ref], Vec::new())
        } else {
            let signal_refs: Vec<wellen::SignalRef> = var_refs
                .iter()
                .map(|vr| hierarchy[*vr].signal_ref())
                .collect();

            // Sort bit VarRefs by MSB descending for reconstruction
            let mut sorted_refs = var_refs.clone();
            sorted_refs.sort_by_key(|vr| hierarchy[*vr].index().map(|idx| idx.msb()).unwrap_or(0));
            sorted_refs.reverse();

            let sorted_bit_positions: Vec<(wellen::SignalRef, Option<u32>)> = sorted_refs
                .iter()
                .map(|vr| {
                    let var = &hierarchy[*vr];
                    (var.signal_ref(), var.index().map(|idx| idx.msb() as u32))
                })
                .collect();

            (var_refs, width, signal_refs, sorted_bit_positions)
        }
    };

    let time_table_owned: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale = waveform.hierarchy().timescale();

    // Single bus variable: use existing find_signal_events
    if var_refs.len() == 1 {
        waveform.load_signals(&signal_refs);
        return find_signal_events(waveform, signal_refs[0], start_idx, end_idx, limit);
    }

    // Bit-slice decomposed: load signals
    waveform.load_signals(&signal_refs);

    // Collect all change time indices from all bit signals
    let mut change_indices: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for sig_ref in &signal_refs {
        if let Some(signal) = waveform.get_signal(*sig_ref) {
            for (time_idx, _) in signal.iter_changes() {
                let idx = time_idx as usize;
                if idx >= start_idx && idx <= end_idx {
                    change_indices.insert(idx);
                }
            }
        }
    }

    let mut events = Vec::new();

    for time_idx in &change_indices {
        if limit >= 0 && events.len() >= limit as usize {
            break;
        }

        let time_table_idx: wellen::TimeTableIdx = (*time_idx)
            .try_into()
            .map_err(|_| WaveAnalyzerError::Other(format!("Time index {} too large", time_idx)))?;

        // Reconstruct composite value from individual bits
        let mut composite = BigUint::zero();
        for (sig_ref, bit_pos) in &sorted_bit_positions {
            if let Some(signal) = waveform.get_signal(*sig_ref) {
                if let Some(offset) = signal.get_offset(time_table_idx) {
                    let value = signal.get_value_at(&offset, 0);
                    let bit_is_high = is_signal_high(&value);
                    if bit_is_high {
                        if let Some(bp) = bit_pos {
                            composite.set_bit(*bp as u64, true);
                        }
                    }
                }
            }
        }

        // Mask to declared width
        if width > 0 {
            let mask = (BigUint::from(1u32) << width) - BigUint::from(1u32);
            composite &= mask;
        }

        let time_value = time_table_owned[*time_idx];
        let formatted_time = format_time(time_value, timescale.as_ref());
        let value_str = format_biguint_verilog(&composite, width);

        events.push(format!(
            "Time index {} ({}): {}",
            time_idx, formatted_time, value_str
        ));
    }

    Ok(events)
}
