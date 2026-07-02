use std::fs;
use std::path::PathBuf;

use wave_analyzer_mcp::analysis_run::{AnalyzeRunRequest, analyze_run};
use wave_analyzer_mcp::deps_extractor::{check_environment, run_deps_extractor};
use wave_analyzer_mcp::run_summary::{parse_run_summary_from_file, suggest_next_step};
use wave_analyzer_mcp::suggest_entry_signals as suggest_entry_signals_fn;
use wave_analyzer_mcp::summary::{export_waveform_to_svg, generate_waveform_summary};
use wave_analyzer_mcp::{Command, list_signals};

use super::CliStore;
use super::help::{print_command_help_detail, print_usage_text};
use super::utils::default_svg_output_path;

pub(super) fn exec_suggest_entry(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::SuggestEntrySignals {
        waveform_id,
        deps_id,
        assertion_name,
        scope_path,
        limit,
        simulator,
    } = cmd
    else {
        unreachable!("exec_suggest_entry only handles SuggestEntrySignals");
    };

    let sim = simulator.as_deref().unwrap_or("modelsim");

    // Infer aliases from waveform hierarchy before suggestion.
    // Access waveforms and dep_graphs directly (separate field borrows).
    {
        let dep_graph_mut = store
            .dep_graphs
            .get_mut(deps_id)
            .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;
        if let Some(waveform) = store.waveforms.get(waveform_id) {
            dep_graph_mut.infer_aliases_from_waveform(waveform.hierarchy(), sim);
        }
    }

    let waveform = store
        .get(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    let hierarchy = waveform.hierarchy();

    let dep_graph = store
        .dep_graphs
        .get(deps_id)
        .ok_or_else(|| format!("Dependency graph not found: {}", deps_id))?;

    let lim = limit.unwrap_or(10);

    let candidates = suggest_entry_signals_fn(
        hierarchy,
        dep_graph,
        assertion_name.as_deref(),
        scope_path.as_deref(),
        sim,
        lim,
    );

    if candidates.is_empty() {
        return Ok(
            "No candidate entry signals found. Ensure waveform and deps_id match the same design."
                .to_string(),
        );
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "Found {} candidate entry signals:",
        candidates.len()
    ));
    lines.push(String::new());
    for c in &candidates {
        let tier_str = match c.tier {
            1 => "T1:deps-output",
            2 => "T2:deps-boundary",
            3 => "T3:not-in-deps",
            _ => "unknown",
        };
        let match_str = if c.matches_assertion {
            " [assertion-match]"
        } else {
            ""
        };
        let fan_in_str = c
            .fan_in_count
            .map(|n| format!(" fan_in={}", n))
            .unwrap_or_default();
        let types_str = if c.dep_types.is_empty() {
            String::new()
        } else {
            format!(" types=[{}]", c.dep_types.join(","))
        };
        lines.push(format!(
            "  {} [{}]{}{}{}{}{}",
            c.signal_path,
            tier_str,
            match_str,
            fan_in_str,
            types_str,
            c.category
                .as_ref()
                .map(|cat| format!(" category={}", cat))
                .unwrap_or_default(),
            c.reason,
        ));
    }
    Ok(lines.join("\n"))
}

pub(super) fn exec_summary(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::GenerateSummary {
        waveform_id,
        signals,
        max_samples,
    } = cmd
    else {
        unreachable!("exec_summary only handles GenerateSummary");
    };

    // BUG-filename fix: get original file path before mutable borrow
    let filename = store
        .original_filenames
        .get(waveform_id)
        .cloned()
        .unwrap_or_else(|| waveform_id.to_string());

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let signal_paths = if signals.is_empty() {
        // Auto-detect: get first 5 top-level signals
        let hierarchy = waveform.hierarchy();
        list_signals(hierarchy, None, None, false, Some(5)).unwrap_or_default()
    } else {
        signals.clone()
    };

    let max_samples = max_samples.unwrap_or(100);
    let waveform_id_clone = waveform_id.clone();

    let summary = generate_waveform_summary(
        waveform,
        &waveform_id_clone,
        &signal_paths,
        Some(max_samples),
    )
    .map_err(|e| format!("Failed to generate summary: {}", e))?;

    // Override filename with the actual file path (not alias)
    let mut summary = summary;
    summary.filename = filename;

    Ok(serde_json::to_string_pretty(&summary).unwrap_or_else(|_| format!("{:?}", summary)))
}

