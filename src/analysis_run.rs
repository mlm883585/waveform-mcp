//! End-to-end simulation run analysis orchestration.
//!
//! This module turns the existing low-level waveform, assertion, deps, and BFS
//! capabilities into a single deterministic workflow driven by `run_summary.json`.

use crate::assertion::{AssertionEvent, Severity, TimeUnit, parse_assertion_log_from_file};
use crate::bfs::{AggregatedCandidate, BfsOptions, BfsResult, RootCauseCandidate};
use crate::deps::{DepGraph, load_deps_from_file};
use crate::deps_extractor::run_deps_extractor;
use crate::entry_signal::suggest_entry_signals;
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::report::{
    BatchBfsReport, BfsTraceEntry, format_batch_bfs_report_html, format_batch_bfs_report_markdown,
    format_bfs_report_html, format_bfs_report_json, format_bfs_report_markdown,
};
use crate::run_summary::{parse_run_summary_from_file, suggest_next_step};
use crate::spec::{SpecLookup, load_spec_from_file};
use crate::{find_time_index_by_value, trace_root_cause};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Request for end-to-end run analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeRunRequest {
    pub run_summary_path: String,
    pub deps_file: Option<String>,
    pub spec_file: Option<String>,
    pub transcript_file: Option<String>,
    pub waveform_file: Option<String>,
    pub severity_filter: Option<Vec<String>>,
    pub max_depth: Option<usize>,
    pub simulator: Option<String>,
    pub report_dir: Option<String>,
    pub report_format: Option<String>,
    /// Whether to penetrate CDC boundaries when synchronizer detected.
    pub penetrate_cdc: Option<bool>,
    /// Maximum depth within a penetrated CDC domain.
    pub cdc_max_depth: Option<usize>,
    /// Minimum synchronizer stages required for CDC penetration.
    pub cdc_min_sync_stages: Option<u32>,
}

/// Selected assertion event consumed by the orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedEvent {
    pub event_name: String,
    pub severity: String,
    pub scope_path: String,
    pub time_value: u64,
    pub time_unit: String,
    pub time_ps: u64,
}

/// How the primary BFS entry signal was chosen for one assertion event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryResolution {
    pub event_name: String,
    pub strategy: String,
    pub primary_entry_signal: Option<String>,
    pub alternatives: Vec<String>,
    pub scope_path: String,
}

/// One trace executed during run analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeRunTrace {
    pub trace_id: String,
    pub event_name: String,
    pub entry_signal: String,
    pub fail_time_index: usize,
    pub fail_time_ps: u64,
    pub result: Option<BfsResult>,
    pub error: Option<String>,
    pub report_path: Option<String>,
}

/// Report artifact produced by the orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportOutput {
    pub kind: String,
    pub format: String,
    pub path: String,
}

/// Final structured result for `analyze_run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeRunResult {
    pub status: String,
    pub summary: String,
    pub run_status: String,
    pub waveform_id: Option<String>,
    pub assertion_id: Option<String>,
    pub deps_id: Option<String>,
    pub spec_id: Option<String>,
    pub selected_events: Vec<SelectedEvent>,
    pub entry_resolution: Vec<EntryResolution>,
    pub traces: Vec<AnalyzeRunTrace>,
    pub aggregated_candidates: Vec<AggregatedCandidate>,
    pub report_outputs: Vec<ReportOutput>,
    pub next_step: String,
}

