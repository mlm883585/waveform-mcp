use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wave_analyzer_mcp::{
    Command, find_conditional_events, find_signal_events_by_path, list_signals,
    read_signal_values_by_path,
};

use super::{CliStore, report_cmds};

const PROTOCOL: &str = "wave-analyzer-agent-stdio-json";
const PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Deserialize)]
struct AgentRequest {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct AgentResponse {
    id: Value,
    ok: bool,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AgentError>,
}

#[derive(Debug, Serialize)]
struct AgentError {
    code: &'static str,
    message: String,
    recoverable: bool,
}

struct MethodResult {
    data: Value,
    summary: String,
}

pub(super) fn run_stdio_json() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut store = CliStore::new();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let response = handle_line(&mut store, &line);
        let shutdown = response
            .data
            .as_ref()
            .and_then(|data| data.get("shutdown"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let encoded = serde_json::to_string(&response).map_err(|e| e.to_string())?;
        writeln!(stdout, "{encoded}").map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;

        if shutdown {
            break;
        }
    }

    Ok(())
}

fn handle_line(store: &mut CliStore, line: &str) -> AgentResponse {
    let request: AgentRequest = match serde_json::from_str(line) {
        Ok(req) => req,
        Err(e) => {
            return error_response(
                Value::Null,
                "unknown",
                AgentError {
                    code: "INVALID_ARGUMENT",
                    message: format!("Invalid JSON request: {e}"),
                    recoverable: true,
                },
            );
        }
    };

    match dispatch(store, &request) {
        Ok(result) => AgentResponse {
            id: request.id,
            ok: true,
            method: request.method,
            data: Some(result.data),
            summary: Some(result.summary),
            error: None,
        },
        Err(error) => error_response(request.id, &request.method, error),
    }
}

fn error_response(id: Value, method: &str, error: AgentError) -> AgentResponse {
    AgentResponse {
        id,
        ok: false,
        method: method.to_string(),
        data: None,
        summary: None,
        error: Some(error),
    }
}

fn dispatch(store: &mut CliStore, request: &AgentRequest) -> Result<MethodResult, AgentError> {
    match request.method.as_str() {
        "health" => Ok(success(
            json!({
                "protocol": PROTOCOL,
                "protocol_version": PROTOCOL_VERSION,
                "crate_version": env!("CARGO_PKG_VERSION"),
                "methods": method_names(),
            }),
            "Agent is ready",
        )),
        "list_methods" => Ok(success(method_catalog(), "Agent methods listed")),
        "reset_session" => {
            *store = CliStore::new();
            Ok(success(json!({"reset": true}), "Session reset"))
        }
        "shutdown" => Ok(MethodResult {
            data: json!({"shutdown": true}),
            summary: "Agent shutting down".to_string(),
        }),
        "check_env" => Ok(handle_check_env()),
        "extract_deps" => handle_extract_deps(&request.params),
        "analyze_run" => handle_analyze_run(store, &request.params),
        "open_waveform" => handle_open_waveform(store, &request.params),
        "close_waveform" => handle_close_waveform(store, &request.params),
        "list_signals" => handle_list_signals(store, &request.params),
        "read_signal" => handle_read_signal(store, &request.params),
        "find_signal_events" => handle_find_signal_events(store, &request.params),
        "find_conditional_events" => handle_find_conditional_events(store, &request.params),
        other => Err(invalid_argument(format!("Unknown method: {other}"))),
    }
}

fn success(data: Value, summary: impl Into<String>) -> MethodResult {
    MethodResult {
        data,
        summary: summary.into(),
    }
}

fn method_names() -> Vec<&'static str> {
    vec![
        "health",
        "list_methods",
        "check_env",
        "reset_session",
        "shutdown",
        "extract_deps",
        "analyze_run",
        "open_waveform",
        "close_waveform",
        "list_signals",
        "read_signal",
        "find_signal_events",
        "find_conditional_events",
    ]
}

