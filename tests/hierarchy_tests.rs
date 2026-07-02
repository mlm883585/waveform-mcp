//! Hierarchy tests

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::find_scope_by_path;
use wave_analyzer_mcp::find_signal_by_path;
use wave_analyzer_mcp::list_signals;
use wave_analyzer_mcp::read_hierarchy;

#[test]
fn test_signal_full_name() {
    // Create a simple VCD file with hierarchy
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$scope module submodule $end\n\
$var wire 1 0 clk $end\n\
$upscope $end\n\
$var wire 8 1 data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
b000000001";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Check full names of variables
    for var in hierarchy.iter_vars() {
        let full_name = var.full_name(hierarchy);
        println!("Variable full name: {}", full_name);
        assert!(!full_name.is_empty(), "Full name should not be empty");
    }
}

#[test]
fn test_list_signals_function() {
    // Create a simple VCD file
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 8 1 data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
b000000001";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Simulate list_signals function
    let mut signals = Vec::new();
    for var in hierarchy.iter_vars() {
        let path = var.full_name(hierarchy);
        signals.push(format!("{} ({})", path, var.signal_ref().index()));
    }

    println!("Found signals: {}", signals.join("\n"));
    assert!(!signals.is_empty(), "Should have found at least one signal");
}

#[test]
fn test_find_signal_by_path() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Debug: print all available signals
    for var in hierarchy.iter_vars() {
        println!("Signal path: {}", var.full_name(hierarchy));
    }

    // Test finding existing signal - use the correct path format
    let result = find_signal_by_path(hierarchy, "top.clk");
    assert!(result.is_some(), "Should find 'top.clk' signal");

    // Test finding non-existing signal
    let result = find_signal_by_path(hierarchy, "nonexistent");
    assert!(result.is_none(), "Should not find 'nonexistent' signal");
}

#[test]
fn test_list_signals_recursive() {
    // Create a VCD file with nested hierarchy
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$scope module submodule1 $end\n\
$var wire 1 @ data1 $end\n\
$scope module inner $end\n\
$var wire 1 # data2 $end\n\
$upscope $end\n\
$upscope $end\n\
$scope module submodule2 $end\n\
$var wire 1 $ data3 $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0@\n\
0#\n\
0$";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Test recursive mode (default): should find all signals at all levels
    let mut all_signals: Vec<String> = Vec::new();
    for var in hierarchy.iter_vars() {
        all_signals.push(var.full_name(hierarchy));
    }
    assert!(
        !all_signals.is_empty(),
        "Should find signals in recursive mode"
    );

    // Test non-recursive mode at top level (top scope)
    let mut top_level_signals: Vec<String> = Vec::new();
    for scope_ref in hierarchy.scopes() {
        let scope = &hierarchy[scope_ref];
        let scope_path = scope.full_name(hierarchy);
        if scope_path == "top" {
            // Top-level scope
            for var_ref in scope.vars(hierarchy) {
                let var = &hierarchy[var_ref];
                top_level_signals.push(var.full_name(hierarchy));
            }
            break;
        }
    }
    assert_eq!(
        top_level_signals.len(),
        1,
        "Should find 1 signal at top level"
    );
    assert!(
        top_level_signals.contains(&"top.clk".to_string()),
        "Should find 'clk' at top level"
    );

    // Test non-recursive mode at submodule level
    let mut submodule1_signals: Vec<String> = Vec::new();

    if let Some(submodule1_ref) = find_scope_by_path(hierarchy, "top.submodule1") {
        let submodule1 = &hierarchy[submodule1_ref];
        for var_ref in submodule1.vars(hierarchy) {
            let var = &hierarchy[var_ref];
            submodule1_signals.push(var.full_name(hierarchy));
        }
    }

    assert_eq!(
        submodule1_signals.len(),
        1,
        "Should find 1 signal at submodule1 level"
    );
    assert!(
        submodule1_signals.contains(&"top.submodule1.data1".to_string()),
        "Should find 'data1' at submodule1 level"
    );

    // Verify that inner module signal is not included in submodule1 non-recursive list
    assert!(
        !submodule1_signals.iter().any(|s| s.contains("inner")),
        "Should not include inner module signals in submodule1 non-recursive list"
    );
}

