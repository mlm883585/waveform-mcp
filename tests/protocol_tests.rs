//! Tests for the protocol analysis and signal measurement module.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::protocol::{
    analyze_handshake, analyze_handshake_with_level_sensitive, measure_clock, measure_pulses,
};

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

// === Handshake Analysis Tests ===

fn create_handshake_vcd() -> NamedTempFile {
    // VCD with:
    // - valid: 1-bit signal, goes high at times 10, 40, 70
    // - ready: 1-bit signal, goes high at times 15, 42, 71 (after valid)
    // - data: 8-bit signal with values at transfer times
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! valid $end
$var wire 1 " ready $end
$var wire 8 # data $end
$enddefinitions $end
#0
0!
0"
b00000000 #
#10
1!
0"
b00000010 #
#15
1!
1"
b00000011 #
#20
0!
0"
b00000100 #
#30
0!
0"
b00000101 #
#40
1!
0"
b00000110 #
#42
1!
1"
b00000111 #
#50
0!
0"
b00001000 #
#60
0!
0"
b00001001 #
#70
1!
0"
b00001010 #
#71
1!
1"
b00001011 #
#80
0!
0"
b00001100 #
"#;
    create_test_vcd(vcd)
}

fn create_level_enable_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! en $end
$var wire 1 " ready $end
$var wire 8 # data $end
$enddefinitions $end
#0
0!
1"
b00000000 #
#10
1!
1"
b00000001 #
#20
1!
1"
b00000010 #
#30
1!
1"
b00000011 #
#40
0!
1"
b00000100 #
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_analyze_handshake_basic() {
    let file = create_handshake_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        0,
        100,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    assert_eq!(result.valid_signal, "valid");
    assert_eq!(result.ready_signal, "ready");
    assert_eq!(result.data_signal, None);
    // Should detect 3 handshakes
    assert_eq!(result.summary.total_handshakes, 3);
    assert_eq!(result.events.len(), 3);

    // Verify all events have valid properties
    for e in &result.events {
        assert!(e.ready_time_index > e.valid_time_index);
        assert_eq!(
            e.latency_time_indices,
            e.ready_time_index - e.valid_time_index
        );
        assert!(!e.latency_formatted.is_empty());
    }

    // Summary stats
    assert!(result.summary.avg_latency > 0.0);
    assert!(result.summary.min_latency <= result.summary.max_latency);
    assert!(result.summary.first_handshake_time <= result.summary.last_handshake_time);
}

#[test]
fn test_analyze_handshake_level_sensitive_counts_each_high_sample() {
    let file = create_level_enable_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake_with_level_sensitive(
        &mut waveform,
        "en",
        "ready",
        Some("data"),
        0,
        4,
        Some(-1),
        "summary",
        false,
        true,
    )
    .expect("Handshake analysis failed");

    assert_eq!(result.summary.total_handshakes, 3);
    assert_eq!(result.events.len(), 3);
    assert_eq!(result.events[0].ready_time_index, 1);
    assert_eq!(result.events[1].ready_time_index, 2);
    assert_eq!(result.events[2].ready_time_index, 3);
    assert!(
        result
            .warning
            .as_deref()
            .unwrap()
            .contains("Level-sensitive")
    );
}

#[test]
fn test_analyze_handshake_with_data() {
    let file = create_handshake_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        Some("data"),
        0,
        100,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    // Data values should be captured at transfer times
    assert!(result.events.iter().all(|e| e.data_value.is_some()));
}

#[test]
fn test_analyze_handshake_stale_not_reported() {
    // VCD where valid goes high but goes low before ready goes high
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! valid $end
$var wire 1 " ready $end
$enddefinitions $end
#0
0!
0"
#10
1!
0"
#15
0!
0"
#20
0!
1"
#30
0!
0"
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        0,
        100,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    // No handshakes should be detected (valid went low before ready went high)
    assert_eq!(result.summary.total_handshakes, 0);
}

#[test]
fn test_analyze_handshake_not_found() {
    let file = create_handshake_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "nonexistent",
        "ready",
        None,
        0,
        100,
        Some(-1),
        "summary",
        false,
    );
    assert!(result.is_err());
}

#[test]
fn test_analyze_handshake_limit() {
    let file = create_handshake_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        0,
        100,
        Some(2),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    // Should only return 2 events (limit applied)
    assert_eq!(result.events.len(), 2);
    assert_eq!(result.summary.total_handshakes, 3);
}

#[test]
fn test_analyze_handshake_window_preserves_initial_state() {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! valid $end
$var wire 1 " ready $end
$enddefinitions $end
#0
0!
0"
#10
1!
0"
#20
1!
1"
#30
0!
0"
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        1,
        3,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    assert_eq!(result.summary.total_handshakes, 1);
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].valid_time_index, 1);
    assert_eq!(result.events[0].ready_time_index, 2);
}

