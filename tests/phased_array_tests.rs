//! Tests for the phased array domain template module.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::phased_array::{analyze_phased_array, format_phased_array_report};

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

// VCD with multi-channel signals (ch0_data, ch1_data, ch0_enable, ch1_enable)
fn create_phased_array_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 8 " ch0_data $end
$var wire 8 # ch1_data $end
$var wire 1 $ ch0_enable $end
$var wire 1 % ch1_enable $end
$var wire 2 & state $end
$enddefinitions $end
#0
0!
b00000000 "
b00000001 #
0$
0%
b00 &
#5
1!
b00000001 "
b00000001 #
1$
1%
b01 &
#10
0!
b00000010 "
b00000011 #
0$
0%
b10 &
#15
1!
b00000010 "
b00000011 #
1$
1%
b11 &
#20
0!
b00000000 "
b00000001 #
0$
0%
b00 &
#25
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_phased_array_basic() {
    let vcd = create_phased_array_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_phased_array(&mut waveform, "ch", None, &[], "clk", 0, 100).unwrap();

    // channel_count is usize, always >= 0; just verify structure was created
    assert!(!result.cross_channel_consistency.is_empty());
}

#[test]
fn test_phased_array_with_fsm() {
    let vcd = create_phased_array_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result =
        analyze_phased_array(&mut waveform, "ch", Some("state"), &[], "clk", 0, 100).unwrap();

    assert!(result.fsm_result.is_some());
    let fsm = result.fsm_result.unwrap();
    assert!(fsm.state_count >= 2);
}

#[test]
fn test_phased_array_with_coeff() {
    let vcd = create_phased_array_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_phased_array(
        &mut waveform,
        "ch",
        None,
        &["ch0_data".to_string()],
        "clk",
        0,
        100,
    )
    .unwrap();

    assert_eq!(result.coefficient_chains.len(), 1);
    assert_eq!(result.coefficient_chains[0].coeff_signal, "ch0_data");
}

#[test]
fn test_phased_array_report() {
    let vcd = create_phased_array_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let result = analyze_phased_array(&mut waveform, "ch", None, &[], "clk", 0, 100).unwrap();

    let report = format_phased_array_report(&result);
    assert!(report.contains("Phased Array Analysis"));
}