#[test]
fn test_list_signals_lib() {
    // Create a VCD file with multiple signals
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 1 1 data $end\n\
$var wire 1 2 enable $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
01\n\
02";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Test listing all signals (recursive)
    let signals = list_signals(hierarchy, None, None, true, None).expect("list_signals failed");
    assert_eq!(signals.len(), 3, "Should find 3 signals");

    // Test filtering by name pattern (regex)
    let clk_signals =
        list_signals(hierarchy, Some("clk"), None, true, None).expect("list_signals failed");
    assert_eq!(clk_signals.len(), 1, "Should find 1 signal matching 'clk'");
    assert!(
        clk_signals[0].contains("clk"),
        "Signal should contain 'clk'"
    );

    // Test filtering by hierarchy prefix
    let top_signals =
        list_signals(hierarchy, None, Some("top"), true, None).expect("list_signals failed");
    assert_eq!(top_signals.len(), 3, "Should find 3 signals under 'top'");

    // Test limit
    let limited_signals =
        list_signals(hierarchy, None, None, true, Some(2)).expect("list_signals failed");
    assert_eq!(limited_signals.len(), 2, "Should limit to 2 signals");

    // Test unlimited limit (-1)
    let unlimited_signals =
        list_signals(hierarchy, None, None, true, Some(-1)).expect("list_signals failed");
    assert_eq!(
        unlimited_signals.len(),
        3,
        "Should return all signals with -1 limit"
    );
}

#[test]
fn test_list_signals_deduplicates_bus_bits() {
    let vcd_content = "\
$date 2026-05-29 $end\n\
$version led-blink regression fixture $end\n\
$timescale 1ps $end\n\
$scope module led_blink_tb $end\n\
$var wire 1 ! clk $end\n\
$var wire 1 \" rst $end\n\
$var wire 1 # led $end\n\
$var wire 1 $ led2 $end\n\
$var wire 1 % status [1] $end\n\
$var wire 1 & status [0] $end\n\
$scope module dut $end\n\
$var wire 1 ' clk $end\n\
$var wire 1 ( rst $end\n\
$var wire 1 ) led $end\n\
$var wire 1 * led2 $end\n\
$var wire 1 + status [1] $end\n\
$var wire 1 , status [0] $end\n\
$var reg 6 - counter [5:0] $end\n\
$var reg 5 . counter2 [4:0] $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
1\"\n\
0#\n\
0$\n\
0%\n\
0&\n\
0'\n\
1(\n\
0)\n\
0*\n\
0+\n\
0,\n\
b0 -\n\
b0 .";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let signals = list_signals(hierarchy, None, Some("led_blink_tb"), true, None)
        .expect("list_signals failed");
    let mut unique = std::collections::HashSet::new();
    assert!(
        signals.iter().all(|signal| unique.insert(signal.clone())),
        "list_signals should not return duplicate paths: {:?}",
        signals
    );

    assert_eq!(
        signals
            .iter()
            .filter(|signal| *signal == "led_blink_tb.status")
            .count(),
        1,
        "top-level status bus should appear exactly once"
    );
    assert_eq!(
        signals
            .iter()
            .filter(|signal| *signal == "led_blink_tb.dut.status")
            .count(),
        1,
        "dut status bus should appear exactly once"
    );
    assert_eq!(
        signals
            .iter()
            .filter(|signal| *signal == "led_blink_tb.dut.counter")
            .count(),
        1,
        "counter bus should appear exactly once"
    );
    assert_eq!(
        signals
            .iter()
            .filter(|signal| *signal == "led_blink_tb.dut.counter2")
            .count(),
        1,
        "counter2 bus should appear exactly once"
    );
    assert!(
        signals.iter().all(|signal| !signal.contains('[')),
        "list_signals should prefer bus names over per-bit duplicates: {:?}",
        signals
    );
}

