//! MCP server module — handler struct, tool router, and main entrypoint.

mod analysis_tools;
mod args;
mod cdc_tools;
mod protocol_tools;
mod report_tools;
mod waveform_tools;

pub use args::Args;

use clap::Parser;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt, handler::server::wrapper::Parameters,
    model::*, tool, tool_handler, tool_router, transport::stdio,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::prelude::*;

use wave_analyzer_mcp::assertion::AssertionParseResult;
use wave_analyzer_mcp::bfs::BfsResult;
use wave_analyzer_mcp::cdc::CdcAnalysisResult;
use wave_analyzer_mcp::deps::DepGraph;
use wave_analyzer_mcp::run_summary::RunSummary;
use wave_analyzer_mcp::spec::SpecLookup;
use wave_analyzer_mcp::summary::{ExportSvgRequest, WaveformSummaryRequest};

// Re-export args structs for tool router
pub use args::*;

// Waveform store - using RwLock for interior mutability
pub type WaveformStore = Arc<RwLock<HashMap<String, wellen::simple::Waveform>>>;
// Dependency graph store
pub type DepGraphStore = Arc<RwLock<HashMap<String, DepGraph>>>;
// Assertion log store
pub type AssertionStore = Arc<RwLock<HashMap<String, AssertionParseResult>>>;
// Design spec store
pub type SpecStore = Arc<RwLock<HashMap<String, SpecLookup>>>;
// BFS result store (for report export)
pub type BfsResultStore = Arc<RwLock<HashMap<String, BfsResult>>>;
// Run summary store
pub type RunSummaryStore = Arc<RwLock<HashMap<String, RunSummary>>>;
// CDC analysis result store
pub type CdcResultStore = Arc<RwLock<HashMap<String, CdcAnalysisResult>>>;

#[derive(Debug, Clone)]
pub struct WaveAnalyzerHandler {
    waveforms: WaveformStore,
    dep_graphs: DepGraphStore,
    assertions: AssertionStore,
    specs: SpecStore,
    bfs_results: BfsResultStore,
    run_summaries: RunSummaryStore,
    cdc_results: CdcResultStore,
}

