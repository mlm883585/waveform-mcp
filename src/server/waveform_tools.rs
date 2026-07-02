//! Helper functions for waveform management tools (open, close, list, read, get, find).

use rmcp::{ErrorData as McpError, model::*};
use std::path::PathBuf;
use wave_analyzer_mcp::WaveAnalyzerError;
use wave_analyzer_mcp::{
    find_conditional_events, find_signal_by_path, find_signal_events, get_signal_metadata,
    list_signals, read_hierarchy, read_signal_values_by_path,
};

use super::args::*;
use super::*;

pub async fn handle_open_waveform(
    waveforms: &WaveformStore,
    args: &OpenWaveformArgs,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    let waveform = match wellen::simple::read(&path) {
        Ok(w) => w,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("Failed to read waveform: {}", e),
                },
                None,
            ));
        }
    };

    let alias = args.alias.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let mut waveforms = waveforms.write().await;
    waveforms.insert(alias.clone(), waveform);

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Waveform opened successfully with alias: {}",
        alias
    ))]))
}

pub async fn handle_list_signals(
    waveforms: &WaveformStore,
    args: &ListSignalsArgs,
) -> Result<CallToolResult, McpError> {
    let waveforms = waveforms.read().await;

    let waveform = waveforms.get(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    if args.limit == Some(0) {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "Invalid limit '0': limit must be greater than 0".into(),
            },
            None,
        ));
    }

    let hierarchy = waveform.hierarchy();
    let recursive = args.recursive.unwrap_or(true);

    let signals = list_signals(
        hierarchy,
        args.name_pattern.as_deref(),
        args.hierarchy_prefix.as_deref(),
        recursive,
        args.limit,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Found {} signals:\n{}",
        signals.len(),
        signals.join("\n")
    ))]))
}

pub async fn handle_read_hierarchy(
    waveforms: &WaveformStore,
    args: &ReadHierarchyArgs,
) -> Result<CallToolResult, McpError> {
    let waveforms = waveforms.read().await;

    let waveform = waveforms.get(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let hierarchy = waveform.hierarchy();
    let lines = read_hierarchy(
        hierarchy,
        args.scope_path.as_deref(),
        args.recursive.unwrap_or(false),
        args.limit,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let header = match args.scope_path.as_deref() {
        Some(path) => format!("Hierarchy rooted at '{}':", path),
        None => "Hierarchy:".to_string(),
    };
    let body = if lines.is_empty() {
        "No modules found".to_string()
    } else {
        lines.join("\n")
    };

    Ok(CallToolResult::success(vec![Content::text(format!(
        "{}\n{}",
        header, body
    ))]))
}

pub async fn handle_read_signal(
    waveforms: &WaveformStore,
    args: &ReadSignalArgs,
) -> Result<CallToolResult, McpError> {
    let mut waveforms = waveforms.write().await;

    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    // Determine which time indices to read
    let indices_to_read: Vec<usize> = if let Some(ref indices) = args.time_indices {
        indices.clone()
    } else if let Some(index) = args.time_index {
        vec![index]
    } else {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "Either time_index or time_indices must be provided".into(),
            },
            None,
        ));
    };

    let results = read_signal_values_by_path(waveform, &args.signal_path, &indices_to_read)
        .map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(
        results.join("\n"),
    )]))
}

pub async fn handle_get_signal_info(
    waveforms: &WaveformStore,
    args: &GetSignalInfoArgs,
) -> Result<CallToolResult, McpError> {
    let waveforms = waveforms.read().await;

    let waveform = waveforms.get(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let hierarchy = waveform.hierarchy();

    let info = get_signal_metadata(hierarchy, &args.signal_path)
        .map_err(|e| McpError::invalid_params(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(info)]))
}

