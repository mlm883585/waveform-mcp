//! Helper functions for analysis tools (trace, batch, fan_in, fan_out, suggest, extract_signal_values).

use rmcp::{ErrorData as McpError, model::*};
use std::path::PathBuf;
use wave_analyzer_mcp::WaveAnalyzerError;
use wave_analyzer_mcp::bfs::BfsOptions;
use wave_analyzer_mcp::deps::{BoundaryKind, ProtocolKind};
use wave_analyzer_mcp::extract::{ExtractRequest, extract_signal_values};
use wave_analyzer_mcp::time_map::time_value_to_ps;
use wave_analyzer_mcp::{
    find_time_index_by_value, load_deps_from_file, suggest_entry_signals, trace_root_cause,
};

use super::args::*;
use super::*;

pub async fn handle_trace_root_cause(
    handler: &WaveAnalyzerHandler,
    args: &TraceRootCauseArgs,
) -> Result<CallToolResult, McpError> {
    // Resolve time_index: direct or via time_value+time_unit conversion
    let time_index = if let Some(idx) = args.time_index {
        idx
    } else if let (Some(tv), Some(tu)) = (args.time_value, &args.time_unit) {
        // Convert time_value+time_unit to picoseconds, then find time_index
        let time_ps = time_value_to_ps(tv, tu).map_err(|e| McpError::invalid_params(e, None))?;
        // Need a read lock on waveforms to use find_time_index_by_value
        let waveforms = handler.waveforms.read().await;
        let waveform = waveforms.get(&args.waveform_id).ok_or_else(|| {
            McpError::invalid_params(
                WaveAnalyzerError::WaveformNotLoaded {
                    id: args.waveform_id.clone(),
                },
                None,
            )
        })?;
        find_time_index_by_value(waveform, time_ps)
            .map_err(|e| McpError::invalid_params(e, None))?
    } else {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "Either time_index or time_value+time_unit must be provided".into(),
            },
            None,
        ));
    };

    // Get dep_graph (write lock needed for alias inference)
    let mut dep_graphs = handler.dep_graphs.write().await;
    let dep_graph = dep_graphs.get_mut(&args.deps_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::DepsError {
                message: format!("Dependency graph not found: {}", args.deps_id),
            },
            None,
        )
    })?;

    // Build BFS options (need simulator before infer)
    let simulator = args
        .simulator
        .clone()
        .unwrap_or_else(|| "modelsim".to_string());
    let max_depth = args.max_depth.unwrap_or(8);

    // Infer aliases from waveform (always run — infer_aliases_from_waveform
    // only populates aliases for canonical names that lack an alias for
    // this simulator, so it won't overwrite existing correct aliases.
    // This handles the case where deps.yaml aliases point to a different
    // testbench than the current waveform.)
    {
        let waveforms_for_infer = handler.waveforms.read().await;
        if let Some(waveform) = waveforms_for_infer.get(&args.waveform_id) {
            dep_graph.infer_aliases_from_waveform(waveform.hierarchy(), &simulator);
        }
    }

    // If spec_id is provided, get stop_signals and debug hints
    let mut stop_signals = Vec::new();
    if let Some(ref spec_id) = args.spec_id {
        let specs = handler.specs.read().await;
        if let Some(spec_lookup) = specs.get(spec_id) {
            stop_signals = spec_lookup.find_stop_signals();
        }
    }

    let options = BfsOptions {
        max_depth,
        stop_signals,
        enable_auto_check: true,
        simulator,
        penetrate_cdc: args.penetrate_cdc.unwrap_or(false),
        cdc_max_depth: args.cdc_max_depth.unwrap_or(4),
        cdc_min_sync_stages: args.cdc_min_sync_stages.unwrap_or(2),
    };

    // Run BFS - needs write lock on waveforms for load_signals
    let mut waveforms = handler.waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let result = trace_root_cause(waveform, dep_graph, &args.signal_path, time_index, &options)
        .map_err(|e| McpError::internal_error(e, None))?;

    // Store the result for later report export (in-memory + disk persistence)
    let trace_id = format!("{}_{}_{}", args.waveform_id, args.signal_path, time_index);
    {
        let mut bfs_results = handler.bfs_results.write().await;
        bfs_results.insert(trace_id.clone(), result.clone());
    }
    // Persist to disk so export_bfs_report can retrieve it across process boundaries.
    // Use tracing::warn! instead of eprintln! — in stdio MCP mode, stderr is owned by
    // the tracing-subscriber layer; raw eprintln! lines would interleave with
    // structured log output and could leak into the JSON-RPC stream if the
    // transport layer ever switches to a stderr-based protocol.
    if let Err(e) = wave_analyzer_mcp::bfs::persist_bfs_result(&trace_id, &result) {
        tracing::warn!("failed to persist BFS result to disk: {}", e);
    }

    let text_summary = result.summary.clone();
    Ok(CallToolResult::success(vec![
        Content::text(format!("{}\n\nTrace ID: {}", text_summary, trace_id)),
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

pub async fn handle_find_fan_in(
    dep_graphs: &DepGraphStore,
    args: &FindFanInArgs,
) -> Result<CallToolResult, McpError> {
    let dep_graphs = dep_graphs.read().await;

    let dep_graph = dep_graphs.get(&args.deps_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::DepsError {
                message: format!("Dependency graph not found: {}", args.deps_id),
            },
            None,
        )
    })?;

    let simulator = args
        .simulator
        .clone()
        .unwrap_or_else(|| "modelsim".to_string());

    // Try exact signal path first, then fuzzy leaf-name fallback
    let canonical_signal = dep_graph
        .canonicalize_signal_fuzzy(&args.signal_path, &simulator)
        .unwrap_or_else(|| args.signal_path.clone());

    let fan_in_edges = dep_graph.fan_in(&canonical_signal);

    match fan_in_edges {
        Some(edges) => {
            let mut lines = Vec::new();
            for edge in edges {
                let resolved = dep_graph
                    .resolve_signal_fuzzy(&edge.signal, &simulator)
                    .unwrap_or_else(|| edge.signal.clone());

                let edge_type_str = edge.dep_type.to_string();

                let clock_str = edge
                    .clock
                    .as_deref()
                    .map(|c| format!(" clock={}", c))
                    .unwrap_or_default();

                let latency_str = edge
                    .latency_cycles
                    .map(|l| format!(" latency={}", l))
                    .unwrap_or_default();

                let desc_str = edge
                    .description
                    .as_deref()
                    .map(|d| format!(" ({})", d))
                    .unwrap_or_default();

                let protocol_str = edge
                    .protocol_kind
                    .as_ref()
                    .map(|p| match p {
                        ProtocolKind::Handshake => " handshake",
                        ProtocolKind::Backpressure => " backpressure",
                        ProtocolKind::NoProtocol => "",
                    })
                    .unwrap_or_default();

                let boundary_str = edge
                    .boundary_kind
                    .as_ref()
                    .map(|b| match b {
                        BoundaryKind::InputPort => " input_port",
                        BoundaryKind::Constant => " constant",
                        BoundaryKind::Cdc => " cdc",
                        BoundaryKind::Blackbox => " blackbox",
                        BoundaryKind::ManualStop => " manual_stop",
                    })
                    .unwrap_or_default();

                lines.push(format!(
                    "- {} -> {} [{}]{}{}{}{}{}{}",
                    edge.signal,
                    resolved,
                    edge_type_str,
                    clock_str,
                    latency_str,
                    protocol_str,
                    boundary_str,
                    desc_str,
                    if edge.signal != resolved {
                        format!(" (alias resolved for {})", simulator)
                    } else {
                        String::new()
                    },
                ));
            }

            Ok(CallToolResult::success(vec![Content::text(format!(
                "Found {} fan-in edges for '{}':\n{}",
                edges.len(),
                args.signal_path,
                lines.join("\n")
            ))]))
        }
        None => Ok(CallToolResult::success(vec![Content::text(format!(
            "No fan-in edges found for '{}' (signal not in dependency graph)",
            args.signal_path
        ))])),
    }
}

pub async fn handle_find_fan_out(
    dep_graphs: &DepGraphStore,
    args: &FindFanOutArgs,
) -> Result<CallToolResult, McpError> {
    let dep_graphs = dep_graphs.read().await;

    let dep_graph = dep_graphs.get(&args.deps_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::DepsError {
                message: format!("Dependency graph not found: {}", args.deps_id),
            },
            None,
        )
    })?;

    let simulator = args
        .simulator
        .clone()
        .unwrap_or_else(|| "modelsim".to_string());

    // Try exact signal path first, then fuzzy leaf-name fallback
    let canonical_signal = dep_graph
        .canonicalize_signal_fuzzy(&args.signal_path, &simulator)
        .unwrap_or_else(|| args.signal_path.clone());

    let fan_out_signals = dep_graph.fan_out(&canonical_signal);

    match fan_out_signals {
        Some(outputs) => {
            let mut lines = Vec::new();
            for output in outputs {
                let resolved = dep_graph
                    .resolve_signal_fuzzy(output, &simulator)
                    .unwrap_or_else(|| output.clone());

                let category = dep_graph
                    .get_category(output)
                    .map(|c| format!(" [{}]", c))
                    .unwrap_or_default();

                // Find the edge from this signal to the output
                let edges = dep_graph.fan_in(output);
                let edge_info = edges
                    .and_then(|e| {
                        e.iter()
                            .find(|edge| edge.signal == args.signal_path)
                            .map(|edge| {
                                let edge_type_str = edge.dep_type.to_string();
                                let latency_str = edge
                                    .latency_cycles
                                    .map(|l| format!(" latency={}", l))
                                    .unwrap_or_default();
                                format!(" [{}{}]", edge_type_str, latency_str)
                            })
                    })
                    .unwrap_or_default();

                let alias_note = if *output != resolved {
                    format!(" (alias resolved for {})", simulator)
                } else {
                    String::new()
                };

                lines.push(format!(
                    "- {} -> {}{}{}{}",
                    args.signal_path, resolved, category, edge_info, alias_note
                ));
            }

            Ok(CallToolResult::success(vec![Content::text(format!(
                "Found {} fan-out signals for '{}':\n{}",
                outputs.len(),
                args.signal_path,
                lines.join("\n")
            ))]))
        }
        None => {
            let message = if dep_graph.has_signal(&args.signal_path) {
                format!(
                    "No fan-out signals found for '{}' (signal has no downstream dependents in the dependency graph)",
                    args.signal_path
                )
            } else {
                format!(
                    "No fan-out signals found for '{}' (signal not in dependency graph)",
                    args.signal_path
                )
            };
            Ok(CallToolResult::success(vec![Content::text(message)]))
        }
    }
}