impl Default for WaveAnalyzerHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl WaveAnalyzerHandler {
    pub fn new() -> Self {
        Self::with_all_stores(
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    pub fn with_store(waveforms: WaveformStore) -> Self {
        Self::with_all_stores(
            waveforms,
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
        )
    }

    pub fn with_all_stores(
        waveforms: WaveformStore,
        dep_graphs: DepGraphStore,
        assertions: AssertionStore,
        specs: SpecStore,
        bfs_results: BfsResultStore,
        run_summaries: RunSummaryStore,
        cdc_results: CdcResultStore,
    ) -> Self {
        Self {
            waveforms,
            dep_graphs,
            assertions,
            specs,
            bfs_results,
            run_summaries,
            cdc_results,
        }
    }

    // --- Waveform tools --- //

    #[tool(description = "Open a VCD or FST waveform file")]
    async fn open_waveform(
        &self,
        args: Parameters<OpenWaveformArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_open_waveform(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "List all signals in an open waveform. Use waveform_id from open_waveform. Optional: filter by name_pattern (regex pattern, e.g., '.*clk', '^TOP\\.rst'), hierarchy_prefix (e.g., 'top.module'), recursive (default: true), and limit."
    )]
    async fn list_signals(
        &self,
        args: Parameters<ListSignalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_list_signals(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Read the waveform module hierarchy as an indented tree. Only module scopes are returned. Use waveform_id from open_waveform. Optional: scope_path to start from a specific scope, recursive (default: false), and limit to cap the number of returned modules."
    )]
    async fn read_hierarchy(
        &self,
        args: Parameters<ReadHierarchyArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_read_hierarchy(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Read signal values from a waveform. Use waveform_id from open_waveform and signal_path from list_signals. Provide either time_index (single) or time_indices (array). For sophisticated usage like finding rising/falling edges, detecting signal transitions, or finding handshake cycles (valid && ready), use find_conditional_events instead."
    )]
    async fn read_signal(
        &self,
        args: Parameters<ReadSignalArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_read_signal(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Get metadata about a signal. Use waveform_id from open_waveform and signal_path from list_signals."
    )]
    async fn get_signal_info(
        &self,
        args: Parameters<GetSignalInfoArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_get_signal_info(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Find events (changes) of a signal within a time range. Use waveform_id from open_waveform and signal_path from list_signals. Optional: start_time_index/end_time_index OR start_time_value/end_time_value with time_unit (ps/ns/us/ms/s) for physical time input. limit."
    )]
    async fn find_signal_events(
        &self,
        args: Parameters<FindSignalEventsArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_find_signal_events(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Find events where a condition is satisfied. Supports signal paths, bitwise operators (~, &, |, ^), boolean operators (&&, ||, !), comparison operators (==, !=), $past(), bit extraction, and Verilog-style literals. Bitwise operators: ~ (NOT), & (AND), | (OR), ^ (XOR). Bit extraction: signal[bit] or signal[msb:lsb]. $past(signal) reads the signal value from the previous time index. Operator precedence: ~, ! (highest), ==, !=, &, ^, |, &&, || (lowest). Examples: rising edge '!$past(TOP.signal) && TOP.signal', falling edge '$past(TOP.signal) && !TOP.signal', handshake cycles 'TOP.valid && TOP.ready', check bit 'TOP.flags & 4'b0001', bit extract 'TOP.data[7:0] == 8'hFF'. Optional: start_time_index, end_time_index, limit."
    )]
    async fn find_conditional_events(
        &self,
        args: Parameters<FindConditionalEventsArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_find_conditional_events(&self.waveforms, &args.0).await
    }

    #[tool(description = "Close a waveform and free its memory")]
    async fn close_waveform(
        &self,
        args: Parameters<CloseWaveformArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_close_waveform(&self.waveforms, &args.0).await
    }

    // --- Load data tools --- //

    #[tool(
        description = "Load a deps.yaml dependency graph file. Builds fan-in/fan-out indices and signal/clock alias mappings for BFS tracing. Returns metadata including node/edge counts and alias counts."
    )]
    async fn load_dependencies(
        &self,
        args: Parameters<LoadDependenciesArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_load_dependencies(&self.dep_graphs, &args.0).await
    }