fn method_catalog() -> Value {
    json!({
        "methods": {
            "health": {"params": []},
            "list_methods": {"params": []},
            "check_env": {"params": []},
            "reset_session": {"params": []},
            "shutdown": {"params": []},
            "extract_deps": {"params": ["rtl_path", "top_module", "engine", "annotations_path", "output_path", "deps_extractor_path"]},
            "analyze_run": {"params": ["run_summary_path", "deps_file", "spec_file", "transcript_file", "waveform_file", "severity_filter", "max_depth", "simulator", "report_dir", "report_format"]},
            "open_waveform": {"params": ["file_path", "alias"]},
            "close_waveform": {"params": ["waveform_id"]},
            "list_signals": {"params": ["waveform_id", "name_pattern", "hierarchy_prefix", "recursive", "limit"]},
            "read_signal": {"params": ["waveform_id", "signal_path", "time_index", "time_indices"]},
            "find_signal_events": {"params": ["waveform_id", "signal_path", "start_time_index", "end_time_index", "limit"]},
            "find_conditional_events": {"params": ["waveform_id", "condition", "start_time_index", "end_time_index", "limit"]}
        }
    })
}

fn handle_check_env() -> MethodResult {
    let raw_text = wave_analyzer_mcp::deps_extractor::check_environment();
    let checks = parse_env_checks(&raw_text);
    let ok = checks.iter().all(|check| {
        let status = check
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        !status.contains("FAIL") && !status.contains("MISSING") && !status.contains("NOT FOUND")
    });

    success(
        json!({
            "ok": ok,
            "checks": checks,
            "raw_text": raw_text,
        }),
        "Environment check complete",
    )
}

fn parse_env_checks(raw_text: &str) -> Vec<Value> {
    let mut checks = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_status: Option<String> = None;
    let mut current_fix: Option<String> = None;

    for line in raw_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.contains(']') {
            if let Some(name) = current_name.take() {
                checks.push(json!({
                    "name": name,
                    "status": current_status.take().unwrap_or_else(|| "UNKNOWN".to_string()),
                    "fix": current_fix.take(),
                }));
            }
            current_name = Some(trimmed.to_string());
        } else if let Some(value) = trimmed.strip_prefix("Status :") {
            current_status = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Engine :") {
            current_status = Some(value.trim().to_string());
        } else if let Some(value) = trimmed.strip_prefix("Fix    :") {
            current_fix = Some(value.trim().to_string());
        }
    }

    if let Some(name) = current_name {
        checks.push(json!({
            "name": name,
            "status": current_status.unwrap_or_else(|| "UNKNOWN".to_string()),
            "fix": current_fix,
        }));
    }

    checks
}

fn handle_extract_deps(params: &Value) -> Result<MethodResult, AgentError> {
    let rtl_path = required_str(params, "rtl_path")?;
    let top_module = required_str(params, "top_module")?;
    if !PathBuf::from(rtl_path).exists() {
        return Err(file_not_found(rtl_path));
    }
    if top_module.trim().is_empty() {
        return Err(invalid_argument("top_module must not be empty"));
    }

    let cmd = Command::ExtractDeps {
        rtl_path: rtl_path.to_string(),
        top_module: top_module.to_string(),
        engine: optional_str(params, "engine").map(str::to_string),
        annotations_path: optional_str(params, "annotations_path").map(str::to_string),
        output_path: optional_str(params, "output_path").map(str::to_string),
        deps_extractor_path: optional_str(params, "deps_extractor_path").map(str::to_string),
    };

    match report_cmds::exec_extract_deps(&cmd) {
        Ok(raw_text) => Ok(success(
            json!({
                "raw_text": raw_text,
                "engine": optional_str(params, "engine").unwrap_or("pyverilog"),
                "output_path": optional_str(params, "output_path"),
            }),
            "Dependencies extracted",
        )),
        Err(e) => Err(classify_error(e)),
    }
}

fn handle_analyze_run(store: &mut CliStore, params: &Value) -> Result<MethodResult, AgentError> {
    let run_summary_path = required_str(params, "run_summary_path")?;
    if !PathBuf::from(run_summary_path).exists() {
        return Err(file_not_found(run_summary_path));
    }

    let cmd = Command::AnalyzeRun {
        run_summary_path: run_summary_path.to_string(),
        deps_file: optional_str(params, "deps_file").map(str::to_string),
        spec_file: optional_str(params, "spec_file").map(str::to_string),
        transcript_file: optional_str(params, "transcript_file").map(str::to_string),
        waveform_file: optional_str(params, "waveform_file").map(str::to_string),
        severity_filter: optional_string_array(params, "severity_filter")?,
        max_depth: optional_usize(params, "max_depth")?.or(Some(8)),
        simulator: optional_str(params, "simulator")
            .map(str::to_string)
            .or_else(|| Some("modelsim".to_string())),
        report_dir: optional_str(params, "report_dir").map(str::to_string),
        report_format: optional_str(params, "report_format").map(str::to_string),
    };

    match report_cmds::exec_analyze_run(store, &cmd) {
        Ok(raw) => {
            let data = serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| {
                json!({
                    "raw_text": raw
                })
            });
            Ok(success(data, "Run analysis complete"))
        }
        Err(e) => Err(classify_error(e)),
    }
}