#[test]
fn test_read_hierarchy_lib_recursive() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$scope module submodule1 $end\n\
$var wire 1 @ data1 $end\n\
$scope module inner $end\n\
$var wire 1 # data2 $end\n\
$upscope $end\n\
$upscope $end\n\
$scope module submodule2 $end\n\
$var wire 1 $ data3 $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0@\n\
0#\n\
0$";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines = read_hierarchy(hierarchy, Some("top"), true, None)
        .expect("Should read hierarchy for 'top'");

    assert_eq!(lines[0], "top");
    assert!(lines.iter().any(|line| line == "  clk"));
    assert!(lines.iter().any(|line| line == "  submodule1"));
    assert!(lines.iter().any(|line| line == "    data1"));
    assert!(lines.iter().any(|line| line == "    inner"));
    assert!(lines.iter().any(|line| line == "      data2"));
    assert!(lines.iter().any(|line| line == "  submodule2"));
    assert!(lines.iter().any(|line| line == "    data3"));
}

#[test]
fn test_read_hierarchy_lib_non_recursive_scope() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$scope module submodule1 $end\n\
$var wire 1 @ data1 $end\n\
$scope module inner $end\n\
$var wire 1 # data2 $end\n\
$upscope $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0@\n\
0#";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines = read_hierarchy(hierarchy, Some("top.submodule1"), false, None)
        .expect("Should read hierarchy for 'top.submodule1'");

    assert_eq!(lines, vec!["top.submodule1", "  inner"]);
}

#[test]
fn test_read_hierarchy_lib_with_limit() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$scope module submodule1 $end\n\
$var wire 1 @ data1 $end\n\
$scope module inner $end\n\
$var wire 1 # data2 $end\n\
$upscope $end\n\
$upscope $end\n\
$scope module submodule2 $end\n\
$var wire 1 $ data3 $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0@\n\
0#\n\
0$";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines =
        read_hierarchy(hierarchy, Some("top"), true, Some(3)).expect("Should read hierarchy");

    assert_eq!(
        lines,
        vec![
            "top",
            "  clk",
            "  submodule1",
            "... truncated after 3 items",
        ]
    );
}

#[test]
fn test_read_hierarchy_lib_missing_scope() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! clk $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let result = read_hierarchy(hierarchy, Some("top.missing"), true, None);
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Scope not found: top.missing")
    );
}

#[test]
fn test_read_hierarchy_lib_skips_non_module_scopes() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$scope begin genblk1 $end\n\
$scope module submodule1 $end\n\
$var wire 1 ! data1 $end\n\
$scope module inner $end\n\
$var wire 1 @ data2 $end\n\
$upscope $end\n\
$upscope $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0@";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines = read_hierarchy(hierarchy, Some("top"), true, None)
        .expect("Should read hierarchy for 'top'");

    assert_eq!(lines[0], "top");
    assert!(lines.iter().any(|line| line == "  submodule1"));
    assert!(lines.iter().any(|line| line == "    data1"));
    assert!(lines.iter().any(|line| line == "    inner"));
    assert!(lines.iter().any(|line| line == "      data2"));
}

#[test]
fn test_read_hierarchy_lib_skips_parameters() {
    let vcd_content = "\
$date 2026-06-05 $end\n\
$version parameter filtering $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var parameter 32 ! WIDTH $end\n\
$var wire 1 \" data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b00000000000000000000000000001000 !\n\
0\"";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines = read_hierarchy(hierarchy, Some("top"), true, None)
        .expect("Should read hierarchy for 'top'");

    assert!(lines.iter().any(|line| line == "  data"));
    assert!(!lines.iter().any(|line| line.contains("WIDTH")));
}

#[test]
fn test_read_hierarchy_lib_deduplicates_duplicate_signal_names_per_scope() {
    let vcd_content = "\
$date 2026-06-01 $end\n\
$version duplicate signal names $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! o_crc $end\n\
$var wire 1 \" o_crc $end\n\
$scope module inner $end\n\
$var wire 1 # o_crc $end\n\
$var wire 1 $ o_crc $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
0!\n\
0\"\n\
0#\n\
0$";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let lines = read_hierarchy(hierarchy, Some("top"), true, None)
        .expect("Should read hierarchy for 'top'");

    assert_eq!(lines[0], "top");
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "  o_crc")
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "  inner")
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.as_str() == "    o_crc")
            .count(),
        1
    );
}
