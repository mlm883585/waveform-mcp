use std::path::PathBuf;

use wave_analyzer_mcp::assertion::{Severity, parse_assertion_log_from_file};
use wave_analyzer_mcp::bfs::BfsOptions;
use wave_analyzer_mcp::deps::{BoundaryKind, DepType, ProtocolKind};
use wave_analyzer_mcp::report::{
    format_bfs_report_html, format_bfs_report_json, format_bfs_report_markdown,
};
use wave_analyzer_mcp::time_map::time_value_to_ps;
use wave_analyzer_mcp::{
    Command, find_time_index_by_value, load_deps_from_file, load_spec_from_file,
};

use super::CliStore;
use super::utils::{
    load_bfs_result_from_cache, persist_bfs_result, run_trace_root_cause,
    trace_root_cause_json_payload,
};

pub(super) fn exec_load_deps(
    store: &mut CliStore,
    file_path: &str,
    alias: Option<String>,
) -> Result<String, String> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let dep_graph = load_deps_from_file(&path)
        .map_err(|e| format!("Failed to load dependency graph: {}", e))?;

    let id = alias.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let meta = dep_graph.meta();
    let summary = format!(
        "Dependency graph loaded with id: {}\nVersion: {}, Nodes: {}, Edges: {}\nSignal aliases: {}, Clock aliases: {}\nHas cycles: {}",
        id,
        meta.format_version,
        meta.node_count,
        meta.edge_count,
        meta.signal_alias_count,
        meta.clock_alias_count,
        meta.has_cycles,
    );

    store.dep_graphs.insert(id.clone(), dep_graph);
    Ok(summary)
}

pub(super) fn exec_load_assertion_log(
    store: &mut CliStore,
    file_path: &str,
    alias: Option<String>,
    severity_filter: Option<&Vec<String>>,
    limit: Option<isize>,
) -> Result<String, String> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let sev_filter: Vec<Severity> = severity_filter
        .map(|filter| {
            filter
                .iter()
                .filter_map(|s| Severity::from_str_name(s))
                .collect()
        })
        .unwrap_or_default();

    let lim = limit.unwrap_or(-1);

    let parse_result = parse_assertion_log_from_file(&path, &sev_filter, lim)
        .map_err(|e| format!("Failed to parse assertion log: {}", e))?;

    let id = alias.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let total = parse_result.events.len();
    let summary = format!(
        "Assertion log loaded with id: {}\nParsed failure events: {}, Unmatched lines: {}",
        id,
        total,
        parse_result.unmatched_lines.len(),
    );

    store.assertions.insert(id.clone(), parse_result);
    Ok(summary)
}

pub(super) fn exec_load_spec(
    store: &mut CliStore,
    file_path: &str,
    alias: Option<String>,
) -> Result<String, String> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let spec_lookup =
        load_spec_from_file(&path).map_err(|e| format!("Failed to load design spec: {}", e))?;

    let id = alias.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let debug_entry_points = spec_lookup.find_debug_entry_points();
    let stop_signals = spec_lookup.find_stop_signals();

    let summary = format!(
        "Design spec loaded with id: {}\nDebug entry points: {}, Stop signals: {}",
        id,
        debug_entry_points.len(),
        stop_signals.len(),
    );

    store.specs.insert(id.clone(), spec_lookup);
    Ok(summary)
}

pub(super) fn exec_trace_root_cause(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::TraceRootCause {
        waveform_id,
        deps_id,
        signal_path,
        time_index,
        time_value,
        time_unit,
        spec_id,
        max_depth,
        simulator,
        penetrate_cdc,
        cdc_max_depth,
        cdc_min_sync_stages,
    } = cmd
    else {
        unreachable!("exec_trace_root_cause only handles TraceRootCause");
    };

    let ti = if let Some(idx) = time_index {
        *idx
    } else if let (Some(tv), Some(tu)) = (time_value, time_unit) {
        let time_ps = time_value_to_ps(*tv, tu)?;
        let waveform = store
            .waveforms
            .get(waveform_id)
            .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
        find_time_index_by_value(waveform, time_ps)
            .map_err(|e| format!("Time value error: {}", e))?
    } else {
        return Err("Either time_index or time_value+time_unit must be provided".to_string());
    };

    let (trace_id, result) = run_trace_root_cause(
        store,
        waveform_id,
        deps_id,
        signal_path,
        ti,
        spec_id.as_deref(),
        *max_depth,
        simulator.as_deref(),
        *penetrate_cdc,
        *cdc_max_depth,
        *cdc_min_sync_stages,
    )?;

    Ok(format!("{}\ntrace_id: {}", result.summary, trace_id))
}