/// Execute the end-to-end run analysis workflow.
pub fn analyze_run(request: &AnalyzeRunRequest) -> WaveResult<AnalyzeRunResult> {
    let run_summary_path = PathBuf::from(&request.run_summary_path);
    if !run_summary_path.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: run_summary_path.display().to_string(),
            message: "Run summary file not found".to_string(),
        });
    }

    let run_summary = parse_run_summary_from_file(&run_summary_path)?;
    let next_step = suggest_next_step(&run_summary);
    let base_dir = run_summary_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if run_summary.status != "assertion_failed" {
        return Ok(AnalyzeRunResult {
            status: "ok".to_string(),
            summary: format!(
                "Run status '{}' does not enter BFS orchestration.",
                run_summary.status
            ),
            run_status: run_summary.status.clone(),
            waveform_id: None,
            assertion_id: None,
            deps_id: None,
            spec_id: None,
            selected_events: Vec::new(),
            entry_resolution: Vec::new(),
            traces: Vec::new(),
            aggregated_candidates: Vec::new(),
            report_outputs: Vec::new(),
            next_step,
        });
    }

    let waveform_path = resolve_required_path(
        &base_dir,
        request.waveform_file.as_deref(),
        Some(run_summary.wave_file.as_str()),
        "waveform",
    )?;
    let transcript_path = resolve_required_path(
        &base_dir,
        request.transcript_file.as_deref(),
        Some(run_summary.transcript_file.as_str()),
        "transcript",
    )?;
    let deps_path =
        match resolve_optional_path(&base_dir, request.deps_file.as_deref(), Some("deps.yaml")) {
            Some(p) if p.exists() => p,
            _ => {
                // 尝试自动提取 deps.yaml
                let top_module = run_summary.top_module.as_str();
                match try_auto_extract_deps(&base_dir, top_module) {
                    Ok(path) => path,
                    Err(extract_err) => {
                        return Err(WaveAnalyzerError::DepsError {
                            message: format!(
                                "deps.yaml not found and auto-extraction failed: {}. \
                                 Provide --deps or place deps.yaml next to run_summary.json.",
                                extract_err
                            ),
                        });
                    }
                }
            }
        };
    let spec_path = resolve_optional_path(
        &base_dir,
        request.spec_file.as_deref(),
        Some("design_spec.yaml"),
    );

    if !waveform_path.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: waveform_path.display().to_string(),
            message: "Waveform file not found".to_string(),
        });
    }
    if !transcript_path.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: transcript_path.display().to_string(),
            message: "Transcript file not found".to_string(),
        });
    }
    if !deps_path.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: deps_path.display().to_string(),
            message: "Dependency file not found".to_string(),
        });
    }

    let dep_graph = load_deps_from_file(&deps_path)?;
    let spec_lookup = match spec_path.as_ref() {
        Some(path) if path.exists() => Some(load_spec_from_file(path)?),
        _ => None,
    };

    let severity_filter = build_severity_filter(request.severity_filter.as_deref())?;
    let assertion_log = parse_assertion_log_from_file(&transcript_path, &severity_filter, -1)?;

    let selected_events: Vec<SelectedEvent> = assertion_log
        .events
        .iter()
        .map(selected_event_from_assertion)
        .collect();

    if selected_events.is_empty() {
        return Ok(AnalyzeRunResult {
            status: "ok".to_string(),
            summary: "No assertion events matched the severity filter.".to_string(),
            run_status: run_summary.status.clone(),
            waveform_id: Some(waveform_path.display().to_string()),
            assertion_id: Some(transcript_path.display().to_string()),
            deps_id: Some(deps_path.display().to_string()),
            spec_id: spec_path.map(|p| p.display().to_string()),
            selected_events,
            entry_resolution: Vec::new(),
            traces: Vec::new(),
            aggregated_candidates: Vec::new(),
            report_outputs: Vec::new(),
            next_step,
        });
    }

    let mut waveform =
        wellen::simple::read(&waveform_path).map_err(|e| WaveAnalyzerError::FileError {
            path: waveform_path.display().to_string(),
            message: format!("Failed to read waveform: {}", e),
        })?;

    let stop_signals = spec_lookup
        .as_ref()
        .map(SpecLookup::find_stop_signals)
        .unwrap_or_default();
    let options = BfsOptions {
        max_depth: request.max_depth.unwrap_or(8),
        stop_signals,
        enable_auto_check: true,
        simulator: request
            .simulator
            .clone()
            .unwrap_or_else(|| "modelsim".to_string()),
        penetrate_cdc: request.penetrate_cdc.unwrap_or(false),
        cdc_max_depth: request.cdc_max_depth.unwrap_or(4),
        cdc_min_sync_stages: request.cdc_min_sync_stages.unwrap_or(2),
    };

    let mut entry_resolution = Vec::new();
    let mut traces = Vec::new();
    let mut report_outputs = Vec::new();

    let report_format = request
        .report_format
        .as_deref()
        .unwrap_or("markdown")
        .to_string();
    let report_dir = if report_format == "none" {
        None
    } else {
        let dir = request
            .report_dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_report_dir(&base_dir, &run_summary_path));
        std::fs::create_dir_all(&dir).map_err(|e| WaveAnalyzerError::FileError {
            path: dir.display().to_string(),
            message: e.to_string(),
        })?;
        Some(dir)
    };

    for event in &assertion_log.events {
        let resolution = resolve_entry_for_event(
            waveform.hierarchy(),
            &dep_graph,
            spec_lookup.as_ref(),
            event,
            &options.simulator,
        );
        let primary_entry_signal = resolution.primary_entry_signal.clone();
        entry_resolution.push(resolution);

        let Some(entry_signal) = primary_entry_signal else {
            traces.push(AnalyzeRunTrace {
                trace_id: trace_id_for_event(event, 0),
                event_name: event.assertion_name.clone(),
                entry_signal: String::new(),
                fail_time_index: 0,
                fail_time_ps: event.time_ps,
                result: None,
                error: Some("No BFS entry signal could be resolved".to_string()),
                report_path: None,
            });
            continue;
        };

        let fail_time_index = find_time_index_by_value(&waveform, event.time_ps).map_err(|e| {
            WaveAnalyzerError::Other(format!(
                "Time conversion error for event at {} ps: {}",
                event.time_ps, e
            ))
        })?;
        let trace_id = trace_id_for_event(event, fail_time_index);

        match trace_root_cause(
            &mut waveform,
            &dep_graph,
            &entry_signal,
            fail_time_index,
            &options,
        ) {
            Ok(result) => {
                let report_path = match &report_dir {
                    Some(dir) => Some(write_trace_report(dir, &trace_id, &report_format, &result)?),
                    None => None,
                };
                if let Some(path) = &report_path {
                    report_outputs.push(ReportOutput {
                        kind: "trace".to_string(),
                        format: report_format.clone(),
                        path: path.clone(),
                    });
                }
                traces.push(AnalyzeRunTrace {
                    trace_id,
                    event_name: event.assertion_name.clone(),
                    entry_signal,
                    fail_time_index,
                    fail_time_ps: event.time_ps,
                    result: Some(result),
                    error: None,
                    report_path,
                });
            }
            Err(error) => {
                traces.push(AnalyzeRunTrace {
                    trace_id,
                    event_name: event.assertion_name.clone(),
                    entry_signal,
                    fail_time_index,
                    fail_time_ps: event.time_ps,
                    result: None,
                    error: Some(error.to_string()),
                    report_path: None,
                });
            }
        }
    }

    let aggregated_candidates = aggregate_candidates(&traces);

    if let Some(dir) = &report_dir {
        let successful_traces: Vec<&AnalyzeRunTrace> = traces
            .iter()
            .filter(|trace| trace.result.is_some())
            .collect();
        if !successful_traces.is_empty() {
            let batch_path = write_batch_report(
                dir,
                &report_format,
                &successful_traces,
                &aggregated_candidates,
            )?;
            report_outputs.push(ReportOutput {
                kind: "batch".to_string(),
                format: report_format.clone(),
                path: batch_path,
            });
        }
    }

    let success_count = traces.iter().filter(|trace| trace.error.is_none()).count();
    let failure_count = traces.len().saturating_sub(success_count);
    let summary = format!(
        "analyze_run complete: {} assertion events selected, {} traces succeeded, {} traces failed, {} unique aggregated candidates.",
        selected_events.len(),
        success_count,
        failure_count,
        aggregated_candidates.len()
    );

    Ok(AnalyzeRunResult {
        status: "ok".to_string(),
        summary,
        run_status: run_summary.status.clone(),
        waveform_id: Some(waveform_path.display().to_string()),
        assertion_id: Some(transcript_path.display().to_string()),
        deps_id: Some(deps_path.display().to_string()),
        spec_id: spec_lookup
            .as_ref()
            .and(spec_path)
            .map(|path| path.display().to_string()),
        selected_events,
        entry_resolution,
        traces,
        aggregated_candidates,
        report_outputs,
        next_step,
    })
}