pub async fn handle_find_signal_events(
    waveforms: &WaveformStore,
    args: &FindSignalEventsArgs,
) -> Result<CallToolResult, McpError> {
    let mut waveforms = waveforms.write().await;

    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let hierarchy = waveform.hierarchy();
    let signal_ref = find_signal_by_path(hierarchy, &args.signal_path).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::SignalNotFound {
                path: args.signal_path.clone(),
            },
            None,
        )
    })?;

    // Load the signal data
    waveform.load_signals(&[signal_ref]);

    let time_table_len = waveform.time_table().len();

    // Resolve time_value → time_index if physical time is provided
    let start_idx = if let (Some(tv), Some(tu)) = (args.start_time_value, &args.time_unit) {
        let ps = wave_analyzer_mcp::time_map::time_value_to_ps(tv, tu)
            .map_err(|e| McpError::invalid_params(e, None))?;
        wave_analyzer_mcp::time_map::find_time_index_by_value(waveform, ps)
            .map_err(|e| McpError::invalid_params(e, None))?
    } else {
        args.start_time_index.unwrap_or(0)
    };

    let end_idx = if let (Some(tv), Some(tu)) = (args.end_time_value, &args.time_unit) {
        let ps = wave_analyzer_mcp::time_map::time_value_to_ps(tv, tu)
            .map_err(|e| McpError::invalid_params(e, None))?;
        wave_analyzer_mcp::time_map::find_time_index_by_value(waveform, ps)
            .map_err(|e| McpError::invalid_params(e, None))?
    } else {
        args.end_time_index
            .unwrap_or(time_table_len.saturating_sub(1))
    };

    let limit = args.limit.unwrap_or(-1);

    let events = find_signal_events(waveform, signal_ref, start_idx, end_idx, limit)
        .map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Found {} events for signal '{}' (time range: {} to {}):\n{}",
        events.len(),
        args.signal_path,
        start_idx,
        end_idx,
        events.join("\n")
    ))]))
}

pub async fn handle_find_conditional_events(
    waveforms: &WaveformStore,
    args: &FindConditionalEventsArgs,
) -> Result<CallToolResult, McpError> {
    let mut waveforms = waveforms.write().await;

    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let time_table = waveform.time_table();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table.len().saturating_sub(1));
    let limit = args.limit.unwrap_or(-1);

    let events = find_conditional_events(waveform, &args.condition, start_idx, end_idx, limit)
        .map_err(|e| McpError::invalid_params(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Found {} events for condition '{}' (time range: {} to {}):\n{}",
        events.len(),
        args.condition,
        start_idx,
        end_idx,
        events.join("\n")
    ))]))
}

pub async fn handle_close_waveform(
    waveforms: &WaveformStore,
    args: &CloseWaveformArgs,
) -> Result<CallToolResult, McpError> {
    let mut waveforms = waveforms.write().await;

    match waveforms.remove(&args.waveform_id) {
        Some(_) => Ok(CallToolResult::success(vec![Content::text(format!(
            "Waveform '{}' closed successfully",
            args.waveform_id
        ))])),
        None => Err(McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )),
    }
}

pub async fn handle_load_dependencies(
    dep_graphs: &DepGraphStore,
    args: &LoadDependenciesArgs,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    let dep_graph = match wave_analyzer_mcp::load_deps_from_file(&path) {
        Ok(g) => g,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::DepsError {
                    message: format!("Failed to load dependency graph: {}", e),
                },
                None,
            ));
        }
    };

    let alias = args.alias.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let meta = dep_graph.meta();
    let summary = format!(
        "Dependency graph loaded with alias: {}\nVersion: {}\nNodes: {}, Edges: {}\nSignal aliases: {}, Clock aliases: {}\nHas cycles: {}",
        alias,
        meta.format_version,
        meta.node_count,
        meta.edge_count,
        meta.signal_alias_count,
        meta.clock_alias_count,
        meta.has_cycles,
    );

    let mut dep_graphs = dep_graphs.write().await;
    dep_graphs.insert(alias.clone(), dep_graph);

    Ok(CallToolResult::success(vec![Content::text(summary)]))
}

