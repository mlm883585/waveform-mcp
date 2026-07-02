//! Wave CLI - Direct command-line interface for waveform tools
//!
//! Usage: wave-analyzer-cli <command1> [args...] [-- <command2> [args...] ...]
//!
//! PATH configuration:
//!   On Windows, several commands require iverilog / Python 3 / Vivado
//!   on your system PATH. Run `wave-analyzer-cli help` for full setup
//!   instructions and example paths (setx commands, env vars, verify).
//!
//! Commands:
//!   open_waveform <file_path> [--alias <alias>]
//!   close_waveform <waveform_id>
//!   list_signals <waveform_id> [--pattern <pattern>] [--hierarchy <prefix>] [--recursive <true|false>] [--limit <n>]
//!   read_hierarchy <waveform_id> [--scope <scope>] [--recursive <true|false>] [--limit <n>]
//!   read_signal <waveform_id> <signal_path> [--time-index <idx> | --time-indices <idx1,idx2,...>]
//!   get_signal_info <waveform_id> <signal_path>
//!   find_signal_events <waveform_id> <signal_path> [--start <idx>] [--end <idx>] [--limit <n>]
//!   find_conditional_events <waveform_id> <condition> [--start <idx>] [--end <idx>] [--limit <n>]
//!   load_deps <file_path> [--alias <alias>]
//!   load_assertion_log <file_path> [--alias <alias>] [--severity-filter <Error,Warning,...>] [--limit <n>]
//!   load_spec <file_path> [--alias <alias>]
//!   trace_root_cause <waveform_id> <deps_id> <signal_path> [--time-index <idx> | --time-value <val> --time-unit <ns>] [--spec-id <id>] [--max-depth <n>] [--simulator <name>]
//!   find_fan_in <deps_id> <signal_path> [--simulator <name>]
//!   batch_trace_root_cause <waveform_id> <deps_id> <assertion_id> [--spec-id <id>] [--max-depth <n>] [--severity-filter <list>] [--simulator <name>]
//!   export_bfs_report <trace_id> [--format json|markdown|html]
//!   extract_signal_values <waveform_id> [--signal <path> | --bit-mapping "0=bit0,1=bit1,..."] [--start <idx>] [--end <idx>] [--format hex|binary|decimal] [--downsample <n>]
//!   analyze_handshake <waveform_id> --valid <path> --ready <path> [--data <path>] [--start <idx>] [--end <idx>] [--limit <n>] [--report-mode summary|detail] [--filter-zero-delay]
//!   measure_signal <waveform_id> --signal <path> --analysis-type clock|pulse [--start <idx>] [--end <idx>] [--edge-type posedge|negedge]
//!   compare_signals <waveform_id> --signals <path1,path2,...> [--mode all_equal|reference_vs_actual] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]
//!   multi_signal_timeline <waveform_id> --signals <path1,path2,...> [--merge union|intersection] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]
//!   auto_discover_signals <waveform_id> [--mode bus_slices|clocks|groups|all] [--scope <path>] [--pattern <regex>]
//!   detect_sequence <waveform_id> --steps "cond1,cond2,cond3" [--max-gap <n>] [--start <idx>] [--end <idx>] [--limit <n>]
//!   compute_crc <waveform_id> --data <path> --polynomial crc8|crc16_ccitt|crc32_ethernet [--crc <path>] [--valid <path>] [--init <value>] [--start <idx>] [--end <idx>] [--limit <n>]
//!   help [<command>]

mod advanced_cmds;
mod agent;
mod analysis_cmds;
mod help;
mod phase3_cmds;
mod report_cmds;
mod utils;
mod waveform_cmds;

use std::collections::HashMap;
use std::path::PathBuf;

use wave_analyzer_mcp::assertion::AssertionParseResult;
use wave_analyzer_mcp::bfs::BfsResult;
use wave_analyzer_mcp::{CliOptions, Command, DepGraph, RunSummary, SpecLookup, parse_args};

struct CliStore {
    waveforms: HashMap<String, wellen::simple::Waveform>,
    /// Maps waveform_id to original file path (to fix filename vs alias BUG)
    original_filenames: HashMap<String, String>,
    dep_graphs: HashMap<String, DepGraph>,
    assertions: HashMap<String, AssertionParseResult>,
    specs: HashMap<String, SpecLookup>,
    bfs_results: HashMap<String, BfsResult>,
    run_summaries: HashMap<String, RunSummary>,
}

