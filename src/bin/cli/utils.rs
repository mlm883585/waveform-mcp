use std::fs;
use std::path::PathBuf;

use wave_analyzer_mcp::bfs::{BfsOptions, BfsResult};
use wave_analyzer_mcp::trace_root_cause;

use super::CliStore;

pub(super) fn bfs_cache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(tmp) = std::env::var_os("TEMP").or_else(|| std::env::var_os("TMP")) {
        dirs.push(
            PathBuf::from(tmp)
                .join("wave-analyzer-cli")
                .join("bfs-results"),
        );
    }

    dirs.push(
        std::env::current_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(".wave-analyzer-cli")
            .join("bfs-results"),
    );

    dirs
}

pub(super) fn bfs_result_cache_file_name(trace_id: &str) -> String {
    let mut safe = String::new();
    for ch in trace_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
    }
    format!("{}.json", safe)
}

pub(super) fn safe_output_stem(input: &str) -> String {
    let mut safe = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
    }
    if safe.is_empty() {
        "waveform".to_string()
    } else {
        safe
    }
}

pub(super) fn default_svg_output_path(waveform_id: &str, signals: &[String]) -> PathBuf {
    let mut stem = safe_output_stem(waveform_id);
    if let Some(signal) = signals.first() {
        stem.push('_');
        stem.push_str(&safe_output_stem(signal));
    }
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!("{}.svg", stem))
}

pub(super) fn persist_bfs_result(trace_id: &str, result: &BfsResult) -> Result<(), String> {
    let json = serde_json::to_string_pretty(result)
        .map_err(|e| format!("Failed to serialize BFS result: {}", e))?;

    let mut errors = Vec::new();
    let file_name = bfs_result_cache_file_name(trace_id);
    for dir in bfs_cache_dirs() {
        if let Err(e) = fs::create_dir_all(&dir) {
            errors.push(format!("{} ({})", dir.display(), e));
            continue;
        }
        let path = dir.join(&file_name);
        match fs::write(&path, &json) {
            Ok(_) => return Ok(()),
            Err(e) => errors.push(format!("{} ({})", path.display(), e)),
        }
    }

    Err(format!(
        "Failed to persist BFS result for trace_id '{}': {}",
        trace_id,
        errors.join("; ")
    ))
}

pub(super) fn load_bfs_result_from_cache(trace_id: &str) -> Result<BfsResult, String> {
    let file_name = bfs_result_cache_file_name(trace_id);
    let mut errors = Vec::new();

    for dir in bfs_cache_dirs() {
        let path = dir.join(&file_name);
        match fs::read_to_string(&path) {
            Ok(json) => {
                return serde_json::from_str(&json).map_err(|e| {
                    format!(
                        "Failed to parse cached BFS result '{}': {}",
                        path.display(),
                        e
                    )
                });
            }
            Err(e) => errors.push(format!("{} ({})", path.display(), e)),
        }
    }

    Err(format!(
        "BFS result not found for trace_id '{}'. Run trace_root_cause first. [{}]",
        trace_id,
        errors.join("; ")
    ))
}

pub(super) fn run_trace_root_cause(
    store: &mut CliStore,
    waveform_id: &str,
    deps_id: &str,
    signal_path: &str,
    time_index: usize,
    spec_id: Option<&str>,
    max_depth: Option<usize>,
    simulator: Option<&str>,
    penetrate_cdc: Option<bool>,
    cdc_max_depth: Option<usize>,
    cdc_min_sync_stages: Option<u32>,
) -> Result<(String, BfsResult), String> {
    let sim = simulator.unwrap_or("modelsim").to_string();

    // Infer aliases from waveform (always run — method only infers
    // for canonical names whose existing alias doesn't point to
    // a valid signal in this waveform)
    {
        let dep_graph = store
            .dep_graphs
            .get_mut(deps_id)
            .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;
        if let Some(waveform) = store.waveforms.get(waveform_id) {
            dep_graph.infer_aliases_from_waveform(waveform.hierarchy(), &sim);
        }
    }

    let dep_graph = store
        .dep_graphs
        .get(deps_id)
        .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;

    let depth = max_depth.unwrap_or(8);

    let mut stop_signals = Vec::new();
    if let Some(sid) = spec_id
        && let Some(spec_lookup) = store.specs.get(sid)
    {
        stop_signals = spec_lookup.find_stop_signals();
    }

    let options = BfsOptions {
        max_depth: depth,
        stop_signals,
        enable_auto_check: true,
        simulator: sim,
        penetrate_cdc: penetrate_cdc.unwrap_or(false),
        cdc_max_depth: cdc_max_depth.unwrap_or(4),
        cdc_min_sync_stages: cdc_min_sync_stages.unwrap_or(2),
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let result = trace_root_cause(waveform, dep_graph, signal_path, time_index, &options)
        .map_err(|e| format!("Error tracing root cause: {}", e))?;

    let trace_id = format!("{}_{}_{}", waveform_id, signal_path, time_index);
    store.bfs_results.insert(trace_id.clone(), result.clone());
    persist_bfs_result(&trace_id, store.bfs_results.get(&trace_id).unwrap())?;

    Ok((trace_id, result))
}

pub(super) fn trace_root_cause_json_payload(
    trace_id: &str,
    result: &BfsResult,
) -> serde_json::Value {
    serde_json::json!({
        "status": "ok",
        "command": "trace_root_cause",
        "trace_id": trace_id,
        "summary": result.summary,
        "result": result,
    })
}
