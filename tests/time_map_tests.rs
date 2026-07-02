//! Tests for time mapping utilities.

use tempfile::NamedTempFile;
use wave_analyzer_mcp::time_map::{
    ClockEdgeEntry, ClockEdgeTable, ClockEdgeType, build_clock_edge_table,
    compute_time_ps_from_table, find_time_index_by_value, time_value_to_ps,
};

// VCD with a clock signal that toggles 0->1->0->1->0
// Timescale: 1ns
// clk (id=0) transitions: 0@0, 1@10, 0@20, 1@30, 0@40
const CLOCK_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n\
#20\n\
00\n\
#30\n\
10\n\
#40\n\
00";

// VCD with multiple time values at different intervals
// Timescale: 1ps
const MULTI_TIME_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ps $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 8 1 data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
b00000000 1\n\
#100\n\
10\n\
b00000001 1\n\
#500\n\
00\n\
#1000\n\
10\n\
b00000010 1\n\
#5000\n\
00\n\
#10000\n\
10\n\
b00000100 1";

// VCD with a multi-bit signal (not suitable as clock)
const WIDE_SIGNAL_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 8 0 data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b00000000 0\n\
#10\n\
b00000001 0";

const FEMTO_VCD: &str = "\
$date 2024-01-01 $end\n\
$version waveform-mcp-test $end\n\
$timescale 1fs $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#1000\n\
10\n\
#2500\n\
00";

fn write_vcd_temp(content: &str) -> NamedTempFile {
    let temp_file = NamedTempFile::new().expect("create temp file");
    std::fs::write(temp_file.path(), content).expect("write VCD content");
    temp_file
}

