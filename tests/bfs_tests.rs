//! Tests for BFS root-cause tracing engine.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::assertion::{AssertionEvent, Severity, TimeUnit};
use wave_analyzer_mcp::bfs::{BfsOptions, NodeStatus, batch_trace_root_cause, trace_root_cause};
use wave_analyzer_mcp::deps::load_deps_from_file;

// Simple VCD: combinational chain data_in -> data_out
// clk toggles at 0,10,20,30,40
// data_in: changes at #5 (1), #15 (0), #25 (1)
// data_out: changes at #5 (1), #15 (0), #25 (1) (instant combinational)
// enable: stays at 1
const SIMPLE_COMB_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 1 1 data_in $end\n\
$var wire 1 2 data_out $end\n\
$var wire 1 3 enable $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
01\n\
12\n\
13\n\
#5\n\
11\n\
12\n\
#10\n\
10\n\
#15\n\
01\n\
02\n\
#20\n\
00\n\
#25\n\
11\n\
12\n\
#30\n\
10\n\
#40\n\
00";

// Deps YAML matching the simple VCD
const SIMPLE_COMB_DEPS_YAML: &str = r#"
format_version: "1.0"
description: "simple combinational test"

signal_aliases:
  - canonical: "top.data_in"
    modelsim: "top.data_in"
  - canonical: "top.data_out"
    modelsim: "top.data_out"
  - canonical: "top.enable"
    modelsim: "top.enable"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.data_out"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: combinational
        clock: null
        edge: null
        latency_cycles: 0
        check: "="
      - signal: "top.enable"
        type: control
        clock: "clk"
        edge: posedge
        latency_cycles: 0
        check: ">0"

  - output: "top.enable"
    category: control
    depends_on:
      - signal: "top.enable"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null

  - output: "top.data_in"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;

// Sequential VCD: data_in registered into data_out
const SEQ_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 1 1 data_in $end\n\
$var wire 1 2 data_out $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
01\n\
02\n\
#5\n\
11\n\
#10\n\
10\n\
12\n\
#20\n\
00\n\
#25\n\
01\n\
#30\n\
10\n\
02\n\
#40\n\
00";

// Sequential deps: data_out = data_in with 1-cycle latency on posedge clk
const SEQ_DEPS_YAML: &str = r#"
format_version: "1.0"
description: "sequential register test"

signal_aliases:
  - canonical: "top.data_in"
    modelsim: "top.data_in"
  - canonical: "top.data_out"
    modelsim: "top.data_out"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.data_out"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="

  - output: "top.data_in"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;

fn write_vcd_temp(content: &str) -> NamedTempFile {
    let temp_file = NamedTempFile::new().expect("create temp file");
    std::fs::write(temp_file.path(), content).expect("write VCD content");
    temp_file
}

fn write_yaml_temp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(content.as_bytes()).expect("write yaml");
    f
}

#[test]
fn test_trace_combinational() {
    // Trace from data_out at time_index where data_out changes
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    // Trace from data_out
    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 2, &options)
        .expect("trace should succeed");

    // Root node should exist
    assert!(!result.tree.is_empty());
    assert_eq!(result.tree[0].signal_path, "top.data_out");
    assert_eq!(result.tree[0].time_index, 2);

    // Summary should be generated
    assert!(!result.summary.is_empty());
}

#[test]
fn test_trace_combinational_at_different_time() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    // At time_index 5 (time #15), data_out=0, data_in=0
    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 5, &options)
        .expect("trace should succeed");

    assert!(!result.tree.is_empty());
}

#[test]
fn test_trace_boundary_signal() {
    // Trace from a boundary signal — should be traced and found
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_in", 2, &options)
        .expect("trace should succeed");

    assert!(!result.tree.is_empty());
    // data_in has only boundary dependency on itself
}

#[test]
fn test_trace_signal_not_found() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.nonexistent", 2, &options);
    assert!(result.is_err());
}

#[test]
fn test_trace_max_depth_truncation() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 1,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 2, &options)
        .expect("trace should succeed");

    // Should still produce a result even with truncation
    assert!(!result.tree.is_empty());
}

