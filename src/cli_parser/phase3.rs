use super::Command;
use super::common::parse_non_negative_limit;

pub(super) fn parse_load_deps(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("load_deps requires a file path".to_string());
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
            _ => return Err(format!("Unknown option '{}' for load_deps", args[i])),
        }
        i += 1;
    }

    Ok(Command::LoadDependencies { file_path, alias })
}

pub(super) fn parse_load_assertion_log(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("load_assertion_log requires a file path".to_string());
    }

    let file_path = args[0].clone();
    let mut alias = None;
    let mut severity_filter = None;
    let mut limit = Some(100isize);

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
            "--severity-filter" | "-s" => {
                i += 1;
                if i < args.len() {
                    severity_filter = Some(args[i].split(',').map(String::from).collect());
                } else {
                    return Err("--severity-filter requires a value".to_string());
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
            _ => {
                return Err(format!(
                    "Unknown option '{}' for load_assertion_log",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::LoadAssertionLog {
        file_path,
        alias,
        severity_filter,
        limit,
    })
}

pub(super) fn parse_load_spec(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("load_spec requires a file path".to_string());
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
            _ => return Err(format!("Unknown option '{}' for load_spec", args[i])),
        }
        i += 1;
    }

    Ok(Command::LoadDesignSpec { file_path, alias })
}

pub(super) fn parse_trace_root_cause(args: &[String]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("trace_root_cause requires waveform_id, deps_id, and signal_path".to_string());
    }

    let waveform_id = args[0].clone();
    let deps_id = args[1].clone();
    let signal_path = args[2].clone();
    let mut time_index = None;
    let mut time_value = None;
    let mut time_unit = None;
    let mut spec_id = None;
    let mut max_depth = Some(8usize);
    let mut simulator = Some("modelsim".to_string());
    let mut penetrate_cdc = None;
    let mut cdc_max_depth = None;
    let mut cdc_min_sync_stages = None;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--time-index" | "-t" => {
                i += 1;
                if i < args.len() {
                    time_index = args[i].parse().ok();
                } else {
                    return Err("--time-index requires a value".to_string());
                }
            }
            "--time-value" => {
                i += 1;
                if i < args.len() {
                    time_value = args[i].parse::<f64>().ok();
                } else {
                    return Err("--time-value requires a value".to_string());
                }
            }
            "--time-unit" => {
                i += 1;
                if i < args.len() {
                    time_unit = Some(args[i].clone());
                } else {
                    return Err("--time-unit requires a value (ps/ns/us/ms/s)".to_string());
                }
            }
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
            "--simulator" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            "--penetrate-cdc" => {
                penetrate_cdc = Some(true);
            }
            "--no-penetrate-cdc" => {
                penetrate_cdc = Some(false);
            }
            "--cdc-max-depth" => {
                i += 1;
                if i < args.len() {
                    cdc_max_depth = args[i].parse().ok();
                } else {
                    return Err("--cdc-max-depth requires a value".to_string());
                }
            }
            "--cdc-min-sync-stages" => {
                i += 1;
                if i < args.len() {
                    cdc_min_sync_stages = args[i].parse().ok();
                } else {
                    return Err("--cdc-min-sync-stages requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for trace_root_cause", args[i])),
        }
        i += 1;
    }

    Ok(Command::TraceRootCause {
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
    })
}

pub(super) fn parse_find_fan_in(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("find_fan_in requires deps_id and signal_path".to_string());
    }

    let deps_id = args[0].clone();
    let signal_path = args[1].clone();
    let mut simulator = Some("modelsim".to_string());

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--simulator" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for find_fan_in", args[i])),
        }
        i += 1;
    }

    Ok(Command::FindFanIn {
        deps_id,
        signal_path,
        simulator,
    })
}

pub(super) fn parse_find_fan_out(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("find_fan_out requires a deps_id".to_string());
    }

    let deps_id = args[0].clone();
    if args.len() < 2 {
        return Err("find_fan_out requires a signal_path".to_string());
    }
    let signal_path = args[1].clone();
    let mut simulator = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--simulator" | "-s" => {
                i += 1;
                if i < args.len() {
                    simulator = Some(args[i].clone());
                } else {
                    return Err("--simulator requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for find_fan_out", args[i])),
        }
        i += 1;
    }

    Ok(Command::FindFanOut {
        deps_id,
        signal_path,
        simulator,
    })
}
