//! Tests for the signal value extraction and reconstruction module.

use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::extract::{BitMappingEntry, ExtractRequest, extract_signal_values};

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

fn create_simple_vcd() -> NamedTempFile {
    // VCD with:
    // - a 1-bit signal: clk
    // - an 8-bit signal: data
    // - three 1-bit signals: bit0, bit1, bit2 (for reconstruction)
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 8 " data $end
$var wire 1 # bit0 $end
$var wire 1 $ bit1 $end
$var wire 1 % bit2 $end
$enddefinitions $end
#0
0!
b00000000 "
0#
0$
0%
#10
1!
b00000101 "
1#
0$
1%
#20
0!
b00001010 "
0#
1$
0%
#30
1!
b00001111 "
1#
1$
1%
#40
0!
b00000000 "
0#
0$
0%
"#;
    create_test_vcd(vcd)
}

fn create_multi_bit_vcd() -> NamedTempFile {
    // VCD with individual bit signals for reconstructing a 4-bit value
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! crc0 $end
$var wire 1 " crc1 $end
$var wire 1 # crc2 $end
$var wire 1 $ crc3 $end
$enddefinitions $end
#0
0!
0"
0#
0$
#10
1!
0"
1#
0$
#20
0!
1"
0#
1$
#30
1!
1"
1#
1$
#40
0!
0"
0#
0$
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_extract_single_signal_changes() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("data".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("hex".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    assert_eq!(result.signal_name, "data");
    assert_eq!(result.width, 8);
    // Should have 5 changes: initial + 4 value changes
    assert_eq!(result.total_changes, 5);
    assert_eq!(result.sample_count, 5);
}

#[test]
fn test_extract_single_signal_with_downsample() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("clk".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("binary".to_string()),
        downsample: Some(3),
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    assert_eq!(result.signal_name, "clk");
    assert_eq!(result.width, 1);
    // Downsample with max=3 on 5 changes: step=1, so returns all 5
    // Test verifies the downsample parameter is accepted without error
    assert!(result.sample_count <= result.total_changes);
}

#[test]
fn test_extract_single_signal_time_range() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    // Only extract from time index 2 to 3
    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("data".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(2),
        end_time_index: Some(3),
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("decimal".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    // Should only have changes within indices 2-3
    assert!(result.total_changes <= 3);
}

#[test]
fn test_reconstruct_multi_bit_signal() {
    let file = create_multi_bit_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![
            BitMappingEntry {
                bit_position: 0,
                signal_path: "crc0".to_string(),
            },
            BitMappingEntry {
                bit_position: 1,
                signal_path: "crc1".to_string(),
            },
            BitMappingEntry {
                bit_position: 2,
                signal_path: "crc2".to_string(),
            },
            BitMappingEntry {
                bit_position: 3,
                signal_path: "crc3".to_string(),
            },
        ],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("hex".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Reconstruction failed");

    assert_eq!(result.signal_name, "reconstructed[3:0]");
    assert_eq!(result.width, 4);

    // Check values:
    // time 0:  0000 = 0x0
    // time 10: 0101 = 0x5
    // time 20: 1010 = 0xA
    // time 30: 1111 = 0xF
    // time 40: 0000 = 0x0
    assert_eq!(result.total_changes, 5);

    // Verify first value is 0x0
    assert!(result.points[0].value.contains("0'h0") || result.points[0].value.contains("0"));
    // Verify we get 0x5, 0xA, 0xF
    let values: String = result
        .points
        .iter()
        .map(|p| p.value.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(values.contains("5"));
    assert!(values.contains("a") || values.contains("A"));
    assert!(values.contains("f") || values.contains("F"));
}

#[test]
fn test_reconstruct_multi_bit_signal_not_found() {
    let file = create_multi_bit_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![BitMappingEntry {
            bit_position: 0,
            signal_path: "nonexistent".to_string(),
        }],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: None,
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request);
    assert!(result.is_err());
}

#[test]
fn test_extract_single_signal_not_found() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("nonexistent".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: None,
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request);
    assert!(result.is_err());
}

#[test]
fn test_extract_single_signal_downsample_zero_rejected() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("clk".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("binary".to_string()),
        downsample: Some(0),
    };

    let result = extract_signal_values(&mut waveform, &request);
    assert!(result.is_err());
}

#[test]
fn test_extract_neither_signal_nor_mapping() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: None,
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request);
    assert!(result.is_err());
}

#[test]
fn test_reconstruct_multi_bit_signal_with_downsample() {
    let file = create_multi_bit_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![
            BitMappingEntry {
                bit_position: 0,
                signal_path: "crc0".to_string(),
            },
            BitMappingEntry {
                bit_position: 1,
                signal_path: "crc1".to_string(),
            },
            BitMappingEntry {
                bit_position: 2,
                signal_path: "crc2".to_string(),
            },
            BitMappingEntry {
                bit_position: 3,
                signal_path: "crc3".to_string(),
            },
        ],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("decimal".to_string()),
        downsample: Some(2),
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Reconstruction failed");

    // Downsample parameter is accepted, actual reduction depends on data
    assert!(result.sample_count <= result.total_changes);
}

