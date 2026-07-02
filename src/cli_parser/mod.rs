//! Command-line parsing for waveform-cli
//!
//! Global flags:
//!   --json, -j    Output results as JSON (text wrapped in a JSON envelope)
//!
//! Commands can be chained using `--` as a separator.

mod advanced;
mod batch;
mod common;
mod extraction;
mod phase3;
#[cfg(test)]
mod tests;
mod waveform;

/// Global CLI options parsed from the command line
#[derive(Debug, Clone, Default)]
pub struct CliOptions {
    /// Output results as JSON instead of human-readable text.
    /// When set, each command's text output is wrapped in
    /// `{"status":"ok","command":"...","output":"..."}`.
    pub json: bool,
    /// Commands to execute, in order. Commands can be chained
    /// using `--` as a separator in the original args.
    /// When `json` is true, ALL commands output JSON (not just
    /// the one following the flag).
    pub commands: Vec<Command>,
}

/// A parsed CLI command
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    OpenWaveform {
        file_path: String,
        alias: Option<String>,
    },
    CloseWaveform {
        waveform_id: String,
    },
    ListSignals {
        waveform_id: String,
        name_pattern: Option<String>,
        hierarchy_prefix: Option<String>,
        recursive: bool,
        limit: Option<isize>,
    },
    ReadHierarchy {
        waveform_id: String,
        scope_path: Option<String>,
        recursive: bool,
        limit: Option<isize>,
    },
    ReadSignal {
        waveform_id: String,
        signal_path: String,
        time_index: Option<usize>,
        time_indices: Option<Vec<usize>>,
    },
    GetSignalInfo {
        waveform_id: String,
        signal_path: String,
    },
    FindSignalEvents {
        waveform_id: String,
        signal_path: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        /// Start time value (e.g., 50.5) for physical time input
        start_time_value: Option<f64>,
        /// End time value for physical time input
        end_time_value: Option<f64>,
        /// Unit for time values (ps/ns/us/ms/s)
        time_unit: Option<String>,
        limit: Option<isize>,
    },
    FindConditionalEvents {
        waveform_id: String,
        condition: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
    },
    // Phase 3 commands
    LoadDependencies {
        file_path: String,
        alias: Option<String>,
    },
    LoadAssertionLog {
        file_path: String,
        alias: Option<String>,
        severity_filter: Option<Vec<String>>,
        limit: Option<isize>,
    },
    LoadDesignSpec {
        file_path: String,
        alias: Option<String>,
    },
    TraceRootCause {
        waveform_id: String,
        deps_id: String,
        signal_path: String,
        time_index: Option<usize>,
        time_value: Option<f64>,
        time_unit: Option<String>,
        spec_id: Option<String>,
        max_depth: Option<usize>,
        simulator: Option<String>,
        penetrate_cdc: Option<bool>,
        cdc_max_depth: Option<usize>,
        cdc_min_sync_stages: Option<u32>,
    },
    FindFanIn {
        deps_id: String,
        signal_path: String,
        simulator: Option<String>,
    },
    FindFanOut {
        deps_id: String,
        signal_path: String,
        simulator: Option<String>,
    },
    // Signal extraction command
    ExtractSignalValues {
        waveform_id: String,
        signal_path: Option<String>,
        bit_mapping: String, // comma-separated "bit=signal_path" pairs
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        start_time_value: Option<f64>,
        end_time_value: Option<f64>,
        time_unit: Option<String>,
        value_format: Option<String>,
        downsample: Option<usize>,
    },
    // Protocol analysis commands
    AnalyzeHandshake {
        waveform_id: String,
        valid_signal: String,
        ready_signal: String,
        data_signal: Option<String>,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
        report_mode: Option<String>,
        filter_zero_delay: Option<bool>,
        level_sensitive: Option<bool>,
    },
    MeasureSignal {
        waveform_id: String,
        signal_path: String,
        analysis_type: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        edge_type: Option<String>,
        /// For interval mode: from-condition (Verilog expression)
        from_condition: Option<String>,
        /// For interval mode: to-condition (Verilog expression)
        to_condition: Option<String>,
        /// For interval mode: expected interval value
        expected_value: Option<f64>,
        /// For interval mode: expected interval unit (ps/ns/us/ms/s)
        expected_unit: Option<String>,
    },
    // Phase 1: new algorithm tools
    CompareSignals {
        waveform_id: String,
        signals: String, // comma-separated signal paths or bit_mapping groups
        comparison_mode: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
        value_format: Option<String>,
    },
    MultiSignalTimeline {
        waveform_id: String,
        signals: String, // comma-separated signal paths
        merge_mode: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
        value_format: Option<String>,
    },
    AutoDiscoverSignals {
        waveform_id: String,
        discovery_mode: Option<String>,
        scope_path: Option<String>,
        pattern: Option<String>,
        limit: Option<isize>,
    },
    DetectSequence {
        waveform_id: String,
        sequence: Vec<String>,
        max_gap_cycles: Option<usize>,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
    },
    ComputeCrc {
        waveform_id: String,
        data_signal_path: String,
        crc_signal_path: Option<String>,
        data_valid_signal_path: Option<String>,
        clear_signal_path: Option<String>,
        clock_signal_path: Option<String>,
        crc_polynomial: String,
        initial_value: Option<u64>,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        limit: Option<isize>,
    },
    // BFS batch and report tools
    BatchTraceRootCause {
        waveform_id: String,
        deps_id: String,
        assertion_id: String,
        spec_id: Option<String>,
        max_depth: Option<usize>,
        severity_filter: Option<String>,
        simulator: Option<String>,
    },
    ExportBfsReport {
        trace_id: String,
        format: Option<String>,
    },
    LoadRunSummary {
        file_path: String,
        alias: Option<String>,
    },
    AnalyzeRun {
        run_summary_path: String,
        deps_file: Option<String>,
        spec_file: Option<String>,
        transcript_file: Option<String>,
        waveform_file: Option<String>,
        severity_filter: Option<Vec<String>>,
        max_depth: Option<usize>,
        simulator: Option<String>,
        report_dir: Option<String>,
        report_format: Option<String>,
    },
    // Waveform summary tools (from MCP server)
    SuggestEntrySignals {
        waveform_id: String,
        deps_id: String,
        assertion_name: Option<String>,
        scope_path: Option<String>,
        limit: Option<isize>,
        simulator: Option<String>,
    },
    GenerateSummary {
        waveform_id: String,
        signals: Vec<String>,
        max_samples: Option<usize>,
    },
    ExportSvg {
        waveform_id: String,
        signals: Vec<String>,
        time_range: Option<String>, // "start,end" format
        width: Option<u32>,
        height: Option<u32>,
    },
    /// Extract deps.yaml from RTL source files using deps-extractor pipeline
    ExtractDeps {
        /// Path to RTL source directory or file
        rtl_path: String,
        /// Top module name
        top_module: String,
        /// Extraction engine: "pyverilog" or "vivado" (default: pyverilog)
        engine: Option<String>,
        /// Path to annotations.yaml for manual overrides
        annotations_path: Option<String>,
        /// Output deps.yaml path (default: deps.yaml next to rtl_path)
        output_path: Option<String>,
        /// Path to deps-extractor directory (default: auto-detect)
        deps_extractor_path: Option<String>,
    },
    /// Diagnose environment configuration (sidecar, iverilog, VC++ Runtime)
    CheckEnv,
    /// List all available commands or show details for a specific command
    Help {
        command_name: Option<String>,
    },
    // CDC analysis
    AnalyzeCdc {
        waveform_id: String,
        deps_id: Option<String>,
        simulator: Option<String>,
    },
    // Pattern analysis
    AnalyzeSignalPatterns {
        waveform_id: String,
        signals: Vec<String>,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
        max_bins: Option<usize>,
        idle_threshold: Option<String>,
    },
    // FSM extraction
    ExtractFsm {
        waveform_id: String,
        signal_path: String,
        clock_signal: Option<String>,
        edge_type: Option<String>,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
    },
    // Protocol template analysis
    AnalyzeProtocol {
        waveform_id: String,
        protocol: String,
        signals: Vec<(String, String)>, // role=path pairs
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
    },
    // Phased array analysis
    AnalyzePhasedArray {
        waveform_id: String,
        channel_prefix: String,
        control_fsm_signal: Option<String>,
        coeff_signals: Option<Vec<String>>,
        clock_signal: String,
        start_time_index: Option<usize>,
        end_time_index: Option<usize>,
    },
    // Time conversion
    TimeConvert {
        waveform_id: String,
        /// Convert from physical time to time_index
        time_value: Option<f64>,
        /// Unit for time_value (ps/ns/us/ms/s)
        time_unit: Option<String>,
        /// Convert from time_index to physical time
        time_index: Option<usize>,
    },
}