    #[tool(
        description = "Parse a ModelSim transcript file for assertion/check failure events. Supports standard two-line format (vsim-10142/10143), short single-line format, and note format. Optional: severity_filter (list of 'Error', 'Warning', 'Note', 'Failure' to include; empty = all), limit (max events, -1 = unlimited). Returns parsed failure event count, unmatched line count, and top events."
    )]
    async fn load_assertion_log(
        &self,
        args: Parameters<LoadAssertionLogArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_load_assertion_log(&self.assertions, &args.0).await
    }

    #[tool(
        description = "Load a design_spec.yaml file (OPTIONAL). Maps assertions to entry signals and provides debug hints/stop signals. \
        If you don't have a design_spec.yaml, use suggest_entry_signals to infer entry signals from waveform + deps. \
        Returns assertion count, behavior count, and debug hint availability."
    )]
    async fn load_design_spec(
        &self,
        args: Parameters<LoadDesignSpecArgs>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_load_design_spec(&self.specs, &args.0).await
    }

    // --- Analysis tools --- //

    #[tool(
        description = "BFS root-cause tracing from a failing signal back through the dependency graph. \
        Requires waveform_id, deps_id, and entry signal_path (use suggest_entry_signals if you don't know the entry signal). \
        Specify the failure time via time_index or time_value+time_unit (e.g., time_value=30, time_unit='ns' — converted to time_index automatically). \
        Optional: spec_id for debug hints and stop signals, max_depth (default 8), simulator (default 'modelsim' for alias resolution). \
        Returns trace tree and root cause candidates."
    )]
    async fn trace_root_cause(
        &self,
        args: Parameters<TraceRootCauseArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_trace_root_cause(self, &args.0).await
    }

    #[tool(
        description = "Query fan-in (upstream dependency) edges for a signal in a loaded dependency graph. Returns each upstream signal with its dependency type, clock, latency, and protocol/boundary info. Optional: simulator (default 'modelsim') to resolve signal aliases."
    )]
    async fn find_fan_in(
        &self,
        args: Parameters<FindFanInArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_find_fan_in(&self.dep_graphs, &args.0).await
    }

    #[tool(
        description = "Query fan-out (downstream dependent) signals for a signal in a loaded dependency graph. Returns list of output signals that depend on this signal, with their dependency type, category, and alias info. Useful for forward impact analysis: 'which outputs are affected if this signal changes'. Optional: simulator (default 'modelsim') to resolve signal aliases."
    )]
    async fn find_fan_out(
        &self,
        args: Parameters<FindFanOutArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_find_fan_out(&self.dep_graphs, &args.0).await
    }

    #[tool(
        description = "Suggest candidate entry signals for BFS root-cause tracing when no design_spec.yaml is available. \
        Uses waveform hierarchy and dependency graph to rank signals by traceability. \
        Requires waveform_id and deps_id. Optional: assertion_name (for substring matching against signal names), \
        scope_path (from assertion event, limits search scope), limit (default 10). \
        Tier 1 = deps output nodes (most traceable), Tier 2 = deps boundary nodes, Tier 3 = signals not in deps. \
        Signals matching assertion name tokens are prioritized within each tier."
    )]
    async fn suggest_entry_signals(
        &self,
        args: Parameters<SuggestEntrySignalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_suggest_entry_signals(self, &args.0).await
    }

    #[tool(
        description = "Extract signal values from a waveform in a specified time range. \
        Supports two modes:\n\
        1. Single signal: provide signal_path to extract all value changes of that signal.\n\
        2. Multi-bit reconstruction: provide bit_mapping (list of {bit_position, signal_path}) to reconstruct a composite signal from individual bit signals.\n\
        Time range: use start_time_ps/end_time_ps (picoseconds) for intuitive time-based queries, or start_time_index/end_time_index for index-based queries. Ps values take precedence.\n\
        Optional: value_format ('hex'/'binary'/'decimal', default 'hex'), downsample (max points to return)."
    )]
    async fn extract_signal_values(
        &self,
        args: Parameters<ExtractSignalValuesArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_extract_signal_values(&self.waveforms, &args.0).await
    }

    // --- Protocol analysis tools --- //

    #[tool(
        description = "Detect valid/ready handshake transactions in a waveform. Analyzes the timing relationship between a valid signal (assertion) and a ready signal (acknowledgment) to identify complete handshake events. Reports each handshake with latency from valid assertion to transfer completion. Optional: data_signal to capture data value at each transfer, start_time_index/end_time_index for time range, limit (max events, -1=unlimited), report_mode ('summary' or 'detail')."
    )]
    async fn analyze_handshake(
        &self,
        args: Parameters<AnalyzeHandshakeArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_analyze_handshake(&self.waveforms, &args.0).await
    }

    #[tool(description = "Measure signal properties. Three modes:\n\
        1. 'clock' mode: Measure clock period, frequency, duty cycle, and jitter from a 1-bit clock signal. Optional: edge_type ('posedge' or 'negedge', default 'posedge').\n\
        2. 'pulse' mode: Measure high and low pulse widths on an arbitrary signal. Returns statistics in physical time units (ns/ps).\n\
        3. 'interval' mode: Measure time intervals between two condition events. Requires from_condition and to_condition (Verilog expressions). Optional: expected_value + expected_unit for deviation calculation.\n\
        Requires waveform_id, signal_path, and analysis_type ('clock', 'pulse', or 'interval'). Optional: start_time_index, end_time_index.")]
    async fn measure_signal(
        &self,
        args: Parameters<MeasureSignalArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_measure_signal(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Compare two or more signals over a time range and report mismatches. \
        Each signal can be a direct signal path or a reconstructed multi-bit signal (using bit_mapping). \
        Modes: 'all_equal' (all signals must match) or 'reference_vs_actual' (first signal is reference). \
        Optional: start_time_index, end_time_index, limit (max mismatches, -1=unlimited), value_format ('hex'/'binary'/'decimal')."
    )]
    async fn compare_signals(
        &self,
        args: Parameters<CompareSignalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_compare_signals(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Build a unified timeline of multiple signals, showing all signal values at each change point. \
        Similar to a logic analyzer view. Each signal can be a direct path or reconstructed from bit_mapping. \
        Merge modes: 'union' (any signal change triggers a row) or 'intersection' (all signals must change simultaneously). \
        Optional: start_time_index, end_time_index, limit, value_format."
    )]
    async fn multi_signal_timeline(
        &self,
        args: Parameters<MultiSignalTimelineArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_multi_signal_timeline(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Auto-discover signal patterns from waveform hierarchy. \
        Discovers bus slices (e.g., crc[0]..crc[15] or data_0..data_7), clock signals (by regular edge behavior), and reset signals. \
        Modes: 'bus_slices' (find grouped bit signals), 'clocks' (detect clock signals), 'groups' (all), 'all' (all). \
        Optional: scope_path (limit search to a scope), pattern (regex filter on signal names), limit (max results per category, -1=unlimited)."
    )]
    async fn auto_discover_signals(
        &self,
        args: Parameters<AutoDiscoverSignalsArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_auto_discover_signals(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Detect a sequence of conditions in a waveform. Each condition is evaluated at every time index. Finds occurrences where all conditions are met in order, with optional max_gap_cycles constraint between consecutive steps. Returns occurrence start/end times and timing gaps. Use for FSM state transition detection, protocol preamble search, or multi-step event pattern matching."
    )]
    async fn detect_sequence(
        &self,
        args: Parameters<DetectSequenceArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_detect_sequence(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Compute CRC over a data bus signal and optionally verify against an observed CRC signal. Supports CRC-8, CRC-16-CCITT, and CRC-32-Ethernet. Incrementally computes CRC at each data change point. Use for validating CRC implementations against waveform dumps, or computing expected CRC for comparison with DUT output."
    )]
    async fn compute_crc(
        &self,
        args: Parameters<ComputeCrcArgs>,
    ) -> Result<CallToolResult, McpError> {
        protocol_tools::handle_compute_crc(&self.waveforms, &args.0).await
    }

    // --- CDC / Pattern / FSM / Protocol / Phased Array tools --- //

    #[tool(
        description = "Analyze data patterns of one or more signals. Computes value distribution histogram (distinct values and their frequency), change frequency statistics (change rate, gap statistics, longest stable period), and idle/active cycle analysis (active/idle durations and fractions). For 1-bit signals, idle=0/active=1. For multi-bit, idle=value matching idle_threshold (default 0). Useful for understanding signal behavior, detecting stuck-at faults, and quantifying bus activity."
    )]
    async fn analyze_signal_patterns(
        &self,
        args: Parameters<AnalyzeSignalPatternsArgs>,
    ) -> Result<CallToolResult, McpError> {
        cdc_tools::handle_analyze_signal_patterns(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Extract FSM state encoding and transition graph from a state register signal. \
        Infers FSM states by value clustering on the signal, then discovers transitions by tracking consecutive state changes. \
        When a clock_signal is provided, observations are aligned to clock edges (posedge/negedge) for accurate synchronous FSM extraction. \
        Returns discovered states (with occurrence counts and fractions), transitions (with counts and duration statistics), self-loops, \
        and a DOT-format state transition graph."
    )]
    async fn extract_fsm(
        &self,
        args: Parameters<ExtractFsmArgs>,
    ) -> Result<CallToolResult, McpError> {
        cdc_tools::handle_extract_fsm(&self.waveforms, &args.0).await
    }

    #[tool(description = "Analyze waveform using a standard protocol template. \
        Supported protocols: spi, uart, i2c, axi_lite. \
        Provide a signal mapping that assigns protocol role names to waveform signal paths. \
        SPI: sclk, cs (required), mosi/miso (optional). \
        UART: tx or rx (required). \
        I2C: scl, sda (required). \
        AXI-Lite: arvalid, arready, awvalid, awready (required), rdata/wdata (optional). \
        Returns protocol-specific measurements and statistics.")]
    async fn analyze_protocol(
        &self,
        args: Parameters<AnalyzeProtocolArgs>,
    ) -> Result<CallToolResult, McpError> {
        cdc_tools::handle_analyze_protocol(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Analyze a phased array (multi-channel) design waveform. \
        Discovers channels from naming patterns (e.g., ch0, ch1), extracts FSM patterns from control logic, \
        traces coefficient loading chains, and checks cross-channel consistency. \
        Requires waveform_id, channel_prefix (e.g., 'ch' or 'channel'), and clock_signal. \
        Optional: control_fsm_signal for FSM extraction, coeff_signals for coefficient chain tracing."
    )]
    async fn analyze_phased_array(
        &self,
        args: Parameters<AnalyzePhasedArrayArgs>,
    ) -> Result<CallToolResult, McpError> {
        cdc_tools::handle_analyze_phased_array(&self.waveforms, &args.0).await
    }

    #[tool(description = "Analyze clock domain crossings (CDC) in a waveform. \
        Identifies clock domains from deps.yaml and waveform, detects CDC crossing points, \
        verifies synchronizer patterns (2-FF chains), and reports unprotected crossings. \
        Requires waveform_id. Optional deps_id for structured CDC metadata from annotations.yaml.")]
    async fn analyze_cdc(
        &self,
        args: Parameters<AnalyzeCdcArgs>,
    ) -> Result<CallToolResult, McpError> {
        cdc_tools::handle_analyze_cdc(self, &args.0).await
    }

    // --- Report / Export tools --- //

    #[tool(
        description = "Batch BFS root-cause tracing for all failure events in a loaded assertion/check log. \
        For each failure event, resolves the entry signal (via spec or deps), runs trace_root_cause, and aggregates root cause candidates across all traces. \
        Requires waveform_id, deps_id, and assertion_id (from load_assertion_log). Optional: spec_id for debug hints, max_depth (default 8), severity_filter (e.g., 'Error,Failure'), simulator (default 'modelsim')."
    )]
    async fn batch_trace_root_cause(
        &self,
        args: Parameters<BatchTraceRootCauseArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_batch_trace_root_cause(self, &args.0).await
    }

    #[tool(
        description = "Export a BFS trace result as a formatted report. Provide the trace_id returned by trace_root_cause and choose format: 'json' (structured data), 'markdown' (table-based report), or 'html' (dark-themed web page). Use this for generating shareable analysis reports from BFS traces."
    )]
    async fn export_bfs_report(
        &self,
        args: Parameters<ExportBfsReportArgs>,
    ) -> Result<CallToolResult, McpError> {
        report_tools::handle_export_bfs_report(&self.bfs_results, &args.0).await
    }

    #[tool(
        description = "Generate a summary of a waveform file for preview display. Opens the file, samples specified signals with downsampling, and returns metadata suitable for rendering a thumbnail waveform. Use this for Chat preview cards that show a quick glimpse of simulation results. Optional: signals (auto-detect top signals if empty), max_samples (default 100)."
    )]
    async fn get_waveform_summary(
        &self,
        args: Parameters<WaveformSummaryRequest>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_get_waveform_summary(&args.0).await
    }

    #[tool(
        description = "Export waveform signals to SVG format for sharing or printing. Returns SVG content as a string that can be rendered in a browser or image viewer. Optional: time_range (start, end in time indices), width (default 800), height (default 600)."
    )]
    async fn export_waveform_svg(
        &self,
        args: Parameters<ExportSvgRequest>,
    ) -> Result<CallToolResult, McpError> {
        waveform_tools::handle_export_waveform_svg(&self.waveforms, &args.0).await
    }

    #[tool(
        description = "Load and parse a run_summary.json file generated by simulation scripts. \
        Reports simulation status (passed/compile_failed/elab_failed/simulation_failed/assertion_failed), \
        compile/elaboration/simulation success, error/warning/assertion counts, wave file path, and transcript path. \
        Handles PowerShell ConvertTo-Json boolean fields (outputs 'true'/'false' as strings) as well as proper JSON booleans. \
        Returns a suggested next step based on status. Optional: alias (default: filename)."
    )]
    async fn load_run_summary(
        &self,
        args: Parameters<LoadRunSummaryArgs>,
    ) -> Result<CallToolResult, McpError> {
        report_tools::handle_load_run_summary(&self.run_summaries, &args.0).await
    }

    #[tool(
        description = "Extract a deps.yaml dependency graph from RTL source files using the deps-extractor pipeline. \
        Runs Pyverilog (or Vivado) extraction to generate deps_raw.json, then deps_converter.py to produce deps.yaml. \
        By default, auto-loads the generated deps.yaml into the dependency store. \
        Requires: rtl_path (directory or file), top_module name. \
        Optional: engine ('pyverilog' default, 'vivado' requires Vivado install), annotations_path, output_path, deps_extractor_path, auto_load."
    )]
    async fn extract_dependencies(
        &self,
        args: Parameters<ExtractDependenciesArgs>,
    ) -> Result<CallToolResult, McpError> {
        analysis_tools::handle_extract_dependencies(self, &args.0).await
    }
}

