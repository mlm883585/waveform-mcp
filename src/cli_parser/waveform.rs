use super::Command;
use super::common::{parse_non_negative_limit, parse_positive_limit};

pub(super) fn parse_open_waveform(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("open_waveform requires a file path".to_string());
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
            _ => return Err(format!("Unknown option '{}' for open_waveform", args[i])),
        }
        i += 1;
    }

    Ok(Command::OpenWaveform { file_path, alias })
}

pub(super) fn parse_close_waveform(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("close_waveform requires a waveform_id".to_string());
    }

    Ok(Command::CloseWaveform {
        waveform_id: args[0].clone(),
    })
}

pub(super) fn parse_list_signals(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("list_signals requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut name_pattern = None;
    let mut hierarchy_prefix = None;
    let mut recursive = true;
    let mut limit = Some(100isize);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pattern" | "-p" => {
                i += 1;
                if i < args.len() {
                    name_pattern = Some(args[i].clone());
                } else {
                    return Err("--pattern requires a value".to_string());
                }
            }
            "--hierarchy" | "-H" => {
                i += 1;
                if i < args.len() {
                    hierarchy_prefix = Some(args[i].clone());
                } else {
                    return Err("--hierarchy requires a value".to_string());
                }
            }
            "--recursive" | "-r" => {
                i += 1;
                if i < args.len() {
                    recursive = args[i].parse().unwrap_or(true);
                } else {
                    return Err("--recursive requires a value (true/false)".to_string());
                }
            }
            "--limit" | "-l" => {
                i += 1;
                if i < args.len() {
                    limit = Some(parse_positive_limit(&args[i])?);
                } else {
                    return Err("--limit requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for list_signals", args[i])),
        }
        i += 1;
    }

    Ok(Command::ListSignals {
        waveform_id,
        name_pattern,
        hierarchy_prefix,
        recursive,
        limit,
    })
}

pub(super) fn parse_read_hierarchy(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("read_hierarchy requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut scope_path = None;
    let mut recursive = false;
    let mut limit = Some(200isize);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scope" | "--hierarchy" | "-s" => {
                i += 1;
                if i < args.len() {
                    scope_path = Some(args[i].clone());
                } else {
                    return Err("--scope requires a value".to_string());
                }
            }
            "--recursive" | "-r" => {
                i += 1;
                if i < args.len() {
                    recursive = args[i].parse().unwrap_or(false);
                } else {
                    return Err("--recursive requires a value (true/false)".to_string());
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
            _ => return Err(format!("Unknown option '{}' for read_hierarchy", args[i])),
        }
        i += 1;
    }

    Ok(Command::ReadHierarchy {
        waveform_id,
        scope_path,
        recursive,
        limit,
    })
}

pub(super) fn parse_read_signal(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("read_signal requires waveform_id and signal_path".to_string());
    }

    let waveform_id = args[0].clone();
    let signal_path = args[1].clone();
    let mut time_index = None;
    let mut time_indices = None;

    let mut i = 2;
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
            "--time-indices" | "-T" => {
                i += 1;
                if i < args.len() {
                    time_indices =
                        Some(args[i].split(',').filter_map(|s| s.parse().ok()).collect());
                } else {
                    return Err("--time-indices requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for read_signal", args[i])),
        }
        i += 1;
    }

    Ok(Command::ReadSignal {
        waveform_id,
        signal_path,
        time_index,
        time_indices,
    })
}

pub(super) fn parse_get_signal_info(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("get_signal_info requires waveform_id and signal_path".to_string());
    }

    Ok(Command::GetSignalInfo {
        waveform_id: args[0].clone(),
        signal_path: args[1].clone(),
    })
}

pub(super) fn parse_find_signal_events(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("find_signal_events requires waveform_id and signal_path".to_string());
    }

    let waveform_id = args[0].clone();
    let signal_path = args[1].clone();
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut start_time_value = None;
    let mut end_time_value = None;
    let mut time_unit = None;
    let mut limit = Some(100isize);

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--start" | "-s" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start requires a value".to_string());
                }
            }
            "--end" | "-e" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end requires a value".to_string());
                }
            }
            "--start-time" => {
                i += 1;
                if i < args.len() {
                    start_time_value = args[i].parse().ok();
                } else {
                    return Err("--start-time requires a value (e.g., 50.5)".to_string());
                }
            }
            "--end-time" => {
                i += 1;
                if i < args.len() {
                    end_time_value = args[i].parse().ok();
                } else {
                    return Err("--end-time requires a value (e.g., 100.0)".to_string());
                }
            }
            "--time-unit" | "-t" => {
                i += 1;
                if i < args.len() {
                    time_unit = Some(args[i].clone());
                } else {
                    return Err("--time-unit requires a value (ps/ns/us/ms/s)".to_string());
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
                    "Unknown option '{}' for find_signal_events",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    // Validate: time-value options require time-unit
    if (start_time_value.is_some() || end_time_value.is_some()) && time_unit.is_none() {
        return Err("--time-unit is required when using --start-time or --end-time".to_string());
    }

    Ok(Command::FindSignalEvents {
        waveform_id,
        signal_path,
        start_time_index,
        end_time_index,
        start_time_value,
        end_time_value,
        time_unit,
        limit,
    })
}

pub(super) fn parse_find_conditional_events(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("find_conditional_events requires waveform_id and condition".to_string());
    }

    let waveform_id = args[0].clone();
    let condition = args[1].clone();
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(100isize);

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--start" | "-s" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start requires a value".to_string());
                }
            }
            "--end" | "-e" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end requires a value".to_string());
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
                    "Unknown option '{}' for find_conditional_events",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::FindConditionalEvents {
        waveform_id,
        condition,
        start_time_index,
        end_time_index,
        limit,
    })
}