/// Parse command line arguments into CliOptions
///
/// Global flags (--json) are extracted first, then commands are parsed.
/// Commands can be chained using "--" as a separator.
pub fn parse_args(args: Vec<String>) -> Result<CliOptions, String> {
    if args.is_empty() {
        return Err("No arguments provided".to_string());
    }

    // Extract global flags before any commands
    let mut json = false;
    let mut command_args: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" | "-j" => {
                json = true;
            }
            "--help" | "-h" => {
                return Ok(CliOptions {
                    json,
                    commands: vec![Command::Help { command_name: None }],
                });
            }
            _ => {
                command_args.push(args[i].clone());
            }
        }
        i += 1;
    }

    // Split command args by "--" separator
    let mut command_groups: Vec<Vec<String>> = vec![vec![]];
    for arg in &command_args {
        if arg == "--" {
            command_groups.push(vec![]);
        } else {
            command_groups.last_mut().unwrap().push(arg.clone());
        }
    }

    // Parse each command group
    let commands: Result<Vec<Command>, String> = command_groups
        .into_iter()
        .filter(|g| !g.is_empty())
        .map(parse_command)
        .collect();

    Ok(CliOptions {
        json,
        commands: commands?,
    })
}

/// Parse a single command from a group of arguments
fn parse_command(group: Vec<String>) -> Result<Command, String> {
    if group.is_empty() {
        return Err("Empty command group".to_string());
    }

    let cmd_name = &group[0];
    let cmd_args: Vec<String> = group[1..].to_vec();

    match cmd_name.as_str() {
        "open_waveform" => waveform::parse_open_waveform(&cmd_args),
        "close_waveform" => waveform::parse_close_waveform(&cmd_args),
        "list_signals" => waveform::parse_list_signals(&cmd_args),
        "read_hierarchy" => waveform::parse_read_hierarchy(&cmd_args),
        "read_signal" => waveform::parse_read_signal(&cmd_args),
        "get_signal_info" => waveform::parse_get_signal_info(&cmd_args),
        "find_signal_events" => waveform::parse_find_signal_events(&cmd_args),
        "find_conditional_events" => waveform::parse_find_conditional_events(&cmd_args),
        "load_deps" => phase3::parse_load_deps(&cmd_args),
        "load_assertion_log" => phase3::parse_load_assertion_log(&cmd_args),
        "load_spec" => phase3::parse_load_spec(&cmd_args),
        "trace_root_cause" => phase3::parse_trace_root_cause(&cmd_args),
        "find_fan_in" => phase3::parse_find_fan_in(&cmd_args),
        "find_fan_out" => phase3::parse_find_fan_out(&cmd_args),
        "extract_signal_values" => extraction::parse_extract_signal_values(&cmd_args),
        "analyze_handshake" => extraction::parse_analyze_handshake(&cmd_args),
        "measure_signal" => extraction::parse_measure_signal(&cmd_args),
        "compare_signals" => extraction::parse_compare_signals(&cmd_args),
        "multi_signal_timeline" => extraction::parse_multi_signal_timeline(&cmd_args),
        "auto_discover_signals" => extraction::parse_auto_discover_signals(&cmd_args),
        "detect_sequence" => extraction::parse_detect_sequence(&cmd_args),
        "compute_crc" => extraction::parse_compute_crc(&cmd_args),
        "suggest_entry_signals" => batch::parse_suggest_entry_signals(&cmd_args),
        "generate_summary" => batch::parse_generate_summary(&cmd_args),
        "export_svg" => batch::parse_export_svg(&cmd_args),
        "batch_trace_root_cause" => batch::parse_batch_trace_root_cause(&cmd_args),
        "export_bfs_report" => batch::parse_export_bfs_report(&cmd_args),
        "load_run_summary" => batch::parse_load_run_summary(&cmd_args),
        "analyze_run" => batch::parse_analyze_run(&cmd_args),
        "extract_deps" => batch::parse_extract_deps(&cmd_args),
        "check_env" => Ok(Command::CheckEnv),
        "help" => batch::parse_help(&cmd_args),
        "analyze_cdc" => advanced::parse_analyze_cdc(&cmd_args),
        "analyze_signal_patterns" => advanced::parse_analyze_signal_patterns(&cmd_args),
        "extract_fsm" => advanced::parse_extract_fsm(&cmd_args),
        "analyze_protocol" => advanced::parse_analyze_protocol(&cmd_args),
        "analyze_phased_array" => advanced::parse_analyze_phased_array(&cmd_args),
        "time_convert" => common::parse_time_convert(&cmd_args),
        _ => Err(format!("Unknown command '{}'", cmd_name)),
    }
}
