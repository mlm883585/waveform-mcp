use super::Command;
use super::common::{parse_non_negative_limit, parse_u64_hex_or_decimal};

pub(super) fn parse_extract_signal_values(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("extract_signal_values requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signal_path = None;
    let mut bit_mapping = String::new();
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut start_time_value = None;
    let mut end_time_value = None;
    let mut time_unit = None;
    let mut value_format = None;
    let mut downsample = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signal" | "-s" => {
                i += 1;
                if i < args.len() {
                    signal_path = Some(args[i].clone());
                } else {
                    return Err("--signal requires a value".to_string());
                }
            }
            "--bit-mapping" | "-b" => {
                i += 1;
                if i < args.len() {
                    bit_mapping = args[i].clone();
                } else {
                    return Err("--bit-mapping requires a value".to_string());
                }
            }
            "--start" => {
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
                    return Err("--start-time requires a value (e.g., 0, 50.5)".to_string());
                }
            }
            "--end-time" => {
                i += 1;
                if i < args.len() {
                    end_time_value = args[i].parse().ok();
                } else {
                    return Err("--end-time requires a value (e.g., 100, 173.5)".to_string());
                }
            }
            "--time-unit" | "-t" => {
                i += 1;
                if i < args.len() {
                    time_unit = Some(args[i].clone());
                } else {
                    return Err("--time-unit requires a value (ps, ns, us, ms, s)".to_string());
                }
            }
            "--format" | "-f" => {
                i += 1;
                if i < args.len() {
                    value_format = Some(args[i].clone());
                } else {
                    return Err("--format requires a value (hex/binary/decimal)".to_string());
                }
            }
            "--downsample" | "-d" => {
                i += 1;
                if i < args.len() {
                    downsample = args[i].parse().ok();
                } else {
                    return Err("--downsample requires a value".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for extract_signal_values",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    // Validate: either signal_path or bit_mapping must be provided
    if signal_path.is_none() && bit_mapping.is_empty() {
        return Err("Either --signal or --bit-mapping must be provided".to_string());
    }

    // Validate: time-value options require time-unit
    if (start_time_value.is_some() || end_time_value.is_some()) && time_unit.is_none() {
        return Err("--time-unit is required when using --start-time or --end-time".to_string());
    }

    Ok(Command::ExtractSignalValues {
        waveform_id,
        signal_path,
        bit_mapping,
        start_time_index,
        end_time_index,
        start_time_value,
        end_time_value,
        time_unit,
        value_format,
        downsample,
    })
}

pub(super) fn parse_analyze_handshake(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("analyze_handshake requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut valid_signal = None;
    let mut ready_signal = None;
    let mut data_signal = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(-1isize);
    let mut report_mode = None;
    let mut filter_zero_delay = None;
    let mut level_sensitive = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--valid" | "-v" => {
                i += 1;
                if i < args.len() {
                    valid_signal = Some(args[i].clone());
                } else {
                    return Err("--valid requires a value".to_string());
                }
            }
            "--ready" | "-r" => {
                i += 1;
                if i < args.len() {
                    ready_signal = Some(args[i].clone());
                } else {
                    return Err("--ready requires a value".to_string());
                }
            }
            "--data" | "-d" => {
                i += 1;
                if i < args.len() {
                    data_signal = Some(args[i].clone());
                } else {
                    return Err("--data requires a value".to_string());
                }
            }
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
            "--report-mode" | "-m" => {
                i += 1;
                if i < args.len() {
                    report_mode = Some(args[i].clone());
                } else {
                    return Err("--report-mode requires a value (summary/detail)".to_string());
                }
            }
            "--filter-zero-delay" | "--fzd" => {
                filter_zero_delay = Some(true);
            }
            "--level-sensitive" => {
                level_sensitive = Some(true);
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for analyze_handshake",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    let valid_signal = valid_signal.ok_or_else(|| "--valid signal path is required".to_string())?;
    let ready_signal = ready_signal.ok_or_else(|| "--ready signal path is required".to_string())?;

    Ok(Command::AnalyzeHandshake {
        waveform_id,
        valid_signal,
        ready_signal,
        data_signal,
        start_time_index,
        end_time_index,
        limit,
        report_mode,
        filter_zero_delay,
        level_sensitive,
    })
}

pub(super) fn parse_measure_signal(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("measure_signal requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signal_path = None;
    let mut analysis_type = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut edge_type = None;
    let mut from_condition = None;
    let mut to_condition = None;
    let mut expected_value = None;
    let mut expected_unit = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signal" | "-s" => {
                i += 1;
                if i < args.len() {
                    signal_path = Some(args[i].clone());
                } else {
                    return Err("--signal requires a value".to_string());
                }
            }
            "--analysis-type" | "-t" => {
                i += 1;
                if i < args.len() {
                    analysis_type = Some(args[i].clone());
                } else {
                    return Err(
                        "--analysis-type requires a value (clock/pulse/interval)".to_string()
                    );
                }
            }
            "--start" => {
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
            "--edge-type" => {
                i += 1;
                if i < args.len() {
                    edge_type = Some(args[i].clone());
                } else {
                    return Err("--edge-type requires a value (posedge/negedge)".to_string());
                }
            }
            "--from-condition" | "--from" => {
                i += 1;
                if i < args.len() {
                    from_condition = Some(args[i].clone());
                } else {
                    return Err(
                        "--from-condition requires a Verilog condition expression".to_string()
                    );
                }
            }
            "--to-condition" | "--to" => {
                i += 1;
                if i < args.len() {
                    to_condition = Some(args[i].clone());
                } else {
                    return Err(
                        "--to-condition requires a Verilog condition expression".to_string()
                    );
                }
            }
            "--expected-value" => {
                i += 1;
                if i < args.len() {
                    expected_value = args[i].parse().ok();
                } else {
                    return Err("--expected-value requires a number".to_string());
                }
            }
            "--expected-unit" => {
                i += 1;
                if i < args.len() {
                    expected_unit = Some(args[i].clone());
                } else {
                    return Err("--expected-unit requires a value (ps/ns/us/ms/s)".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for measure_signal", args[i])),
        }
        i += 1;
    }

    let signal_path = signal_path.ok_or_else(|| "--signal path is required".to_string())?;
    let analysis_type = analysis_type
        .ok_or_else(|| "--analysis-type is required (clock/pulse/interval)".to_string())?;

    if analysis_type != "clock" && analysis_type != "pulse" && analysis_type != "interval" {
        return Err(format!(
            "Invalid --analysis-type '{}'. Must be 'clock', 'pulse', or 'interval'",
            analysis_type
        ));
    }

    // Validate: interval mode requires from_condition and to_condition
    if analysis_type == "interval" {
        if from_condition.is_none() {
            return Err("--from-condition is required for interval mode".to_string());
        }
        if to_condition.is_none() {
            return Err("--to-condition is required for interval mode".to_string());
        }
        if expected_value.is_some() && expected_unit.is_none() {
            return Err("--expected-unit is required when using --expected-value".to_string());
        }
    }

    Ok(Command::MeasureSignal {
        waveform_id,
        signal_path,
        analysis_type,
        start_time_index,
        end_time_index,
        edge_type,
        from_condition,
        to_condition,
        expected_value,
        expected_unit,
    })
}

pub(super) fn parse_compare_signals(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("compare_signals requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signals = None;
    let mut comparison_mode = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(-1isize);
    let mut value_format = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signals" | "-s" => {
                i += 1;
                if i < args.len() {
                    signals = Some(args[i].clone());
                } else {
                    return Err("--signals requires a value".to_string());
                }
            }
            "--mode" | "-m" => {
                i += 1;
                if i < args.len() {
                    comparison_mode = Some(args[i].clone());
                } else {
                    return Err("--mode requires a value".to_string());
                }
            }
            "--start" => {
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
            "--format" | "-f" => {
                i += 1;
                if i < args.len() {
                    value_format = Some(args[i].clone());
                } else {
                    return Err("--format requires a value (hex/binary/decimal)".to_string());
                }
            }
            _ => return Err(format!("Unknown option '{}' for compare_signals", args[i])),
        }
        i += 1;
    }

    let signals = signals
        .ok_or_else(|| "--signals is required (comma-separated signal paths)".to_string())?;
    let comparison_mode = comparison_mode.unwrap_or_else(|| "all_equal".to_string());

    Ok(Command::CompareSignals {
        waveform_id,
        signals,
        comparison_mode,
        start_time_index,
        end_time_index,
        limit,
        value_format,
    })
}

pub(super) fn parse_multi_signal_timeline(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("multi_signal_timeline requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut signals = None;
    let mut merge_mode = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(-1isize);
    let mut value_format = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--signals" | "-s" => {
                i += 1;
                if i < args.len() {
                    signals = Some(args[i].clone());
                } else {
                    return Err("--signals requires a value".to_string());
                }
            }
            "--merge" | "-m" => {
                i += 1;
                if i < args.len() {
                    merge_mode = Some(args[i].clone());
                } else {
                    return Err("--merge requires a value".to_string());
                }
            }
            "--start" => {
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
            "--format" | "-f" => {
                i += 1;
                if i < args.len() {
                    value_format = Some(args[i].clone());
                } else {
                    return Err("--format requires a value (hex/binary/decimal)".to_string());
                }
            }
            _ => {
                return Err(format!(
                    "Unknown option '{}' for multi_signal_timeline",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    let signals = signals
        .ok_or_else(|| "--signals is required (comma-separated signal paths)".to_string())?;
    let merge_mode = merge_mode.unwrap_or_else(|| "union".to_string());

    Ok(Command::MultiSignalTimeline {
        waveform_id,
        signals,
        merge_mode,
        start_time_index,
        end_time_index,
        limit,
        value_format,
    })
}

pub(super) fn parse_auto_discover_signals(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("auto_discover_signals requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut discovery_mode = None;
    let mut scope_path = None;
    let mut pattern = None;
    let mut limit = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" | "-m" => {
                i += 1;
                if i < args.len() {
                    discovery_mode = Some(args[i].clone());
                } else {
                    return Err("--mode requires a value".to_string());
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
            "--pattern" | "-p" => {
                i += 1;
                if i < args.len() {
                    pattern = Some(args[i].clone());
                } else {
                    return Err("--pattern requires a value".to_string());
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
                    "Unknown option '{}' for auto_discover_signals",
                    args[i]
                ));
            }
        }
        i += 1;
    }

    Ok(Command::AutoDiscoverSignals {
        waveform_id,
        discovery_mode,
        scope_path,
        pattern,
        limit,
    })
}

pub(super) fn parse_detect_sequence(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("detect_sequence requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut sequence = Vec::new();
    let mut max_gap_cycles = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(-1isize);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--steps" | "-s" => {
                i += 1;
                if i < args.len() {
                    sequence = args[i].split(',').map(|s| s.trim().to_string()).collect();
                } else {
                    return Err("--steps requires a value".to_string());
                }
            }
            "--max-gap" | "-g" => {
                i += 1;
                if i < args.len() {
                    max_gap_cycles = args[i].parse().ok();
                } else {
                    return Err("--max-gap requires a value".to_string());
                }
            }
            "--start" => {
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
            _ => return Err(format!("Unknown option '{}' for detect_sequence", args[i])),
        }
        i += 1;
    }

    if sequence.is_empty() {
        return Err("--steps is required (comma-separated condition strings)".to_string());
    }

    Ok(Command::DetectSequence {
        waveform_id,
        sequence,
        max_gap_cycles,
        start_time_index,
        end_time_index,
        limit,
    })
}

pub(super) fn parse_compute_crc(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("compute_crc requires a waveform_id".to_string());
    }

    let waveform_id = args[0].clone();
    let mut data_signal_path = None;
    let mut crc_signal_path = None;
    let mut data_valid_signal_path = None;
    let mut clear_signal_path = None;
    let mut clock_signal_path = None;
    let mut crc_polynomial = None;
    let mut initial_value = None;
    let mut start_time_index = None;
    let mut end_time_index = None;
    let mut limit = Some(-1isize);

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--data" | "-d" => {
                i += 1;
                if i < args.len() {
                    data_signal_path = Some(args[i].clone());
                } else {
                    return Err("--data requires a value".to_string());
                }
            }
            "--crc" | "-c" => {
                i += 1;
                if i < args.len() {
                    crc_signal_path = Some(args[i].clone());
                } else {
                    return Err("--crc requires a value".to_string());
                }
            }
            "--valid" | "-v" => {
                i += 1;
                if i < args.len() {
                    data_valid_signal_path = Some(args[i].clone());
                } else {
                    return Err("--valid requires a value (data_valid signal path)".to_string());
                }
            }
            "--clear" | "-r" => {
                i += 1;
                if i < args.len() {
                    clear_signal_path = Some(args[i].clone());
                } else {
                    return Err("--clear requires a value (clear/reset signal path)".to_string());
                }
            }
            "--clock" | "-k" => {
                i += 1;
                if i < args.len() {
                    clock_signal_path = Some(args[i].clone());
                } else {
                    return Err(
                        "--clock requires a value (clock signal path for per-cycle sampling)"
                            .to_string(),
                    );
                }
            }
            "--polynomial" | "-p" => {
                i += 1;
                if i < args.len() {
                    crc_polynomial = Some(args[i].clone());
                } else {
                    return Err("--polynomial requires a value".to_string());
                }
            }
            "--init" => {
                i += 1;
                if i < args.len() {
                    initial_value = Some(parse_u64_hex_or_decimal(&args[i])?);
                } else {
                    return Err("--init requires a value (decimal or hex e.g. 0xFFFF)".to_string());
                }
            }
            "--start" => {
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
            _ => return Err(format!("Unknown option '{}' for compute_crc", args[i])),
        }
        i += 1;
    }

    let data_signal_path =
        data_signal_path.ok_or_else(|| "--data signal path is required".to_string())?;
    let crc_polynomial = crc_polynomial
        .ok_or_else(|| "--polynomial is required (crc8/crc16_ccitt/crc32_ethernet)".to_string())?;

    Ok(Command::ComputeCrc {
        waveform_id,
        data_signal_path,
        crc_signal_path,
        data_valid_signal_path,
        clear_signal_path,
        clock_signal_path,
        crc_polynomial,
        initial_value,
        start_time_index,
        end_time_index,
        limit,
    })
}