#[test]
fn test_trace_self_feedback_should_collapse_to_cyclic_before_truncation() {
    let self_feedback_vcd = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 1 1 rst $end\n\
$var wire 4 2 counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
01\n\
b0000 2\n\
#5\n\
10\n\
#10\n\
00\n\
b0001 2\n\
#15\n\
10\n\
#20\n\
00\n\
b0010 2\n\
#25\n\
10\n\
#30\n\
00\n\
b0011 2\n\
#35\n\
10\n\
#40\n\
00\n\
b0100 2\n\
#45\n\
10\n\
#50\n\
00\n\
b0101 2\n\
#55\n\
10\n\
#60\n\
00\n\
b0110 2\n\
#65\n\
10\n\
#70\n\
00\n\
b0111 2\n\
#75\n\
10\n\
#80\n\
00\n\
b1000 2\n\
";
    let self_feedback_deps = r#"
format_version: "1.0"
description: "self feedback register test"

signal_aliases:
  - canonical: "top.counter"
    modelsim: "top.counter"
  - canonical: "top.clk"
    modelsim: "top.clk"
  - canonical: "top.rst"
    modelsim: "top.rst"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.counter"
    category: state
    depends_on:
      - signal: "top.clk"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "top.rst"
        type: control
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "==0"
      - signal: "top.counter"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
  - output: "top.clk"
    category: control
    depends_on:
      - signal: "top.clk"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
  - output: "top.rst"
    category: control
    depends_on:
      - signal: "top.rst"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;

    let vcd_file = write_vcd_temp(self_feedback_vcd);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(self_feedback_deps);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 8,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.counter", 16, &options)
        .expect("trace should succeed");

    assert!(
        result
            .tree
            .iter()
            .any(|node| node.status == NodeStatus::Cyclic),
        "self-feedback history should be collapsed to a cyclic marker"
    );
    assert!(
        !result
            .tree
            .iter()
            .any(|node| node.status == NodeStatus::Truncated),
        "self-feedback history should stop via cyclic marker before hitting depth truncation"
    );
}

#[test]
fn test_trace_with_stop_signals() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 8,
        stop_signals: vec!["top.enable".to_string()], // Stop at enable
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 2, &options)
        .expect("trace should succeed");

    // enable should be marked as Stopped if it appears in the tree
    let enable_node = result.tree.iter().find(|n| n.signal_path == "top.enable");
    if let Some(node) = enable_node {
        assert_eq!(node.status, NodeStatus::Stopped);
    }
    // If enable doesn't appear in tree (BFS didn't expand to it), that's also fine
}

#[test]
fn test_trace_sequential() {
    let vcd_file = write_vcd_temp(SEQ_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SEQ_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 3, &options)
        .expect("trace should succeed");

    assert!(!result.tree.is_empty());
    assert_eq!(result.tree[0].signal_path, "top.data_out");
}

#[test]
fn test_trace_reference_clock_edge_should_be_ok() {
    let vcd_file = write_vcd_temp(SEQ_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_yaml = r#"
format_version: "1.0"
description: "clock edge test"

signal_aliases:
  - canonical: "top.clk"
    modelsim: "top.clk"
  - canonical: "top.data_out"
    modelsim: "top.data_out"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.data_out"
    category: data
    depends_on:
      - signal: "top.clk"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
  - output: "top.clk"
    category: control
    depends_on:
      - signal: "top.clk"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;
    let deps_file = write_yaml_temp(deps_yaml);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 3, &options)
        .expect("trace should succeed");

    let clk_node = result.tree.iter().find(|n| n.signal_path == "top.clk");
    assert!(clk_node.is_some());
    assert_eq!(clk_node.unwrap().status, NodeStatus::Ok);
}

#[test]
fn test_trace_should_populate_expected_hint() {
    let vcd_file = write_vcd_temp(SEQ_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_yaml = r#"
format_version: "1.0"
description: "expected hint test"

signal_aliases:
  - canonical: "top.clk"
    modelsim: "top.clk"
  - canonical: "top.data_out"
    modelsim: "top.data_out"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.data_out"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
  - output: "top.data_in"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;
    let deps_file = write_yaml_temp(deps_yaml);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 3, &options)
        .expect("trace should succeed");

    let data_in_node = result.tree.iter().find(|n| n.signal_path == "top.data_in");
    assert!(data_in_node.is_some());
    assert!(data_in_node.unwrap().expected_hint.is_some());
}