fn resolve_required_path(
    base_dir: &Path,
    override_path: Option<&str>,
    fallback: Option<&str>,
    label: &str,
) -> WaveResult<PathBuf> {
    let raw = override_path
        .or(fallback)
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: format!("Missing {} path", label),
        })?;
    Ok(resolve_path(base_dir, raw))
}

fn resolve_optional_path(
    base_dir: &Path,
    override_path: Option<&str>,
    default_file: Option<&str>,
) -> Option<PathBuf> {
    match override_path {
        Some(path) => Some(resolve_path(base_dir, path)),
        None => default_file
            .map(|name| base_dir.join(name))
            .filter(|path| path.exists()),
    }
}

fn resolve_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn build_severity_filter(filter: Option<&[String]>) -> WaveResult<Vec<Severity>> {
    match filter {
        Some(values) if !values.is_empty() => values
            .iter()
            .map(|value| {
                Severity::from_str_name(value.trim()).ok_or_else(|| {
                    WaveAnalyzerError::InvalidArgument {
                        message: format!("Invalid severity filter value: {}", value),
                    }
                })
            })
            .collect(),
        _ => Ok(vec![Severity::Error, Severity::Failure]),
    }
}

fn selected_event_from_assertion(event: &AssertionEvent) -> SelectedEvent {
    SelectedEvent {
        event_name: event.assertion_name.clone(),
        severity: severity_name(&event.severity).to_string(),
        scope_path: event.scope_path.clone(),
        time_value: event.time_value,
        time_unit: time_unit_name(&event.time_unit).to_string(),
        time_ps: event.time_ps,
    }
}

