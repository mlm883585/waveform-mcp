/// Return the full usage text as a String (for help command and JSON output)
pub(super) fn print_usage_text() -> String {
    let mut out = String::new();
    out.push_str("wave-analyzer-cli - Command-line interface for waveform analysis\n");
    out.push('\n');
    out.push_str(
        "Usage: wave-analyzer-cli [--json] <command1> [args...] [-- <command2> [args...] ...]\n",
    );
    out.push('\n');
    out.push_str("Global flags:\n");
    out.push_str("  --json, -j          Output results as JSON (text wrapped in {\"status\",\"command\",\"output\"})\n");
    out.push('\n');
    out.push_str("Commands can be chained using '--' as a separator.\n");
    out.push_str("Use 'help <command>' for details on a specific command.\n");
    out.push('\n');
    out.push_str("External Tools (PATH configuration on Windows):\n");
    out.push_str("  The commands below may invoke external tools. On Windows, add the\n");
    out.push_str("  tool's bin/ directory to your system PATH. Changes via setx only\n");
    out.push_str("  affect NEW cmd windows.\n");
    out.push('\n');
    out.push_str("  iverilog  (used by extract_deps via the sidecar extractor)\n");
    out.push_str("    Install:        http://iverilog.icarus.com/\n");
    out.push_str("    Example path:   D:\\software\\iverilog\\bin\n");
    out.push_str("    Add to PATH:    setx PATH \"%PATH%;D:\\software\\iverilog\\bin\"\n");
    out.push_str("    Verify:         where iverilog\n");
    out.push_str("    Alt env vars:   IVERILOG_HOME=D:\\software\\iverilog\n");
    out.push_str("                    IVERILOG_PATH=D:\\software\\iverilog (legacy)\n");
    out.push('\n');
    out.push_str("  Python 3 + pyverilog  (fallback for extract_deps when no sidecar)\n");
    out.push_str("    Install:        https://www.python.org/downloads/  (3.8+)\n");
    out.push_str("    Must be on PATH as: python3 | python | py\n");
    out.push_str("    Install deps:   pip install -r tools/deps-extractor/requirements.txt\n");
    out.push_str("    Verify:         python --version\n");
    out.push('\n');
    out.push_str("  Vivado  (only for extract_deps --engine vivado)\n");
    out.push_str("    Install:        Xilinx Vivado (tested with 2018.3)\n");
    out.push_str("    Example path:   D:\\software\\Xilinx\\Vivado\\2018.3\\bin\n");
    out.push_str(
        "    Add to PATH:    setx PATH \"%PATH%;D:\\software\\Xilinx\\Vivado\\2018.3\\bin\"\n",
    );
    out.push_str("    Verify:         vivado -version\n");
    out.push_str("    Alt env vars:   VIVADO_PATH=D:\\software\\Xilinx\n");
    out.push_str("                    (consumed by tools/sim-scripts/run_sim_modelsim.ps1)\n");
    out.push('\n');
    out.push_str("  On Linux/macOS, use your package manager (apt, brew, dnf) and the\n");
    out.push_str("  same IVERILOG_HOME / VIVADO_PATH environment variables apply.\n");
    out.push('\n');
    out.push_str("Commands:\n");
    out.push_str("  open_waveform <file_path> [--alias <alias>]\n");
    out.push_str("  close_waveform <waveform_id>\n");
    out.push_str("  list_signals <waveform_id> [--pattern <pattern>] [--hierarchy <prefix>] [--recursive <true|false>] [--limit <n>]\n");
    out.push_str("  read_hierarchy <waveform_id> [--scope <scope>] [--recursive <true|false>] [--limit <n>]\n");
    out.push_str("  read_signal <waveform_id> <signal_path> [--time-index <idx> | --time-indices <idx1,idx2,...>]\n");
    out.push_str("  get_signal_info <waveform_id> <signal_path>\n");
    out.push_str("  find_signal_events <waveform_id> <signal_path> [--start <idx> | --start-time <val> --time-unit <ns>] [--end <idx> | --end-time <val> --time-unit <ns>] [--limit <n>]\n");
    out.push_str("  find_conditional_events <waveform_id> <condition> [--start <idx>] [--end <idx>] [--limit <n>]\n");
    out.push_str("  load_deps <file_path> [--alias <alias>]\n");
    out.push_str("  load_assertion_log <file_path> [--alias <alias>] [--severity-filter <Error,Warning,...>] [--limit <n>]\n");
    out.push_str("  load_spec <file_path> [--alias <alias>]\n");
    out.push_str("  trace_root_cause <waveform_id> <deps_id> <signal_path> [--time-index <idx> | --time-value <val> --time-unit <ns>] [--spec-id <id>] [--max-depth <n>] [--simulator <name>]\n");
    out.push_str("  find_fan_in <deps_id> <signal_path> [--simulator <name>]\n");
    out.push_str("  batch_trace_root_cause <waveform_id> <deps_id> <assertion_id> [--spec-id <id>] [--max-depth <n>] [--severity-filter <list>] [--simulator <name>]\n");
    out.push_str("  export_bfs_report <trace_id> [--format json|markdown|html]\n");
    out.push_str("  extract_signal_values <waveform_id> [--signal <path> | --bit-mapping \"0=bit0,1=bit1,...\"] [--start <idx>] [--end <idx>] [--format hex|binary|decimal] [--downsample <n>]\n");
    out.push_str("  analyze_handshake <waveform_id> --valid <path> --ready <path> [--data <path>] [--start <idx>] [--end <idx>] [--limit <n>] [--report-mode summary|detail] [--filter-zero-delay] [--level-sensitive]\n");
    out.push_str("  measure_signal <waveform_id> --signal <path> --analysis-type clock|pulse|interval [--start <idx>] [--end <idx>] [--edge-type posedge|negedge] [--from-condition <expr>] [--to-condition <expr>] [--expected-value <val> --expected-unit <ps|ns|us|ms|s>]\n");
    out.push_str("  compare_signals <waveform_id> --signals <path1,path2,...> [--mode all_equal|reference_vs_actual] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]\n");
    out.push_str("  multi_signal_timeline <waveform_id> --signals <path1,path2,...> [--merge union|intersection] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]\n");
    out.push_str("  auto_discover_signals <waveform_id> [--mode bus_slices|clocks|groups|all] [--scope <path>] [--pattern <regex>]\n");
    out.push_str("  detect_sequence <waveform_id> --steps \"cond1,cond2,cond3\" [--max-gap <n>] [--start <idx>] [--end <idx>] [--limit <n>]\n");
    out.push_str("  compute_crc <waveform_id> --data <path> --polynomial crc8|crc16_ccitt|crc32_ethernet [--crc <path>] [--valid <path>] [--clear <path>] [--clock <path>] [--init <value>] [--start <idx>] [--end <idx>] [--limit <n>]\n");
    out.push_str("  load_run_summary <file_path> [--alias <alias>]\n");
    out.push_str("  analyze_run <run_summary.json> [--deps <deps.yaml>] [--spec <design_spec.yaml>] [--transcript <transcript.log>] [--waveform <dump.vcd|dump.fst>] [--severity-filter <Error,Failure>] [--max-depth <n>] [--simulator <name>] [--report-dir <dir>] [--report-format json|markdown|html|none]\n");
    out.push_str("  suggest_entry_signals <waveform_id> <deps_id> [--assertion <name>] [--scope <path>] [--limit <n>] [--simulator <name>]\n");
    out.push_str("  generate_summary <waveform_id> [--signal <path>] [--max-samples <n>]\n");
    out.push_str("  export_svg <waveform_id> [--signal <path>] [--time-range <start,end>] [--width <px>] [--height <px>]\n");
    out.push_str("  extract_deps <rtl_path> <top_module> [--engine pyverilog|vivado] [--annotate <annotations.yaml>] [--output <deps.yaml>] [--deps-extractor-path <dir>]\n");
    out.push_str(
        "  check_env                     Diagnose environment (sidecar, iverilog, VC++ Runtime)\n",
    );
    out.push_str("  time_convert <waveform_id> [--time-value <val> --time-unit <ps|ns|us|ms|s> | --time-index <idx>]\n");
    out.push_str(
        "  help [<command>]              Show this help or details for a specific command\n",
    );
    out
}

