use wave_analyzer_mcp::formatting::format_time;
use wave_analyzer_mcp::time_map::{
    compute_time_ps_from_table, find_time_index_by_value, time_value_to_ps,
};
use wave_analyzer_mcp::{
    find_conditional_events, find_signal_events_by_path, get_signal_metadata, list_signals,
    read_hierarchy, read_signal_values_by_path,
};

use super::CliStore;

pub(super) fn exec_open_waveform(
    store: &mut CliStore,
    file_path: &str,
    alias: Option<String>,
) -> Result<String, String> {
    let id = store.open_waveform(file_path, alias)?;
    Ok(format!("Waveform opened successfully with id: {}", id))
}

pub(super) fn exec_close_waveform(
    store: &mut CliStore,
    waveform_id: &str,
) -> Result<String, String> {
    store.close_waveform(waveform_id)?;
    Ok(format!("Waveform '{}' closed successfully", waveform_id))
}

pub(super) fn exec_list_signals(
    store: &mut CliStore,
    waveform_id: &str,
    name_pattern: Option<&str>,
    hierarchy_prefix: Option<&str>,
    recursive: bool,
    limit: Option<isize>,
) -> Result<String, String> {
    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    if matches!(limit, Some(0)) {
        return Err("Invalid limit '0': limit must be greater than 0".to_string());
    }

    let hierarchy = waveform.hierarchy();
    let signals = list_signals(hierarchy, name_pattern, hierarchy_prefix, recursive, limit)?;

    Ok(format!(
        "Found {} signals:\n{}",
        signals.len(),
        signals.join("\n")
    ))
}

pub(super) fn exec_read_hierarchy(
    store: &mut CliStore,
    waveform_id: &str,
    scope_path: Option<&str>,
    recursive: bool,
    limit: Option<isize>,
) -> Result<String, String> {
    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let hierarchy = waveform.hierarchy();
    let lines = read_hierarchy(hierarchy, scope_path, recursive, limit)
        .map_err(|e| format!("Error reading hierarchy: {}", e))?;

    let header = match scope_path {
        Some(path) => format!("Hierarchy rooted at '{}':", path),
        None => "Hierarchy:".to_string(),
    };
    let body = if lines.is_empty() {
        "No modules found".to_string()
    } else {
        lines.join("\n")
    };

    Ok(format!("{}\n{}", header, body))
}

pub(super) fn exec_read_signal(
    store: &mut CliStore,
    waveform_id: &str,
    signal_path: &str,
    time_index: Option<usize>,
    time_indices: Option<&Vec<usize>>,
) -> Result<String, String> {
    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let indices_to_read: Vec<usize> = if let Some(indices) = time_indices {
        indices.clone()
    } else if let Some(index) = time_index {
        vec![index]
    } else {
        return Err("Either time_index or time_indices must be provided".to_string());
    };

    let results = read_signal_values_by_path(waveform, signal_path, &indices_to_read)
        .map_err(|e| format!("Error reading signal: {}", e))?;

    Ok(results.join("\n"))
}

pub(super) fn exec_get_signal_info(
    store: &mut CliStore,
    waveform_id: &str,
    signal_path: &str,
) -> Result<String, String> {
    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let hierarchy = waveform.hierarchy();
    let info = get_signal_metadata(hierarchy, signal_path)
        .map_err(|e| format!("Error getting signal info: {}", e))?;

    Ok(info)
}