fn resolve_entry_for_event(
    hierarchy: &wellen::Hierarchy,
    dep_graph: &DepGraph,
    spec_lookup: Option<&SpecLookup>,
    event: &AssertionEvent,
    simulator: &str,
) -> EntryResolution {
    if let Some(spec) = spec_lookup {
        let entries = spec.find_entry_signals_by_assertion(&event.assertion_name);
        if !entries.is_empty() {
            return EntryResolution {
                event_name: event.assertion_name.clone(),
                strategy: "design_spec".to_string(),
                primary_entry_signal: Some(entries[0].clone()),
                alternatives: entries.iter().skip(1).cloned().collect(),
                scope_path: event.scope_path.clone(),
            };
        }
    }

    let candidates = suggest_entry_signals(
        hierarchy,
        dep_graph,
        Some(&event.assertion_name),
        Some(&event.scope_path),
        simulator,
        5,
    );
    EntryResolution {
        event_name: event.assertion_name.clone(),
        strategy: "suggest_entry_signals".to_string(),
        primary_entry_signal: candidates
            .first()
            .map(|candidate| candidate.signal_path.clone()),
        alternatives: candidates
            .iter()
            .skip(1)
            .map(|candidate| candidate.signal_path.clone())
            .collect(),
        scope_path: event.scope_path.clone(),
    }
}

fn aggregate_candidates(traces: &[AnalyzeRunTrace]) -> Vec<AggregatedCandidate> {
    let mut all_candidates: Vec<RootCauseCandidate> = Vec::new();
    for trace in traces {
        let Some(result) = &trace.result else {
            continue;
        };
        all_candidates.extend(result.candidates.clone());
    }
    crate::bfs::aggregate_candidates_from_results(&all_candidates)
}

fn default_report_dir(base_dir: &Path, run_summary_path: &Path) -> PathBuf {
    let stem = run_summary_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("run");
    base_dir.join("analysis_reports").join(stem)
}

