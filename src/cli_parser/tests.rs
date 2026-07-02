use super::*;

#[test]
fn test_parse_empty_args() {
    let result = parse_args(vec![]);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "No arguments provided");
}

#[test]
fn test_parse_open_waveform() {
    let args = vec!["open_waveform".to_string(), "/path/to/test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::OpenWaveform {
            file_path: "/path/to/test.vcd".to_string(),
            alias: None,
        }
    );
}

#[test]
fn test_parse_open_waveform_with_alias() {
    let args = vec![
        "open_waveform".to_string(),
        "/path/to/test.vcd".to_string(),
        "--alias".to_string(),
        "mywave".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::OpenWaveform {
            file_path: "/path/to/test.vcd".to_string(),
            alias: Some("mywave".to_string()),
        }
    );
}

#[test]
fn test_parse_open_waveform_with_short_alias() {
    let args = vec![
        "open_waveform".to_string(),
        "/path/to/test.vcd".to_string(),
        "-a".to_string(),
        "wave1".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::OpenWaveform {
            file_path: "/path/to/test.vcd".to_string(),
            alias: Some("wave1".to_string()),
        }
    );
}

#[test]
fn test_parse_close_waveform() {
    let args = vec!["close_waveform".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::CloseWaveform {
            waveform_id: "test.vcd".to_string(),
        }
    );
}

#[test]
fn test_parse_list_signals_minimal() {
    let args = vec!["list_signals".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::ListSignals {
            waveform_id: "test.vcd".to_string(),
            name_pattern: None,
            hierarchy_prefix: None,
            recursive: true,
            limit: Some(100),
        }
    );
}

#[test]
fn test_parse_list_signals_with_pattern() {
    let args = vec![
        "list_signals".to_string(),
        "test.vcd".to_string(),
        "--pattern".to_string(),
        "clk".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ListSignals {
            waveform_id: "test.vcd".to_string(),
            name_pattern: Some("clk".to_string()),
            hierarchy_prefix: None,
            recursive: true,
            limit: Some(100),
        }
    );
}

#[test]
fn test_parse_list_signals_with_all_options() {
    let args = vec![
        "list_signals".to_string(),
        "test.vcd".to_string(),
        "-p".to_string(),
        "data".to_string(),
        "-H".to_string(),
        "top".to_string(),
        "-r".to_string(),
        "false".to_string(),
        "-l".to_string(),
        "50".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ListSignals {
            waveform_id: "test.vcd".to_string(),
            name_pattern: Some("data".to_string()),
            hierarchy_prefix: Some("top".to_string()),
            recursive: false,
            limit: Some(50),
        }
    );
}

#[test]
fn test_parse_read_signal_with_time_index() {
    let args = vec![
        "read_signal".to_string(),
        "test.vcd".to_string(),
        "top.clk".to_string(),
        "--time-index".to_string(),
        "5".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ReadSignal {
            waveform_id: "test.vcd".to_string(),
            signal_path: "top.clk".to_string(),
            time_index: Some(5),
            time_indices: None,
        }
    );
}

#[test]
fn test_parse_read_hierarchy_minimal() {
    let args = vec!["read_hierarchy".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ReadHierarchy {
            waveform_id: "test.vcd".to_string(),
            scope_path: None,
            recursive: false,
            limit: Some(200),
        }
    );
}

#[test]
fn test_parse_read_hierarchy_with_options() {
    let args = vec![
        "read_hierarchy".to_string(),
        "test.vcd".to_string(),
        "--scope".to_string(),
        "top.submodule".to_string(),
        "--recursive".to_string(),
        "true".to_string(),
        "--limit".to_string(),
        "50".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ReadHierarchy {
            waveform_id: "test.vcd".to_string(),
            scope_path: Some("top.submodule".to_string()),
            recursive: true,
            limit: Some(50),
        }
    );
}

#[test]
fn test_parse_read_signal_with_time_indices() {
    let args = vec![
        "read_signal".to_string(),
        "test.vcd".to_string(),
        "top.data".to_string(),
        "--time-indices".to_string(),
        "0,1,2,3".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::ReadSignal {
            waveform_id: "test.vcd".to_string(),
            signal_path: "top.data".to_string(),
            time_index: None,
            time_indices: Some(vec![0, 1, 2, 3]),
        }
    );
}

#[test]
fn test_parse_get_signal_info() {
    let args = vec![
        "get_signal_info".to_string(),
        "test.vcd".to_string(),
        "top.clk".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::GetSignalInfo {
            waveform_id: "test.vcd".to_string(),
            signal_path: "top.clk".to_string(),
        }
    );
}

