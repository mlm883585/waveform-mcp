use wave_analyzer_mcp::protocol::{
    analyze_handshake_with_level_sensitive, format_clock_report, format_handshake_report,
    format_interval_report, format_pulse_report, measure_clock, measure_intervals, measure_pulses,
};
use wave_analyzer_mcp::time_map::time_value_to_ps;
use wave_analyzer_mcp::{
    BitMappingEntry, Command, CompareSignalRef, ExtractRequest, SignalEntry, auto_discover_signals,
    build_multi_signal_timeline, compare_signals_values, compute_and_verify_crc, detect_sequence,
    extract_signal_values, format_compare_report, format_crc_report, format_discovery_report,
    format_sequence_report, format_timeline_report,
};

use super::CliStore;

pub(super) fn exec_extract(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::ExtractSignalValues {
        waveform_id,
        signal_path,
        bit_mapping,
        start_time_index,
        end_time_index,
        start_time_value,
        end_time_value,
        time_unit,
        value_format,
        downsample,
    } = cmd
    else {
        unreachable!("exec_extract only handles ExtractSignalValues");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    // Parse bit_mapping string: "0=path0,1=path1,..."
    let parsed_bit_mapping: Vec<BitMappingEntry> = if !bit_mapping.is_empty() {
        bit_mapping
            .split(',')
            .filter_map(|pair| {
                let parts: Vec<&str> = pair.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let bit_pos = parts[0].parse::<u32>().ok()?;
                    Some(BitMappingEntry {
                        bit_position: bit_pos,
                        signal_path: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    // Convert time values to picoseconds if provided
    let start_time_ps = if let (Some(tv), Some(tu)) = (start_time_value, time_unit) {
        Some(time_value_to_ps(*tv, tu)?)
    } else {
        None
    };
    let end_time_ps = if let (Some(tv), Some(tu)) = (end_time_value, time_unit) {
        Some(time_value_to_ps(*tv, tu)?)
    } else {
        None
    };

    let request = ExtractRequest {
        waveform_id: waveform_id.clone(),
        signal_path: signal_path.clone(),
        bit_mapping: parsed_bit_mapping,
        start_time_index: *start_time_index,
        end_time_index: *end_time_index,
        start_time_ps,
        end_time_ps,
        value_format: value_format.clone(),
        downsample: *downsample,
    };

    let result = extract_signal_values(waveform, &request)
        .map_err(|e| format!("Error extracting signal values: {}", e))?;

    // Build formatted table output
    let mut lines = Vec::new();
    lines.push(format!(
        "Signal: {}, Width: {} bits",
        result.signal_name, result.width
    ));
    lines.push(format!(
        "Total changes: {}, Returned: {}",
        result.total_changes, result.sample_count
    ));
    lines.push(format!("Timescale: {}", result.timescale));
    lines.push("-".repeat(60));
    lines.push(format!("{:<15} {:<20}", "Time Index", "Value"));
    lines.push("-".repeat(60));

    for point in &result.points {
        lines.push(format!("{:<15} {}", point.time_index, point.value));
    }

    Ok(lines.join("\n"))
}

pub(super) fn exec_handshake(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AnalyzeHandshake {
        waveform_id,
        valid_signal,
        ready_signal,
        data_signal,
        start_time_index,
        end_time_index,
        limit,
        report_mode,
        filter_zero_delay,
        level_sensitive,
    } = cmd
    else {
        unreachable!("exec_handshake only handles AnalyzeHandshake");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));
    let lim = limit.unwrap_or(-1);
    let mode = report_mode.as_deref().unwrap_or("summary");
    let fzd = filter_zero_delay.unwrap_or(false);
    let level_sensitive = level_sensitive.unwrap_or(false);

    let result = analyze_handshake_with_level_sensitive(
        waveform,
        valid_signal,
        ready_signal,
        data_signal.as_deref(),
        start_idx,
        end_idx,
        Some(lim),
        mode,
        fzd,
        level_sensitive,
    )
    .map_err(|e| format!("Error analyzing handshake: {}", e))?;

    Ok(format_handshake_report(&result))
}

pub(super) fn exec_measure(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::MeasureSignal {
        waveform_id,
        signal_path,
        analysis_type,
        start_time_index,
        end_time_index,
        edge_type,
        from_condition,
        to_condition,
        expected_value,
        expected_unit,
    } = cmd
    else {
        unreachable!("exec_measure only handles MeasureSignal");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));

    if analysis_type == "clock" {
        let et = edge_type.as_deref().unwrap_or("posedge");
        let result = measure_clock(waveform, signal_path, et, start_idx, end_idx)
            .map_err(|e| format!("Error measuring clock: {}", e))?;

        Ok(format_clock_report(&result))
    } else if analysis_type == "pulse" {
        let result = measure_pulses(waveform, signal_path, start_idx, end_idx)
            .map_err(|e| format!("Error measuring pulses: {}", e))?;

        Ok(format_pulse_report(&result))
    } else {
        // interval mode
        let from = from_condition
            .as_deref()
            .ok_or_else(|| "--from-condition is required for interval mode".to_string())?;
        let to = to_condition
            .as_deref()
            .ok_or_else(|| "--to-condition is required for interval mode".to_string())?;

        let expected_sec = if let (Some(ev), Some(eu)) = (expected_value, expected_unit) {
            let ps = time_value_to_ps(*ev, eu)?;
            Some(ps as f64 / 1e12) // ps → seconds
        } else {
            None
        };

        let result = measure_intervals(
            waveform,
            from,
            to,
            start_idx,
            end_idx,
            expected_sec,
            Some(100),
        )
        .map_err(|e| format!("Error measuring intervals: {}", e))?;

        Ok(format_interval_report(&result))
    }
}

pub(super) fn exec_compare(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::CompareSignals {
        waveform_id,
        signals,
        comparison_mode,
        start_time_index,
        end_time_index,
        limit,
        value_format,
    } = cmd
    else {
        unreachable!("exec_compare only handles CompareSignals");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));
    let lim = *limit;
    let fmt = value_format.as_deref().unwrap_or("hex");

    // Parse signals: comma-separated paths
    let signal_refs: Vec<CompareSignalRef> = signals
        .split(',')
        .map(|s| CompareSignalRef {
            signal_path: Some(s.trim().to_string()),
            bit_mapping: vec![],
            alias: None,
        })
        .collect();

    let result = compare_signals_values(
        waveform,
        &signal_refs,
        comparison_mode,
        start_idx,
        end_idx,
        fmt,
        lim,
        0, // tolerance: 0 by default, BUG-26 fix adds this parameter
    )
    .map_err(|e| format!("Error comparing signals: {}", e))?;

    Ok(format_compare_report(&result))
}

pub(super) fn exec_timeline(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::MultiSignalTimeline {
        waveform_id,
        signals,
        merge_mode,
        start_time_index,
        end_time_index,
        limit,
        value_format,
    } = cmd
    else {
        unreachable!("exec_timeline only handles MultiSignalTimeline");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));
    let lim = *limit;
    let fmt = value_format.as_deref().unwrap_or("hex");

    // Parse signals: comma-separated paths
    let signal_entries: Vec<SignalEntry> = signals
        .split(',')
        .map(|s| SignalEntry {
            signal_path: Some(s.trim().to_string()),
            bit_mapping: vec![],
            alias: None,
        })
        .collect();

    let result = build_multi_signal_timeline(
        waveform,
        &signal_entries,
        start_idx,
        end_idx,
        merge_mode,
        fmt,
        lim,
    )
    .map_err(|e| format!("Error building timeline: {}", e))?;

    Ok(format_timeline_report(&result))
}

pub(super) fn exec_discover(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AutoDiscoverSignals {
        waveform_id,
        discovery_mode,
        scope_path,
        pattern,
        limit,
    } = cmd
    else {
        unreachable!("exec_discover only handles AutoDiscoverSignals");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let mode = discovery_mode.as_deref().unwrap_or("bus_slices");
    let scope = scope_path.as_deref();
    let pat = pattern.as_deref();

    let result = auto_discover_signals(waveform, mode, scope, pat, *limit)
        .map_err(|e| format!("Error discovering signals: {}", e))?;

    Ok(format_discovery_report(&result))
}

pub(super) fn exec_sequence(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::DetectSequence {
        waveform_id,
        sequence,
        max_gap_cycles,
        start_time_index,
        end_time_index,
        limit,
    } = cmd
    else {
        unreachable!("exec_sequence only handles DetectSequence");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));

    let result = detect_sequence(
        waveform,
        sequence,
        *max_gap_cycles,
        start_idx,
        end_idx,
        *limit,
    )
    .map_err(|e| format!("Error detecting sequence: {}", e))?;

    Ok(format_sequence_report(&result))
}

pub(super) fn exec_crc(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::ComputeCrc {
        waveform_id,
        data_signal_path,
        crc_signal_path,
        data_valid_signal_path,
        clear_signal_path,
        clock_signal_path,
        crc_polynomial,
        initial_value,
        start_time_index,
        end_time_index,
        limit,
    } = cmd
    else {
        unreachable!("exec_crc only handles ComputeCrc");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table_len.saturating_sub(1));

    let result = compute_and_verify_crc(
        waveform,
        data_signal_path,
        crc_signal_path.as_deref(),
        data_valid_signal_path.as_deref(),
        clear_signal_path.as_deref(),
        clock_signal_path.as_deref(),
        crc_polynomial,
        *initial_value,
        start_idx,
        end_idx,
        *limit,
    )
    .map_err(|e| format!("Error computing CRC: {}", e))?;

    Ok(format_crc_report(&result))
}