fn write_trace_report(
    report_dir: &Path,
    trace_id: &str,
    format: &str,
    result: &BfsResult,
) -> WaveResult<String> {
    let (file_name, content) = match format {
        "json" => (
            format!("{}.json", trace_id),
            format_bfs_report_json(result)?,
        ),
        "html" => (format!("{}.html", trace_id), format_bfs_report_html(result)),
        "markdown" => (
            format!("{}.md", trace_id),
            format_bfs_report_markdown(result),
        ),
        other => {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!(
                    "Unsupported report format '{}'. Use json, markdown, html, or none.",
                    other
                ),
            });
        }
    };
    let output_path = report_dir.join(file_name);
    std::fs::write(&output_path, content).map_err(|e| WaveAnalyzerError::FileError {
        path: output_path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(output_path.display().to_string())
}

fn write_batch_report(
    report_dir: &Path,
    format: &str,
    traces: &[&AnalyzeRunTrace],
    aggregated_candidates: &[AggregatedCandidate],
) -> WaveResult<String> {
    let batch_report = BatchBfsReport {
        traces: traces
            .iter()
            .filter_map(|trace| {
                trace.result.as_ref().map(|result| BfsTraceEntry {
                    event_name: trace.event_name.clone(),
                    entry_signal: trace.entry_signal.clone(),
                    fail_time_ps: trace.fail_time_ps,
                    result: result.clone(),
                })
            })
            .collect(),
        aggregated_candidates: aggregated_candidates
            .iter()
            .map(|candidate| RootCauseCandidate {
                signal_path: candidate.signal_path.clone(),
                time_index: 0,
                time_ps: candidate.time_ps,
                status: candidate.status.clone(),
                reason: candidate.reasons.join("; "),
            })
            .collect(),
        summary: format!(
            "{} successful traces, {} aggregated candidates",
            traces.len(),
            aggregated_candidates.len()
        ),
    };

    let (file_name, content) = match format {
        "html" => (
            "batch_report.html".to_string(),
            format_batch_bfs_report_html(&batch_report),
        ),
        "json" => (
            "batch_report.json".to_string(),
            serde_json::to_string_pretty(&batch_report).map_err(|e| {
                WaveAnalyzerError::Other(format!("Failed to serialize batch report: {}", e))
            })?,
        ),
        "markdown" => (
            "batch_report.md".to_string(),
            format_batch_bfs_report_markdown(&batch_report),
        ),
        other => {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!(
                    "Unsupported report format '{}'. Use json, markdown, html, or none.",
                    other
                ),
            });
        }
    };
    let output_path = report_dir.join(file_name);
    std::fs::write(&output_path, content).map_err(|e| WaveAnalyzerError::FileError {
        path: output_path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(output_path.display().to_string())
}

fn trace_id_for_event(event: &AssertionEvent, fail_time_index: usize) -> String {
    format!(
        "trace_{}_{}",
        sanitize_identifier(&event.assertion_name),
        fail_time_index
    )
}

fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn severity_name(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Note => "Note",
        Severity::Failure => "Failure",
    }
}

fn time_unit_name(unit: &TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Ps => "ps",
        TimeUnit::Ns => "ns",
        TimeUnit::Us => "us",
        TimeUnit::Ms => "ms",
        TimeUnit::S => "s",
    }
}