pub async fn handle_load_assertion_log(
    assertions: &AssertionStore,
    args: &LoadAssertionLogArgs,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    // Convert string severity filter to Severity enum
    let severity_filter: Vec<wave_analyzer_mcp::assertion::Severity> = args
        .severity_filter
        .as_deref()
        .map(|filter| {
            filter
                .iter()
                .filter_map(|s| wave_analyzer_mcp::assertion::Severity::from_str_name(s))
                .collect()
        })
        .unwrap_or_default();

    let limit = args.limit.unwrap_or(-1);

    let parse_result = match wave_analyzer_mcp::assertion::parse_assertion_log_from_file(
        &path,
        &severity_filter,
        limit,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::AssertionError {
                    message: format!("Failed to parse assertion log: {}", e),
                },
                None,
            ));
        }
    };

    let alias = args.alias.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // Build summary
    let mut event_lines = Vec::new();
    for event in parse_result.events.iter().take(20) {
        let source_info = match (&event.source_file, &event.source_line) {
            (Some(f), Some(l)) => format!(" [{}:{}]", f, l),
            (Some(f), None) => format!(" [{}]", f),
            _ => String::new(),
        };
        event_lines.push(format!(
            "- {} {} @ {} {} in {}{}",
            match &event.severity {
                wave_analyzer_mcp::assertion::Severity::Error => "Error",
                wave_analyzer_mcp::assertion::Severity::Warning => "Warning",
                wave_analyzer_mcp::assertion::Severity::Note => "Note",
                wave_analyzer_mcp::assertion::Severity::Failure => "Failure",
            },
            event.assertion_name,
            event.time_value,
            match &event.time_unit {
                wave_analyzer_mcp::assertion::TimeUnit::Ps => "ps",
                wave_analyzer_mcp::assertion::TimeUnit::Ns => "ns",
                wave_analyzer_mcp::assertion::TimeUnit::Us => "us",
                wave_analyzer_mcp::assertion::TimeUnit::Ms => "ms",
                wave_analyzer_mcp::assertion::TimeUnit::S => "s",
            },
            event.scope_path,
            source_info,
        ));
    }

    let total = parse_result.events.len();
    let shown = event_lines.len();
    let truncated_note = if total > shown {
        format!(" (showing first {} of {})", shown, total)
    } else {
        String::new()
    };

    let summary = format!(
        "Assertion log loaded with alias: {}\nParsed failure events: {}, Unmatched lines: {}\nTop events{}:\n{}",
        alias,
        total,
        parse_result.unmatched_lines.len(),
        truncated_note,
        event_lines.join("\n"),
    );

    let mut assertions = assertions.write().await;
    assertions.insert(alias.clone(), parse_result);

    Ok(CallToolResult::success(vec![Content::text(summary)]))
}

pub async fn handle_load_design_spec(
    specs: &SpecStore,
    args: &LoadDesignSpecArgs,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    let spec_lookup = match wave_analyzer_mcp::load_spec_from_file(&path) {
        Ok(s) => s,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("Failed to load design spec: {}", e),
                },
                None,
            ));
        }
    };

    let alias = args.alias.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // Build summary - we can't directly access internal fields, so use available methods
    let debug_entry_points = spec_lookup.find_debug_entry_points();
    let stop_signals = spec_lookup.find_stop_signals();
    let has_debug_hints = !debug_entry_points.is_empty() || !stop_signals.is_empty();

    let summary = format!(
        "Design spec loaded with alias: {}\nDebug hints available: {}\nDebug entry points: {}, Stop signals: {}",
        alias,
        has_debug_hints,
        debug_entry_points.len(),
        stop_signals.len(),
    );

    let mut specs = specs.write().await;
    specs.insert(alias.clone(), spec_lookup);

    Ok(CallToolResult::success(vec![Content::text(summary)]))
}

pub async fn handle_get_waveform_summary(
    args: &WaveformSummaryRequest,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    let mut waveform = match wellen::simple::read(&path) {
        Ok(w) => w,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("Failed to read waveform: {}", e),
                },
                None,
            ));
        }
    };

    let waveform_id = args.file_path.clone();
    let signal_paths = if args.signals.is_empty() {
        // Auto-detect: get first 5 top-level signals
        let hierarchy = waveform.hierarchy();

        list_signals(hierarchy, None, None, false, Some(5)).unwrap_or_default()
    } else {
        args.signals.clone()
    };

    let max_samples = args.max_samples;

    match wave_analyzer_mcp::generate_waveform_summary(
        &mut waveform,
        &waveform_id,
        &signal_paths,
        max_samples,
    ) {
        Ok(summary) => {
            let json = serde_json::to_string_pretty(&summary).map_err(|e| {
                McpError::internal_error(
                    WaveAnalyzerError::InvalidArgument {
                        message: format!("JSON serialization failed: {}", e),
                    },
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Err(e) => Err(McpError::internal_error(
            WaveAnalyzerError::InvalidArgument {
                message: format!("Failed to generate summary: {}", e),
            },
            None,
        )),
    }
}

pub async fn handle_export_waveform_svg(
    waveforms: &WaveformStore,
    args: &ExportSvgRequest,
) -> Result<CallToolResult, McpError> {
    let mut waveforms = waveforms.write().await;

    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    match wave_analyzer_mcp::export_waveform_to_svg(
        waveform,
        &args.signals,
        args.time_range,
        args.width,
        args.height,
    ) {
        Ok(response) => {
            let json = serde_json::to_string_pretty(&response).map_err(|e| {
                McpError::internal_error(
                    WaveAnalyzerError::InvalidArgument {
                        message: format!("JSON serialization failed: {}", e),
                    },
                    None,
                )
            })?;
            Ok(CallToolResult::success(vec![Content::text(json)]))
        }
        Err(e) => Err(McpError::internal_error(
            WaveAnalyzerError::InvalidArgument {
                message: format!("Failed to export SVG: {}", e),
            },
            None,
        )),
    }
}