/// Execute trace_root_cause for JSON output (returns structured JSON payload)
pub(super) fn exec_trace_root_cause_json(
    store: &mut CliStore,
    cmd: &Command,
) -> Result<String, String> {
    let Command::TraceRootCause {
        waveform_id,
        deps_id,
        signal_path,
        time_index,
        time_value,
        time_unit,
        spec_id,
        max_depth,
        simulator,
        penetrate_cdc,
        cdc_max_depth,
        cdc_min_sync_stages,
    } = cmd
    else {
        unreachable!("exec_trace_root_cause_json only handles TraceRootCause");
    };

    let ti = if let Some(idx) = time_index {
        *idx
    } else if let (Some(tv), Some(tu)) = (time_value, time_unit) {
        let time_ps = time_value_to_ps(*tv, tu)?;
        let waveform = store
            .waveforms
            .get(waveform_id)
            .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
        find_time_index_by_value(waveform, time_ps)
            .map_err(|e| format!("Time value error: {}", e))?
    } else {
        return Err("Either time_index or time_value+time_unit must be provided".to_string());
    };

    let (trace_id, result) = run_trace_root_cause(
        store,
        waveform_id,
        deps_id,
        signal_path,
        ti,
        spec_id.as_deref(),
        *max_depth,
        simulator.as_deref(),
        *penetrate_cdc,
        *cdc_max_depth,
        *cdc_min_sync_stages,
    )?;

    Ok(trace_root_cause_json_payload(&trace_id, &result).to_string())
}

pub(super) fn exec_find_fan_in(
    store: &mut CliStore,
    deps_id: &str,
    signal_path: &str,
    simulator: Option<String>,
) -> Result<String, String> {
    let dep_graph = store
        .dep_graphs
        .get(deps_id)
        .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;

    let sim = simulator.unwrap_or_else(|| "modelsim".to_string());

    // Resolve signal_path using fuzzy matching (same as MCP handler)
    let canonical_signal = dep_graph
        .canonicalize_signal_fuzzy(signal_path, &sim)
        .unwrap_or_else(|| signal_path.to_string());

    let fan_in_edges = dep_graph.fan_in(&canonical_signal);

    match fan_in_edges {
        Some(edges) => {
            let mut lines = Vec::new();
            for edge in edges {
                let resolved = dep_graph
                    .resolve_signal(&edge.signal, &sim)
                    .unwrap_or_else(|| edge.signal.clone());

                let edge_type_str = match &edge.dep_type {
                    DepType::Combinational => "combinational",
                    DepType::Sequential => "sequential",
                    DepType::Memory => "memory",
                    DepType::Control => "control",
                    DepType::Protocol => "protocol",
                    DepType::Boundary => "boundary",
                };

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
                        format!(" (alias resolved for {})", sim)
                    } else {
                        String::new()
                    },
                ));
            }

            Ok(format!(
                "Found {} fan-in edges for '{}':\n{}",
                edges.len(),
                signal_path,
                lines.join("\n"),
            ))
        }
        None => Ok(format!(
            "No fan-in edges found for '{}' (signal not in dependency graph)",
            signal_path
        )),
    }
}

pub(super) fn exec_find_fan_out(
    store: &mut CliStore,
    deps_id: &str,
    signal_path: &str,
    simulator: Option<String>,
) -> Result<String, String> {
    let dep_graph = store
        .dep_graphs
        .get(deps_id)
        .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;

    let sim = simulator.unwrap_or_else(|| "modelsim".to_string());

    let fan_out_signals = dep_graph.fan_out(signal_path);

    match fan_out_signals {
        Some(outputs) => {
            let mut lines = Vec::new();
            for output in outputs {
                let resolved = dep_graph
                    .resolve_signal(output, &sim)
                    .unwrap_or_else(|| output.clone());

                let category = dep_graph
                    .get_category(output)
                    .map(|c| format!(" [{}]", c))
                    .unwrap_or_default();

                let edges = dep_graph.fan_in(output);
                let edge_info = edges
                    .and_then(|e| {
                        e.iter()
                            .find(|edge| edge.signal == *signal_path)
                            .map(|edge| {
                                let edge_type_str = match &edge.dep_type {
                                    DepType::Combinational => "combinational",
                                    DepType::Sequential => "sequential",
                                    DepType::Memory => "memory",
                                    DepType::Control => "control",
                                    DepType::Protocol => "protocol",
                                    DepType::Boundary => "boundary",
                                };
                                let latency_str = edge
                                    .latency_cycles
                                    .map(|l| format!(" latency={}", l))
                                    .unwrap_or_default();
                                format!(" [{}{}]", edge_type_str, latency_str)
                            })
                    })
                    .unwrap_or_default();

                let alias_note = if *output != resolved {
                    format!(" (alias resolved for {})", sim)
                } else {
                    String::new()
                };

                lines.push(format!(
                    "- {} -> {}{}{}{}",
                    signal_path, resolved, category, edge_info, alias_note
                ));
            }

            Ok(format!(
                "Found {} fan-out signals for '{}':\n{}",
                outputs.len(),
                signal_path,
                lines.join("\n"),
            ))
        }
        None => {
            if dep_graph.has_signal(signal_path) {
                Ok(format!(
                    "No fan-out signals found for '{}' (signal has no downstream dependents in the dependency graph)",
                    signal_path
                ))
            } else {
                Ok(format!(
                    "No fan-out signals found for '{}' (signal not in dependency graph)",
                    signal_path
                ))
            }
        }
    }
}

