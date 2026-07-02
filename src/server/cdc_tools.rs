//! Helper functions for CDC/pattern/FSM/protocol/phased_array analysis tools.

use rmcp::{ErrorData as McpError, model::*};
use wave_analyzer_mcp::WaveAnalyzerError;
use wave_analyzer_mcp::{
    ProtocolTemplate, analyze_cdc, analyze_phased_array, analyze_protocol_template,
    analyze_signal_patterns, extract_fsm, format_cdc_report, format_fsm_report,
    format_pattern_report, format_phased_array_report, format_protocol_template_report,
};

use super::args::*;
use super::*;

pub async fn handle_analyze_signal_patterns(
    waveforms: &WaveformStore,
    args: &AnalyzeSignalPatternsArgs,
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

    let result = analyze_signal_patterns(
        waveform,
        &args.signals,
        start_idx,
        end_idx,
        args.max_bins,
        args.idle_threshold.clone(),
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_pattern_report(&result);

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

pub async fn handle_extract_fsm(
    waveforms: &WaveformStore,
    args: &ExtractFsmArgs,
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
    let edge_type = args.edge_type.as_deref().unwrap_or("posedge");

    let result = extract_fsm(
        waveform,
        &args.signal_path,
        args.clock_signal.as_deref(),
        edge_type,
        start_idx,
        end_idx,
        None,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_fsm_report(&result);

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

pub async fn handle_analyze_protocol(
    waveforms: &WaveformStore,
    args: &AnalyzeProtocolArgs,
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

    let protocol = match args.protocol.to_lowercase().as_str() {
        "spi" => ProtocolTemplate::Spi,
        "uart" => ProtocolTemplate::Uart,
        "i2c" => ProtocolTemplate::I2c,
        "axi_lite" | "axilite" | "axi-lite" => ProtocolTemplate::AxiLite,
        _ => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: format!(
                        "Unknown protocol '{}'. Supported: spi, uart, i2c, axi_lite",
                        args.protocol
                    ),
                },
                None,
            ));
        }
    };

    let result = analyze_protocol_template(waveform, &protocol, &args.signals, start_idx, end_idx)
        .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_protocol_template_report(&result);

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

pub async fn handle_analyze_phased_array(
    waveforms: &WaveformStore,
    args: &AnalyzePhasedArrayArgs,
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

    let coeff_signals = args.coeff_signals.clone().unwrap_or_default();

    let result = analyze_phased_array(
        waveform,
        &args.channel_prefix,
        args.control_fsm_signal.as_deref(),
        &coeff_signals,
        &args.clock_signal,
        start_idx,
        end_idx,
    )
    .map_err(|e| McpError::invalid_params(e, None))?;

    let text_output = format_phased_array_report(&result);

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

pub async fn handle_analyze_cdc(
    handler: &WaveAnalyzerHandler,
    args: &AnalyzeCdcArgs,
) -> Result<CallToolResult, McpError> {
    let simulator = args
        .simulator
        .clone()
        .unwrap_or_else(|| "modelsim".to_string());
    let verify_synchronizers = args.verify_synchronizers.unwrap_or(true);
    let min_sync_stages = args.min_sync_stages.unwrap_or(2);

    // Get waveform
    let mut waveforms = handler.waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    // Get dep_graph if provided. BUG-fix (MCP/CLI parity): when no deps_id
    // is supplied we now perform the waveform-only heuristic analysis that
    // the tool description advertises, instead of erroring out. The CLI
    // already supported this path; the MCP layer was inconsistent.
    let result = match &args.deps_id {
        Some(deps_id) => {
            let dep_graphs = handler.dep_graphs.read().await;
            let dep_graph = dep_graphs.get(deps_id).ok_or_else(|| {
                McpError::invalid_params(
                    WaveAnalyzerError::DepsError {
                        message: format!("Dependency graph not found: {}", deps_id),
                    },
                    None,
                )
            })?;
            analyze_cdc(
                waveform,
                dep_graph,
                &simulator,
                verify_synchronizers,
                min_sync_stages,
            )
            .map_err(|e| McpError::internal_error(e, None))?
        }
        None => {
            // Waveform-only heuristic path: discover clock-domain candidates
            // from the waveform and report them. No crossings can be detected
            // without deps.yaml boundary edges; the result summary reflects
            // that (total_crossings = 0).
            wave_analyzer_mcp::cdc::analyze_cdc_waveform_only(waveform, &simulator)
                .map_err(|e| McpError::internal_error(e, None))?
        }
    };

    // Format report
    let report = format_cdc_report(&result);

    // Store result
    let cdc_id = format!("cdc_{}", args.waveform_id);
    handler
        .cdc_results
        .write()
        .await
        .insert(cdc_id.clone(), result);

    Ok(CallToolResult::success(vec![Content::text(report)]))
}
