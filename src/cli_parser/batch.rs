use super::Command;
use super::common::parse_non_negative_limit;

pub(super) fn parse_suggest_entry_signals(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("suggest_entry_signals requires waveform_id and deps_id".to_string());
    }

    let waveform_id = args[0].clone();
    let deps_id = args[1].clone();
    let mut assertion_name = None;
    let mut scope_path = None;
    let mut limit = Some(10isize);
    let mut simulator = Some("modelsim".to_string());

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--assertion" | "-a" => {
                i += 1;
                if i < args.len() {
                    assertion_name = Some(args[i].clone());
                } else {
                    return Err("--assertion requires a value".to_string());
                }
            }
            "--scope" | "-s" => {
                i += 1;
                if i < args.len() {
                    scope_path = Some(args[i].clone());
                } else {
                    return Err("--scope requires a value".to_string());
                }
            }
            "--limit" | "-l" => {
                i += 1;
                if i < args.len() {
                    limit = Some(parse_non_negative_limit(&args[i])?);
                } else {
                    return Err("--limit requires a value".to_string());
                }
            }
            "--simulator" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for suggest_entry_signals",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::SuggestEntrySignals {
        waveform_id,
        deps_id,
        assertion_name,
        scope_path,
        limit,
        simulator,
    })
}

pub(super) fn parse_generate_summary(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("generate_summary requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signals = Vec::new();
    let mut max_samples = Some(100usize);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signal" | "-s" => {
                i += 1;
                if i < args.len() {
                    signals.push(args[i].clone());
                } else {
                    return Err("--signal requires a value".to_string());
                }
            }
            "--max-samples" => {
                i += 1;
                if i < args.len() {
                    max_samples = args[i].parse().ok();
                } else {
                    return Err("--max-samples requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for generate_summary", args[i])),
        }
        i += 1;
    }

    Ok(Command::GenerateSummary {
        waveform_id,
        signals,
        max_samples,
    })
}

pub(super) fn parse_export_svg(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("export_svg requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signals = Vec::new();
    let mut time_range = None;
    let mut width = Some(800u32);
    let mut height = Some(600u32);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signal" | "-s" => {
                i += 1;
                if i < args.len() {
                    signals.push(args[i].clone());
                } else {
                    return Err("--signal requires a value".to_string());
                }
            }
            "--time-range" | "-t" => {
                i += 1;
                if i < args.len() {
                    time_range = Some(args[i].clone());
                } else {
                    return Err("--time-range requires a value (start,end)".to_string());
                }
            }
            "--width" | "-w" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().ok();
                } else {
                    return Err("--width requires a value".to_string());
                }
            }
            "--height" => {
                i += 1;
                if i < args.len() {
                    height = args[i].parse().ok();
                } else {
                    return Err("--height requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for export_svg", args[i])),
        }
        i += 1;
    }

    Ok(Command::ExportSvg {
        waveform_id,
        signals,
        time_range,
        width,
        height,
    })
}

pub(super) fn parse_batch_trace_root_cause(args: &[String]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(
            "batch_trace_root_cause requires waveform_id, deps_id, and assertion_id".to_string(),
        );
    }

    let waveform_id = args[0].clone();
    let deps_id = args[1].clone();
    let assertion_id = args[2].clone();
    let mut spec_id = None;
    let mut max_depth = Some(8usize);
    let mut severity_filter = None;
    let mut simulator = Some("modelsim".to_string());

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--spec-id" => {
                i += 1;
                if i < args.len() {
                    spec_id = Some(args[i].clone());
                } else {
                    return Err("--spec-id requires a value".to_string());
                }
            }
            "--max-depth" | "-d" => {
                i += 1;
                if i < args.len() {
                    max_depth = args[i].parse().ok();
                } else {
                    return Err("--max-depth requires a value".to_string());
                }
            }
            "--severity-filter" | "-s" => {
                i += 1;
                if i < args.len() {
                    severity_filter = Some(args[i].clone());
                } else {
                    return Err("--severity-filter requires a value".to_string());
                }
            }
            "--simulator" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for batch_trace_root_cause",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::BatchTraceRootCause {
        waveform_id,
        deps_id,
        assertion_id,
        spec_id,
        max_depth,
        severity_filter,
        simulator,
    })
}

