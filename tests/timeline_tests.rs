//! Tests for multi-signal timeline module.

use wave_analyzer_mcp::{SignalEntry, build_multi_signal_timeline, format_timeline_report};

const VCD: &str = r#"$date test $end
$version 1.0 $end
$timescale 1ns $end
$scope module top $end
$var reg 1 ! sig_a $end
$var reg 8 @ sig_b $end
$upscope $end
$enddefinitions $end
#0
0!
b00000000 @
#10
1!
b00000001 @
#20
0!
b00000010 @
#30
1!
b00000011 @
"#;

fn create_test_waveform() -> wellen::simple::Waveform {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), VCD).unwrap();
    wellen::simple::read(temp.path()).unwrap()
}

#[test]
fn test_timeline_union_mode() {
    let mut waveform = create_test_waveform();
    let signals = vec![
        SignalEntry {
            signal_path: Some("top.sig_a".to_string()),
            bit_mapping: vec![],
            alias: Some("a".to_string()),
        },
        SignalEntry {
            signal_path: Some("top.sig_b".to_string()),
            bit_mapping: vec![],
            alias: Some("b".to_string()),
        },
    ];
    let result =
        build_multi_signal_timeline(&mut waveform, &signals, 0, 100, "union", "hex", None).unwrap();
    assert!(!result.rows.is_empty());
}

#[test]
fn test_timeline_with_limit() {
    let mut waveform = create_test_waveform();
    let signals = vec![
        SignalEntry {
            signal_path: Some("top.sig_a".to_string()),
            bit_mapping: vec![],
            alias: None,
        },
        SignalEntry {
            signal_path: Some("top.sig_b".to_string()),
            bit_mapping: vec![],
            alias: None,
        },
    ];
    let result =
        build_multi_signal_timeline(&mut waveform, &signals, 0, 100, "union", "hex", Some(2))
            .unwrap();
    assert!(result.rows.len() <= 2);
}

#[test]
fn test_format_timeline_report() {
    let mut waveform = create_test_waveform();
    let signals = vec![SignalEntry {
        signal_path: Some("top.sig_a".to_string()),
        bit_mapping: vec![],
        alias: None,
    }];
    let result =
        build_multi_signal_timeline(&mut waveform, &signals, 0, 100, "union", "hex", None).unwrap();
    let report = format_timeline_report(&result);
    assert!(report.contains("Timeline"));
}
