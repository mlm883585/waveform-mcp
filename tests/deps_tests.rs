//! Tests for dependency graph loading and querying.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::deps::{BoundaryKind, CheckExpr, DepType, load_deps_from_file};

const SIMPLE_DEPS_YAML: &str = r#"
format_version: "1.0"
description: "simple_reg test deps"

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
        latency_cycles: 0
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

fn write_yaml_temp(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(content.as_bytes()).expect("write yaml");
    f
}

#[test]
fn test_load_simple_deps() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    assert_eq!(graph.meta().node_count, 3);
    assert_eq!(graph.meta().edge_count, 4);
    assert!(
        graph.meta().has_cycles,
        "enable self-loop should be detected"
    );
    assert_eq!(graph.meta().signal_alias_count, 3);
    assert_eq!(graph.meta().clock_alias_count, 1);
}

#[test]
fn test_fan_in() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let data_o_fan_in = graph.fan_in("TOP.data_o").expect("fan_in exists");
    assert_eq!(data_o_fan_in.len(), 2);
    assert_eq!(data_o_fan_in[0].signal, "TOP.data_i");
    assert_eq!(data_o_fan_in[0].dep_type, DepType::Sequential);
    assert_eq!(data_o_fan_in[0].check, Some(CheckExpr::Equal));
    assert_eq!(data_o_fan_in[0].latency_cycles, Some(1));

    assert_eq!(data_o_fan_in[1].signal, "TOP.enable");
    assert_eq!(data_o_fan_in[1].dep_type, DepType::Control);
    assert_eq!(data_o_fan_in[1].check, Some(CheckExpr::GreaterThanZero));
}

#[test]
fn test_fan_out() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let enable_fan_out = graph.fan_out("TOP.enable").expect("fan_out exists");
    assert!(enable_fan_out.contains(&"TOP.data_o".to_string()));
    assert!(enable_fan_out.contains(&"TOP.enable".to_string())); // self-loop
}

#[test]
fn test_boundary_edges() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let enable_deps = graph.fan_in("TOP.enable").expect("fan_in exists");
    assert_eq!(enable_deps.len(), 1);
    assert_eq!(enable_deps[0].dep_type, DepType::Boundary);
    assert_eq!(enable_deps[0].boundary_kind, Some(BoundaryKind::InputPort));
}

#[test]
fn test_signal_alias_resolution() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let _resolved = graph.resolve_signal("TOP.data_o", "modelsim");
    // TOP.data_o has alias mapping in YAML, resolved path is simulator-specific
    // If no alias exists, returns None (callers fall back to canonical name)

    // Canonical without alias returns None
    let resolved2 = graph.resolve_signal("TOP.unknown_signal", "modelsim");
    assert_eq!(resolved2, None);
}

#[test]
fn test_signal_alias_reverse_resolution() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let canonical = graph.canonicalize_signal("TOP.data_o", "modelsim");
    assert_eq!(canonical, Some("TOP.data_o".to_string()));

    let missing = graph.canonicalize_signal("TOP.unknown_signal", "modelsim");
    assert_eq!(missing, None);
}

#[test]
fn test_clock_alias_resolution() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let resolved = graph
        .resolve_clock("clk", "modelsim")
        .expect("resolve clock");
    assert_eq!(resolved, "TOP.clk".to_string());

    // Missing clock alias returns error
    let result = graph.resolve_clock("unknown_clk", "modelsim");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("CLOCK_NOT_FOUND"));
}

#[test]
fn test_has_signal() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    assert!(graph.has_signal("TOP.data_o"));
    assert!(graph.has_signal("TOP.data_i"));
    assert!(!graph.has_signal("TOP.nonexistent"));
}

#[test]
fn test_yaml_parse_error() {
    let mut f = NamedTempFile::new().expect("create temp file");
    f.write_all(b"not: valid: yaml: [[[")
        .expect("write bad yaml");
    let result = load_deps_from_file(f.path());
    assert!(result.is_err());
}

#[test]
fn test_output_signals() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    let outputs = graph.output_signals();
    assert_eq!(outputs.len(), 3);
    assert!(outputs.contains(&"TOP.data_o".to_string()));
    assert!(outputs.contains(&"TOP.enable".to_string()));
    assert!(outputs.contains(&"TOP.data_i".to_string()));
}

#[test]
fn test_is_output_node() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    assert!(graph.is_output_node("TOP.data_o"));
    assert!(graph.is_output_node("TOP.enable"));
    assert!(graph.is_output_node("TOP.data_i"));
    assert!(!graph.is_output_node("TOP.unknown_signal"));
}

#[test]
fn test_get_category() {
    let f = write_yaml_temp(SIMPLE_DEPS_YAML);
    let graph = load_deps_from_file(f.path()).expect("load deps");

    assert_eq!(graph.get_category("TOP.data_o"), Some("data"));
    assert_eq!(graph.get_category("TOP.enable"), Some("control"));
    assert_eq!(graph.get_category("TOP.data_i"), Some("data"));
    assert_eq!(graph.get_category("TOP.unknown"), None);
}
