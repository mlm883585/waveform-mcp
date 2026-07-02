use super::Command;

pub(super) fn parse_analyze_cdc(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("analyze_cdc requires waveform_id".to_string());
    }
    let waveform_id = args[0].clone();
    let mut deps_id = None;
    let mut simulator = Some("modelsim".to_string());
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--deps-id" => {
                i += 1;
                if i < args.len() {
                    deps_id = Some(args[i].clone());
                } else {
                    return Err("--deps-id requires a value".to_string());
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
            _ => return Err(format!("Unknown option '{}' for analyze_cdc", args[i])),
        }
        i += 1;
    }
    Ok(Command::AnalyzeCdc {
        waveform_id,
        deps_id,
        simulator,
    })
}

pub(super) fn parse_analyze_signal_patterns(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err(
            "analyze_signal_patterns requires waveform_id and at least one signal".to_string(),
        );
    }
    let waveform_id = args[0].clone();
    let mut signals = Vec::new();
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut max_bins = None;
    let mut idle_threshold = None;
    let mut i = 1;
    // First positional arg after waveform_id could be a comma-separated signal list
    // or --signals flag
    while i < args.len() {
        match args[i].as_str() {
            "--signals" => {
                i += 1;
                if i < args.len() {
                    signals = args[i].split(',').map(|s| s.trim().to_string()).collect();
                } else {
                    return Err("--signals requires a value".to_string());
                }
            }
            "--start-time-index" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start-time-index requires a value".to_string());
                }
            }
            "--end-time-index" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end-time-index requires a value".to_string());
                }
            }
            "--max-bins" => {
                i += 1;
                if i < args.len() {
                    max_bins = args[i].parse().ok();
                } else {
                    return Err("--max-bins requires a value".to_string());
                }
            }
            "--idle-threshold" => {
                i += 1;
                if i < args.len() {
                    idle_threshold = Some(args[i].clone());
                } else {
                    return Err("--idle-threshold requires a value".to_string());
                }
            }
            _ => {
                // Treat as comma-separated signal list if signals not yet set
                if signals.is_empty() {
                    signals = args[i].split(',').map(|s| s.trim().to_string()).collect();
                } else {
                    return Err(format!(
                        "Unknown option '{}' for analyze_signal_patterns",
                        args[i]
                    ));
                }
            }
        }
        i += 1;
    }
    Ok(Command::AnalyzeSignalPatterns {
        waveform_id,
        signals,
        start_time_index,
        end_time_index,
        max_bins,
        idle_threshold,
    })
}

pub(super) fn parse_extract_fsm(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("extract_fsm requires waveform_id and signal_path".to_string());
    }
    let waveform_id = args[0].clone();
    let signal_path = args[1].clone();
    let mut clock_signal = None;
    let mut edge_type = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--clock-signal" => {
                i += 1;
                if i < args.len() {
                    clock_signal = Some(args[i].clone());
                } else {
                    return Err("--clock-signal requires a value".to_string());
                }
            }
            "--edge-type" => {
                i += 1;
                if i < args.len() {
                    edge_type = Some(args[i].clone());
                } else {
                    return Err("--edge-type requires a value".to_string());
                }
            }
            "--start-time-index" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start-time-index requires a value".to_string());
                }
            }
            "--end-time-index" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end-time-index requires a value".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for extract_fsm", args[i])),
        }
        i += 1;
    }
    Ok(Command::ExtractFsm {
        waveform_id,
        signal_path,
        clock_signal,
        edge_type,
        start_time_index,
        end_time_index,
    })
}

pub(super) fn parse_analyze_protocol(args: &[String]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(
            "analyze_protocol requires waveform_id, protocol type, and signal mapping".to_string(),
        );
    }
    let waveform_id = args[0].clone();
    let protocol = args[1].clone();
    let mut signals: Vec<(String, String)> = Vec::new();
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--signals" => {
                i += 1;
                if i < args.len() {
                    // Parse key=value pairs separated by commas
                    for pair in args[i].split(',') {
                        let pair = pair.trim();
                        if let Some(eq_pos) = pair.find('=') {
                            let key = pair[..eq_pos].trim().to_string();
                            let value = pair[eq_pos + 1..].trim().to_string();
                            signals.push((key, value));
                        } else {
                            return Err(format!(
                                "Invalid signal mapping '{}', expected key=value format",
                                pair
                            ));
                        }
                    }
                } else {
                    return Err("--signals requires a value".to_string());
                }
            }
            "--start-time-index" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start-time-index requires a value".to_string());
                }
            }
            "--end-time-index" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end-time-index requires a value".to_string());
                }
            }
            _ => {
                // Treat unrecognized positional arg as signal mapping if signals not yet set
                if signals.is_empty() {
                    for pair in args[i].split(',') {
                        let pair = pair.trim();
                        if let Some(eq_pos) = pair.find('=') {
                            let key = pair[..eq_pos].trim().to_string();
                            let value = pair[eq_pos + 1..].trim().to_string();
                            signals.push((key, value));
                        } else {
                            return Err(format!(
                                "Invalid signal mapping '{}', expected key=value format",
                                pair
                            ));
                        }
                    }
                } else {
                    return Err(format!("Unknown option '{}' for analyze_protocol", args[i]));
                }
            }
        }
        i += 1;
    }
    Ok(Command::AnalyzeProtocol {
        waveform_id,
        protocol,
        signals,
        start_time_index,
        end_time_index,
    })
}

pub(super) fn parse_analyze_phased_array(args: &[String]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(
            "analyze_phased_array requires waveform_id, channel_prefix, and clock_signal"
                .to_string(),
        );
    }
    let waveform_id = args[0].clone();
    let channel_prefix = args[1].clone();
    let mut clock_signal = args[2].clone();
    let mut control_fsm_signal = None;
    let mut coeff_signals = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut i = 3;
    // If args[2] starts with --, treat as flag and require --clock-signal
    if args.len() > 2 && args[2].starts_with("--") {
        clock_signal = String::new(); // Will be set by --clock-signal flag
        i = 2;
    }
    while i < args.len() {
        match args[i].as_str() {
            "--clock-signal" => {
                i += 1;
                if i < args.len() {
                    clock_signal = args[i].clone();
                } else {
                    return Err("--clock-signal requires a value".to_string());
                }
            }
            "--control-fsm-signal" => {
                i += 1;
                if i < args.len() {
                    control_fsm_signal = Some(args[i].clone());
                } else {
                    return Err("--control-fsm-signal requires a value".to_string());
                }
            }
            "--coeff-signals" => {
                i += 1;
                if i < args.len() {
                    coeff_signals =
                        Some(args[i].split(',').map(|s| s.trim().to_string()).collect());
                } else {
                    return Err("--coeff-signals requires a value".to_string());
                }
            }
            "--start-time-index" => {
                i += 1;
                if i < args.len() {
                    start_time_index = args[i].parse().ok();
                } else {
                    return Err("--start-time-index requires a value".to_string());
                }
            }
            "--end-time-index" => {
                i += 1;
                if i < args.len() {
                    end_time_index = args[i].parse().ok();
                } else {
                    return Err("--end-time-index requires a value".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for analyze_phased_array",
                    args[i]
                ));
            }
        }
        i += 1;
    }
    if clock_signal.is_empty() {
        return Err("analyze_phased_array requires --clock-signal".to_string());
    }
    Ok(Command::AnalyzePhasedArray {
        waveform_id,
        channel_prefix,
        control_fsm_signal,
        coeff_signals,
        clock_signal,
        start_time_index,
        end_time_index,
    })
}