#[test]
fn test_parse_find_signal_events() {
    let args = vec![
        "find_signal_events".to_string(),
        "test.vcd".to_string(),
        "top.clk".to_string(),
        "--start".to_string(),
        "0".to_string(),
        "--end".to_string(),
        "100".to_string(),
        "--limit".to_string(),
        "10".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::FindSignalEvents {
            waveform_id: "test.vcd".to_string(),
            signal_path: "top.clk".to_string(),
            start_time_index: Some(0),
            end_time_index: Some(100),
            start_time_value: None,
            end_time_value: None,
            time_unit: None,
            limit: Some(10),
        }
    );
}

#[test]
fn test_parse_find_conditional_events() {
    let args = vec![
        "find_conditional_events".to_string(),
        "test.vcd".to_string(),
        "top.clk == 1'b1".to_string(),
        "-s".to_string(),
        "10".to_string(),
        "-e".to_string(),
        "50".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0].clone(),
        Command::FindConditionalEvents {
            waveform_id: "test.vcd".to_string(),
            condition: "top.clk == 1'b1".to_string(),
            start_time_index: Some(10),
            end_time_index: Some(50),
            limit: Some(100),
        }
    );
}

#[test]
fn test_parse_chained_commands() {
    let args = vec![
        "open_waveform".to_string(),
        "test.vcd".to_string(),
        "--".to_string(),
        "list_signals".to_string(),
        "test.vcd".to_string(),
        "--".to_string(),
        "close_waveform".to_string(),
        "test.vcd".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 3);

    // First command: open_waveform
    assert_eq!(
        options.commands[0].clone(),
        Command::OpenWaveform {
            file_path: "test.vcd".to_string(),
            alias: None,
        }
    );

    // Second command: list_signals
    assert_eq!(
        options.commands[1].clone(),
        Command::ListSignals {
            waveform_id: "test.vcd".to_string(),
            name_pattern: None,
            hierarchy_prefix: None,
            recursive: true,
            limit: Some(100),
        }
    );

    // Third command: close_waveform
    assert_eq!(
        options.commands[2].clone(),
        Command::CloseWaveform {
            waveform_id: "test.vcd".to_string(),
        }
    );
}

#[test]
fn test_parse_unknown_command() {
    let args = vec!["unknown_command".to_string(), "arg1".to_string()];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Unknown command 'unknown_command'");
}

#[test]
fn test_parse_missing_required_args() {
    let args = vec!["open_waveform".to_string()];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "open_waveform requires a file path");
}

#[test]
fn test_parse_missing_list_signals_waveform_id() {
    let args = vec!["list_signals".to_string()];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "list_signals requires a waveform_id");
}

#[test]
fn test_parse_missing_read_signal_args() {
    let args = vec!["read_signal".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "read_signal requires waveform_id and signal_path"
    );
}

// Phase 3 CLI tests

#[test]
fn test_parse_load_deps() {
    let args = vec!["load_deps".to_string(), "deps.yaml".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::LoadDependencies {
            file_path: "deps.yaml".to_string(),
            alias: None,
        }
    );
}

#[test]
fn test_parse_load_deps_with_alias() {
    let args = vec![
        "load_deps".to_string(),
        "deps.yaml".to_string(),
        "--alias".to_string(),
        "mydeps".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::LoadDependencies {
            file_path: "deps.yaml".to_string(),
            alias: Some("mydeps".to_string()),
        }
    );
}

#[test]
fn test_parse_load_assertion_log() {
    let args = vec![
        "load_assertion_log".to_string(),
        "transcript.log".to_string(),
        "--severity-filter".to_string(),
        "Error,Warning".to_string(),
        "--limit".to_string(),
        "50".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::LoadAssertionLog {
            file_path: "transcript.log".to_string(),
            alias: None,
            severity_filter: Some(vec!["Error".to_string(), "Warning".to_string()]),
            limit: Some(50),
        }
    );
}

#[test]
fn test_parse_load_spec() {
    let args = vec![
        "load_spec".to_string(),
        "spec.yaml".to_string(),
        "-a".to_string(),
        "myspec".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::LoadDesignSpec {
            file_path: "spec.yaml".to_string(),
            alias: Some("myspec".to_string()),
        }
    );
}

#[test]
fn test_parse_trace_root_cause() {
    let args = vec![
        "trace_root_cause".to_string(),
        "wave.vcd".to_string(),
        "mydeps".to_string(),
        "TOP.data_out".to_string(),
        "--time-index".to_string(),
        "10".to_string(),
        "--max-depth".to_string(),
        "5".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::TraceRootCause {
            waveform_id: "wave.vcd".to_string(),
            deps_id: "mydeps".to_string(),
            signal_path: "TOP.data_out".to_string(),
            time_index: Some(10),
            time_value: None,
            time_unit: None,
            spec_id: None,
            max_depth: Some(5),
            simulator: Some("modelsim".to_string()),
            penetrate_cdc: None,
            cdc_max_depth: None,
            cdc_min_sync_stages: None,
        }
    );
}