impl CliStore {
    fn new() -> Self {
        Self {
            waveforms: HashMap::new(),
            original_filenames: HashMap::new(),
            dep_graphs: HashMap::new(),
            assertions: HashMap::new(),
            specs: HashMap::new(),
            bfs_results: HashMap::new(),
            run_summaries: HashMap::new(),
        }
    }

    fn open_waveform(&mut self, file_path: &str, alias: Option<String>) -> Result<String, String> {
        let path = PathBuf::from(file_path);

        if !path.exists() {
            return Err(format!("File not found: {}", file_path));
        }

        let waveform =
            wellen::simple::read(&path).map_err(|e| format!("Failed to read waveform: {}", e))?;

        let id = alias.unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

        self.waveforms.insert(id.clone(), waveform);
        // BUG-filename fix: store original file path separately from alias
        self.original_filenames
            .insert(id.clone(), file_path.to_string());
        Ok(id)
    }

    fn close_waveform(&mut self, waveform_id: &str) -> Result<(), String> {
        match self.waveforms.remove(waveform_id) {
            Some(_) => Ok(()),
            None => Err(format!("Waveform not found: {}", waveform_id)),
        }
    }

    fn get(&self, waveform_id: &str) -> Option<&wellen::simple::Waveform> {
        self.waveforms.get(waveform_id)
    }

    fn get_mut(&mut self, waveform_id: &str) -> Option<&mut wellen::simple::Waveform> {
        self.waveforms.get_mut(waveform_id)
    }
}

fn print_usage() {
    println!("{}", help::print_usage_text());
}

fn command_name(cmd: &Command) -> &'static str {
    match cmd {
        Command::OpenWaveform { .. } => "open_waveform",
        Command::CloseWaveform { .. } => "close_waveform",
        Command::ListSignals { .. } => "list_signals",
        Command::ReadHierarchy { .. } => "read_hierarchy",
        Command::ReadSignal { .. } => "read_signal",
        Command::GetSignalInfo { .. } => "get_signal_info",
        Command::FindSignalEvents { .. } => "find_signal_events",
        Command::FindConditionalEvents { .. } => "find_conditional_events",
        Command::LoadDependencies { .. } => "load_deps",
        Command::LoadAssertionLog { .. } => "load_assertion_log",
        Command::LoadDesignSpec { .. } => "load_spec",
        Command::TraceRootCause { .. } => "trace_root_cause",
        Command::FindFanIn { .. } => "find_fan_in",
        Command::FindFanOut { .. } => "find_fan_out",
        Command::BatchTraceRootCause { .. } => "batch_trace_root_cause",
        Command::ExportBfsReport { .. } => "export_bfs_report",
        Command::ExtractSignalValues { .. } => "extract_signal_values",
        Command::AnalyzeHandshake { .. } => "analyze_handshake",
        Command::MeasureSignal { .. } => "measure_signal",
        Command::CompareSignals { .. } => "compare_signals",
        Command::MultiSignalTimeline { .. } => "multi_signal_timeline",
        Command::AutoDiscoverSignals { .. } => "auto_discover_signals",
        Command::DetectSequence { .. } => "detect_sequence",
        Command::ComputeCrc { .. } => "compute_crc",
        Command::SuggestEntrySignals { .. } => "suggest_entry_signals",
        Command::GenerateSummary { .. } => "generate_summary",
        Command::ExportSvg { .. } => "export_svg",
        Command::LoadRunSummary { .. } => "load_run_summary",
        Command::AnalyzeRun { .. } => "analyze_run",
        Command::ExtractDeps { .. } => "extract_deps",
        Command::CheckEnv => "check_env",
        Command::AnalyzeCdc { .. } => "analyze_cdc",
        Command::AnalyzeSignalPatterns { .. } => "analyze_signal_patterns",
        Command::ExtractFsm { .. } => "extract_fsm",
        Command::AnalyzeProtocol { .. } => "analyze_protocol",
        Command::AnalyzePhasedArray { .. } => "analyze_phased_array",
        Command::TimeConvert { .. } => "time_convert",
        Command::Help { .. } => "help",
    }
}