pub(super) fn parse_export_bfs_report(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("export_bfs_report requires a trace_id".to_string());
    }

    let trace_id = args[0].clone();
    let mut format = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--format" | "-f" => {
                i += 1;
                if i < args.len() {
                    format = Some(args[i].clone());
                } else {
                    return Err("--format requires a value (json/markdown/html)".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for export_bfs_report",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::ExportBfsReport { trace_id, format })
}

pub(super) fn parse_load_run_summary(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("load_run_summary requires a file path".to_string());
    }

    let file_path = args[0].clone();
    let mut alias = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--alias" | "-a" => {
                i += 1;
                if i < args.len() {
                    alias = Some(args[i].clone());
                } else {
                    return Err("--alias requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for load_run_summary", args[i])),
        }
        i += 1;
    }

    Ok(Command::LoadRunSummary { file_path, alias })
}

pub(super) fn parse_analyze_run(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("analyze_run requires a run_summary_path".to_string());
    }

    let run_summary_path = args[0].clone();
    let mut deps_file = None;
    let mut spec_file = None;
    let mut transcript_file = None;
    let mut waveform_file = None;
    let mut severity_filter = None;
    let mut max_depth = Some(8usize);
    let mut simulator = Some("modelsim".to_string());
    let mut report_dir = None;
    let mut report_format = Some("markdown".to_string());

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--deps" => {
                i += 1;
                if i < args.len() {
                    deps_file = Some(args[i].clone());
                } else {
                    return Err("--deps requires a value".to_string());
                }
            }
            "--spec" => {
                i += 1;
                if i < args.len() {
                    spec_file = Some(args[i].clone());
                } else {
                    return Err("--spec requires a value".to_string());
                }
            }
            "--transcript" => {
                i += 1;
                if i < args.len() {
                    transcript_file = Some(args[i].clone());
                } else {
                    return Err("--transcript requires a value".to_string());
                }
            }
            "--waveform" => {
                i += 1;
                if i < args.len() {
                    waveform_file = Some(args[i].clone());
                } else {
                    return Err("--waveform requires a value".to_string());
                }
            }
            "--severity-filter" => {
                i += 1;
                if i < args.len() {
                    severity_filter = Some(
                        args[i]
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    );
                } else {
                    return Err("--severity-filter requires a value".to_string());
                }
            }
            "--max-depth" => {
                i += 1;
                if i < args.len() {
                    max_depth = args[i].parse().ok();
                } else {
                    return Err("--max-depth requires a value".to_string());
                }
            }
            "--simulator" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            "--report-dir" => {
                i += 1;
                if i < args.len() {
                    report_dir = Some(args[i].clone());
                } else {
                    return Err("--report-dir requires a value".to_string());
                }
            }
            "--report-format" => {
                i += 1;
                if i < args.len() {
                    report_format = Some(args[i].clone());
                } else {
                    return Err("--report-format requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for analyze_run", args[i])),
        }
        i += 1;
    }

    Ok(Command::AnalyzeRun {
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
    })
}

pub(super) fn parse_extract_deps(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("extract_deps requires rtl_path and top_module".to_string());
    }

    let rtl_path = args[0].clone();
    let top_module = args[1].clone();
    let mut engine = None;
    let mut annotations_path = None;
    let mut output_path = None;
    let mut deps_extractor_path = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--engine" | "-e" => {
                i += 1;
                if i < args.len() {
                    engine = Some(args[i].clone());
                } else {
                    return Err("--engine requires a value (pyverilog/vivado)".to_string());
                }
            }
            "--annotate" | "-a" => {
                i += 1;
                if i < args.len() {
                    annotations_path = Some(args[i].clone());
                } else {
                    return Err("--annotate requires a path to annotations.yaml".to_string());
                }
            }
            "--output" | "-o" => {
                i += 1;
                if i < args.len() {
                    output_path = Some(args[i].clone());
                } else {
                    return Err("--output requires a path".to_string());
                }
            }
            "--deps-extractor-path" => {
                i += 1;
                if i < args.len() {
                    deps_extractor_path = Some(args[i].clone());
                } else {
                    return Err("--deps-extractor-path requires a directory path".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for extract_deps", args[i])),
        }
        i += 1;
    }

    Ok(Command::ExtractDeps {
        rtl_path,
        top_module,
        engine,
        annotations_path,
        output_path,
        deps_extractor_path,
    })
}

pub(super) fn parse_help(args: &[String]) -> Result<Command, String> {
    let command_name = if args.is_empty() {
        None
    } else {
        Some(args[0].clone())
    };

    Ok(Command::Help { command_name })
}