pub(super) fn exec_find_events(
    store: &mut CliStore,
    waveform_id: &str,
    signal_path: &str,
    start_time_index: Option<usize>,
    end_time_index: Option<usize>,
    start_time_value: Option<f64>,
    end_time_value: Option<f64>,
    time_unit: Option<&str>,
    limit: Option<isize>,
) -> Result<String, String> {
    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table_len = waveform.time_table().len();

    // Resolve time_value → time_index if physical time is provided
    let start_idx = if let (Some(tv), Some(tu)) = (start_time_value, time_unit) {
        let ps = time_value_to_ps(tv, tu)?;
        find_time_index_by_value(waveform, ps)?
    } else {
        start_time_index.unwrap_or(0)
    };

    let end_idx = if let (Some(tv), Some(tu)) = (end_time_value, time_unit) {
        let ps = time_value_to_ps(tv, tu)?;
        find_time_index_by_value(waveform, ps)?
    } else {
        end_time_index.unwrap_or(time_table_len.saturating_sub(1))
    };

    let lim = limit.unwrap_or(-1);

    // BUG-10 fix: use find_signal_events_by_path which correctly handles
    // multi-bit wire signals decomposed into individual 1-bit variables.
    let events = find_signal_events_by_path(waveform, signal_path, start_idx, end_idx, lim)
        .map_err(|e| format!("Error finding signal events: {}", e))?;

    Ok(format!(
        "Found {} events for signal '{}' (time range: {} to {}):\n{}",
        events.len(),
        signal_path,
        start_idx,
        end_idx,
        events.join("\n")
    ))
}

pub(super) fn exec_find_conditional_events(
    store: &mut CliStore,
    waveform_id: &str,
    condition: &str,
    start_time_index: Option<usize>,
    end_time_index: Option<usize>,
    limit: Option<isize>,
) -> Result<String, String> {
    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table = waveform.time_table();
    let start_idx = start_time_index.unwrap_or(0);
    let end_idx = end_time_index.unwrap_or(time_table.len().saturating_sub(1));
    let lim = limit.unwrap_or(-1);

    let events = find_conditional_events(waveform, condition, start_idx, end_idx, lim)
        .map_err(|e| format!("Error finding conditional events: {}", e))?;

    Ok(format!(
        "Found {} events for condition '{}' (time range: {} to {}):\n{}",
        events.len(),
        condition,
        start_idx,
        end_idx,
        events.join("\n")
    ))
}

pub(super) fn exec_time_convert(
    store: &mut CliStore,
    cmd: &wave_analyzer_mcp::Command,
) -> Result<String, String> {
    let wave_analyzer_mcp::Command::TimeConvert {
        waveform_id,
        time_value,
        time_unit,
        time_index,
    } = cmd
    else {
        unreachable!("exec_time_convert only handles TimeConvert");
    };

    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();

    if let (Some(tv), Some(tu)) = (time_value, time_unit) {
        // Physical time → time_index
        let ps = time_value_to_ps(*tv, tu)?;
        let idx = find_time_index_by_value(waveform, ps)?;
        let actual_ps = compute_time_ps_from_table(time_table, idx, timescale.as_ref());

        let formatted = format_time(time_table[idx], timescale.as_ref());

        Ok(format!(
            "Input: {} {} (= {}ps)\nResolved: time_index {} (actual: {}ps = {})\nWaveform range: 0..{}",
            tv,
            tu,
            ps,
            idx,
            actual_ps,
            formatted,
            time_table.len().saturating_sub(1)
        ))
    } else if let Some(idx) = time_index {
        // time_index → physical time
        if *idx >= time_table.len() {
            return Err(format!(
                "time_index {} exceeds waveform range (max: {})",
                idx,
                time_table.len().saturating_sub(1)
            ));
        }

        let ps = compute_time_ps_from_table(time_table, *idx, timescale.as_ref());
        let formatted = format_time(time_table[*idx], timescale.as_ref());

        let (display_value, display_unit) = if ps >= 1_000_000_000_000 {
            (ps as f64 / 1e12, "s")
        } else if ps >= 1_000_000_000 {
            (ps as f64 / 1e9, "ms")
        } else if ps >= 1_000_000 {
            (ps as f64 / 1e6, "us")
        } else if ps >= 1_000 {
            (ps as f64 / 1e3, "ns")
        } else {
            (ps as f64, "ps")
        };

        Ok(format!(
            "Input: time_index {}\nResolved: {} {} (= {}ps, raw: {})\nWaveform range: 0..{}",
            idx,
            display_value,
            display_unit,
            ps,
            formatted,
            time_table.len().saturating_sub(1)
        ))
    } else {
        unreachable!("TimeConvert requires either time_value or time_index")
    }
}