/// Try to auto-generate deps.yaml when it doesn't exist.
///
/// Searches for `.v` files in common RTL directories relative to `base_dir`,
/// then runs the Pyverilog extraction pipeline.
fn try_auto_extract_deps(base_dir: &Path, top_module: &str) -> WaveResult<PathBuf> {
    // Search for RTL files in common locations
    let search_dirs = [
        base_dir.to_path_buf(),
        base_dir.join("rtl"),
        base_dir.join("src"),
        base_dir.join("hdl"),
        base_dir.join("../rtl"),
    ];

    let mut v_files: Vec<String> = Vec::new();
    for dir in &search_dirs {
        if dir.exists()
            && dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "v") {
                    v_files.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    if v_files.is_empty() {
        return Err(WaveAnalyzerError::DepsError {
            message: "No .v files found for auto deps extraction".to_string(),
        });
    }

    if top_module.is_empty() {
        return Err(WaveAnalyzerError::DepsError {
            message: "Cannot auto-extract deps.yaml: top_module not specified in run_summary.json"
                .to_string(),
        });
    }

    // Use the first .v file directory as the RTL path
    let rtl_path = &v_files[0];
    let output_path = base_dir.join("deps.yaml");

    let result = run_deps_extractor(
        rtl_path,
        top_module,
        Some("pyverilog"),
        None,
        Some(output_path.to_str().unwrap_or("deps.yaml")),
        None,
    )?;

    let deps_path = PathBuf::from(&result.deps_yaml_path);
    if deps_path.exists() {
        Ok(deps_path)
    } else {
        Err(WaveAnalyzerError::DepsError {
            message: format!(
                "Auto-extraction completed but deps.yaml not found at {}",
                deps_path.display()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SIMPLE_REG_VCD: &str = "\
$date 2026-05-09 $end\n\
$version minimal example $end\n\
$timescale 1ns $end\n\
$scope module TOP $end\n\
$var wire 1 ! clk $end\n\
$var wire 1 \" enable $end\n\
$var wire 8 # data_i $end\n\
$var wire 8 $ data_o $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0\"\n\
b00000000 #\n\
b00000000 $\n\
#10\n\
1!\n\
0\"\n\
b01011010 #\n\
#20\n\
0!\n\
#30\n\
1!\n\
0\"\n\
b00000000 $\n";

    const SIMPLE_REG_DEPS_YAML: &str = r#"
format_version: "1.0"
description: "simple_reg minimal reference example"

signal_aliases:
  - canonical: "TOP.data_o"
    modelsim: "TOP.data_o"
  - canonical: "TOP.data_i"
    modelsim: "TOP.data_i"
  - canonical: "TOP.enable"
    modelsim: "TOP.enable"

clock_aliases:
  - clock_name: "clk"
    modelsim: "TOP.clk"

dependencies:
  - output: "TOP.data_o"
    category: data
    depends_on:
      - signal: "TOP.data_i"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "TOP.enable"
        type: control
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: ">0"

  - output: "TOP.enable"
    category: control
    depends_on:
      - signal: "TOP.enable"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null

  - output: "TOP.data_i"
    category: data
    depends_on:
      - signal: "TOP.data_i"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;

    const SIMPLE_SPEC_YAML: &str = r#"
spec_version: "1.0"
assertions:
  - name: "assert_data_transfer"
    observe_signals:
      - "TOP.data_o"
debug_hints:
  stop_signals:
    - "TOP.enable"
"#;

    const SIMPLE_TRANSCRIPT: &str = "\
# ** Error: (vsim-10142) TOP.tb_top.assert_data_transfer:\n\
#    Time: 30 ns  Scope: TOP File: tb/tb_top.sv Line: 42\n";

    fn write_file(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write test file");
        path
    }

    fn write_run_summary(dir: &Path, status: &str) -> PathBuf {
        let json = format!(
            r#"{{
  "status": "{}",
  "project_name": "test_proj",
  "top_module": "TOP",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 1,
  "warning_count": 0,
  "error_count": 1,
  "wave_file": "dump.vcd",
  "wave_format": "vcd",
  "transcript_file": "transcript.log",
  "simulator": "modelsim",
  "finished_at": "2026-05-26T10:00:00"
}}"#,
            status
        );
        write_file(dir, "run_summary.json", &json)
    }

    #[test]
    fn analyze_run_should_short_circuit_non_assertion_status() {
        let dir = TempDir::new().expect("temp dir");
        let run_summary_path = write_run_summary(dir.path(), "compile_failed");

        let request = AnalyzeRunRequest {
            run_summary_path: run_summary_path.display().to_string(),
            deps_file: None,
            spec_file: None,
            transcript_file: None,
            waveform_file: None,
            severity_filter: None,
            max_depth: None,
            simulator: None,
            report_dir: None,
            report_format: Some("none".to_string()),
            penetrate_cdc: None,
            cdc_max_depth: None,
            cdc_min_sync_stages: None,
        };

        let result = analyze_run(&request).expect("analyze run");
        assert_eq!(result.run_status, "compile_failed");
        assert!(result.traces.is_empty());
        assert!(result.aggregated_candidates.is_empty());
    }

    #[test]
    fn analyze_run_should_use_design_spec_entries_when_available() {
        let dir = TempDir::new().expect("temp dir");
        let run_summary_path = write_run_summary(dir.path(), "assertion_failed");
        write_file(dir.path(), "dump.vcd", SIMPLE_REG_VCD);
        write_file(dir.path(), "deps.yaml", SIMPLE_REG_DEPS_YAML);
        write_file(dir.path(), "design_spec.yaml", SIMPLE_SPEC_YAML);
        write_file(dir.path(), "transcript.log", SIMPLE_TRANSCRIPT);

        let request = AnalyzeRunRequest {
            run_summary_path: run_summary_path.display().to_string(),
            deps_file: None,
            spec_file: None,
            transcript_file: None,
            waveform_file: None,
            severity_filter: None,
            max_depth: Some(6),
            simulator: None,
            report_dir: None,
            report_format: Some("none".to_string()),
            penetrate_cdc: None,
            cdc_max_depth: None,
            cdc_min_sync_stages: None,
        };

        let result = analyze_run(&request).expect("analyze run");
        assert_eq!(result.selected_events.len(), 1);
        assert_eq!(result.entry_resolution[0].strategy, "design_spec");
        assert_eq!(
            result.entry_resolution[0].primary_entry_signal.as_deref(),
            Some("TOP.data_o")
        );
        assert_eq!(result.traces.len(), 1);
        assert!(result.traces[0].error.is_none());
        assert!(result.traces[0].result.is_some());
    }

    #[test]
    fn analyze_run_should_fallback_to_suggested_entries_without_spec() {
        let dir = TempDir::new().expect("temp dir");
        let run_summary_path = write_run_summary(dir.path(), "assertion_failed");
        write_file(dir.path(), "dump.vcd", SIMPLE_REG_VCD);
        write_file(dir.path(), "deps.yaml", SIMPLE_REG_DEPS_YAML);
        write_file(dir.path(), "transcript.log", SIMPLE_TRANSCRIPT);

        let request = AnalyzeRunRequest {
            run_summary_path: run_summary_path.display().to_string(),
            deps_file: None,
            spec_file: None,
            transcript_file: None,
            waveform_file: None,
            severity_filter: None,
            max_depth: Some(6),
            simulator: None,
            report_dir: None,
            report_format: Some("none".to_string()),
            penetrate_cdc: None,
            cdc_max_depth: None,
            cdc_min_sync_stages: None,
        };

        let result = analyze_run(&request).expect("analyze run");
        assert_eq!(result.entry_resolution[0].strategy, "suggest_entry_signals");
        assert!(result.entry_resolution[0].primary_entry_signal.is_some());
        assert_eq!(result.traces.len(), 1);
    }

    #[test]
    fn analyze_run_should_write_reports_when_enabled() {
        let dir = TempDir::new().expect("temp dir");
        let run_summary_path = write_run_summary(dir.path(), "assertion_failed");
        write_file(dir.path(), "dump.vcd", SIMPLE_REG_VCD);
        write_file(dir.path(), "deps.yaml", SIMPLE_REG_DEPS_YAML);
        write_file(dir.path(), "transcript.log", SIMPLE_TRANSCRIPT);

        let report_dir = dir.path().join("reports");
        let request = AnalyzeRunRequest {
            run_summary_path: run_summary_path.display().to_string(),
            deps_file: None,
            spec_file: None,
            transcript_file: None,
            waveform_file: None,
            severity_filter: None,
            max_depth: Some(6),
            simulator: None,
            report_dir: Some(report_dir.display().to_string()),
            report_format: Some("markdown".to_string()),
            penetrate_cdc: None,
            cdc_max_depth: None,
            cdc_min_sync_stages: None,
        };

        let result = analyze_run(&request).expect("analyze run");
        assert!(!result.report_outputs.is_empty());
        for output in &result.report_outputs {
            assert!(Path::new(&output.path).exists());
        }
    }
}
