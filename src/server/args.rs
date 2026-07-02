//! MCP tool argument structs and their default/deserialize helpers.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wave_analyzer_mcp::CompareSignalRef;
use wave_analyzer_mcp::SignalEntry as TimelineSignalEntry;
use wave_analyzer_mcp::extract::BitMappingEntry;

// --- CLI args (for the server binary) --- //

/// Command line arguments for the waveform MCP server
#[derive(Debug, clap::Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Run the server in HTTP mode instead of stdio
    #[arg(long)]
    pub http: bool,

    /// Bind address for HTTP server (default: 127.0.0.1:8000)
    #[arg(long, default_value = "127.0.0.1:8000")]
    pub bind_address: String,
}

// --- Tool argument structs --- //

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OpenWaveformArgs {
    pub file_path: String,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListSignalsArgs {
    pub waveform_id: String,
    #[serde(default)]
    pub name_pattern: Option<String>,
    #[serde(default)]
    pub hierarchy_prefix: Option<String>,
    #[serde(default = "default_recursive")]
    pub recursive: Option<bool>,
    #[serde(default = "default_list_signals_limit")]
    pub limit: Option<isize>,
}

fn default_recursive() -> Option<bool> {
    Some(true)
}

fn default_list_signals_limit() -> Option<isize> {
    Some(100)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadHierarchyArgs {
    pub waveform_id: String,
    #[serde(default)]
    pub scope_path: Option<String>,
    #[serde(default = "default_read_hierarchy_recursive")]
    pub recursive: Option<bool>,
    #[serde(default = "default_read_hierarchy_limit")]
    pub limit: Option<isize>,
}

fn default_read_hierarchy_recursive() -> Option<bool> {
    Some(false)
}

fn default_read_hierarchy_limit() -> Option<isize> {
    Some(200)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadSignalArgs {
    pub waveform_id: String,
    pub signal_path: String,
    #[serde(default = "default_time_index")]
    pub time_index: Option<usize>,
    #[serde(default)]
    pub time_indices: Option<Vec<usize>>,
}

fn default_time_index() -> Option<usize> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetSignalInfoArgs {
    pub waveform_id: String,
    pub signal_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FindSignalEventsArgs {
    pub waveform_id: String,
    pub signal_path: String,
    #[serde(default = "default_start_time")]
    pub start_time_index: Option<usize>,
    #[serde(default = "default_end_time")]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub start_time_value: Option<f64>,
    #[serde(default)]
    pub end_time_value: Option<f64>,
    #[serde(default)]
    pub time_unit: Option<String>,
    #[serde(default = "default_find_signal_events_limit")]
    pub limit: Option<isize>,
}

fn default_start_time() -> Option<usize> {
    None
}

fn default_end_time() -> Option<usize> {
    None
}

fn default_find_signal_events_limit() -> Option<isize> {
    Some(100)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FindConditionalEventsArgs {
    pub waveform_id: String,
    pub condition: String,
    #[serde(default = "default_start_time")]
    pub start_time_index: Option<usize>,
    #[serde(default = "default_end_time")]
    pub end_time_index: Option<usize>,
    #[serde(default = "default_find_conditional_events_limit")]
    pub limit: Option<isize>,
}

fn default_find_conditional_events_limit() -> Option<isize> {
    Some(100)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CloseWaveformArgs {
    pub waveform_id: String,
}

// --- Phase 3 tool args structs --- //

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadDependenciesArgs {
    pub file_path: String,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadAssertionLogArgs {
    pub file_path: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub severity_filter: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<isize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadDesignSpecArgs {
    pub file_path: String,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TraceRootCauseArgs {
    pub waveform_id: String,
    pub deps_id: String,
    pub signal_path: String,
    #[serde(default)]
    pub time_index: Option<usize>,
    #[serde(default)]
    pub time_value: Option<f64>,
    #[serde(default)]
    pub time_unit: Option<String>,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: Option<usize>,
    #[serde(default = "default_simulator")]
    pub simulator: Option<String>,
    /// Whether to penetrate CDC boundaries when synchronizer detected.
    #[serde(default)]
    pub penetrate_cdc: Option<bool>,
    /// Maximum depth within a penetrated CDC domain.
    #[serde(default)]
    pub cdc_max_depth: Option<usize>,
    /// Minimum synchronizer stages required for CDC penetration.
    #[serde(default)]
    pub cdc_min_sync_stages: Option<u32>,
}

fn default_max_depth() -> Option<usize> {
    Some(8)
}

fn default_simulator() -> Option<String> {
    Some("modelsim".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FindFanInArgs {
    pub deps_id: String,
    pub signal_path: String,
    #[serde(default = "default_simulator")]
    pub simulator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FindFanOutArgs {
    pub deps_id: String,
    pub signal_path: String,
    #[serde(default = "default_simulator")]
    pub simulator: Option<String>,
}

fn default_suggest_limit() -> Option<isize> {
    Some(10)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SuggestEntrySignalsArgs {
    pub waveform_id: String,
    pub deps_id: String,
    #[serde(default)]
    pub assertion_name: Option<String>,
    #[serde(default)]
    pub scope_path: Option<String>,
    #[serde(default = "default_suggest_limit")]
    pub limit: Option<isize>,
    #[serde(default = "default_simulator")]
    pub simulator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractSignalValuesArgs {
    pub waveform_id: String,
    #[serde(default)]
    pub signal_path: Option<String>,
    #[serde(default)]
    pub bit_mapping: Vec<BitMappingEntry>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub start_time_ps: Option<u64>,
    #[serde(default)]
    pub end_time_ps: Option<u64>,
    #[serde(default)]
    pub value_format: Option<String>,
    #[serde(default)]
    pub downsample: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeHandshakeArgs {
    pub waveform_id: String,
    pub valid_signal: String,
    pub ready_signal: String,
    #[serde(default)]
    pub data_signal: Option<String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub limit: Option<isize>,
    #[serde(default)]
    pub report_mode: Option<String>,
    #[serde(default)]
    pub filter_zero_delay: Option<bool>,
    #[serde(default)]
    pub level_sensitive: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MeasureSignalArgs {
    pub waveform_id: String,
    pub signal_path: String,
    pub analysis_type: String,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub edge_type: Option<String>,
    /// For interval mode: Verilog condition expression for the start event.
    #[serde(default)]
    pub from_condition: Option<String>,
    /// For interval mode: Verilog condition expression for the end event.
    #[serde(default)]
    pub to_condition: Option<String>,
    /// For interval mode: expected interval value (with expected_unit).
    #[serde(default)]
    pub expected_value: Option<f64>,
    /// For interval mode: unit for expected_value (ps, ns, us, ms, s).
    #[serde(default)]
    pub expected_unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompareSignalsArgs {
    pub waveform_id: String,
    pub signals: Vec<CompareSignalRef>,
    #[serde(default)]
    pub comparison_mode: Option<String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub limit: Option<isize>,
    #[serde(default)]
    pub value_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MultiSignalTimelineArgs {
    pub waveform_id: String,
    pub signals: Vec<TimelineSignalEntry>,
    #[serde(default)]
    pub merge_mode: Option<String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub limit: Option<isize>,
    #[serde(default)]
    pub value_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AutoDiscoverSignalsArgs {
    pub waveform_id: String,
    #[serde(default)]
    pub discovery_mode: Option<String>,
    #[serde(default)]
    pub scope_path: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
    /// Maximum number of bus groups, clock signals, and reset signals to return each.
    /// Use -1 for unlimited.
    #[serde(default)]
    pub limit: Option<isize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DetectSequenceArgs {
    pub waveform_id: String,
    pub sequence: Vec<String>,
    #[serde(default)]
    pub max_gap_cycles: Option<usize>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub limit: Option<isize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ComputeCrcArgs {
    pub waveform_id: String,
    pub data_signal_path: String,
    #[serde(default)]
    pub crc_signal_path: Option<String>,
    /// Optional data_valid signal path. When provided, only data values at
    /// time indices where this signal transitions 0→1 (posedge) are processed
    /// for CRC. This matches hardware behavior where CRC is only updated
    /// when data_valid pulses, filtering out reset/initialization values.
    #[serde(default)]
    pub data_valid_signal_path: Option<String>,
    /// Optional clear signal path. When provided, the computed CRC is reset
    /// to the init value whenever this signal is high (at a data_valid posedge
    /// or as a separate clear event). In RTL: `if (clear) crc <= INIT;` has
    /// priority over data_valid.
    #[serde(default)]
    pub clear_signal_path: Option<String>,
    /// Optional clock signal path. When provided (and data_valid is not),
    /// data is sampled at every clock posedge for per-cycle CRC computation.
    /// This correctly handles data held stable across multiple clock cycles.
    #[serde(default)]
    pub clock_signal_path: Option<String>,
    pub crc_polynomial: String,
    /// Initial CRC value. Accepts decimal (e.g. "65535") or hex (e.g. "0xFFFF").
    #[serde(default, deserialize_with = "deserialize_hex_or_decimal_u64")]
    pub initial_value: Option<u64>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub limit: Option<isize>,
}

/// Deserialize a string value as u64, supporting hex (0x...) and decimal formats.
/// Returns None for null/missing values.
pub fn deserialize_hex_or_decimal_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => {
            let lower = s.to_lowercase();
            if let Some(hex_digits) = lower.strip_prefix("0x") {
                let val = u64::from_str_radix(hex_digits, 16)
                    .map_err(|e| serde::de::Error::custom(format!("Invalid hex '{}': {}", s, e)))?;
                Ok(Some(val))
            } else {
                let val = s.parse::<u64>().map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Invalid value '{}': expected decimal or 0x... hex: {}",
                        s, e
                    ))
                })?;
                Ok(Some(val))
            }
        }
        Some(serde_json::Value::Number(n)) => {
            let val = n
                .as_u64()
                .ok_or_else(|| serde::de::Error::custom("initial_value number out of u64 range"))?;
            Ok(Some(val))
        }
        Some(other) => Err(serde::de::Error::custom(format!(
            "initial_value must be a string or number, got {}",
            other
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeSignalPatternsArgs {
    pub waveform_id: String,
    pub signals: Vec<String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
    #[serde(default)]
    pub max_bins: Option<usize>,
    #[serde(default)]
    pub idle_threshold: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractFsmArgs {
    pub waveform_id: String,
    pub signal_path: String,
    #[serde(default)]
    pub clock_signal: Option<String>,
    #[serde(default)]
    pub edge_type: Option<String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeProtocolArgs {
    pub waveform_id: String,
    pub protocol: String,
    pub signals: HashMap<String, String>,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzePhasedArrayArgs {
    pub waveform_id: String,
    pub channel_prefix: String,
    #[serde(default)]
    pub control_fsm_signal: Option<String>,
    #[serde(default)]
    pub coeff_signals: Option<Vec<String>>,
    pub clock_signal: String,
    #[serde(default)]
    pub start_time_index: Option<usize>,
    #[serde(default)]
    pub end_time_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportBfsReportArgs {
    /// Trace ID returned by trace_root_cause
    pub trace_id: String,
    /// Output format: "json", "markdown", or "html"
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BatchTraceRootCauseArgs {
    /// Waveform ID (from open_waveform)
    pub waveform_id: String,
    /// Dependency graph ID (from load_dependencies)
    pub deps_id: String,
    /// Assertion log ID (from load_assertion_log) - traces all events in this log
    pub assertion_id: String,
    /// Optional design spec ID for entry signal resolution
    #[serde(default)]
    pub spec_id: Option<String>,
    /// Optional max BFS depth per trace (default: 8)
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Optional severity filter: only trace events matching these severities (e.g., "Error,Failure")
    #[serde(default)]
    pub severity_filter: Option<String>,
    /// Optional simulator name for alias resolution (default: "modelsim")
    #[serde(default)]
    pub simulator: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadRunSummaryArgs {
    /// Path to run_summary.json file generated by simulation scripts
    pub file_path: String,
    /// Optional alias for this run summary (default: filename)
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtractDependenciesArgs {
    /// Path to RTL source directory or Verilog file
    pub rtl_path: String,
    /// Top module name for dependency extraction
    pub top_module: String,
    /// Extraction engine: "pyverilog" (default) or "vivado"
    #[serde(default)]
    pub engine: Option<String>,
    /// Path to annotations.yaml for manual overrides (CDC, blackbox, latency)
    #[serde(default)]
    pub annotations_path: Option<String>,
    /// Output deps.yaml path (default: deps.yaml next to rtl_path)
    #[serde(default)]
    pub output_path: Option<String>,
    /// Path to deps-extractor directory (default: auto-detect via DEPS_EXTRACTOR_PATH env var or relative path)
    #[serde(default)]
    pub deps_extractor_path: Option<String>,
    /// Whether to auto-load the generated deps.yaml into the dependency store (default: true)
    #[serde(default = "default_auto_load")]
    pub auto_load: Option<bool>,
}

fn default_auto_load() -> Option<bool> {
    Some(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeCdcArgs {
    /// Waveform ID (from open_waveform)
    pub waveform_id: String,
    /// Dependency graph ID (from load_dependencies). Optional - uses waveform-only heuristic if not provided.
    #[serde(default)]
    pub deps_id: Option<String>,
    /// Simulator identifier for alias resolution (default: "modelsim")
    #[serde(default)]
    pub simulator: Option<String>,
    /// Whether to verify synchronizer patterns in the waveform (default: true)
    #[serde(default)]
    pub verify_synchronizers: Option<bool>,
    /// Minimum synchronizer stages for "protected" classification (default: 2)
    #[serde(default)]
    pub min_sync_stages: Option<u32>,
}