/// Print detailed help for a specific command
pub(super) fn print_command_help_detail(name: &str) -> String {
    match name {
        "open_waveform" => r#"open_waveform <file_path> [--alias <alias>]

Open a VCD or FST waveform file and assign it an ID for later commands.

Parameters:
  file_path         (required) Path to .vcd or .fst waveform file
  --alias, -a       (optional) Custom ID for this waveform (default: filename)

Examples:
  open_waveform sim/transcript.vcd
  open_waveform output.fst --alias my_wave

Related: close_waveform"#.to_string(),

        "close_waveform" => r#"close_waveform <waveform_id>

Close a waveform and free its memory. After this, the waveform_id is invalid.

Parameters:
  waveform_id       (required) ID of the waveform to close (from open_waveform)

Examples:
  close_waveform transcript.vcd
  close_waveform my_wave

Related: open_waveform"#.to_string(),

        "list_signals" => r#"list_signals <waveform_id> [--pattern <pattern>] [--hierarchy <prefix>] [--recursive <true|false>] [--limit <n>]

List signals matching an optional pattern or hierarchy prefix.

Parameters:
  waveform_id       (required) ID of the waveform to query
  --pattern, -p     (optional) Filter by signal name (supports regex, e.g., '.*clk', '^TOP\.rst')
  --hierarchy, -h   (optional) Filter by hierarchy scope prefix
  --recursive, -r   (optional) Include nested scopes (default: true)
  --limit, -l       (optional) Max signals to show (default: 100)

Examples:
  list_signals my_wave
  list_signals my_wave --pattern ".*clk"
  list_signals my_wave --hierarchy TOP.u_ctrl --recursive false
  list_signals my_wave --pattern "data.*" --limit 50

Related: read_hierarchy, get_signal_info"#.to_string(),

        "read_hierarchy" => r#"read_hierarchy <waveform_id> [--scope <scope>] [--recursive <true|false>] [--limit <n>]

Read the waveform module hierarchy as an indented tree.

Parameters:
  waveform_id       (required) ID of the waveform to query
  --scope, -s       (optional) Start from this scope path (default: root)
  --recursive, -r   (optional) Show nested scopes (default: false)
  --limit, -l       (optional) Max entries to show (default: 200)

Examples:
  read_hierarchy my_wave
  read_hierarchy my_wave --scope TOP.u_channel__0
  read_hierarchy my_wave --scope TOP --recursive true

Related: list_signals"#.to_string(),

        "read_signal" => r#"read_signal <waveform_id> <signal_path> [--time-index <idx> | --time-indices <idx1,idx2,...>]

Read signal values at specific time indices.

Parameters:
  waveform_id       (required) ID of the waveform
  signal_path       (required) Full path to the signal (e.g., TOP.data_o)
  --time-index, -t  (optional) Single time index to read
  --time-indices, -T (optional) Comma-separated list of time indices

Examples:
  read_signal my_wave TOP.data_o --time-index 100
  read_signal my_wave TOP.addr --time-indices 0,50,100,150

Related: get_signal_info, find_signal_events"#.to_string(),

        "get_signal_info" => r#"get_signal_info <waveform_id> <signal_path>

Get metadata about a signal: width, direction, format.

Parameters:
  waveform_id       (required) ID of the waveform
  signal_path       (required) Full path to the signal

Examples:
  get_signal_info my_wave TOP.clk
  get_signal_info my_wave TOP.data_o[7:0]

Related: list_signals, read_signal"#.to_string(),

        "find_signal_events" => r#"find_signal_events <waveform_id> <signal_path> [--start <idx>] [--end <idx>] [--limit <n>]

Find all value changes (events) of a signal within a time range.

Parameters:
  waveform_id       (required) ID of the waveform
  signal_path       (required) Full path to the signal
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max events to return (default: 100)

Examples:
  find_signal_events my_wave TOP.clk
  find_signal_events my_wave TOP.valid --start 0 --end 1000
  find_signal_events my_wave TOP.ready --limit 50

Related: read_signal, find_conditional_events"#.to_string(),

        "find_conditional_events" => r#"find_conditional_events <waveform_id> <condition> [--start <idx>] [--end <idx>] [--limit <n>]

Find time indices where a Verilog-style condition is satisfied.

Condition syntax:
  Signal paths      TOP.valid, TOP.u_ctrl.state
  ~ (NOT)           ~TOP.reset
  & (AND)           TOP.valid & TOP.ready
  | (OR)            sig_a | sig_b
  ^ (XOR)           TOP.a ^ TOP.b
  &&, ||            Logical AND, OR
  ==, !=            Equality comparison
  $past(sig)        Previous value of signal
  Bit extraction    TOP.bus[3], TOP.bus[7:4]
  Verilog literals  8'hFF, 1'b0, 4'b1010

Parameters:
  waveform_id       (required) ID of the waveform
  condition         (required) Verilog-style condition expression
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max events to return (default: 100)

Examples:
  find_conditional_events my_wave "TOP.valid & TOP.ready"
  find_conditional_events my_wave "~TOP.reset"
  find_conditional_events my_wave "TOP.state == 3'b010"
  find_conditional_events my_wave "$past(TOP.valid) && TOP.ready" --limit 20

Related: find_signal_events"#.to_string(),

        "load_deps" => r#"load_deps <file_path> [--alias <alias>]

Load a deps.yaml dependency graph file for BFS root-cause analysis.

Parameters:
  file_path         (required) Path to deps.yaml file
  --alias, -a       (optional) Custom ID for this dependency graph (default: filename)

Examples:
  load_deps deps.yaml
  load_deps output/deps.yaml --alias my_deps

Related: trace_root_cause, find_fan_in"#.to_string(),

        "load_assertion_log" => r#"load_assertion_log <file_path> [--alias <alias>] [--severity-filter <Error,Warning,...>] [--limit <n>]

Parse a ModelSim/Questa transcript for assertion/check failure events (SystemVerilog assertions, SVA, cover statements).

Parameters:
  file_path         (required) Path to transcript/log file
  --alias, -a       (optional) Custom ID (default: filename)
  --severity-filter (optional) Comma-separated: Error,Warning,Info
  --severity-filter (optional) Filter by severity (default: all)
  --limit, -l       (optional) Max events to parse (default: 100)

Examples:
  load_assertion_log sim/transcript.log
  load_assertion_log sim.log --severity-filter Error,Warning
  load_assertion_log sim.log --alias my_assertions --limit 50

Related: trace_root_cause"#.to_string(),

        "load_spec" => r#"load_spec <file_path> [--alias <alias>]

Load a design_spec.yaml file for debug entry points and stop signals.

Parameters:
  file_path         (required) Path to design_spec.yaml file
  --alias, -a       (optional) Custom ID (default: filename)

Examples:
  load_spec design_spec.yaml
  load_spec specs/beam_ctrl.yaml --alias my_spec

Related: trace_root_cause"#.to_string(),

        "trace_root_cause" => r#"trace_root_cause <waveform_id> <deps_id> <signal_path> [--time-index <idx> | --time-value <val> --time-unit <ns>] [--spec-id <id>] [--max-depth <n>] [--simulator <name>]

BFS root-cause trace from a failing signal backward through dependency graph.

Parameters:
  waveform_id       (required) ID of the waveform
  deps_id           (required) ID of the loaded dependency graph
  signal_path       (required) Signal path to trace from (output signal)
  --time-index      (optional) Time index in waveform
  --time-value      (optional) Time value (with --time-unit)
  --time-unit       (optional) Time unit: ps, ns, us, ms, s
  --spec-id         (optional) ID of loaded design spec (for stop signals)
  --max-depth, -d   (optional) Max BFS depth (default: 8)
  --simulator       (optional) Simulator name (default: modelsim)

Examples:
  trace_root_cause my_wave my_deps TOP.data_o --time-index 100
  trace_root_cause my_wave my_deps TOP.out --time-value 50 --time-unit ns
  trace_root_cause my_wave my_deps TOP.result --time-index 200 --max-depth 5
  trace_root_cause my_wave my_deps TOP.flag --time-index 100 --spec-id my_spec

Related: load_deps, load_spec, find_fan_in"#.to_string(),

        "find_fan_in" => r#"find_fan_in <deps_id> <signal_path> [--simulator <name>]

Query fan-in (upstream) dependency edges for a signal.

Parameters:
  deps_id           (required) ID of the loaded dependency graph
  signal_path       (required) Signal path to query (output signal)
  --simulator       (optional) Simulator name (default: modelsim)

Examples:
  find_fan_in my_deps TOP.data_o
  find_fan_in my_deps TOP.result --simulator modelsim

Related: load_deps, trace_root_cause"#.to_string(),

        "batch_trace_root_cause" => r#"batch_trace_root_cause <waveform_id> <deps_id> <assertion_id> [--spec-id <id>] [--max-depth <n>] [--severity-filter <list>] [--simulator <name>]

Batch BFS root-cause tracing for all failure events from a loaded assertion log. For each event, resolves entry signal (via spec or deps), runs trace_root_cause, and aggregates root cause candidates.

Parameters:
  waveform_id       (required) ID of the waveform
  deps_id           (required) ID of the loaded dependency graph
  assertion_id      (required) ID of the loaded assertion log (from load_assertion_log)
  --spec-id         (optional) ID of loaded design spec for entry signal resolution
  --max-depth, -d   (optional) Max BFS depth per trace (default: 8)
  --severity-filter (optional) Comma-separated: Error,Failure (default: all)
  --simulator       (optional) Simulator name (default: modelsim)

Examples:
  batch_trace_root_cause my_wave my_deps my_assertions
  batch_trace_root_cause my_wave my_deps my_assertions --spec-id my_spec
  batch_trace_root_cause my_wave my_deps my_assertions --severity-filter Error,Failure --max-depth 5

Related: trace_root_cause, load_assertion_log, export_bfs_report"#.to_string(),

        "export_bfs_report" => r#"export_bfs_report <trace_id> [--format json|markdown|html]

Export a BFS trace result as a formatted report. Use the trace_id returned by trace_root_cause.

Parameters:
  trace_id          (required) Trace ID from trace_root_cause output
  --format, -f      (optional) Output format: json, markdown, html (default: markdown)

Examples:
  export_bfs_report my_wave_TOP.data_o_100
  export_bfs_report my_wave_TOP.data_o_100 --format html
  export_bfs_report batch_my_wave_ASSERT_PASSED_5 --format json

Related: trace_root_cause, batch_trace_root_cause"#.to_string(),

        "extract_signal_values" => r#"extract_signal_values <waveform_id> [--signal <path> | --bit-mapping "0=bit0,1=bit1,..."] [--start <idx>] [--end <idx>] [--format hex|binary|decimal] [--downsample <n>]

Extract signal values or reconstruct composite signals from individual bits.

Parameters:
  waveform_id       (required) ID of the waveform
  --signal, -s      (optional) Single signal path to extract
  --bit-mapping     (optional) Comma-separated "bit=signal_path" for bus reconstruction
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --format          (optional) Output format: hex, binary, decimal (default: hex)
  --downsample      (optional) Show every Nth value

Examples:
  extract_signal_values my_wave --signal TOP.data_o
  extract_signal_values my_wave --bit-mapping "0=TOP.d[0],1=TOP.d[1],2=TOP.d[2],3=TOP.d[3]" --format hex
  extract_signal_values my_wave --signal TOP.clk --start 0 --end 100 --downsample 10

Related: read_signal, compare_signals"#.to_string(),

        "analyze_handshake" => r#"analyze_handshake <waveform_id> --valid <path> --ready <path> [--data <path>] [--start <idx>] [--end <idx>] [--limit <n>] [--report-mode summary|detail] [--filter-zero-delay]

Detect valid/ready handshake transactions with latency analysis.

Parameters:
  waveform_id       (required) ID of the waveform
  --valid           (required) Valid signal path
  --ready           (required) Ready signal path
  --data            (optional) Data signal path (for transaction value display)
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max transactions to report (default: 100)
  --report-mode     (optional) summary or detail (default: summary)
  --level-sensitive (optional) Count every sampled valid=1 and ready=1 index as one transfer

Examples:
  analyze_handshake my_wave --valid TOP.axi_valid --ready TOP.axi_ready
  analyze_handshake my_wave --valid TOP.v --ready TOP.r --data TOP.d --report-mode detail
  analyze_handshake my_wave --valid TOP.v --ready TOP.r --start 0 --end 5000
  analyze_handshake my_wave --valid TOP.o_enb --ready TOP.o_enb --level-sensitive

Related: measure_signal, multi_signal_timeline"#.to_string(),

        "measure_signal" => r#"measure_signal <waveform_id> --signal <path> --analysis-type clock|pulse|interval [--start <idx>] [--end <idx>] [--edge-type posedge|negedge] [--from-condition <expr>] [--to-condition <expr>] [--expected-value <val> --expected-unit <unit>]

Measure clock properties, pulse widths, or time intervals between condition events.

Modes:
  clock    Measure period, frequency, duty cycle, and jitter from a 1-bit signal.
  pulse    Measure high/low pulse widths in physical time (ns/ps).
  interval Measure time between two condition events (nanosecond-level precision).

Parameters:
  waveform_id       (required) ID of the waveform
  --signal          (required) Signal path to analyze
  --analysis-type   (required) clock, pulse, or interval
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --edge-type       (optional) posedge or negedge (default: posedge, clock mode only)

Interval mode parameters:
  --from-condition  (required for interval) Verilog condition for start event
  --to-condition    (required for interval) Verilog condition for end event
  --expected-value  (optional) Expected interval value (e.g., 8680)
  --expected-unit   (required with expected-value) Unit: ps, ns, us, ms, s

Examples:
  measure_signal my_wave --signal TOP.clk --analysis-type clock
  measure_signal my_wave --signal TOP.pulse --analysis-type pulse
  measure_signal my_wave --signal TOP.clk --analysis-type interval --from-condition "TOP.rst==0 & TOP.state==2" --to-condition "TOP.r_baud_cnt==433 & TOP.state==2" --expected-value 4340 --expected-unit ns

Related: analyze_handshake, find_signal_events"#.to_string(),

        "compare_signals" => r#"compare_signals <waveform_id> --signals <path1,path2,...> [--mode all_equal|reference_vs_actual] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]

Compare multiple signals and report mismatch points.

Parameters:
  waveform_id       (required) ID of the waveform
  --signals         (required) Comma-separated signal paths
  --mode            (optional) all_equal or reference_vs_actual (default: all_equal)
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max mismatches to report (default: 100)
  --format          (optional) hex, binary, decimal (default: hex)

Examples:
  compare_signals my_wave --signals TOP.out1,TOP.out2
  compare_signals my_wave --signals TOP.ref,TOP.actual --mode reference_vs_actual
  compare_signals my_wave --signals TOP.a,TOP.b,TOP.c --start 0 --end 500

Related: multi_signal_timeline, extract_signal_values"#.to_string(),

        "multi_signal_timeline" => r#"multi_signal_timeline <waveform_id> --signals <path1,path2,...> [--merge union|intersection] [--start <idx>] [--end <idx>] [--limit <n>] [--format hex|binary|decimal]

Generate unified timeline table for multiple signals (logic analyzer view).

Parameters:
  waveform_id       (required) ID of the waveform
  --signals         (required) Comma-separated signal paths
  --merge           (optional) union or intersection of time points (default: union)
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max rows to show (default: 100)
  --format          (optional) hex, binary, decimal (default: hex)

Examples:
  multi_signal_timeline my_wave --signals TOP.clk,TOP.valid,TOP.data
  multi_signal_timeline my_wave --signals TOP.a,TOP.b --merge intersection
  multi_signal_timeline my_wave --signals TOP.v,TOP.r,TOP.d --start 0 --end 200

Related: compare_signals, extract_signal_values"#.to_string(),

        "auto_discover_signals" => r#"auto_discover_signals <waveform_id> [--mode bus_slices|clocks|groups|all] [--scope <path>] [--pattern <regex>]

Auto-discover bus groups, clock signals, and reset signals from waveform hierarchy.

Parameters:
  waveform_id       (required) ID of the waveform
  --mode            (optional) Discovery mode: bus_slices, clocks, groups, all (default: all)
  --scope           (optional) Limit search to this scope path
  --pattern         (optional) Regex filter for signal names

Examples:
  auto_discover_signals my_wave
  auto_discover_signals my_wave --mode clocks
  auto_discover_signals my_wave --mode bus_slices --scope TOP.u_channel
  auto_discover_signals my_wave --mode groups --pattern ".*_valid"

Related: list_signals, read_hierarchy"#.to_string(),

        "detect_sequence" => r#"detect_sequence <waveform_id> --steps "cond1,cond2,cond3" [--max-gap <n>] [--start <idx>] [--end <idx>] [--limit <n>]

Detect ordered sequence of conditions with timing constraints.

Parameters:
  waveform_id       (required) ID of the waveform
  --steps           (required) Comma-separated condition expressions
  --max-gap         (optional) Max cycles between steps (default: unlimited)
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max sequences to report (default: 100)

Examples:
  detect_sequence my_wave --steps "TOP.valid,TOP.ready,TOP.data_valid"
  detect_sequence my_wave --steps "TOP.start,TOP.middle,TOP.done" --max-gap 10
  detect_sequence my_wave --steps "TOP.a,TOP.b" --start 0 --end 5000

Related: find_conditional_events"#.to_string(),

        "compute_crc" => r#"compute_crc <waveform_id> --data <path> --polynomial crc8|crc16_ccitt|crc32_ethernet [--crc <path>] [--valid <path>] [--clear <path>] [--clock <path>] [--init <value>] [--start <idx>] [--end <idx>] [--limit <n>]

Compute CRC over data bus, optionally verify against observed CRC signal.

Parameters:
  waveform_id       (required) ID of the waveform
  --data, -d        (required) Data bus signal path
  --polynomial, -p  (required) CRC polynomial: crc8, crc16_ccitt, crc32_ethernet
  --crc, -c         (optional) Observed CRC signal for verification
  --valid, -v       (optional) Data_valid signal path. When provided, only data
                     values at posedge (0→1) transitions are processed for CRC.
                     This filters out reset/initial values and matches hardware
                     behavior where CRC is only updated when data_valid=1.
  --clear, -r       (optional) Clear/reset signal path. When provided, the computed
                     CRC is reset to the init value whenever this signal is high.
                     In RTL: `if (clear) crc <= INIT;` has priority over data_valid.
  --clock, -k       (optional) Clock signal path for per-cycle sampling when no
                     data_valid is available. Data is sampled at every clock posedge,
                     correctly handling data held stable across multiple cycles.
                     Without --valid or --clock, only data change events are used,
                     which may miss stable data across cycles.
  --init            (optional) Initial CRC value (default: depends on polynomial)
  --start, -s       (optional) Start time index (default: 0)
  --end, -e         (optional) End time index (default: last)
  --limit, -l       (optional) Max CRC computations to perform (default: 100)

Examples:
  compute_crc my_wave --data TOP.data_bus --polynomial crc32_ethernet
  compute_crc my_wave --data TOP.d --polynomial crc16_ccitt --crc TOP.crc_out --valid TOP.d_valid --clear TOP.d_clear --init 0xFFFF
  compute_crc my_wave --data TOP.d --polynomial crc16_ccitt --clock TOP.clk --init 0xFFFF

Related: extract_signal_values, compare_signals"#.to_string(),

        "help" => r#"help [<command>]

Show this help summary or detailed help for a specific command.

Parameters:
  command_name      (optional) Name of the command to get details for

Examples:
  help
  help open_waveform
  help trace_root_cause

Related: all commands"#.to_string(),

        "suggest_entry_signals" => r#"suggest_entry_signals <waveform_id> <deps_id> [--assertion <name>] [--scope <path>] [--limit <n>] [--simulator <name>]

Auto-suggest candidate entry signals for BFS root-cause tracing from waveform hierarchy + dependency graph. Ranks signals by tier (T1: deps output, T2: deps boundary, T3: not in deps).

Parameters:
  waveform_id       (required) ID of the waveform
  deps_id           (required) ID of the loaded dependency graph
  --assertion, -a   (optional) Assertion/check event name to match signal names against
  --scope, -s       (optional) Limit search to this scope path
  --limit, -l       (optional) Max candidates to return (default: 10)
  --simulator       (optional) Simulator name for path resolution (default: modelsim)

Examples:
  suggest_entry_signals my_wave my_deps
  suggest_entry_signals my_wave my_deps --assertion p_fifo_full
  suggest_entry_signals my_wave my_deps --scope TOP.u_ctrl --limit 20

Related: load_deps, trace_root_cause, load_spec"#.to_string(),

        "generate_summary" => r#"generate_summary <waveform_id> [--signal <path>] [--max-samples <n>]

Generate a summary of signal changes in the waveform. Returns JSON with signal names, value changes, and timing information.

Parameters:
  waveform_id       (required) ID of the waveform
  --signal, -s      (optional) Signal path to summarize (can be repeated, default: auto-detect top signals)
  --max-samples     (optional) Max samples per signal (default: 100)

Examples:
  generate_summary my_wave
  generate_summary my_wave --signal TOP.clk --signal TOP.data
  generate_summary my_wave --max-samples 50

Related: list_signals, extract_signal_values"#.to_string(),

        "export_svg" => r#"export_svg <waveform_id> [--signal <path>] [--time-range <start,end>] [--width <px>] [--height <px>]

Export waveform signals to SVG format for sharing or printing. Returns SVG content that can be rendered in a browser.

Parameters:
  waveform_id       (required) ID of the waveform
  --signal, -s      (optional) Signal path to include (can be repeated, default: auto-detect)
  --time-range, -t  (optional) Time range to export: "start,end" in time indices
  --width, -w       (optional) Output image width in pixels (default: 800)
  --height, -h      (optional) Output image height in pixels (default: 600)

Examples:
  export_svg my_wave
  export_svg my_wave --signal TOP.clk --signal TOP.data
  export_svg my_wave --time-range 0,1000 --width 1200 --height 800

Related: generate_summary, list_signals"#.to_string(),

        "load_run_summary" => r#"load_run_summary <file_path> [--alias <alias>]

Parse a simulation run_summary.json file. Reports status, compile/elab/simulation results, error counts, and suggests next steps.

Handles PowerShell ConvertTo-Json boolean strings ("true"/"false") as well as proper JSON booleans.

Parameters:
  file_path         (required) Path to run_summary.json file
  --alias, -a       (optional) Custom ID for this run summary (default: filename)

Examples:
  load_run_summary sim/run_summary.json
  load_run_summary output/run_summary.json --alias my_run

Related: load_assertion_log, trace_root_cause"#.to_string(),

        "extract_deps" => r#"extract_deps <rtl_path> <top_module> [--engine pyverilog|vivado] [--annotate <annotations.yaml>] [--output <deps.yaml>] [--deps-extractor-path <dir>]

Extract a deps.yaml dependency graph from RTL source files using the deps-extractor pipeline.

Steps: RTL source → extract_deps_pyverilog.py → deps_raw.json → deps_converter.py → deps.yaml

Prerequisites (Windows):
  iverilog  on PATH (or IVERILOG_HOME set), if using the embedded sidecar.
  Python 3  on PATH as python3|python|py, plus `pip install -r
             tools/deps-extractor/requirements.txt` (pyverilog, pyyaml),
             if the sidecar is not embedded.
  Vivado    on PATH (or VIVADO_PATH set) only when --engine vivado.
  Run `help` for full PATH configuration examples and setx commands.

Parameters:
  rtl_path             (required) Path to RTL source directory or Verilog file
  top_module           (required) Top module name for extraction
  --engine, -e         (optional) Extraction engine: pyverilog (default) or vivado
  --annotate, -a       (optional) Path to annotations.yaml for manual overrides
  --output, -o         (optional) Output deps.yaml path (default: deps.yaml next to rtl_path)
  --deps-extractor-path (optional) Path to deps-extractor directory (default: auto-detect via DEPS_EXTRACTOR_PATH)

Examples:
  extract_deps rtl/ beam_ctrl
  extract_deps rtl/top.v beam_ctrl --engine pyverilog --annotate annotations.yaml
  extract_deps rtl/ beam_ctrl -o sim/deps.yaml --deps-extractor-path /path/to/deps-extractor

Related: load_deps, trace_root_cause"#.to_string(),

        "analyze_run" => r#"analyze_run <run_summary.json> [--deps <deps.yaml>] [--spec <design_spec.yaml>] [--transcript <transcript.log>] [--waveform <dump.vcd|dump.fst>] [--severity-filter <Error,Failure>] [--max-depth <n>] [--simulator <name>] [--report-dir <dir>] [--report-format json|markdown|html|none]

Run the full failure-analysis workflow from run_summary.json. Assertion/check failures are parsed from transcript.log, entry signals are resolved from design_spec.yaml or suggest_entry_signals, and BFS traces are generated automatically.

Parameters:
  run_summary_path  (required) Path to run_summary.json file
  --deps            (optional) deps.yaml path (default: deps.yaml next to run_summary.json)
  --spec            (optional) design_spec.yaml path (default: design_spec.yaml next to run_summary.json)
  --transcript      (optional) transcript.log path (default: transcript_file in run_summary.json)
  --waveform        (optional) waveform path (default: wave_file in run_summary.json)
  --severity-filter (optional) Comma-separated severities (default: Error,Failure)
  --max-depth       (optional) BFS depth limit (default: 8)
  --simulator       (optional) Simulator name for alias resolution (default: modelsim)
  --report-dir      (optional) Output directory for generated reports
  --report-format   (optional) json, markdown, html, or none (default: markdown)

Examples:
  analyze_run sim/run_summary.json
  analyze_run sim/run_summary.json --deps sim/deps.yaml --spec sim/design_spec.yaml --report-format html

Related: load_run_summary, load_assertion_log, trace_root_cause, batch_trace_root_cause"#.to_string(),

        "check_env" => r#"check_env

Diagnose the environment configuration required for extract_deps and BFS analysis.
Checks: embedded sidecar (pyverilog engine), external sidecar, iverilog, VC++ Runtime.

No parameters required.

Output example:
  [1/4] Embedded sidecar (pyverilog engine)
    Status : OK — released to %TEMP%\wave-analyzer-mcp\wave-analyzer-deps-extractor.exe
    Launch : OK — sidecar starts successfully
  [3/4] iverilog (Verilog preprocessor)
    Root   : D:\software\iverilog
    Exe    : D:\software\iverilog\bin\iverilog.exe — OK

Fix any FAIL items, then re-run check_env to confirm.

Related: extract_deps"#.to_string(),

        _ => format!("Unknown command '{}'. Run 'help' to see all available commands.", name),
    }
}
