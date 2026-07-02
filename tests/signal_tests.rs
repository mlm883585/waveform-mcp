//! Signal tests

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::find_signal_by_path;
use wave_analyzer_mcp::find_signal_events;
use wave_analyzer_mcp::get_signal_metadata;
use wave_analyzer_mcp::read_signal_values;
use wave_analyzer_mcp::read_signal_values_by_path;

#[test]
fn test_read_signal_values_lib() {
    // Create a VCD file with signal changes
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 9 1 test $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
b101010101 1\n\
#10\n\
10\n\
b010010111 1\n\
#20\n\
00\n\
b000011011 1";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Find the signal
    let signal_ref =
        find_signal_by_path(hierarchy, "top.clk").expect("Should find 'top.clk' signal");

    // Load the signal
    waveform.load_signals(&[signal_ref]);

    // Read values at valid time indices only (time table has max index 2)
    let values = read_signal_values(&waveform, signal_ref, &[0, 1, 2], 0)
        .expect("Should read signal values");

    assert_eq!(values.len(), 3, "Should read 3 values");
    assert!(values[0].contains("0ns"), "First value should be at 0ns");
    assert!(values[1].contains("10ns"), "Second value should be at 10ns");

    // Test that out-of-range indices now return an error (Bug 2 fix)
    let result = read_signal_values(&waveform, signal_ref, &[0, 1, 2, 3], 0);
    assert!(
        result.is_err(),
        "Should return error when any time index is out of range"
    );
    assert!(
        result.unwrap_err().to_string().contains("out of range"),
        "Error should mention out of range"
    );

    // Find the signal
    let hierarchy = waveform.hierarchy();
    let signal_ref =
        find_signal_by_path(hierarchy, "top.test").expect("Should find 'top.test' signal");

    // Load the signal
    waveform.load_signals(&[signal_ref]);

    // Read values at valid time indices only (time table has max index 2)
    let values = read_signal_values(&waveform, signal_ref, &[0, 1, 2], 0)
        .expect("Should read signal values for top.test");

    assert_eq!(values.len(), 3, "Should read 3 values");
    assert!(values[0].contains("9'h155"), "First value should be 9'h155");
}

#[test]
fn test_get_signal_metadata_lib() {
    // Create a VCD file with different signal types
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 4 1 data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
b0000 1";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Test getting metadata for a 1-bit signal
    let clk_metadata =
        get_signal_metadata(hierarchy, "top.clk").expect("Should get metadata for 'top.clk'");
    assert!(
        clk_metadata.contains("top.clk"),
        "Metadata should contain signal name"
    );
    assert!(
        clk_metadata.contains("1 bits"),
        "Metadata should show 1 bit width"
    );

    // Test getting metadata for a 4-bit signal
    let data_metadata =
        get_signal_metadata(hierarchy, "top.data").expect("Should get metadata for 'top.data'");
    assert!(
        data_metadata.contains("top.data"),
        "Metadata should contain signal name"
    );
    assert!(
        data_metadata.contains("4 bits"),
        "Metadata should show 4 bit width"
    );

    // Test getting metadata for non-existent signal
    let error = get_signal_metadata(hierarchy, "nonexistent");
    assert!(
        error.is_err(),
        "Should return error for non-existent signal"
    );
}

#[test]
fn test_find_signal_events_lib() {
    // Create a VCD file with multiple signal changes
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
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

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    // Find the signal
    let signal_ref =
        find_signal_by_path(hierarchy, "top.clk").expect("Should find 'top.clk' signal");

    // Load the signal
    waveform.load_signals(&[signal_ref]);

    // Find all events
    let events =
        find_signal_events(&waveform, signal_ref, 0, 10, -1).expect("Should find signal events");
    assert!(!events.is_empty(), "Should find at least one event");

    // Find events with limit
    let limited_events =
        find_signal_events(&waveform, signal_ref, 0, 10, 2).expect("Should find limited events");
    assert_eq!(limited_events.len(), 2, "Should limit to 2 events");

    // Find events in a specific time range
    let range_events =
        find_signal_events(&waveform, signal_ref, 2, 3, -1).expect("Should find events in range");
    assert!(
        !range_events.is_empty(),
        "Should find events in specified range"
    );
}

#[test]
fn test_bus_signal_metadata_and_read_uses_full_width() {
    let vcd_content = "\
$date 2026-05-29 $end\n\
$version led-blink bus regression $end\n\
$timescale 1ps $end\n\
$scope module led_blink_tb $end\n\
$var wire 1 ! status [1] $end\n\
$var wire 1 \" status [0] $end\n\
$scope module dut $end\n\
$var wire 1 # status [1] $end\n\
$var wire 1 $ status [0] $end\n\
$var reg 6 % counter [5:0] $end\n\
$upscope $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
$dumpvars\n\
0!\n\
1\"\n\
1#\n\
0$\n\
b101010 %\n\
$end\n\
#10\n\
1!\n\
0\"\n\
0#\n\
1$\n\
b000111 %";

    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    write!(temp_file, "{}", vcd_content).expect("Failed to write VCD content");
    temp_file.flush().expect("Failed to flush");

    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");
    let hierarchy = waveform.hierarchy();

    let status_metadata = get_signal_metadata(hierarchy, "led_blink_tb.dut.status")
        .expect("Should get metadata for dut.status");
    assert!(
        status_metadata.contains("Width: 2 bits"),
        "Expected 2-bit status metadata, got: {}",
        status_metadata
    );
    assert!(
        status_metadata.contains("Index: [1:0]"),
        "Expected [1:0] status index, got: {}",
        status_metadata
    );

    let counter_metadata = get_signal_metadata(hierarchy, "led_blink_tb.dut.counter")
        .expect("Should get metadata for dut.counter");
    assert!(
        counter_metadata.contains("Width: 6 bits"),
        "Expected 6-bit counter metadata, got: {}",
        counter_metadata
    );
    assert!(
        counter_metadata.contains("Index: [5:0]"),
        "Expected [5:0] counter index, got: {}",
        counter_metadata
    );

    let status_values =
        read_signal_values_by_path(&mut waveform, "led_blink_tb.dut.status", &[0, 1])
            .expect("Should read bus status values");
    assert!(
        status_values[0].contains("2'b10"),
        "Expected full-width status at t0, got: {:?}",
        status_values
    );
    assert!(
        status_values[1].contains("2'b01"),
        "Expected full-width status at t1, got: {:?}",
        status_values
    );

    let counter_values =
        read_signal_values_by_path(&mut waveform, "led_blink_tb.dut.counter", &[0, 1])
            .expect("Should read bus counter values");
    assert!(
        counter_values[0].contains("6'h2a"),
        "Expected full-width counter at t0, got: {:?}",
        counter_values
    );
    assert!(
        counter_values[1].contains("6'h07"),
        "Expected full-width counter at t1, got: {:?}",
        counter_values
    );
}
