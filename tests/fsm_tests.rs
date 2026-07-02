//! Tests for the FSM extraction module.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::fsm::{extract_fsm, format_fsm_report};

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

// VCD with a 2-bit state signal cycling through states 0, 1, 2, 3
// Clock signal for alignment
fn create_fsm_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 2 " state $end
$enddefinitions $end
#0
0!
b00 "
#5
1!
b00 "
#10
0!
b01 "
#15
1!
b01 "
#20
0!
b10 "
#25
1!
b10 "
#30
0!
b11 "
#35
1!
b11 "
#40
0!
b00 "
#45
1!
b00 "
#50
0!
b01 "
#55
1!
b01 "
#60
"#;
    create_test_vcd(vcd)
}

// VCD with a 4-bit FSM with one-hot encoding and self-loops
fn create_fsm_selfloop_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 4 " state $end
$enddefinitions $end
#0
0!
b0001 "
#5
1!
b0001 "
#10
0!
b0010 "
#15
1!
b0010 "
#20
0!
b0010 "
#25
1!
b0100 "
#30
0!
b0100 "
#35
1!
b0001 "
#40
0!
b0001 "
#45
1!
b0001 "
#50
0!
b0010 "
#55
1!
b0010 "
#60
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_extract_fsm_raw() {
    let vcd = create_fsm_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    // Extract FSM without clock alignment
    let result = extract_fsm(&mut waveform, "state", None, "posedge", 0, 100, None).unwrap();

    assert_eq!(result.signal_path, "state");
    assert_eq!(result.width, 2);
    assert!(result.clock_signal.is_none());
    // Should discover at least 3 distinct states (0, 1, 2, 3)
    assert!(result.state_count >= 3);
    // Should have transitions
    assert!(result.transition_count >= 2);
    // DOT graph should be generated
    assert!(result.dot_graph.contains("digraph"));
}

#[test]
fn test_extract_fsm_clock_aligned() {
    let vcd = create_fsm_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    // Extract FSM with clock alignment
    let result = extract_fsm(&mut waveform, "state", Some("clk"), "posedge", 0, 100, None).unwrap();

    assert_eq!(result.signal_path, "state");
    assert_eq!(result.width, 2);
    assert_eq!(result.clock_signal, Some("clk".to_string()));
    assert!(result.state_count >= 2);

    // All transitions should be at clock edges
    for trans in &result.transitions {
        assert!(trans.at_clock_edge);
    }
}

#[test]
fn test_extract_fsm_self_loops() {
    let vcd = create_fsm_selfloop_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = extract_fsm(&mut waveform, "state", Some("clk"), "posedge", 0, 100, None).unwrap();

    // Should detect self-loops (state stays same across clock edge)
    assert!(!result.self_loops.is_empty());
}

#[test]
fn test_extract_fsm_onehot() {
    let vcd = create_fsm_selfloop_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = extract_fsm(&mut waveform, "state", Some("clk"), "posedge", 0, 100, None).unwrap();

    assert_eq!(result.width, 4);
    // One-hot states: 1, 2, 4
    assert!(result.state_count >= 2);
}

#[test]
fn test_fsm_report_format() {
    let vcd = create_fsm_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = extract_fsm(&mut waveform, "state", Some("clk"), "posedge", 0, 100, None).unwrap();

    let report = format_fsm_report(&result);
    assert!(report.contains("FSM Extraction"));
    assert!(report.contains("States"));
    assert!(report.contains("Transitions"));
    assert!(report.contains("DOT Graph"));
}

#[test]
fn test_fsm_signal_not_found() {
    let vcd = create_fsm_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = extract_fsm(&mut waveform, "nonexistent", None, "posedge", 0, 100, None);
    assert!(result.is_err());
}

#[test]
fn test_fsm_invalid_edge_type() {
    let vcd = create_fsm_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = extract_fsm(
        &mut waveform,
        "state",
        Some("clk"),
        "invalid_edge",
        0,
        100,
        None,
    );
    assert!(result.is_err());
}