pub async fn handle_suggest_entry_signals(
    handler: &WaveAnalyzerHandler,
    args: &SuggestEntrySignalsArgs,
) -> Result<CallToolResult, McpError> {
    // Get waveform (read lock)
    let waveforms = handler.waveforms.read().await;
    let waveform = waveforms.get(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;
    let hierarchy = waveform.hierarchy();

    let simulator = args.simulator.as_deref().unwrap_or("modelsim");

    // Infer aliases from waveform hierarchy before suggestion
    // (method only infers for canonical names whose existing alias
    // doesn't point to a valid signal in this waveform)
    {
        let mut dep_graphs_mut = handler.dep_graphs.write().await;
        if let Some(dep_graph_mut) = dep_graphs_mut.get_mut(&args.deps_id) {
            dep_graph_mut.infer_aliases_from_waveform(hierarchy, simulator);
        }
    }

    // Get dep_graph (read lock)
    let dep_graphs = handler.dep_graphs.read().await;
    let dep_graph = dep_graphs.get(&args.deps_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::DepsError {
                message: format!("Dependency graph not found: {}", args.deps_id),
            },
            None,
        )
    })?;

    let limit = args.limit.unwrap_or(10);

    let candidates = suggest_entry_signals(
        hierarchy,
        dep_graph,
        args.assertion_name.as_deref(),
        args.scope_path.as_deref(),
        simulator,
        limit,
    );

    if candidates.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No candidate entry signals found. Ensure waveform and deps_id match the same design."
                .to_string(),
        )]));
    }

    let mut lines = Vec::new();
    for candidate in &candidates {
        let tier_str = match candidate.tier {
            1 => "T1:deps-output",
            2 => "T2:deps-boundary",
            3 => "T3:not-in-deps",
            _ => "unknown",
        };
        let match_str = if candidate.matches_assertion {
            " [assertion-match]"
        } else {
            ""
        };
        let fan_in_str = candidate
            .fan_in_count
            .map(|c| format!(" fan_in={}", c))
            .unwrap_or_default();
        let types_str = if candidate.dep_types.is_empty() {
            String::new()
        } else {
            format!(" types={}", candidate.dep_types.join(","))
        };

        lines.push(format!(
            "- {} [{}]{}{}{} | {}",
            candidate.signal_path, tier_str, match_str, fan_in_str, types_str, candidate.reason,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Suggested entry signals ({}):\n{}",
        candidates.len(),
        lines.join("\n")
    ))]))
}

