//! Tests for entry signal suggestion.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::deps::load_deps_from_file;
use wave_analyzer_mcp::entry_signal::{
    extract_assertion_tokens, signal_matches_assertion, suggest_entry_signals,
};

// Simple VCD with TOP module containing clk, enable, data_i, data_o
const SIMPLE_REG_VCD: &str = "\
$date 2026-05-09 $end\n\
$version minimal example $end\n\
$timescale 1ns $end\n\
$scope module TOP $end\n\
$var wire 1 ! clk $end\n\
$var wire 1 \" enable $end\n\
$var wire 8 # data_i $end\n\
$var wire 8 $ data_o $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0\"\n\
b00000000 #\n\
b00000000 $\n\
#10\n\
1!\n\
0\"\n\
b01011010 #\n\
#20\n\
0!\n\
#30\n\
1!\n\
0\"\n\
b00000000 $\n";

// Deps YAML matching the simple_reg VCD (same as MINIMAL_REFERENCE_EXAMPLE)
const SIMPLE_REG_DEPS_YAML: &str = r#"
format_version: "1.0"
description: "simple_reg minimal reference example"

signal_aliases:
  - canonical: "TOP.data_o"
    modelsim: "TOP.data_o"
  - canonical: "TOP.data_i"
    modelsim: "TOP.data_i"
  - canonical: "TOP.enable"
    modelsim: "TOP.enable"

clock_aliases:
  - clock_name: "clk"
    modelsim: "TOP.clk"

dependencies:
  - output: "TOP.data_o"
    category: data
    depends_on:
      - signal: "TOP.data_i"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "TOP.enable"
        type: control
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: ">0"

  - output: "TOP.enable"
    category: control
    depends_on:
      - signal: "TOP.enable"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null

  - output: "TOP.data_i"
    category: data
    depends_on:
      - signal: "TOP.data_i"
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
fn test_suggest_with_assertion_name() {
    let vcd_file = write_vcd_temp(SIMPLE_REG_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_REG_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let candidates = suggest_entry_signals(
        waveform.hierarchy(),
        &dep_graph,
        Some("assert_data_transfer"),
        Some("TOP"),
        "modelsim",
        10,
    );

    assert!(!candidates.is_empty());

    let data_o = candidates.iter().find(|c| c.signal_path == "TOP.data_o");
    assert!(data_o.is_some(), "TOP.data_o should be a candidate");
    let data_o = data_o.unwrap();
    assert_eq!(data_o.tier, 1);
    assert!(data_o.matches_assertion, "data_o should match 'data' token");
    assert_eq!(data_o.fan_in_count, Some(2));

    // TOP.data_o should be the first candidate (Tier 1 + assertion match)
    assert_eq!(candidates[0].signal_path, "TOP.data_o");
}

#[test]
fn test_suggest_without_assertion_name() {
    let vcd_file = write_vcd_temp(SIMPLE_REG_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_REG_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let candidates =
        suggest_entry_signals(waveform.hierarchy(), &dep_graph, None, None, "modelsim", 10);

    assert!(!candidates.is_empty());

    let tier1_count = candidates.iter().filter(|c| c.tier == 1).count();
    assert_eq!(tier1_count, 3, "3 output nodes: data_o, enable, data_i");

    for c in &candidates {
        assert!(!c.matches_assertion);
    }
}

#[test]
fn test_suggest_invalid_scope_path_fallback() {
    let vcd_file = write_vcd_temp(SIMPLE_REG_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_REG_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let candidates = suggest_entry_signals(
        waveform.hierarchy(),
        &dep_graph,
        Some("assert_data_transfer"),
        Some("NONEXISTENT_SCOPE"),
        "modelsim",
        10,
    );

    assert!(!candidates.is_empty(), "should fall back to all signals");
}

#[test]
fn test_suggest_limit_enforcement() {
    let vcd_file = write_vcd_temp(SIMPLE_REG_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_REG_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let candidates =
        suggest_entry_signals(waveform.hierarchy(), &dep_graph, None, None, "modelsim", 2);

    assert_eq!(candidates.len(), 2);
}

#[test]
fn test_assertion_token_extraction() {
    let tokens = extract_assertion_tokens("assert_data_transfer");
    assert_eq!(tokens, vec!["data", "transfer"]);

    let tokens = extract_assertion_tokens("check_fifo_full");
    assert_eq!(tokens, vec!["fifo", "full"]);

    let tokens = extract_assertion_tokens("p0_verify_protocol_handshake");
    assert_eq!(tokens, vec!["protocol", "handshake"]);

    let tokens = extract_assertion_tokens("assert_enable");
    assert_eq!(tokens, vec!["enable"]);
}

#[test]
fn test_signal_matches_assertion() {
    let tokens = vec!["data".to_string(), "transfer".to_string()];
    assert!(signal_matches_assertion("TOP.data_o", &tokens));
    assert!(signal_matches_assertion("TOP.transfer_ack", &tokens));
    assert!(!signal_matches_assertion("TOP.clk", &tokens));
    assert!(!signal_matches_assertion("TOP.enable", &tokens));

    let tokens = vec!["enable".to_string()];
    assert!(signal_matches_assertion("TOP.enable", &tokens));
    assert!(signal_matches_assertion("TOP.output_enable", &tokens));
}

#[test]
fn test_suggest_category_from_deps() {
    let vcd_file = write_vcd_temp(SIMPLE_REG_VCD);
    let waveform = wellen::simple::read(vcd_file.path()).expect("read VCD");

    let deps_file = write_yaml_temp(SIMPLE_REG_DEPS_YAML);
    let dep_graph = load_deps_from_file(deps_file.path()).expect("load deps");

    let candidates =
        suggest_entry_signals(waveform.hierarchy(), &dep_graph, None, None, "modelsim", 10);

    let data_o = candidates.iter().find(|c| c.signal_path == "TOP.data_o");
    assert!(data_o.is_some());
    assert_eq!(data_o.unwrap().category.as_deref(), Some("data"));

    let enable = candidates.iter().find(|c| c.signal_path == "TOP.enable");
    assert!(enable.is_some());
    assert_eq!(enable.unwrap().category.as_deref(), Some("control"));
}