#[test]
fn test_analyze_handshake_window_detects_immediate_transfer() {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! valid $end
$var wire 1 " ready $end
$enddefinitions $end
#0
0!
0"
#10
1!
1"
#20
0!
0"
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        1,
        2,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    assert_eq!(result.summary.total_handshakes, 1);
    assert_eq!(result.events[0].latency_time_indices, 0);
}

// === Clock Measurement Tests ===

fn create_clock_vcd() -> NamedTempFile {
    // Simple clock with period=20 (high for 10, low for 10)
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$enddefinitions $end
#0
0!
#5
1!
#10
0!
#15
1!
#20
0!
#25
1!
#30
0!
#35
1!
#40
0!
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_measure_clock_posedge() {
    let file = create_clock_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result =
        measure_clock(&mut waveform, "clk", "posedge", 0, 100).expect("Clock measurement failed");

    assert_eq!(result.signal_path, "clk");
    assert_eq!(result.edge_type, "posedge");
    // Clock has edges at 5,10,15,20,25,30,35,40 => 4 posedges at 5,15,25,35 => 3 periods
    assert!(result.period.count >= 1);
    assert!(result.period.avg > 0.0);
    assert!(result.period.min > 0.0);
    // Jitter should be 0 for perfect clock
    assert!(result.jitter < 0.01);
    // Duty cycle should be close to 50%
    if let Some(duty) = result.duty_cycle_pct {
        assert!(
            (duty - 50.0).abs() < 5.0,
            "Duty cycle should be ~50%, got {}",
            duty
        );
    }
    // Frequency should be present with 1ns timescale
    assert!(result.frequency_hz.is_some());
}

#[test]
fn test_measure_clock_not_found() {
    let file = create_clock_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = measure_clock(&mut waveform, "nonexistent", "posedge", 0, 100);
    assert!(result.is_err());
}

#[test]
fn test_measure_clock_invalid_edge_type() {
    let file = create_clock_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = measure_clock(&mut waveform, "clk", "invalid", 0, 100);
    assert!(result.is_err());
}

// === Pulse Measurement Tests ===

fn create_pulse_vcd() -> NamedTempFile {
    // Signal with varying pulse widths
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! pulse $end
$enddefinitions $end
#0
0!
#5
1!
#10
0!
#20
1!
#25
0!
#40
1!
#50
0!
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_measure_pulses() {
    let file = create_pulse_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = measure_pulses(&mut waveform, "pulse", 0, 100).expect("Pulse measurement failed");

    assert_eq!(result.signal_path, "pulse");

    // Should detect multiple high and low pulses
    assert!(result.high_pulse_count >= 1);
    assert!(result.low_pulse_count >= 1);
    assert!(result.high_pulses.avg > 0.0);
    assert!(result.high_pulses.min > 0.0);
    assert!(result.high_pulses.max > 0.0);
    assert!(result.low_pulses.avg > 0.0);
}

#[test]
fn test_measure_pulses_not_found() {
    let file = create_pulse_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = measure_pulses(&mut waveform, "nonexistent", 0, 100);
    assert!(result.is_err());
}

// === Format Report Tests ===

#[test]
fn test_format_handshake_report() {
    let file = create_handshake_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_handshake(
        &mut waveform,
        "valid",
        "ready",
        None,
        0,
        100,
        Some(-1),
        "summary",
        false,
    )
    .expect("Handshake analysis failed");

    let text = wave_analyzer_mcp::format_handshake_report(&result);
    assert!(text.contains("Handshake Analysis"));
    assert!(text.contains("Total handshakes: 3"));
    assert!(text.contains("Average latency"));
}

#[test]
fn test_format_clock_report() {
    let file = create_clock_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result =
        measure_clock(&mut waveform, "clk", "posedge", 0, 100).expect("Clock measurement failed");

    let text = wave_analyzer_mcp::format_clock_report(&result);
    assert!(text.contains("Clock Measurement"));
    assert!(text.contains("Periods measured"));
    assert!(text.contains("Frequency"));
    assert!(
        text.contains("ns"),
        "Expected physical time unit in report, got: {}",
        text
    );
    assert!(
        !text.contains("time indices"),
        "Clock report should not claim physical periods are time indices: {}",
        text
    );
}

#[test]
fn test_format_pulse_report() {
    let file = create_pulse_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = measure_pulses(&mut waveform, "pulse", 0, 100).expect("Pulse measurement failed");

    let text = wave_analyzer_mcp::format_pulse_report(&result);
    assert!(text.contains("Pulse Measurement"));
    assert!(text.contains("High pulses:"));
    assert!(text.contains("Low pulses:"));
}
