//! Tests for CRC computation module.

use wave_analyzer_mcp::{compute_and_verify_crc, format_crc_report, parse_crc_polynomial};

const VCD: &str = r#"$date test $end
$version 1.0 $end
$timescale 1ns $end
$scope module top $end
$var reg 8 ! data_bus $end
$upscope $end
$enddefinitions $end
#0
b00000000 !
#10
b00000001 !
#20
b00000010 !
#30
b00000011 !
"#;

fn create_test_waveform() -> wellen::simple::Waveform {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), VCD).unwrap();
    wellen::simple::read(temp.path()).unwrap()
}

#[test]
fn test_parse_crc_polynomial_valid() {
    assert!(parse_crc_polynomial("crc8").is_ok());
    assert!(parse_crc_polynomial("crc16_ccitt").is_ok());
    assert!(parse_crc_polynomial("CRC32_ETHERNET").is_ok());
}

#[test]
fn test_parse_crc_polynomial_invalid() {
    assert!(parse_crc_polynomial("crc64").is_err());
    assert!(parse_crc_polynomial("unknown").is_err());
}

#[test]
fn test_compute_crc_basic() {
    let mut waveform = create_test_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.data_bus",
        None, // no crc signal
        None, // no data_valid
        None, // no clear
        None, // no clock (BUG-27: event-only sampling)
        "crc8",
        None, // initial_value
        0,    // start
        100,  // end
        None, // limit
    )
    .unwrap();
    assert_eq!(result.data_points, 4);
    assert!(!result.final_computed_crc.is_empty());
    // BUG-27: should have warning about event-only sampling
    assert!(result.warning.is_some());
}

#[test]
fn test_compute_crc16() {
    let mut waveform = create_test_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.data_bus",
        None, // no crc signal
        None, // no data_valid
        None, // no clear
        None, // no clock
        "crc16_ccitt",
        None, // initial_value
        0,    // start
        100,  // end
        None, // limit
    )
    .unwrap();
    assert_eq!(result.data_points, 4);
}

#[test]
fn test_compute_crc_with_limit() {
    let mut waveform = create_test_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.data_bus",
        None, // no crc signal
        None, // no data_valid
        None, // no clear
        None, // no clock
        "crc8",
        None,    // initial_value
        0,       // start
        100,     // end
        Some(2), // limit
    )
    .unwrap();
    assert_eq!(result.data_points, 2);
}

#[test]
fn test_format_crc_report() {
    let mut waveform = create_test_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.data_bus",
        None, // no crc signal
        None, // no data_valid
        None, // no clear
        None, // no clock
        "crc8",
        None, // initial_value
        0,    // start
        100,  // end
        None, // limit
    )
    .unwrap();
    let report = format_crc_report(&result);
    assert!(report.contains("CRC Computation Report"));
    assert!(report.contains("Polynomial"));
}

#[test]
fn test_compute_crc_data_not_found() {
    let mut waveform = create_test_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.nonexistent",
        None, // no crc signal
        None, // no data_valid
        None, // no clear
        None, // no clock
        "crc8",
        None, // initial_value
        0,    // start
        100,  // end
        None, // limit
    );
    assert!(result.is_err());
}

// BUG-27 test: clock-driven per-cycle sampling
const CLOCKED_VCD: &str = r#"$date test $end
$version 1.0 $end
$timescale 1ns $end
$scope module top $end
$var wire 1 ! clk $end
$var reg 8 @ data_bus $end
$upscope $end
$enddefinitions $end
#0
0!
b00000000 @
#5
1!
#10
0!
b00000001 @
#15
1!
#20
0!
b00000001 @
#25
1!
#30
0!
b00000010 @
#35
1!
#40
0!
#45
1!
"#;

fn create_clocked_waveform() -> wellen::simple::Waveform {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), CLOCKED_VCD).unwrap();
    wellen::simple::read(temp.path()).unwrap()
}

#[test]
fn test_compute_crc_with_clock_per_cycle_sampling() {
    // BUG-27: When clock is provided (and no data_valid), data should be
    // sampled at every clock posedge, even when data stays stable across
    // multiple cycles (e.g., data=1 at both #15 and #25 posedges).
    let mut waveform = create_clocked_waveform();
    let result = compute_and_verify_crc(
        &mut waveform,
        "top.data_bus",
        None,            // no crc signal
        None,            // no data_valid
        None,            // no clear
        Some("top.clk"), // BUG-27: clock-driven per-cycle sampling
        "crc8",
        None,
        0,
        100,
        None,
    )
    .unwrap();

    // Clock posedges at indices: 1(#5), 3(#15), 5(#25), 7(#35), 9(#45) = 5 events
    // Data at those indices: 0(#0→#5), 1(#15), 1(#25), 2(#35), 2(#45)
    assert!(
        result.data_points >= 4,
        "clock-driven sampling should produce at least 4 data points"
    );
    // No warning because clock is provided
    assert!(
        result.warning.is_none(),
        "no warning when clock signal is provided"
    );
}