pub(super) fn exec_export_svg(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::ExportSvg {
        waveform_id,
        signals,
        time_range,
        width,
        height,
    } = cmd
    else {
        unreachable!("exec_export_svg only handles ExportSvg");
    };

    let waveform = store
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;

    let time_range_parsed: Option<(u64, u64)> = time_range.as_ref().and_then(|tr| {
        let parts: Vec<&str> = tr.split(',').collect();
        if parts.len() == 2 {
            let start = parts[0].parse().ok()?;
            let end = parts[1].parse().ok()?;
            Some((start, end))
        } else {
            None
        }
    });

    let response = export_waveform_to_svg(
        waveform,
        signals,
        time_range_parsed,
        Some(width.unwrap_or(800)),
        Some(height.unwrap_or(600)),
    )
    .map_err(|e| format!("Failed to export SVG: {}", e))?;

    let output_path = default_svg_output_path(waveform_id, signals);
    fs::write(&output_path, &response.svg_content)
        .map_err(|e| format!("Failed to save SVG to '{}': {}", output_path.display(), e))?;

    Ok(format!(
        "SVG exported successfully.\nFile: {}\nSVG length: {} characters",
        output_path.display(),
        response.svg_content.len()
    ))
}

pub(super) fn exec_load_run_summary(
    store: &mut CliStore,
    file_path: &str,
    alias: Option<String>,
) -> Result<String, String> {
    let path = PathBuf::from(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    let run_summary = parse_run_summary_from_file(&path)
        .map_err(|e| format!("Failed to parse run_summary.json: {}", e))?;

    let id = alias.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let next_step = suggest_next_step(&run_summary);

    let summary = format!(
        "Run summary loaded with id: {}\n\
        Status: {}\n\
        Project: {}, Top module: {}\n\
        Compile OK: {}, Elab OK: {}, Simulation OK: {}\n\
        Assertion/check failures: {}, Warnings: {}, Errors: {}\n\
        Wave file: {} ({})\n\
        Transcript: {}\n\
        Simulator: {}\n\
        Finished at: {}\n\
        Suggested next step: {}",
        id,
        run_summary.status,
        run_summary.project_name,
        run_summary.top_module,
        run_summary.compile_ok,
        run_summary.elab_ok,
        run_summary.simulation_ok,
        run_summary.assertion_fail_count,
        run_summary.warning_count,
        run_summary.error_count,
        run_summary.wave_file,
        run_summary.wave_format,
        run_summary.transcript_file,
        run_summary.simulator,
        run_summary.finished_at,
        next_step,
    );

    store.run_summaries.insert(id.clone(), run_summary);
    Ok(summary)
}

pub(super) fn exec_analyze_run(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AnalyzeRun {
        run_summary_path,
        deps_file,
        spec_file,
        transcript_file,
        waveform_file,
        severity_filter,
        max_depth,
        simulator,
        report_dir,
        report_format,
    } = cmd
    else {
        unreachable!("exec_analyze_run only handles AnalyzeRun");
    };

    let request = AnalyzeRunRequest {
        run_summary_path: run_summary_path.clone(),
        deps_file: deps_file.clone(),
        spec_file: spec_file.clone(),
        transcript_file: transcript_file.clone(),
        waveform_file: waveform_file.clone(),
        severity_filter: severity_filter.clone(),
        max_depth: *max_depth,
        simulator: simulator.clone(),
        report_dir: report_dir.clone(),
        report_format: report_format.clone(),
        penetrate_cdc: None,
        cdc_max_depth: None,
        cdc_min_sync_stages: None,
    };

    let result = analyze_run(&request)?;
    for trace in &result.traces {
        if let Some(trace_result) = &trace.result {
            store
                .bfs_results
                .insert(trace.trace_id.clone(), trace_result.clone());
        }
    }
    Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.summary.clone()))
}

pub(super) fn exec_help(cmd: &Command) -> Result<String, String> {
    let Command::Help { command_name } = cmd else {
        unreachable!("exec_help only handles Help");
    };

    if let Some(name) = command_name {
        Ok(print_command_help_detail(name))
    } else {
        Ok(print_usage_text())
    }
}

pub(super) fn exec_extract_deps(cmd: &Command) -> Result<String, String> {
    let Command::ExtractDeps {
        rtl_path,
        top_module,
        engine,
        annotations_path,
        output_path,
        deps_extractor_path,
    } = cmd
    else {
        unreachable!("exec_extract_deps only handles ExtractDeps");
    };

    let result = run_deps_extractor(
        rtl_path,
        top_module,
        engine.as_deref(),
        annotations_path.as_deref(),
        output_path.as_deref(),
        deps_extractor_path.as_deref(),
    )
    .map_err(|e| format!("Error extracting dependencies: {}", e))?;

    Ok(format!(
        "Dependencies extracted successfully.\nEngine: {}\nOutput: {}",
        result.engine, result.deps_yaml_path,
    ))
}

pub(super) fn exec_check_env() -> Result<String, String> {
    Ok(check_environment())
}