fn handle_open_waveform(store: &mut CliStore, params: &Value) -> Result<MethodResult, AgentError> {
    let file_path = required_str(params, "file_path")?;
    if !PathBuf::from(file_path).exists() {
        return Err(file_not_found(file_path));
    }
    let alias = optional_str(params, "alias").map(str::to_string);
    match store.open_waveform(file_path, alias) {
        Ok(id) => Ok(success(
            json!({
                "waveform_id": id,
                "file_path": file_path,
            }),
            "Waveform opened",
        )),
        Err(e) => Err(classify_error(e)),
    }
}

fn handle_close_waveform(store: &mut CliStore, params: &Value) -> Result<MethodResult, AgentError> {
    let waveform_id = required_str(params, "waveform_id")?;
    match store.close_waveform(waveform_id) {
        Ok(()) => Ok(success(
            json!({
                "waveform_id": waveform_id,
                "closed": true,
            }),
            "Waveform closed",
        )),
        Err(e) => Err(classify_error(e)),
    }
}

fn handle_list_signals(store: &mut CliStore, params: &Value) -> Result<MethodResult, AgentError> {
    let waveform_id = required_str(params, "waveform_id")?;
    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| waveform_not_found(waveform_id))?;
    let recursive = optional_bool(params, "recursive")?.unwrap_or(true);
    let limit = optional_isize(params, "limit")?.or(Some(100));
    if matches!(limit, Some(0)) {
        return Err(invalid_argument(
            "Invalid limit '0': limit must be greater than 0",
        ));
    }
    let signals = list_signals(
        waveform.hierarchy(),
        optional_str(params, "name_pattern"),
        optional_str(params, "hierarchy_prefix"),
        recursive,
        limit,
    )
    .map_err(|e| classify_error(e.to_string()))?;

    Ok(success(
        json!({
            "waveform_id": waveform_id,
            "signals": signals,
            "count": signals.len(),
        }),
        format!("Found {} signals", signals.len()),
    ))
}

fn handle_read_signal(store: &mut CliStore, params: &Value) -> Result<MethodResult, AgentError> {
    let waveform_id = required_str(params, "waveform_id")?;
    let signal_path = required_str(params, "signal_path")?;
    let indices = if let Some(values) = optional_usize_array(params, "time_indices")? {
        values
    } else if let Some(index) = optional_usize(params, "time_index")? {
        vec![index]
    } else {
        return Err(invalid_argument(
            "Either time_index or time_indices must be provided",
        ));
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| waveform_not_found(waveform_id))?;
    let values = read_signal_values_by_path(waveform, signal_path, &indices)
        .map_err(|e| classify_error(e.to_string()))?;

    Ok(success(
        json!({
            "waveform_id": waveform_id,
            "signal_path": signal_path,
            "time_indices": indices,
            "values": values,
        }),
        "Signal values read",
    ))
}

fn handle_find_signal_events(
    store: &mut CliStore,
    params: &Value,
) -> Result<MethodResult, AgentError> {
    let waveform_id = required_str(params, "waveform_id")?;
    let signal_path = required_str(params, "signal_path")?;
    let limit = optional_isize(params, "limit")?.or(Some(100));
    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| waveform_not_found(waveform_id))?;
    let end_default = waveform.time_table().len().saturating_sub(1);
    let start = optional_usize(params, "start_time_index")?.unwrap_or(0);
    let end = optional_usize(params, "end_time_index")?.unwrap_or(end_default);
    let events = find_signal_events_by_path(waveform, signal_path, start, end, limit.unwrap_or(-1))
        .map_err(|e| classify_error(e.to_string()))?;

    Ok(success(
        json!({
            "waveform_id": waveform_id,
            "signal_path": signal_path,
            "start_time_index": start,
            "end_time_index": end,
            "events": events,
            "count": events.len(),
        }),
        format!("Found {} signal events", events.len()),
    ))
}