pub async fn handle_extract_signal_values(
    waveforms: &WaveformStore,
    args: &ExtractSignalValuesArgs,
) -> Result<CallToolResult, McpError> {
    // Validate: either signal_path or bit_mapping must be provided
    if args.signal_path.is_none() && args.bit_mapping.is_empty() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::InvalidArgument {
                message: "Either signal_path or bit_mapping must be provided".into(),
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

    let request = ExtractRequest {
        waveform_id: args.waveform_id.clone(),
        signal_path: args.signal_path.clone(),
        bit_mapping: args.bit_mapping.clone(),
        start_time_index: args.start_time_index,
        end_time_index: args.end_time_index,
        start_time_ps: args.start_time_ps,
        end_time_ps: args.end_time_ps,
        value_format: args.value_format.clone(),
        downsample: args.downsample,
    };

    let result =
        extract_signal_values(waveform, &request).map_err(|e| McpError::invalid_params(e, None))?;

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

    Ok(CallToolResult::success(vec![
        Content::text(lines.join("\n")),
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

pub async fn handle_batch_trace_root_cause(
    handler: &WaveAnalyzerHandler,
    args: &BatchTraceRootCauseArgs,
) -> Result<CallToolResult, McpError> {
    // Get assertion events
    let assertions = handler.assertions.read().await;
    let assertion_result = assertions.get(&args.assertion_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::AssertionError {
                message: format!(
                    "Assertion log '{}' not found. Load it first with load_assertion_log.",
                    args.assertion_id
                ),
            },
            None,
        )
    })?;

    // Filter events by severity if specified
    let events: Vec<wave_analyzer_mcp::AssertionEvent> = if let Some(filter) = &args.severity_filter
    {
        let severities: Vec<wave_analyzer_mcp::assertion::Severity> = filter
            .split(',')
            .filter_map(|s| wave_analyzer_mcp::assertion::Severity::from_str_name(s.trim()))
            .collect();
        assertion_result
            .events
            .iter()
            .filter(|e| e.severity.matches_filter(&severities))
            .cloned()
            .collect()
    } else {
        assertion_result.events.clone()
    };

    if events.is_empty() {
        return Ok(CallToolResult::success(vec![Content::text(
            "No assertion events found matching the filter criteria".to_string(),
        )]));
    }

    // Get dep_graph (write lock needed for alias inference)
    let mut dep_graphs = handler.dep_graphs.write().await;
    let dep_graph = dep_graphs.get_mut(&args.deps_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::DepsError {
                message: format!("Dependency graph not found: {}", args.deps_id),
            },
            None,
        )
    })?;

    // Build BFS options
    let simulator = args
        .simulator
        .clone()
        .unwrap_or_else(|| "modelsim".to_string());
    let max_depth = args.max_depth.unwrap_or(8);

    // Infer aliases from waveform (always run — method only infers
    // for canonical names without existing aliases)
    {
        let waveforms_for_infer = handler.waveforms.read().await;
        if let Some(waveform) = waveforms_for_infer.get(&args.waveform_id) {
            dep_graph.infer_aliases_from_waveform(waveform.hierarchy(), &simulator);
        }
    }

    // Get spec if provided
    let spec_lookup = if let Some(ref spec_id) = args.spec_id {
        let specs = handler.specs.read().await;
        specs.get(spec_id).cloned()
    } else {
        None
    };

    // Build BFS options (simulator and max_depth already defined above)
    let mut stop_signals = Vec::new();
    if let Some(ref spec) = spec_lookup {
        stop_signals = spec.find_stop_signals();
    }

    let options = wave_analyzer_mcp::bfs::BfsOptions {
        max_depth,
        stop_signals,
        enable_auto_check: true,
        simulator,
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    // Run batch BFS
    let mut waveforms = handler.waveforms.write().await;
    let waveform = waveforms.get_mut(&args.waveform_id).ok_or_else(|| {
        McpError::invalid_params(
            WaveAnalyzerError::WaveformNotLoaded {
                id: args.waveform_id.clone(),
            },
            None,
        )
    })?;

    let spec_ref = spec_lookup.as_ref();
    let batch_result = wave_analyzer_mcp::bfs::batch_trace_root_cause(
        waveform, dep_graph, &events, &options, spec_ref,
    )
    .map_err(|e| McpError::internal_error(e, None))?;

    // Store all individual trace results for later report export
    {
        let mut bfs_results = handler.bfs_results.write().await;
        for entry in &batch_result.traces {
            if entry.error.is_none() {
                let trace_id = format!(
                    "batch_{}_{}_{}",
                    args.waveform_id, entry.event_name, entry.fail_time_index
                );
                bfs_results.insert(trace_id.clone(), entry.result.clone());
                // Persist to disk for cross-process retrieval.
                // Use tracing::warn! instead of eprintln! — see trace_root_cause handler
                // for the rationale on stdio JSON-RPC transport safety.
                if let Err(e) = wave_analyzer_mcp::bfs::persist_bfs_result(&trace_id, &entry.result)
                {
                    tracing::warn!("failed to persist BFS result to disk: {}", e);
                }
            }
        }
    }

    Ok(CallToolResult::success(vec![
        Content::text(batch_result.summary.clone()),
        Content::json(&batch_result).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}

pub async fn handle_extract_dependencies(
    handler: &WaveAnalyzerHandler,
    args: &ExtractDependenciesArgs,
) -> Result<CallToolResult, McpError> {
    let result = wave_analyzer_mcp::run_deps_extractor(
        &args.rtl_path,
        &args.top_module,
        args.engine.as_deref(),
        args.annotations_path.as_deref(),
        args.output_path.as_deref(),
        args.deps_extractor_path.as_deref(),
    )
    .map_err(|e| McpError::internal_error(e, None))?;

    let auto_load = args.auto_load.unwrap_or(true);

    if auto_load {
        let path = PathBuf::from(&result.deps_yaml_path);
        let dep_graph = load_deps_from_file(&path).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::DepsError {
                    message: format!("Failed to auto-load generated deps.yaml: {}", e),
                },
                None,
            )
        })?;

        let alias = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("auto-generated")
            .to_string();

        let meta = dep_graph.meta();
        let summary = format!(
            "Dependencies extracted and loaded with alias: {}\n\
            Engine: {}\n\
            Output: {}\n\
            Version: {}\n\
            Nodes: {}, Edges: {}\n\
            Signal aliases: {}, Clock aliases: {}\n\
            Has cycles: {}",
            alias,
            result.engine,
            result.deps_yaml_path,
            meta.format_version,
            meta.node_count,
            meta.edge_count,
            meta.signal_alias_count,
            meta.clock_alias_count,
            meta.has_cycles,
        );

        let mut dep_graphs = handler.dep_graphs.write().await;
        dep_graphs.insert(alias.clone(), dep_graph);

        Ok(CallToolResult::success(vec![Content::text(summary)]))
    } else {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Dependencies extracted successfully.\n\
            Engine: {}\n\
            Output: {}\n\
            Use load_dependencies to load the generated deps.yaml into the dependency store.",
            result.engine, result.deps_yaml_path,
        ))]))
    }
}
