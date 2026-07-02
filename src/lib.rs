//! Waveform MCP Server Library
//!
//! This library provides utilities for working with waveform files,
//! including dependency graph loading, assertion log parsing,
//! design spec lookup, time mapping, and BFS root-cause tracing.

pub mod error;

pub mod analysis_run;
pub mod cdc;
pub mod cli_parser;
pub mod compare;
pub mod condition;
pub mod crc;
pub mod deps_extractor;
pub mod discovery;
pub mod extract;
pub mod formatting;
pub mod hierarchy;
pub mod protocol;
pub mod report;
pub mod run_summary;
pub mod sequence;
pub mod signal;
pub mod summary;

pub mod assertion;
pub mod bfs;
pub mod deps;
pub mod entry_signal;
pub mod fsm;
pub mod pattern;
pub mod phased_array;
pub mod protocol_template;
pub mod spec;
pub mod time_map;

// Re-export public functions
pub use cli_parser::{CliOptions, Command, parse_args};
pub use condition::find_conditional_events;
pub use error::{WaveAnalyzerError, WaveResult};
pub use formatting::{
    ReportWriter, format_biguint_value, format_biguint_verilog, format_signal_value, format_time,
    packed_bytes_to_biguint, parse_verilog_literal, signal_value_to_biguint_lenient,
    signal_value_to_biguint_strict,
};
pub use hierarchy::find_scope_by_path;
pub use hierarchy::find_signal_by_path;
pub use hierarchy::get_signal_width;
pub use hierarchy::read_hierarchy;
pub use hierarchy::resolve_signal_with_width;
pub use signal::find_signal_events;
pub use signal::find_signal_events_by_path;
pub use signal::get_signal_metadata;
pub use signal::list_signals;
pub use signal::read_signal_values;
pub use signal::read_signal_values_by_path;

// Re-export new modules
pub use analysis_run::{
    AnalyzeRunRequest, AnalyzeRunResult, AnalyzeRunTrace, EntryResolution, ReportOutput,
    SelectedEvent, analyze_run,
};
pub use assertion::{AssertionEvent, parse_assertion_log};
pub use bfs::{
    AggregatedCandidate, BatchBfsResult, BatchTraceEntry, BfsNode, BfsResult, NodeStatus,
    RootCauseCandidate, aggregate_candidates_from_results, batch_trace_root_cause,
    load_bfs_result_from_cache, persist_bfs_result, trace_root_cause,
};
pub use cdc::{
    CdcAnalysisResult, CdcCrossing, CdcSummary, ClockDomain, ClockDomainInfo, SynchronizerInfo,
    analyze_cdc, analyze_cdc_waveform_only, detect_synchronizer, find_cdc_crossings,
    format_cdc_report, identify_clock_domains, identify_clock_domains_from_waveform,
    map_time_cross_domain,
};
pub use compare::{
    CompareResult, MismatchEvent, SignalRef as CompareSignalRef, compare_signals_values,
    format_compare_report,
};
pub use condition::{Condition, evaluate_condition, extract_signal_names, parse_condition};
pub use crc::{
    CrcDataPoint, CrcPolynomial, CrcResult, compute_and_verify_crc, format_crc_report,
    parse_crc_polynomial,
};
pub use deps::{DepGraph, load_deps_from_file};
pub use deps_extractor::{DepsExtractorResult, run_deps_extractor};
pub use discovery::{
    BusGroup, DiscoveryResult, SignalInfo, auto_discover_signals, format_discovery_report,
};
pub use entry_signal::{
    EntrySignalCandidate, extract_assertion_tokens, signal_matches_assertion, suggest_entry_signals,
};
pub use extract::{
    BitMappingEntry, ExtractRequest, ExtractResult, ExtractedPoint, extract_signal_values,
};
pub use fsm::{FsmExtractionResult, FsmState, FsmTransition, extract_fsm, format_fsm_report};
pub use pattern::{
    ChangeFrequency, HistogramBin, IdleActiveStats, PatternAnalysisResult, ValueDistribution,
    analyze_signal_patterns, format_pattern_report,
};
pub use phased_array::{
    CdcBoundarySummary, CoefficientChainResult, PhasedArrayAnalysisResult, PhasedArrayChannel,
    analyze_phased_array, format_phased_array_report,
};
pub use protocol::{
    ClockMeasurement, HandshakeEvent, HandshakeReport, HandshakeSummary, IntervalEvent,
    IntervalMeasurement, MeasurementStats, PulseMeasurement, analyze_handshake,
    analyze_handshake_with_level_sensitive, compute_stats, format_clock_report,
    format_handshake_report, format_interval_report, format_pulse_report, measure_clock,
    measure_intervals, measure_pulses,
};
pub use protocol_template::{
    AxiLiteAnalysisResult, I2cAnalysisResult, ProtocolAnalysisResult, ProtocolTemplate,
    SpiAnalysisResult, SpiTransaction, UartAnalysisResult, analyze_protocol_template,
    format_protocol_template_report,
};
pub use report::{
    BatchBfsReport, BfsTraceEntry, ReportFormat, format_batch_bfs_report_html,
    format_batch_bfs_report_markdown, format_bfs_report_html, format_bfs_report_json,
    format_bfs_report_markdown,
};
pub use run_summary::{RunSummary, parse_run_summary_from_file};
pub use sequence::{SequenceOccurrence, SequenceResult, detect_sequence, format_sequence_report};
pub use spec::{SpecLookup, load_spec_from_file};
pub use summary::{
    ExportSvgRequest, ExportSvgResponse, WaveformSummary, WaveformSummaryRequest,
    export_waveform_to_svg, generate_waveform_summary,
};
pub use summary::{
    SignalEntry, TimelineResult, TimelineRow, build_multi_signal_timeline, format_timeline_report,
};
pub use time_map::{
    ClockEdgeEntry, ClockEdgeTable, ClockEdgeType, build_clock_edge_table,
    compute_time_ps_from_table, compute_time_ps_from_table_checked, find_time_index_by_value,
};