fn handle_find_conditional_events(
    store: &mut CliStore,
    params: &Value,
) -> Result<MethodResult, AgentError> {
    let waveform_id = required_str(params, "waveform_id")?;
    let condition = required_str(params, "condition")?;
    let limit = optional_isize(params, "limit")?.or(Some(100));
    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| waveform_not_found(waveform_id))?;
    let end_default = waveform.time_table().len().saturating_sub(1);
    let start = optional_usize(params, "start_time_index")?.unwrap_or(0);
    let end = optional_usize(params, "end_time_index")?.unwrap_or(end_default);
    let events = find_conditional_events(waveform, condition, start, end, limit.unwrap_or(-1))
        .map_err(|e| classify_error(e.to_string()))?;

    Ok(success(
        json!({
            "waveform_id": waveform_id,
            "condition": condition,
            "start_time_index": start,
            "end_time_index": end,
            "events": events,
            "count": events.len(),
        }),
        format!("Found {} conditional events", events.len()),
    ))
}

fn required_str<'a>(params: &'a Value, field: &str) -> Result<&'a str, AgentError> {
    optional_str(params, field)
        .ok_or_else(|| invalid_argument(format!("Missing required parameter: {field}")))
}

fn optional_str<'a>(params: &'a Value, field: &str) -> Option<&'a str> {
    params.get(field).and_then(Value::as_str)
}

fn optional_bool(params: &Value, field: &str) -> Result<Option<bool>, AgentError> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| invalid_argument(format!("{field} must be a boolean"))),
    }
}

fn optional_usize(params: &Value, field: &str) -> Result<Option<usize>, AgentError> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| invalid_argument(format!("{field} must be a non-negative integer"))),
    }
}

fn optional_isize(params: &Value, field: &str) -> Result<Option<isize>, AgentError> {
    match params.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .and_then(|n| isize::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| invalid_argument(format!("{field} must be an integer"))),
    }
}

fn optional_usize_array(params: &Value, field: &str) -> Result<Option<Vec<usize>>, AgentError> {
    let Some(value) = params.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let values = value
        .as_array()
        .ok_or_else(|| invalid_argument(format!("{field} must be an array")))?;
    let mut parsed = Vec::with_capacity(values.len());
    for item in values {
        let n = item
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| {
                invalid_argument(format!("{field} must contain non-negative integers"))
            })?;
        parsed.push(n);
    }
    Ok(Some(parsed))
}

fn optional_string_array(params: &Value, field: &str) -> Result<Option<Vec<String>>, AgentError> {
    let Some(value) = params.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let values = value
        .as_array()
        .ok_or_else(|| invalid_argument(format!("{field} must be an array of strings")))?;
    let mut parsed = Vec::with_capacity(values.len());
    for item in values {
        let s = item
            .as_str()
            .ok_or_else(|| invalid_argument(format!("{field} must be an array of strings")))?;
        parsed.push(s.to_string());
    }
    Ok(Some(parsed))
}

fn classify_error(message: String) -> AgentError {
    if let Some(path) = message.strip_prefix("File not found: ") {
        return file_not_found(path);
    }
    if message.contains("Waveform not found") {
        return AgentError {
            code: "WAVEFORM_NOT_FOUND",
            message,
            recoverable: true,
        };
    }
    if message.contains("Dependency graph not found") {
        return AgentError {
            code: "DEPS_NOT_FOUND",
            message,
            recoverable: true,
        };
    }
    if message.contains("iverilog")
        || message.contains("VC++ Runtime")
        || message.contains("sidecar")
        || message.contains("Could not locate deps-extractor")
    {
        return AgentError {
            code: "ENV_MISSING_DEPENDENCY",
            message,
            recoverable: true,
        };
    }
    if message.contains("Invalid") || message.contains("Either ") || message.contains("Unknown") {
        return AgentError {
            code: "INVALID_ARGUMENT",
            message,
            recoverable: true,
        };
    }
    AgentError {
        code: "TOOL_FAILED",
        message,
        recoverable: true,
    }
}

fn invalid_argument(message: impl Into<String>) -> AgentError {
    AgentError {
        code: "INVALID_ARGUMENT",
        message: message.into(),
        recoverable: true,
    }
}

fn file_not_found(path: &str) -> AgentError {
    AgentError {
        code: "FILE_NOT_FOUND",
        message: format!("File not found: {path}"),
        recoverable: true,
    }
}

fn waveform_not_found(id: &str) -> AgentError {
    AgentError {
        code: "WAVEFORM_NOT_FOUND",
        message: format!("Waveform not found: {id}"),
        recoverable: true,
    }
}