#[test]
fn test_find_time_index_exact_match() {
    let f = write_vcd_temp(CLOCK_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    // ns timescale -> time_table values in ns: 0, 10, 20, 30, 40
    // compute_time_ps_from_table converts ns -> ps (multiply by 1000)
    // So ps values in table are: 0, 10000, 20000, 30000, 40000
    let idx = find_time_index_by_value(&waveform, 20000).unwrap(); // 20ns = 20000ps
    assert_eq!(idx, 2); // time_index 2 has value 20ns
}

#[test]
fn test_find_time_index_between_entries() {
    let f = write_vcd_temp(CLOCK_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    let idx = find_time_index_by_value(&waveform, 15000).unwrap(); // 15ns = 15000ps
    assert_eq!(idx, 1); // Nearest not later than 15ns
}

#[test]
fn test_find_time_index_before_start() {
    let f = write_vcd_temp(CLOCK_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    let idx = find_time_index_by_value(&waveform, 0).unwrap();
    assert_eq!(idx, 0);
}

#[test]
fn test_find_time_index_at_end() {
    let f = write_vcd_temp(CLOCK_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    let idx = find_time_index_by_value(&waveform, 40000).unwrap(); // 40ns = 40000ps
    assert_eq!(idx, 4);
}

#[test]
fn test_find_time_index_past_end() {
    let f = write_vcd_temp(CLOCK_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    // Past-end now clamps to the last index instead of erroring.
    // This is more user-friendly: users often specify end times that
    // slightly exceed the waveform duration.
    let idx = find_time_index_by_value(&waveform, 50000).unwrap(); // 50ns = 50000ps
    assert_eq!(idx, 4, "Past-end should clamp to last time index");
}

#[test]
fn test_find_time_index_ps_timescale() {
    let f = write_vcd_temp(MULTI_TIME_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    // ps timescale: raw values are already in ps, factor=1
    let idx = find_time_index_by_value(&waveform, 750).unwrap();
    // With ps timescale, raw values are already ps: 0, 100, 500, 1000, 5000, 10000
    // 750ps falls between 500ps (idx 2) and 1000ps (idx 3)
    assert_eq!(idx, 2); // Nearest not later than 750ps
}

#[test]
fn test_compute_time_ps_from_table_femtoseconds_preserves_order() {
    let f = write_vcd_temp(FEMTO_VCD);
    let waveform = wellen::simple::read(f.path()).expect("read VCD");
    let time_table = waveform.time_table();
    let timescale = waveform.hierarchy().timescale();

    assert_eq!(
        compute_time_ps_from_table(time_table, 0, timescale.as_ref()),
        0
    );
    assert_eq!(
        compute_time_ps_from_table(time_table, 1, timescale.as_ref()),
        1
    );
    assert_eq!(
        compute_time_ps_from_table(time_table, 2, timescale.as_ref()),
        2
    );
}

#[test]
fn test_build_clock_edge_table_posedge() {
    let f = write_vcd_temp(CLOCK_VCD);
    let mut waveform = wellen::simple::read(f.path()).expect("read VCD");
    let table = build_clock_edge_table(&mut waveform, "top.clk", ClockEdgeType::Posedge)
        .expect("build posedge table");

    // clk transitions: 0@0, 1@10, 0@20, 1@30, 0@40
    // Posedge events (0->1): at time 10 and time 30
    assert_eq!(table.edges.len(), 2);
    assert_eq!(table.edges[0].time_value, 10);
    assert_eq!(table.edges[1].time_value, 30);
}

#[test]
fn test_build_clock_edge_table_negedge() {
    let f = write_vcd_temp(CLOCK_VCD);
    let mut waveform = wellen::simple::read(f.path()).expect("read VCD");
    let table = build_clock_edge_table(&mut waveform, "top.clk", ClockEdgeType::Negedge)
        .expect("build negedge table");

    // Negedge events (1->0): at time 20 and time 40
    assert_eq!(table.edges.len(), 2);
    assert_eq!(table.edges[0].time_value, 20);
    assert_eq!(table.edges[1].time_value, 40);
}

#[test]
fn test_build_clock_edge_table_wrong_width() {
    let f = write_vcd_temp(WIDE_SIGNAL_VCD);
    let mut waveform = wellen::simple::read(f.path()).expect("read VCD");
    let result = build_clock_edge_table(&mut waveform, "top.data", ClockEdgeType::Posedge);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("CLOCK_NOT_FOUND"));
}

#[test]
fn test_build_clock_edge_table_signal_not_found() {
    let f = write_vcd_temp(CLOCK_VCD);
    let mut waveform = wellen::simple::read(f.path()).expect("read VCD");
    let result = build_clock_edge_table(&mut waveform, "top.nonexistent", ClockEdgeType::Posedge);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_step_back_basic() {
    // Manually construct a ClockEdgeTable for step_back testing
    let table = ClockEdgeTable {
        clock_name: "clk".to_string(),
        resolved_path: "top.clk".to_string(),
        edge_type: ClockEdgeType::Posedge,
        edges: vec![
            ClockEdgeEntry {
                time_index: 1,
                time_value: 10,
            },
            ClockEdgeEntry {
                time_index: 3,
                time_value: 30,
            },
            ClockEdgeEntry {
                time_index: 5,
                time_value: 50,
            },
            ClockEdgeEntry {
                time_index: 7,
                time_value: 70,
            },
        ],
    };

    // Step back 1 cycle from time_index 5
    let result = table.step_back(5, 1);
    assert_eq!(result, 3);

    // Step back 2 cycles from time_index 5
    let result = table.step_back(5, 2);
    assert_eq!(result, 1);

    // Step back 0 cycles
    let result = table.step_back(5, 0);
    assert_eq!(result, 5);
}

#[test]
fn test_step_back_before_first_edge_stays_put() {
    let table = ClockEdgeTable {
        clock_name: "clk".to_string(),
        resolved_path: "top.clk".to_string(),
        edge_type: ClockEdgeType::Posedge,
        edges: vec![
            ClockEdgeEntry {
                time_index: 3,
                time_value: 30,
            },
            ClockEdgeEntry {
                time_index: 5,
                time_value: 50,
            },
        ],
    };

    let result = table.step_back(1, 0);
    assert_eq!(result, 1);
}

#[test]
fn test_step_back_exceeds_available() {
    let table = ClockEdgeTable {
        clock_name: "clk".to_string(),
        resolved_path: "top.clk".to_string(),
        edge_type: ClockEdgeType::Posedge,
        edges: vec![
            ClockEdgeEntry {
                time_index: 1,
                time_value: 10,
            },
            ClockEdgeEntry {
                time_index: 3,
                time_value: 30,
            },
        ],
    };

    // Step back 5 cycles from time_index 3
    let result = table.step_back(3, 5);
    assert_eq!(result, 1); // Falls back to earliest edge
}

#[test]
fn test_step_back_empty_table() {
    let table = ClockEdgeTable {
        clock_name: "clk".to_string(),
        resolved_path: "top.clk".to_string(),
        edge_type: ClockEdgeType::Posedge,
        edges: vec![],
    };

    let result = table.step_back(5, 1);
    assert_eq!(result, 5); // No backtracking possible
}

// === time_value_to_ps f64 Tests ===

#[test]
fn test_time_value_to_ps_integer_ns() {
    let result = time_value_to_ps(545.0, "ns").unwrap();
    assert_eq!(result, 545000);
}

#[test]
fn test_time_value_to_ps_fractional_ns() {
    let result = time_value_to_ps(0.5, "ns").unwrap();
    assert_eq!(result, 500);
}

#[test]
fn test_time_value_to_ps_fractional_ps() {
    let result = time_value_to_ps(1.5, "ps").unwrap();
    assert_eq!(result, 2); // rounds 1.5 to 2
}

#[test]
fn test_time_value_to_ps_zero() {
    let result = time_value_to_ps(0.0, "ns").unwrap();
    assert_eq!(result, 0);
}

#[test]
fn test_time_value_to_ps_negative() {
    let result = time_value_to_ps(-1.0, "ns");
    assert!(result.is_err());
}

#[test]
fn test_time_value_to_ps_unknown_unit() {
    let result = time_value_to_ps(100.0, "xyz");
    assert!(result.is_err());
}