pub(super) fn exec_batch_trace(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::BatchTraceRootCause {
        waveform_id,
        deps_id,
        assertion_id,
        spec_id,
        max_depth,
        severity_filter,
        simulator,
    } = cmd
    else {
        unreachable!("exec_batch_trace only handles BatchTraceRootCause");
    };

    let assertions = store.assertions.get(assertion_id).ok_or_else(|| {
        format!(
            "Assertion log '{}' not found. Load it first with load_assertion_log.",
            assertion_id
        )
    })?;

    let events: Vec<wave_analyzer_mcp::AssertionEvent> = if let Some(filter) = severity_filter {
        let severities: Vec<wave_analyzer_mcp::assertion::Severity> = filter
            .split(',')
            .filter_map(|s| wave_analyzer_mcp::assertion::Severity::from_str_name(s.trim()))
            .collect();
        assertions
            .events
            .iter()
            .filter(|e| e.severity.matches_filter(&severities))
            .cloned()
            .collect()
    } else {
        assertions.events.clone()
    };

    if events.is_empty() {
        return Ok("No failure events found in the assertion log matching the filter criteria. Check that load_assertion_log found events, or broaden the severity filter.".to_string());
    }

    // Infer aliases from waveform if signal_aliases is empty
    let sim = simulator.clone().unwrap_or_else(|| "modelsim".to_string());
    {
        let dep_graph_mut = store
            .dep_graphs
            .get_mut(deps_id)
            .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;
        if let Some(waveform) = store.waveforms.get(waveform_id) {
            dep_graph_mut.infer_aliases_from_waveform(waveform.hierarchy(), &sim);
        }
    }

    let dep_graph = store
        .dep_graphs
        .get(deps_id)
        .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;

    let spec_lookup = if let Some(sid) = spec_id {
        store.specs.get(sid).cloned()
    } else {
        None
    };

    let mut stop_signals = Vec::new();
    if let Some(ref spec) = spec_lookup {
        stop_signals = spec.find_stop_signals();
    }

    let sim = simulator.clone().unwrap_or_else(|| "modelsim".to_string());
    let depth = max_depth.unwrap_or(8);

    let options = BfsOptions {
        max_depth: depth,
        stop_signals,
        enable_auto_check: true,
        simulator: sim,
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let spec_ref = spec_lookup.as_ref();
    let batch_result = wave_analyzer_mcp::bfs::batch_trace_root_cause(
        waveform, dep_graph, &events, &options, spec_ref,
    )
    .map_err(|e| format!("Error in batch trace: {}", e))?;

    // Store individual trace results for later report export
    for entry in &batch_result.traces {
        if entry.error.is_none() {
            let trace_id = format!(
                "batch_{}_{}_{}",
                waveform_id, entry.event_name, entry.fail_time_index
            );
            store
                .bfs_results
                .insert(trace_id.clone(), entry.result.clone());
            persist_bfs_result(&trace_id, store.bfs_results.get(&trace_id).unwrap())?;
        }
    }

    Ok(batch_result.summary)
}

pub(super) fn exec_export_bfs_report(
    store: &mut CliStore,
    trace_id: &str,
    format: Option<&str>,
) -> Result<String, String> {
    let result = store
        .bfs_results
        .get(trace_id)
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| load_bfs_result_from_cache(trace_id));
    let result = result?;

    let fmt = format.unwrap_or("markdown");

    #[allow(clippy::wildcard_in_or_patterns)]
    match fmt {
        "json" => {
            let json_str = format_bfs_report_json(&result)
                .map_err(|e| format!("Error generating JSON report: {}", e))?;
            Ok(json_str)
        }
        "html" => {
            let html_str = format_bfs_report_html(&result);
            Ok(html_str)
        }
        "markdown" | _ => {
            let md_str = format_bfs_report_markdown(&result);
            Ok(md_str)
        }
    }
}
