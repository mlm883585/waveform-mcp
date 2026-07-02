//! Tests for the pattern analysis module.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::pattern::{analyze_signal_patterns, format_pattern_report};
use wellen::simple::Waveform;

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

fn open_waveform(vcd: &NamedTempFile) -> Waveform {
    let path = vcd.path();
    wellen::simple::read(path).expect("Failed to read VCD")
}

// === Value Distribution Tests ===

fn create_toggle_vcd() -> NamedTempFile {
    // 1-bit signal toggling 0->1->0->1
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 1 " enable $end
$var wire 8 # data $end
$enddefinitions $end
#0
0!
0"
b00000000 #
#5
1!
0"
b00000001 #
#10
0!
1"
b00000010 #
#15
1!
1"
b00000011 #
#20
0!
0"
b00000100 #
#25
1!
0"
b00000101 #
#30
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_pattern_1bit_toggle() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    let result =
        analyze_signal_patterns(&mut waveform, &["enable".to_string()], 0, 100, None, None)
            .unwrap();

    assert_eq!(result.value_distributions.len(), 1);
    let vd = &result.value_distributions[0];
    assert_eq!(vd.signal_path, "enable");
    assert_eq!(vd.width, 1);
    // Enable toggles between 0 and 1
    assert!(vd.distinct_values >= 2);

    // Change frequency should show changes
    let cf = &result.change_frequencies[0];
    assert!(cf.change_count > 0);

    // Idle/active: idle=0, active=1
    let ia = &result.idle_active_stats[0];
    assert_eq!(ia.threshold, "1'b0");
    assert!(ia.idle_to_active_count > 0);
}

#[test]
fn test_pattern_multibit_data() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    let result =
        analyze_signal_patterns(&mut waveform, &["data".to_string()], 0, 100, Some(10), None)
            .unwrap();

    assert_eq!(result.value_distributions.len(), 1);
    let vd = &result.value_distributions[0];
    assert_eq!(vd.signal_path, "data");
    assert_eq!(vd.width, 8);
    // Data changes to several distinct values
    assert!(vd.distinct_values >= 2);
}

#[test]
fn test_pattern_multiple_signals() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    let result = analyze_signal_patterns(
        &mut waveform,
        &["clk".to_string(), "enable".to_string(), "data".to_string()],
        0,
        100,
        None,
        None,
    )
    .unwrap();

    assert_eq!(result.value_distributions.len(), 3);
    assert_eq!(result.change_frequencies.len(), 3);
    assert_eq!(result.idle_active_stats.len(), 3);

    // Change frequency ranking should exist
    assert_eq!(result.change_frequency_ranking.len(), 3);

    // clk should have the highest change rate (most transitions)
    assert!(result.change_frequency_ranking[0].contains("clk"));
}

#[test]
fn test_pattern_idle_threshold() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    // Use idle_threshold="1" for enable signal (treat 1 as idle)
    let result = analyze_signal_patterns(
        &mut waveform,
        &["enable".to_string()],
        0,
        100,
        None,
        Some("1".to_string()),
    )
    .unwrap();

    let ia = &result.idle_active_stats[0];
    assert_eq!(ia.threshold, "1'b1");
}

#[test]
fn test_pattern_verilog_threshold() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    // Use Verilog literal as threshold
    let result = analyze_signal_patterns(
        &mut waveform,
        &["data".to_string()],
        0,
        100,
        None,
        Some("8'h00".to_string()),
    )
    .unwrap();

    let ia = &result.idle_active_stats[0];
    assert_eq!(ia.threshold, "8'h00");
}

#[test]
fn test_pattern_report_format() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    let result =
        analyze_signal_patterns(&mut waveform, &["enable".to_string()], 0, 100, None, None)
            .unwrap();

    let report = format_pattern_report(&result);
    assert!(report.contains("Value Distribution"));
    assert!(report.contains("Change Frequency"));
    assert!(report.contains("Idle/Active Analysis"));
}

#[test]
fn test_pattern_signal_not_found() {
    let vcd = create_toggle_vcd();
    let mut waveform = open_waveform(&vcd);

    let result = analyze_signal_patterns(
        &mut waveform,
        &["nonexistent".to_string()],
        0,
        100,
        None,
        None,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}
