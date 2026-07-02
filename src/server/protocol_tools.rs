//! Helper functions for protocol analysis tools (handshake, measure, compare, timeline, sequence, crc).

use rmcp::{ErrorData as McpError, model::*};
use wave_analyzer_mcp::WaveAnalyzerError;
use wave_analyzer_mcp::time_map::time_value_to_ps;
use wave_analyzer_mcp::{
    analyze_handshake_with_level_sensitive, format_clock_report, format_handshake_report,
    format_interval_report, format_pulse_report, measure_clock, measure_intervals, measure_pulses,
};
use wave_analyzer_mcp::{auto_discover_signals, format_discovery_report};
use wave_analyzer_mcp::{build_multi_signal_timeline, format_timeline_report};
use wave_analyzer_mcp::{compare_signals_values, format_compare_report};
use wave_analyzer_mcp::{compute_and_verify_crc, format_crc_report};
use wave_analyzer_mcp::{detect_sequence, format_sequence_report};

use super::args::*;
use super::*;

pub async fn handle_analyze_handshake(
    waveforms: &WaveformStore,
    args: &AnalyzeHandshakeArgs,
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

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));
    let limit = args.limit.unwrap_or(-1);
    let report_mode = args.report_mode.as_deref().unwrap_or("summary");

    let result = analyze_handshake_with_level_sensitive(
        waveform,
        &args.valid_signal,
        &args.ready_signal,
        args.data_signal.as_deref(),
        start_idx,
        end_idx,
        Some(limit),
        report_mode,
        args.filter_zero_delay.unwrap_or(false),
        args.level_sensitive.unwrap_or(false),
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_handshake_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_measure_signal(
    waveforms: &WaveformStore,
    args: &MeasureSignalArgs,
) -> Result<CallToolResult, McpError> {
    if args.analysis_type != "clock"
        && args.analysis_type != "pulse"
        && args.analysis_type != "interval"
    {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: format!(
                    "Invalid analysis_type: '{}'. Must be 'clock', 'pulse', or 'interval'",
                    args.analysis_type
                ),
            },
            None,
        ));
    }

    let mut waveforms = waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));

    let text_output: String;
    let json_content: Content;

    if args.analysis_type == "clock" {
        let edge_type = args.edge_type.as_deref().unwrap_or("posedge");
        let result = measure_clock(waveform, &args.signal_path, edge_type, start_idx, end_idx)
            .map_err(|e| McpError::invalid_params(e, None))?;

        text_output = format_clock_report(&result);
        json_content = Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?;
    } else if args.analysis_type == "pulse" {
        let result = measure_pulses(waveform, &args.signal_path, start_idx, end_idx)
            .map_err(|e| McpError::invalid_params(e, None))?;

        text_output = format_pulse_report(&result);
        json_content = Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?;
    } else {
        // interval mode
        let from_condition = args.from_condition.as_deref().ok_or_else(|| {
            McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: "from_condition is required for interval mode".into(),
                },
                None,
            )
        })?;
        let to_condition = args.to_condition.as_deref().ok_or_else(|| {
            McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: "to_condition is required for interval mode".into(),
                },
                None,
            )
        })?;

        // Convert expected_value + expected_unit to seconds
        let expected_sec = if let (Some(ev), Some(eu)) = (args.expected_value, &args.expected_unit)
        {
            let expected_ps =
                time_value_to_ps(ev, eu).map_err(|e| McpError::invalid_params(e, None))?;
            Some(expected_ps as f64 / 1e12) // ps → seconds
        } else {
            None
        };

        let result = measure_intervals(
            waveform,
            from_condition,
            to_condition,
            start_idx,
            end_idx,
            expected_sec,
            Some(100),
        )
        .map_err(|e| McpError::invalid_params(e, None))?;

        text_output = format_interval_report(&result);
        json_content = Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?;
    }

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        json_content,
    ]))
}

pub async fn handle_compare_signals(
    waveforms: &WaveformStore,
    args: &CompareSignalsArgs,
) -> Result<CallToolResult, McpError> {
    if args.signals.len() < 2 {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "At least 2 signals must be provided for comparison".into(),
            },
            None,
        ));
    }

    let mut waveforms = waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));
    let limit = args.limit.unwrap_or(-1);
    let mode = args.comparison_mode.as_deref().unwrap_or("all_equal");
    let format = args.value_format.as_deref().unwrap_or("hex");

    let result = compare_signals_values(
        waveform,
        &args.signals,
        mode,
        start_idx,
        end_idx,
        format,
        Some(limit),
        0, // BUG-26 fix: tolerance=0 (no cross-hierarchy delay tolerance by default)
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_compare_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_multi_signal_timeline(
    waveforms: &WaveformStore,
    args: &MultiSignalTimelineArgs,
) -> Result<CallToolResult, McpError> {
    if args.signals.is_empty() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "At least one signal must be provided".into(),
            },
            None,
        ));
    }

    let mut waveforms = waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));
    let limit = args.limit.unwrap_or(-1);
    let merge_mode = args.merge_mode.as_deref().unwrap_or("union");
    let format = args.value_format.as_deref().unwrap_or("hex");

    let result = build_multi_signal_timeline(
        waveform,
        &args.signals,
        start_idx,
        end_idx,
        merge_mode,
        format,
        Some(limit),
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_timeline_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_auto_discover_signals(
    waveforms: &WaveformStore,
    args: &AutoDiscoverSignalsArgs,
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

    let mode = args.discovery_mode.as_deref().unwrap_or("bus_slices");
    let limit = args.limit;

    let result = auto_discover_signals(
        waveform,
        mode,
        args.scope_path.as_deref(),
        args.pattern.as_deref(),
        limit,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_discovery_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_detect_sequence(
    waveforms: &WaveformStore,
    args: &DetectSequenceArgs,
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

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));

    let result = detect_sequence(
        waveform,
        &args.sequence,
        args.max_gap_cycles,
        start_idx,
        end_idx,
        args.limit,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_sequence_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_compute_crc(
    waveforms: &WaveformStore,
    args: &ComputeCrcArgs,
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

    let time_table_len = waveform.time_table().len();
    let start_idx = args.start_time_index.unwrap_or(0);
    let end_idx = args
        .end_time_index
        .unwrap_or(time_table_len.saturating_sub(1));

    let result = compute_and_verify_crc(
        waveform,
        &args.data_signal_path,
        args.crc_signal_path.as_deref(),
        args.data_valid_signal_path.as_deref(),
        args.clear_signal_path.as_deref(),
        args.clock_signal_path.as_deref(),
        &args.crc_polynomial,
        args.initial_value,
        start_idx,
        end_idx,
        args.limit,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_crc_report(&result);

    Ok(CallToolResult::success(vec![
        Content::text(text_output),
        Content::json(&result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}