#[test]
fn test_parse_find_fan_in() {
    let args = vec![
        "find_fan_in".to_string(),
        "mydeps".to_string(),
        "TOP.data_out".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::FindFanIn {
            deps_id: "mydeps".to_string(),
            signal_path: "TOP.data_out".to_string(),
            simulator: Some("modelsim".to_string()),
        }
    );
}

#[test]
fn test_parse_extract_signal_values_single() {
    let args = vec![
        "extract_signal_values".to_string(),
        "test.vcd".to_string(),
        "--signal".to_string(),
        "TOP.data".to_string(),
        "--start".to_string(),
        "0".to_string(),
        "--end".to_string(),
        "1000".to_string(),
        "--format".to_string(),
        "hex".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    match &options.commands[0] {
        Command::ExtractSignalValues {
            waveform_id,
            signal_path,
            bit_mapping,
            start_time_index,
            end_time_index,
            start_time_value: _,
            end_time_value: _,
            time_unit: _,
            value_format,
            downsample,
        } => {
            assert_eq!(waveform_id, "test.vcd");
            assert_eq!(signal_path, &Some("TOP.data".to_string()));
            assert!(bit_mapping.is_empty());
            assert_eq!(start_time_index, &Some(0));
            assert_eq!(end_time_index, &Some(1000));
            assert_eq!(value_format, &Some("hex".to_string()));
            assert!(downsample.is_none());
        }
        _ => panic!("Expected ExtractSignalValues"),
    }
}

#[test]
fn test_parse_extract_signal_values_bit_mapping() {
    let args = vec![
        "extract_signal_values".to_string(),
        "test.vcd".to_string(),
        "--bit-mapping".to_string(),
        "0=TOP.crc0,1=TOP.crc1,2=TOP.crc2".to_string(),
        "--format".to_string(),
        "binary".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    match &options.commands[0] {
        Command::ExtractSignalValues {
            waveform_id,
            bit_mapping,
            value_format,
            ..
        } => {
            assert_eq!(waveform_id, "test.vcd");
            assert_eq!(bit_mapping, "0=TOP.crc0,1=TOP.crc1,2=TOP.crc2");
            assert_eq!(value_format, &Some("binary".to_string()));
        }
        _ => panic!("Expected ExtractSignalValues"),
    }
}

#[test]
fn test_parse_extract_signal_values_missing_required() {
    let args = vec!["extract_signal_values".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Either --signal or --bit-mapping must be provided"
    );
}

#[test]
fn test_parse_analyze_handshake() {
    let args = vec![
        "analyze_handshake".to_string(),
        "test.vcd".to_string(),
        "--valid".to_string(),
        "TOP.valid".to_string(),
        "--ready".to_string(),
        "TOP.ready".to_string(),
        "--data".to_string(),
        "TOP.data".to_string(),
        "--start".to_string(),
        "0".to_string(),
        "--end".to_string(),
        "1000".to_string(),
        "--report-mode".to_string(),
        "detail".to_string(),
        "--level-sensitive".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    match &options.commands[0] {
        Command::AnalyzeHandshake {
            waveform_id,
            valid_signal,
            ready_signal,
            data_signal,
            start_time_index,
            end_time_index,
            report_mode,
            level_sensitive,
            ..
        } => {
            assert_eq!(waveform_id, "test.vcd");
            assert_eq!(valid_signal, "TOP.valid");
            assert_eq!(ready_signal, "TOP.ready");
            assert_eq!(data_signal, &Some("TOP.data".to_string()));
            assert_eq!(start_time_index, &Some(0));
            assert_eq!(end_time_index, &Some(1000));
            assert_eq!(report_mode, &Some("detail".to_string()));
            assert_eq!(level_sensitive, &Some(true));
        }
        _ => panic!("Expected AnalyzeHandshake"),
    }
}

#[test]
fn test_parse_analyze_handshake_missing_valid() {
    let args = vec![
        "analyze_handshake".to_string(),
        "test.vcd".to_string(),
        "--ready".to_string(),
        "TOP.ready".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_err());
}

#[test]
fn test_parse_measure_signal_clock() {
    let args = vec![
        "measure_signal".to_string(),
        "test.vcd".to_string(),
        "--signal".to_string(),
        "TOP.clk".to_string(),
        "--analysis-type".to_string(),
        "clock".to_string(),
        "--edge-type".to_string(),
        "posedge".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    match &options.commands[0] {
        Command::MeasureSignal {
            waveform_id,
            signal_path,
            analysis_type,
            edge_type,
            ..
        } => {
            assert_eq!(waveform_id, "test.vcd");
            assert_eq!(signal_path, "TOP.clk");
            assert_eq!(analysis_type, "clock");
            assert_eq!(edge_type, &Some("posedge".to_string()));
        }
        _ => panic!("Expected MeasureSignal"),
    }
}

#[test]
fn test_parse_measure_signal_pulse() {
    let args = vec![
        "measure_signal".to_string(),
        "test.vcd".to_string(),
        "--signal".to_string(),
        "TOP.pulse_sig".to_string(),
        "--analysis-type".to_string(),
        "pulse".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    match &options.commands[0] {
        Command::MeasureSignal {
            signal_path,
            analysis_type,
            ..
        } => {
            assert_eq!(signal_path, "TOP.pulse_sig");
            assert_eq!(analysis_type, "pulse");
        }
        _ => panic!("Expected MeasureSignal"),
    }
}

#[test]
fn test_parse_measure_signal_invalid_type() {
    let args = vec![
        "measure_signal".to_string(),
        "test.vcd".to_string(),
        "--signal".to_string(),
        "TOP.clk".to_string(),
        "--analysis-type".to_string(),
        "invalid".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_err());
}

#[test]
fn test_parse_negative_limit_rejected() {
    let args = vec![
        "compare_signals".to_string(),
        "test.vcd".to_string(),
        "--signals".to_string(),
        "a,b".to_string(),
        "--limit".to_string(),
        "-1".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Invalid limit '-1': limit must be non-negative"
    );
}

#[test]
fn test_parse_zero_list_signals_limit_rejected() {
    let args = vec![
        "list_signals".to_string(),
        "test.vcd".to_string(),
        "--limit".to_string(),
        "0".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Invalid limit '0': limit must be greater than 0"
    );
}

// --json flag and help command tests

#[test]
fn test_parse_json_flag() {
    let args = vec![
        "--json".to_string(),
        "open_waveform".to_string(),
        "test.vcd".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert!(options.json);
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::OpenWaveform {
            file_path: "test.vcd".to_string(),
            alias: None,
        }
    );
}

#[test]
fn test_parse_json_flag_short() {
    let args = vec![
        "-j".to_string(),
        "list_signals".to_string(),
        "test.vcd".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert!(options.json);
    assert_eq!(options.commands.len(), 1);
}

#[test]
fn test_parse_json_flag_with_chained_commands() {
    let args = vec![
        "--json".to_string(),
        "open_waveform".to_string(),
        "test.vcd".to_string(),
        "--".to_string(),
        "list_signals".to_string(),
        "test.vcd".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert!(options.json);
    assert_eq!(options.commands.len(), 2);
}

#[test]
fn test_parse_no_json_flag_default() {
    let args = vec!["open_waveform".to_string(), "test.vcd".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert!(!options.json);
}

#[test]
fn test_parse_help_no_args() {
    let args = vec!["help".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(options.commands[0], Command::Help { command_name: None });
}

#[test]
fn test_parse_help_with_command_name() {
    let args = vec!["help".to_string(), "open_waveform".to_string()];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 1);
    assert_eq!(
        options.commands[0],
        Command::Help {
            command_name: Some("open_waveform".to_string())
        }
    );
}

#[test]
fn test_parse_analyze_run() {
    let args = vec![
        "analyze_run".to_string(),
        "sim/run_summary.json".to_string(),
        "--deps".to_string(),
        "sim/deps.yaml".to_string(),
        "--spec".to_string(),
        "sim/design_spec.yaml".to_string(),
        "--severity-filter".to_string(),
        "Error,Failure".to_string(),
        "--report-format".to_string(),
        "html".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(
        options.commands[0],
        Command::AnalyzeRun {
            run_summary_path: "sim/run_summary.json".to_string(),
            deps_file: Some("sim/deps.yaml".to_string()),
            spec_file: Some("sim/design_spec.yaml".to_string()),
            transcript_file: None,
            waveform_file: None,
            severity_filter: Some(vec!["Error".to_string(), "Failure".to_string()]),
            max_depth: Some(8),
            simulator: Some("modelsim".to_string()),
            report_dir: None,
            report_format: Some("html".to_string()),
        }
    );
}

#[test]
fn test_parse_help_chained_with_other_commands() {
    let args = vec![
        "help".to_string(),
        "--".to_string(),
        "open_waveform".to_string(),
        "test.vcd".to_string(),
    ];
    let result = parse_args(args);
    assert!(result.is_ok());
    let options = result.unwrap();
    assert_eq!(options.commands.len(), 2);
    assert_eq!(options.commands[0], Command::Help { command_name: None });
    assert_eq!(
        options.commands[1],
        Command::OpenWaveform {
            file_path: "test.vcd".to_string(),
            alias: None,
        }
    );
}