fn parse_signal_info_output(output: &str) -> Option<serde_json::Value> {
    let mut signal = None;
    let mut signal_type = None;
    let mut width_bits = None;
    let mut index = None;

    for line in output.lines() {
        if let Some(value) = line.strip_prefix("Signal: ") {
            signal = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("Type: ") {
            signal_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("Width: ") {
            if let Some(bits_str) = value.trim().strip_suffix(" bits") {
                width_bits = bits_str.trim().parse::<u32>().ok();
            }
        } else if let Some(value) = line.strip_prefix("Index: ") {
            index = Some(value.trim().to_string());
        }
    }

    Some(serde_json::json!({
        "signal": signal?,
        "type": signal_type?,
        "width": width_bits?,
        "index": index?
    }))
}

#[cfg(test)]
fn chain_continued_message(cmd: &Command, remaining: usize) -> String {
    format!(
        "Continuing despite error in '{}'; {} command(s) remaining",
        command_name(cmd),
        remaining
    )
}

fn execute_command_text(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    match cmd {
        Command::OpenWaveform { file_path, alias } => {
            waveform_cmds::exec_open_waveform(store, file_path, alias.clone())
        }
        Command::CloseWaveform { waveform_id } => {
            waveform_cmds::exec_close_waveform(store, waveform_id)
        }
        Command::ListSignals {
            waveform_id,
            name_pattern,
            hierarchy_prefix,
            recursive,
            limit,
        } => waveform_cmds::exec_list_signals(
            store,
            waveform_id,
            name_pattern.as_deref(),
            hierarchy_prefix.as_deref(),
            *recursive,
            *limit,
        ),
        Command::ReadHierarchy {
            waveform_id,
            scope_path,
            recursive,
            limit,
        } => waveform_cmds::exec_read_hierarchy(
            store,
            waveform_id,
            scope_path.as_deref(),
            *recursive,
            *limit,
        ),
        Command::ReadSignal {
            waveform_id,
            signal_path,
            time_index,
            time_indices,
        } => waveform_cmds::exec_read_signal(
            store,
            waveform_id,
            signal_path,
            *time_index,
            time_indices.as_ref().map(|v| v as &Vec<usize>),
        ),
        Command::GetSignalInfo {
            waveform_id,
            signal_path,
        } => waveform_cmds::exec_get_signal_info(store, waveform_id, signal_path),
        Command::FindSignalEvents {
            waveform_id,
            signal_path,
            start_time_index,
            end_time_index,
            start_time_value,
            end_time_value,
            time_unit,
            limit,
        } => waveform_cmds::exec_find_events(
            store,
            waveform_id,
            signal_path,
            *start_time_index,
            *end_time_index,
            *start_time_value,
            *end_time_value,
            time_unit.as_deref(),
            *limit,
        ),
        Command::FindConditionalEvents {
            waveform_id,
            condition,
            start_time_index,
            end_time_index,
            limit,
        } => waveform_cmds::exec_find_conditional_events(
            store,
            waveform_id,
            condition,
            *start_time_index,
            *end_time_index,
            *limit,
        ),
        // Phase 3 commands
        Command::LoadDependencies { file_path, alias } => {
            phase3_cmds::exec_load_deps(store, file_path, alias.clone())
        }
        Command::LoadAssertionLog {
            file_path,
            alias,
            severity_filter,
            limit,
        } => phase3_cmds::exec_load_assertion_log(
            store,
            file_path,
            alias.clone(),
            severity_filter.as_ref().map(|v| v as &Vec<String>),
            *limit,
        ),
        Command::LoadDesignSpec { file_path, alias } => {
            phase3_cmds::exec_load_spec(store, file_path, alias.clone())
        }
        Command::TraceRootCause { .. } => phase3_cmds::exec_trace_root_cause(store, cmd),
        Command::FindFanIn {
            deps_id,
            signal_path,
            simulator,
        } => phase3_cmds::exec_find_fan_in(store, deps_id, signal_path, simulator.clone()),
        Command::FindFanOut {
            deps_id,
            signal_path,
            simulator,
        } => phase3_cmds::exec_find_fan_out(store, deps_id, signal_path, simulator.clone()),
        Command::BatchTraceRootCause { .. } => phase3_cmds::exec_batch_trace(store, cmd),
        Command::ExportBfsReport { trace_id, format } => {
            phase3_cmds::exec_export_bfs_report(store, trace_id, format.as_deref())
        }
        // Analysis commands
        Command::ExtractSignalValues { .. } => analysis_cmds::exec_extract(store, cmd),
        Command::AnalyzeHandshake { .. } => analysis_cmds::exec_handshake(store, cmd),
        Command::MeasureSignal { .. } => analysis_cmds::exec_measure(store, cmd),
        Command::CompareSignals { .. } => analysis_cmds::exec_compare(store, cmd),
        Command::MultiSignalTimeline { .. } => analysis_cmds::exec_timeline(store, cmd),
        Command::AutoDiscoverSignals { .. } => analysis_cmds::exec_discover(store, cmd),
        Command::DetectSequence { .. } => analysis_cmds::exec_sequence(store, cmd),
        Command::ComputeCrc { .. } => analysis_cmds::exec_crc(store, cmd),
        // Report commands
        Command::SuggestEntrySignals { .. } => report_cmds::exec_suggest_entry(store, cmd),
        Command::GenerateSummary { .. } => report_cmds::exec_summary(store, cmd),
        Command::ExportSvg { .. } => report_cmds::exec_export_svg(store, cmd),
        Command::LoadRunSummary { file_path, alias } => {
            report_cmds::exec_load_run_summary(store, file_path, alias.clone())
        }
        Command::AnalyzeRun { .. } => report_cmds::exec_analyze_run(store, cmd),
        Command::Help { .. } => report_cmds::exec_help(cmd),
        Command::CheckEnv => report_cmds::exec_check_env(),
        Command::ExtractDeps { .. } => report_cmds::exec_extract_deps(cmd),
        // Advanced commands
        Command::AnalyzeCdc { .. } => advanced_cmds::exec_analyze_cdc(store, cmd),
        Command::AnalyzeSignalPatterns { .. } => advanced_cmds::exec_analyze_patterns(store, cmd),
        Command::ExtractFsm { .. } => advanced_cmds::exec_extract_fsm(store, cmd),
        Command::AnalyzeProtocol { .. } => advanced_cmds::exec_analyze_protocol(store, cmd),
        Command::AnalyzePhasedArray { .. } => advanced_cmds::exec_analyze_phased_array(store, cmd),
        Command::TimeConvert { .. } => waveform_cmds::exec_time_convert(store, cmd),
    }
}

/// Execute a command and return JSON output
/// For simplicity, we reuse the text output and wrap it in JSON.
/// This avoids duplicating complex API calls and field access patterns.
fn execute_command_json(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    if let Command::GetSignalInfo {
        waveform_id,
        signal_path,
    } = cmd
    {
        let waveform = store
            .get(waveform_id)
            .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
        let hierarchy = waveform.hierarchy();
        let info = wave_analyzer_mcp::get_signal_metadata(hierarchy, signal_path)
            .map_err(|e| format!("Error getting signal info: {}", e))?;
        let structured = parse_signal_info_output(&info).unwrap_or_else(|| {
            serde_json::json!({
                "output": info
            })
        });
        return Ok(serde_json::json!({
            "status": "ok",
            "command": command_name(cmd),
            "result": structured
        })
        .to_string());
    }

    if let Command::TraceRootCause { .. } = cmd {
        return phase3_cmds::exec_trace_root_cause_json(store, cmd);
    }

    let text_output = execute_command_text(store, cmd)?;
    if matches!(cmd, Command::AnalyzeRun { .. }) {
        return Ok(text_output);
    }

    Ok(serde_json::json!({
        "status": "ok",
        "command": command_name(cmd),
        "output": text_output
    })
    .to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.as_slice() == ["agent", "--stdio-json"] {
        if let Err(e) = agent::run_stdio_json() {
            eprintln!("agent error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    if args.is_empty() {
        print_usage();
        std::process::exit(0);
    }

    let options: CliOptions = match parse_args(args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut store = CliStore::new();
    let mut error_count = 0;

    for (i, cmd) in options.commands.iter().enumerate() {
        if i > 0 && !options.json {
            println!();
        }

        let result = if options.json {
            execute_command_json(&mut store, cmd)
        } else {
            execute_command_text(&mut store, cmd)
        };

        match result {
            Ok(output) => println!("{}", output),
            Err(e) => {
                error_count += 1;
                let remaining = options.commands.len().saturating_sub(i + 1);
                if options.json {
                    let mut err_json = serde_json::json!({
                        "status": "error",
                        "command": command_name(cmd),
                        "message": e
                    });
                    if remaining > 0 {
                        err_json["chain_continued"] = serde_json::Value::String(format!(
                            "Continuing despite error; {} command(s) remaining",
                            remaining
                        ));
                    }
                    eprintln!("{}", serde_json::to_string(&err_json).unwrap());
                } else {
                    eprintln!("Error in '{}': {}", command_name(cmd), e);
                    if remaining > 0 {
                        eprintln!(
                            "Continuing despite error; {} command(s) remaining",
                            remaining
                        );
                    }
                }
                // BUG-19 fix: continue executing remaining commands instead of aborting
            }
        }
    }

    if error_count > 0 && error_count == options.commands.len() {
        // All commands failed: exit with error code
        std::process::exit(1);
    } else if error_count > 0 {
        // Some commands succeeded, some failed: partial success.
        // Exit with code 0 (success) since at least one command completed.
        // The error messages are already printed above.
    }
}

#[cfg(test)]
mod tests {
    use super::{
        chain_continued_message, parse_signal_info_output, utils::trace_root_cause_json_payload,
    };
    use wave_analyzer_mcp::Command;
    use wave_analyzer_mcp::bfs::{BfsNode, BfsResult, NodeStatus, RootCauseCandidate};

    #[test]
    fn test_parse_signal_info_output_to_structured_json() {
        let parsed =
            parse_signal_info_output("Signal: top.data\nType: Wire\nWidth: 4 bits\nIndex: [3:0]")
                .expect("signal info should parse");

        assert_eq!(parsed["signal"], "top.data");
        assert_eq!(parsed["type"], "Wire");
        assert_eq!(parsed["width"], 4);
        assert_eq!(parsed["index"], "[3:0]");
    }

    #[test]
    fn test_chain_continued_message_mentions_failing_command() {
        let cmd = Command::ReadSignal {
            waveform_id: "w1".to_string(),
            signal_path: "top.data".to_string(),
            time_index: Some(1),
            time_indices: None,
        };

        assert_eq!(
            chain_continued_message(&cmd, 2),
            "Continuing despite error in 'read_signal'; 2 command(s) remaining"
        );
    }

    #[test]
    fn test_trace_root_cause_json_payload_includes_expected_hint() {
        let result = BfsResult {
            root_signal: "top.out".to_string(),
            root_time_index: 3,
            root_time_ps: 3000,
            tree: vec![BfsNode {
                signal_path: "top.in".to_string(),
                resolved_signal_path: "top.in".to_string(),
                time_index: 2,
                time_ps: 2000,
                depth: 1,
                status: NodeStatus::Ok,
                actual_value: Some("1'b1".to_string()),
                expected_hint: Some("expected 1'b1".to_string()),
                edge_type: Some("sequential".to_string()),
                clock_name: Some("clk".to_string()),
                latency_cycles: Some(1),
                note: None,
                parent_id: Some("n0".to_string()),
                node_id: "n1".to_string(),
                source_clock_domain: None,
                dest_clock_domain: None,
                synchronizer_info: None,
            }],
            candidates: vec![RootCauseCandidate {
                signal_path: "top.in".to_string(),
                time_index: 2,
                time_ps: 2000,
                status: NodeStatus::Ok,
                reason: "ok".to_string(),
            }],
            summary: "summary".to_string(),
        };

        let payload = trace_root_cause_json_payload("trace-1", &result);
        assert_eq!(payload["trace_id"], "trace-1");
        assert_eq!(
            payload["result"]["tree"][0]["expected_hint"],
            "expected 1'b1"
        );
    }
}
