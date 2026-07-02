//! Tests for design spec lookup.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::spec::load_spec_from_file;

const SIMPLE_SPEC_YAML: &str = r#"
spec_version: "1.0"
module_name: "simple_reg"
description: "单级寄存器与使能控制样板"

assertions:
  - name: "assert_data_transfer"
    requirement_ids: ["REQ-001"]
    description: "测试场景要求传输时，输出必须在下一拍更新"
    clock: "clk"
    severity: error
    observe_signals:
      - "TOP.data_o"
      - "TOP.data_i"
      - "TOP.enable"

behaviors:
  - id: "BEH-001"
    requirement_ids: ["REQ-001"]
    description: "单周期数据传递"
    kind: latency
    check_engine: sva
    fail_entry_signals:
      - "TOP.data_o"
      - "TOP.enable"

debug_hints:
  entry_points:
    - signal: "TOP.data_o"
      reason: "用户最先观察到的异常输出"
  stop_signals:
    - "TOP.cfg_valid"
"#;

fn write_yaml_temp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(content.as_bytes()).expect("write yaml");
    f
}

#[test]
fn test_load_spec() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    assert!(lookup.has_assertion("assert_data_transfer"));
    assert!(!lookup.has_assertion("nonexistent_assertion"));
}

#[test]
fn test_find_entry_signals_by_assertion() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let signals = lookup.find_entry_signals_by_assertion("assert_data_transfer");
    assert_eq!(signals.len(), 3);
    assert_eq!(signals[0], "TOP.data_o");
    assert_eq!(signals[1], "TOP.data_i");
    assert_eq!(signals[2], "TOP.enable");
}

#[test]
fn test_find_entry_signals_missing_assertion() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let signals = lookup.find_entry_signals_by_assertion("nonexistent");
    assert_eq!(signals.len(), 0); // Returns empty list, not error
}

#[test]
fn test_find_entry_signals_by_behavior() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let signals = lookup.find_entry_signals_by_behavior("BEH-001");
    assert_eq!(signals.len(), 2);
    assert_eq!(signals[0], "TOP.data_o");
}

#[test]
fn test_find_debug_entry_points() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let entry_points = lookup.find_debug_entry_points();
    assert_eq!(entry_points.len(), 1);
    assert_eq!(entry_points[0].signal, "TOP.data_o");
    assert_eq!(
        entry_points[0].reason,
        Some("用户最先观察到的异常输出".to_string())
    );
}

#[test]
fn test_find_stop_signals() {
    let f = write_yaml_temp(SIMPLE_SPEC_YAML);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let stop_signals = lookup.find_stop_signals();
    assert_eq!(stop_signals.len(), 1);
    assert_eq!(stop_signals[0], "TOP.cfg_valid");
}

#[test]
fn test_spec_parse_error() {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(b"not: valid: yaml: [[[")
        .expect("write bad yaml");
    let result = load_spec_from_file(f.path());
    assert!(result.is_err());
}

#[test]
fn test_spec_without_debug_hints() {
    let yaml = r#"
spec_version: "1.0"
assertions:
  - name: "assert_x"
    observe_signals:
      - "TOP.x"
"#;
    let f = write_yaml_temp(yaml);
    let lookup = load_spec_from_file(f.path()).expect("load spec");

    let entry_points = lookup.find_debug_entry_points();
    assert_eq!(entry_points.len(), 0);

    let stop_signals = lookup.find_stop_signals();
    assert_eq!(stop_signals.len(), 0);
}
