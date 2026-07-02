//! Tests for sequence detection module.

use wave_analyzer_mcp::{detect_sequence, format_sequence_report};

const VCD: &str = r#"$date test $end
$version 1.0 $end
$timescale 1ns $end
$scope module top $end
$var reg 1 ! state_a $end
$var reg 1 " state_b $end
$var reg 1 # state_c $end
$upscope $end
$enddefinitions $end
#0
0!
0"
0#
#10
1!
0"
0#
#20
0!
1"
0#
#30
0!
0"
1#
#40
1!
0"
0#
#50
0!
1"
0#
#60
0!
0"
1#
"#;

fn create_test_waveform() -> wellen::simple::Waveform {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), VCD).unwrap();
    wellen::simple::read(temp.path()).unwrap()
}

#[test]
fn test_detect_sequence_found() {
    let mut waveform = create_test_waveform();
    let conditions = vec![
        "top.state_a == 1'b1".to_string(),
        "top.state_b == 1'b1".to_string(),
        "top.state_c == 1'b1".to_string(),
    ];
    let result = detect_sequence(&mut waveform, &conditions, Some(20), 0, 100, None).unwrap();
    assert!(result.occurrence_count > 0);
}

#[test]
fn test_detect_sequence_not_found_tight_gap() {
    let mut waveform = create_test_waveform();
    let conditions = vec![
        "top.state_c == 1'b1".to_string(),
        "top.state_b == 1'b1".to_string(),
        "top.state_a == 1'b1".to_string(),
    ];
    let result = detect_sequence(&mut waveform, &conditions, Some(5), 0, 100, None).unwrap();
    assert_eq!(result.occurrence_count, 0);
}

#[test]
fn test_detect_single_condition() {
    let mut waveform = create_test_waveform();
    let conditions = vec!["top.state_a == 1'b1".to_string()];
    let result = detect_sequence(&mut waveform, &conditions, None, 0, 100, None).unwrap();
    assert!(result.occurrence_count > 0);
}

#[test]
fn test_detect_sequence_with_bare_decimal_literals() {
    let mut waveform = create_test_waveform();
    let conditions = vec![
        "top.state_a == 1".to_string(),
        "top.state_b == 1".to_string(),
        "top.state_c == 1".to_string(),
    ];
    let result = detect_sequence(&mut waveform, &conditions, Some(20), 0, 100, None).unwrap();
    assert!(result.occurrence_count > 0);
}

#[test]
fn test_format_sequence_report() {
    let mut waveform = create_test_waveform();
    let conditions = vec!["top.state_a == 1'b1".to_string()];
    let result = detect_sequence(&mut waveform, &conditions, None, 0, 100, None).unwrap();
    let report = format_sequence_report(&result);
    assert!(report.contains("Sequence Detection Report"));
}

#[test]
fn test_detect_empty_conditions_errors() {
    let mut waveform = create_test_waveform();
    let result = detect_sequence(&mut waveform, &[], None, 0, 100, None);
    assert!(result.is_err());
}