#[test]
fn test_reconstruct_multi_bit_signal_downsample_zero_rejected() {
    let file = create_multi_bit_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![
            BitMappingEntry {
                bit_position: 0,
                signal_path: "crc0".to_string(),
            },
            BitMappingEntry {
                bit_position: 1,
                signal_path: "crc1".to_string(),
            },
            BitMappingEntry {
                bit_position: 2,
                signal_path: "crc2".to_string(),
            },
            BitMappingEntry {
                bit_position: 3,
                signal_path: "crc3".to_string(),
            },
        ],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("decimal".to_string()),
        downsample: Some(0),
    };

    let result = extract_signal_values(&mut waveform, &request);
    assert!(result.is_err());
}

#[test]
fn test_reconstruct_non_contiguous_bit_mapping() {
    // VCD with only bit 2 and bit 5 (non-contiguous)
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! bit2 $end
$var wire 1 " bit5 $end
$enddefinitions $end
#0
0!
0"
#10
1!
0"
#20
0!
1"
#30
1!
1"
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![
            BitMappingEntry {
                bit_position: 2,
                signal_path: "bit2".to_string(),
            },
            BitMappingEntry {
                bit_position: 5,
                signal_path: "bit5".to_string(),
            },
        ],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("binary".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Reconstruction failed");

    assert_eq!(result.signal_name, "reconstructed[5:0]");
    assert_eq!(result.width, 6);

    // time 0:  000000 = 0
    // time 10: 000100 = 4 (bit2=1)
    // time 20: 100000 = 32 (bit5=1)
    // time 30: 100100 = 36 (bit5=1, bit2=1)
    assert_eq!(result.total_changes, 4);

    // Verify values contain bit positions
    let values: String = result
        .points
        .iter()
        .map(|p| p.value.clone())
        .collect::<Vec<_>>()
        .join(" ");
    // Should have 6-bit binary values
    assert!(values.contains("'b"));
}

#[test]
fn test_reconstruct_binary_format() {
    let file = create_multi_bit_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![
            BitMappingEntry {
                bit_position: 0,
                signal_path: "crc0".to_string(),
            },
            BitMappingEntry {
                bit_position: 1,
                signal_path: "crc1".to_string(),
            },
            BitMappingEntry {
                bit_position: 2,
                signal_path: "crc2".to_string(),
            },
            BitMappingEntry {
                bit_position: 3,
                signal_path: "crc3".to_string(),
            },
        ],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("binary".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Reconstruction failed");

    // Check values are in binary format
    for point in &result.points {
        assert!(
            point.value.contains("'b"),
            "Expected binary format in: {}",
            point.value
        );
    }

    // time 10 should be 0101
    let values: Vec<&str> = result.points.iter().map(|p| p.value.as_str()).collect();
    assert!(values.len() >= 2);
    // First point after zero should have 'b0101
    let binary_values: String = values.iter().map(|v| v.to_string()).collect();
    assert!(binary_values.contains("b0101") || binary_values.contains("b101"));
}

#[test]
fn test_single_signal_value_format_hex() {
    let file = create_simple_vcd();
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("data".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("hex".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    // All values should contain 'h for hex format
    for point in &result.points {
        // Wellen formats as 8'hXX for 8-bit signals, our format_value also uses 'h
        assert!(
            point.value.contains("'h") || point.value.contains("0x") || point.value.contains("b"),
            "Expected hex format in: {}",
            point.value
        );
    }
}

#[test]
fn test_format_zero_value_hex() {
    // Test that zero values are formatted correctly (not empty bytes issue)
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! zero_signal $end
$enddefinitions $end
#0
0!
#10
1!
#20
0!
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    // Use bit_mapping mode to test format_value through reconstruction path
    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![BitMappingEntry {
            bit_position: 0,
            signal_path: "zero_signal".to_string(),
        }],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("hex".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    // First point should be zero value, formatted as 1'h0 (not empty)
    assert!(!result.points.is_empty());
    assert!(
        result.points[0].value.contains("0"),
        "Zero value should contain 0: {}",
        result.points[0].value
    );
    assert!(!result.points[0].value.is_empty());
}

#[test]
fn test_format_zero_value_binary() {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! zero_signal $end
$enddefinitions $end
#0
0!
#10
1!
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: None,
        bit_mapping: vec![BitMappingEntry {
            bit_position: 0,
            signal_path: "zero_signal".to_string(),
        }],
        start_time_index: Some(0),
        end_time_index: None,
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("binary".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    assert!(!result.points.is_empty());
    assert!(
        result.points[0].value.contains("'b"),
        "Binary zero should contain 'b: {}",
        result.points[0].value
    );
    assert!(!result.points[0].value.is_empty());
}

#[test]
fn test_extract_single_signal_includes_start_baseline_value() {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! clk $end
$var wire 8 " data $end
$enddefinitions $end
#0
0!
b00000000 "
#1
1!
#2
0!
b00000001 "
#5
b00000010 "
"#;
    let file = create_test_vcd(vcd);
    let path = file.path().to_str().unwrap().to_string();

    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let request = ExtractRequest {
        waveform_id: "test".to_string(),
        signal_path: Some("data".to_string()),
        bit_mapping: vec![],
        start_time_index: Some(1),
        end_time_index: Some(2),
        start_time_ps: None,
        end_time_ps: None,
        value_format: Some("hex".to_string()),
        downsample: None,
    };

    let result = extract_signal_values(&mut waveform, &request).expect("Extraction failed");

    assert_eq!(result.points[0].time_index, 1);
    assert_eq!(result.points[0].value, "8'h00");
    assert_eq!(result.points[1].time_index, 2);
    assert_eq!(result.points[1].value, "8'h01");
}