#[test]
fn test_trace_should_not_duplicate_expandable_nodes() {
    let vcd_file = write_vcd_temp(SEQ_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_yaml = r#"
format_version: "1.0"
description: "duplicate node test"

signal_aliases:
  - canonical: "top.clk"
    modelsim: "top.clk"
  - canonical: "top.data_in"
    modelsim: "top.data_in"
  - canonical: "top.data_out"
    modelsim: "top.data_out"

clock_aliases:
  - clock_name: "clk"
    modelsim: "top.clk"

dependencies:
  - output: "top.data_out"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
  - output: "top.data_in"
    category: data
    depends_on:
      - signal: "top.data_in"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;
    let deps_file = write_yaml_temp(deps_yaml);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 3, &options)
        .expect("trace should succeed");

    let data_in_count = result
        .tree
        .iter()
        .filter(|n| n.signal_path == "top.data_in" && n.time_index == 2)
        .count();
    assert_eq!(data_in_count, 1);
}

#[test]
fn test_trace_root_cause_should_preload_alias_resolved_root_signal() {
    let vcd = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module tb $end\n\
$scope module dut $end\n\
$var wire 1 0 led $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n";
    let deps_yaml = r#"
format_version: "1.0"
description: "alias preload test"

signal_aliases:
  - canonical: "TOP.led"
    modelsim: "tb.dut.led"

dependencies:
  - output: "TOP.led"
    category: data
    depends_on:
      - signal: "TOP.led"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
"#;

    let vcd_file = write_vcd_temp(vcd);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");
    let deps_file = write_yaml_temp(deps_yaml);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 2,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "TOP.led", 1, &options)
        .expect("trace should succeed with alias-resolved root preload");

    assert_eq!(result.tree[0].signal_path, "TOP.led");
    assert_eq!(result.tree[0].resolved_signal_path, "tb.dut.led");
    assert_eq!(result.tree[0].actual_value.as_deref(), Some("1'b1"));
}

#[test]
fn test_trace_auto_check_disabled() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let mut waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: false,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let result = trace_root_cause(&mut waveform, &dep_graph, "top.data_out", 2, &options)
        .expect("trace should succeed");

    // With auto check disabled, children should be Context (if they exist)
    assert!(!result.tree.is_empty());
    for node in result.tree.iter().skip(1) {
        if node.status != NodeStatus::Boundary {
            assert_eq!(node.status, NodeStatus::Context);
        }
    }
}

#[test]
fn test_batch_trace_root_cause_basic() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let events = vec![
        AssertionEvent {
            assertion_name: "ASSERT_DATA".to_string(),
            severity: Severity::Error,
            scope_path: "top".to_string(),
            time_value: 15,
            time_unit: TimeUnit::Ns,
            time_ps: 15000,
            source_file: None,
            source_line: None,
        },
        AssertionEvent {
            assertion_name: "ASSERT_EN".to_string(),
            severity: Severity::Failure,
            scope_path: "top".to_string(),
            time_value: 25,
            time_unit: TimeUnit::Ns,
            time_ps: 25000,
            source_file: None,
            source_line: None,
        },
    ];

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let mut waveform_mut = waveform;
    let result = batch_trace_root_cause(&mut waveform_mut, &dep_graph, &events, &options, None);

    assert!(result.is_ok(), "batch trace should succeed");
    let batch = result.unwrap();
    // Should have traces for 2 events (some may have errors if entry signal can't be resolved)
    assert!(!batch.traces.is_empty(), "should have at least one trace");
    assert!(!batch.summary.is_empty(), "should have a summary");
}

#[test]
fn test_batch_trace_root_cause_empty_events() {
    let vcd_file = write_vcd_temp(SIMPLE_COMB_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_COMB_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let options = BfsOptions {
        max_depth: 4,
        stop_signals: vec![],
        enable_auto_check: true,
        simulator: "modelsim".to_string(),
        penetrate_cdc: false,
        cdc_max_depth: 4,
        cdc_min_sync_stages: 2,
    };

    let mut waveform_mut = waveform;
    let result = batch_trace_root_cause(&mut waveform_mut, &dep_graph, &[], &options, None);

    assert!(result.is_ok());
    let batch = result.unwrap();
    assert!(
        batch.traces.is_empty(),
        "empty events should produce empty traces"
    );
    assert!(batch.aggregated_candidates.is_empty());
}