#[tool_handler]
impl ServerHandler for WaveAnalyzerHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2025_06_18)
            .with_server_info(Implementation::from_build_env())
            .with_instructions(
                    "MCP server for VCD/FST waveform analysis and RTL root-cause tracing.\n\n\
                     ## Input Files\n\
                     Before starting, prepare these files for your project:\n\
                     1. deps.yaml (REQUIRED) — signal dependency graph with clock/latency semantics and signal/clock aliases\n\
                     You can generate deps.yaml from RTL source files using the extract_dependencies tool.\n\
                     2. transcript.log (REQUIRED for assertion workflow) — ModelSim assertion log (vsim-10142/10143 format)\n\
                     3. dump.vcd or dump.fst (REQUIRED) — simulation waveform\n\
                     4. design_spec.yaml (OPTIONAL) — requirements, assertions, BFS entry signals and stop signals\n\
                     If available, use load_design_spec and find entry signals from assertions[].observe_signals or behaviors[].fail_entry_signals.\n\
                     If NOT available, use suggest_entry_signals tool to infer candidate entry signals from waveform hierarchy + deps graph.\n\
                     For unstructured requirements documents (PDF, Word, text), read them yourself and identify relevant signals manually.\n\n\
                     ## deps.yaml Generation\n\
                     If you don't have a deps.yaml file, use extract_dependencies to generate one from RTL source files:\n\
                     extract_dependencies(rtl_path='path/to/rtl/dir', top_module='my_module')\n\
                     This runs the deps-extractor pipeline (Pyverilog by default) and auto-loads the result.\n\
                     You can also provide annotations.yaml for CDC boundaries, blackbox modules, and latency overrides.\n\n\
                     ## Workflow (7 Steps)\n\
                     Step 0: Determine simulation status from run_summary.json. \
                     If status=compile_failed or elab_failed, fix build issues first — do NOT enter BFS. \
                     Only proceed when status=assertion_failed.\n\
                     Step 1: open_waveform — load the VCD/FST file. Use list_signals to verify key signals exist.\n\
                     Step 2: load_assertion_log — parse transcript.log. \
                     Returns assertion name, severity, time, scope, source location. \
                     This tells you WHICH assertion failed and WHEN.\n\
                     Step 3: Determine BFS entry signal.\n\
                     Option A: If design_spec.yaml is available, load it via load_design_spec. \
                     Find the assertion name from Step 2 in assertions[].observe_signals or behaviors[].fail_entry_signals. \
                     Use the FIRST signal in the list as primary entry.\n\
                     Option B: If NO design_spec.yaml, use suggest_entry_signals with waveform_id, deps_id, \
                     and optionally assertion_name + scope_path from Step 2. \
                     This returns ranked candidates. Use the top Tier-1 signal as BFS entry.\n\
                     Option C: If you have unstructured requirements documents, read them yourself, \
                     identify relevant signals, then verify with list_signals and find_fan_in.\n\
                     Step 4: load_dependencies + find_fan_in — load deps.yaml and verify the entry signal's upstream chain. \
                     Confirm clock_aliases resolve correctly (e.g., clk -> TOP.clk) and latency_cycles match RTL.\n\
                     Step 5: read_signal — confirm the entry signal value at the failure time \
                     (use time_value+time_unit from Step 2, e.g., time_value=30, time_unit='ns').\n\
                     Step 6: trace_root_cause — BFS from entry signal at failure time. \
                     Requires waveform_id, deps_id, signal_path, and time (time_index or time_value+time_unit). \
                     Optional: spec_id for debug_hints stop_signals.\n\
                     Step 7: Analyze BFS result — RootCauseCandidate = high-priority root cause, \
                     Suspect = needs further checking, Ok = passed expected check, \
                     Boundary = debug boundary (input port, CDC, blackbox — stop expanding), \
                     Stopped = reached stop_signal or max_depth.\n\n\
                     ## Working Without design_spec.yaml\n\
                     When no structured spec is available:\n\
                     - Use suggest_entry_signals to get ranked candidate entry signals based on waveform hierarchy + deps graph.\n\
                     - The tool uses assertion event scope_path to narrow the search scope, and assertion_name tokens for substring matching.\n\
                     - Tier 1 candidates (deps output nodes) are most traceable — BFS can expand their fan-in chain.\n\
                     - Tier 2 candidates (deps boundary nodes) are leaf signals with no upstream in deps.\n\
                     - Tier 3 candidates (waveform-only signals not in deps) require adding deps entries before BFS can trace them.\n\
                     - For unstructured requirements: read the doc, identify signal names, verify with list_signals and find_fan_in.\n\n\
                     ## deps.yaml Incremental Strategy\n\
                     Do NOT try to model every signal in a large RTL design (100K+ lines). \
                     Build deps incrementally along FAILURE PATHS only.\n\
                     V1 (5-10 signals): Start with only the failing output signal as a boundary node. \
                     Run BFS — it stops at the boundary. Then expand that boundary into its direct upstream (1 register stage + control enable). \
                     V2 (10-20 signals): Add direct upstream register stages and control signals for the failure path. \
                     Unresolved signals remain as boundary nodes for later expansion.\n\
                     V3 (20-50 signals): Add BRAM read paths (type=memory, latency_cycles=2), FSM self-loops, pipeline intermediate stages.\n\
                     V4 (50-100 signals): Add generate channel aliases (signal_aliases: canonical -> modelsim path), CDC boundaries (type=boundary, boundary_kind=cdc).\n\
                     Each assertion failure only adds 5-10 signals. The deps graph grows organically with debugging experience, not upfront.\n\
                     Typical patterns: sequential register (type=sequential, latency_cycles=1), \
                     control gate (type=control, check='>0'), BRAM read (type=memory, latency_cycles=2), \
                     FSM self-loop (type=sequential on same signal), \
                     input port boundary (type=boundary, boundary_kind=input_port), \
                     CDC stop (type=boundary, boundary_kind=cdc).\n\n\
                     ## Key Constraints\n\
                     - trace_root_cause accepts ONE entry signal per call. \
                     If observe_signals has multiple entries, trace primary first, then retry others.\n\
                     - deps.yaml clock field uses logical names (clk_sys), resolved via clock_aliases to waveform paths (TOP.clk_sys).\n\
                     - latency_cycles is clock-period count, NOT time_index delta. BFS uses clock edge tables internally.\n\
                     - boundary type (input_port, cdc, blackbox) stops automatic BFS expansion — expand manually by adding more deps entries.\n\
                     - Self-loop dependencies (counter/FSM) are normal; BFS has cycle detection.\n\
                     - When BFS stops at a boundary, that is the expected workflow: review the boundary signal value, \
                     then decide whether to add its upstream deps for a deeper trace.\n\n\
                     ## Waveform-Only Tools\n\
                     open_waveform, close_waveform, list_signals, read_hierarchy, read_signal, \
                     get_signal_info, find_signal_events, find_conditional_events — \
                     usable independently for waveform exploration without deps/spec.\n\n\
                     ## Phase 3 Tools\n\
                     load_dependencies, load_assertion_log, load_design_spec, trace_root_cause, find_fan_in, suggest_entry_signals — \
                     require structured input files for root-cause analysis workflow.\n\n\
                     ## Protocol Analysis Tools\n\
                     analyze_handshake — detect valid/ready handshake transactions with latency analysis.\n\
                     measure_signal — measure clock properties (period, frequency, duty cycle, jitter) or pulse widths."
                )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".to_string().into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    if args.http {
        // HTTP mode
        let ct = CancellationToken::new();

        // Create shared stores for all HTTP sessions
        let shared_waveforms: WaveformStore = Arc::new(RwLock::new(HashMap::new()));
        let shared_dep_graphs: DepGraphStore = Arc::new(RwLock::new(HashMap::new()));
        let shared_assertions: AssertionStore = Arc::new(RwLock::new(HashMap::new()));
        let shared_specs: SpecStore = Arc::new(RwLock::new(HashMap::new()));
        let shared_bfs_results: BfsResultStore = Arc::new(RwLock::new(HashMap::new()));
        let shared_run_summaries: RunSummaryStore = Arc::new(RwLock::new(HashMap::new()));

        let service = StreamableHttpService::new(
            move || {
                Ok(WaveAnalyzerHandler::with_all_stores(
                    shared_waveforms.clone(),
                    shared_dep_graphs.clone(),
                    shared_assertions.clone(),
                    shared_specs.clone(),
                    shared_bfs_results.clone(),
                    shared_run_summaries.clone(),
                    Arc::new(RwLock::new(HashMap::new())),
                ))
            },
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
        );

        let router = axum::Router::new().nest_service("/mcp", service);
        let tcp_listener = tokio::net::TcpListener::bind(&args.bind_address).await?;
        tracing::info!("HTTP server listening on {}", args.bind_address);

        let _ = axum::serve(tcp_listener, router)
            .with_graceful_shutdown(async move {
                tokio::signal::ctrl_c().await.unwrap();
                tracing::info!("Shutting down...");
                ct.cancel();
            })
            .await;
    } else {
        // stdio mode (default)
        let handler = WaveAnalyzerHandler::new();

        let service = handler.serve(stdio()).await.inspect_err(|e| {
            tracing::error!("Serving error: {:?}", e);
        })?;

        tracing::info!("Server running in stdio mode");

        service.waiting().await?;
    }

    Ok(())
}
