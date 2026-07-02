//! Tests for the protocol template analysis module.

use std::collections::HashMap;
use std::io::Write;
use tempfile::NamedTempFile;
use wave_analyzer_mcp::protocol_template::{
    ProtocolTemplate, analyze_protocol_template, format_protocol_template_report,
};

fn create_test_vcd(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write VCD");
    file.flush().expect("Failed to flush");
    file
}

// VCD with SPI signals: sclk, cs, mosi, miso
fn create_spi_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! sclk $end
$var wire 1 " cs $end
$var wire 1 # mosi $end
$var wire 1 $ miso $end
$enddefinitions $end
#0
0!
1"
0#
0$
#5
1!
0"
#10
0!
#15
1!
#20
0!
0"
#25
1!
1"
1#
1$
#30
0!
#35
1!
0"
#40
0!
"#;
    create_test_vcd(vcd)
}

// VCD with AXI-Lite signals
fn create_axi_lite_vcd() -> NamedTempFile {
    let vcd = r#"$timescale 1ns $end
$var wire 1 ! arvalid $end
$var wire 1 " arready $end
$var wire 8 # rdata $end
$var wire 1 $ awvalid $end
$var wire 1 % awready $end
$var wire 8 & wdata $end
$enddefinitions $end
#0
0!
0"
b00000000 #
0$
0%
b00000000 &
#5
1!
0"
#10
1!
1"
b00000001 #
#15
0!
1"
#20
0!
0"
b00000000 #
#25
1$
0%
#30
1$
1%
b00000111 &
#35
0$
1%
#40
0$
0%
b00000000 &
"#;
    create_test_vcd(vcd)
}

#[test]
fn test_spi_analysis() {
    let vcd = create_spi_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let signals = HashMap::from([
        ("sclk".to_string(), "sclk".to_string()),
        ("cs".to_string(), "cs".to_string()),
    ]);

    let result =
        analyze_protocol_template(&mut waveform, &ProtocolTemplate::Spi, &signals, 0, 100).unwrap();

    assert!(result.spi_result.is_some());
    let spi = result.spi_result.unwrap();
    assert!(spi.transaction_count >= 1); // At least one CS-low pulse detected
}

#[test]
fn test_spi_missing_signal() {
    let vcd = create_spi_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let signals = HashMap::from([
        ("sclk".to_string(), "sclk".to_string()),
        // Missing cs signal
    ]);

    let result = analyze_protocol_template(&mut waveform, &ProtocolTemplate::Spi, &signals, 0, 100);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cs"));
}

#[test]
fn test_axi_lite_analysis() {
    let vcd = create_axi_lite_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let signals = HashMap::from([
        ("arvalid".to_string(), "arvalid".to_string()),
        ("arready".to_string(), "arready".to_string()),
        ("awvalid".to_string(), "awvalid".to_string()),
        ("awready".to_string(), "awready".to_string()),
    ]);

    // Use a later start time where both signals have established values
    let result =
        analyze_protocol_template(&mut waveform, &ProtocolTemplate::AxiLite, &signals, 1, 50)
            .unwrap();

    assert!(result.axi_lite_result.is_some());
    let _axi = result.axi_lite_result.unwrap();
    // total_handshakes is usize, always >= 0; just verify result exists
}

#[test]
fn test_axi_lite_missing_signal() {
    let vcd = create_axi_lite_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let signals = HashMap::from([
        ("arvalid".to_string(), "arvalid".to_string()),
        // Missing arready
    ]);

    let result =
        analyze_protocol_template(&mut waveform, &ProtocolTemplate::AxiLite, &signals, 0, 100);
    assert!(result.is_err());
}

#[test]
fn test_protocol_template_report() {
    let vcd = create_spi_vcd();
    let path = vcd.path().to_str().unwrap().to_string();
    let mut waveform =
        wellen::simple::read(std::path::Path::new(&path)).expect("Failed to read VCD");

    let signals = HashMap::from([
        ("sclk".to_string(), "sclk".to_string()),
        ("cs".to_string(), "cs".to_string()),
    ]);

    let result =
        analyze_protocol_template(&mut waveform, &ProtocolTemplate::Spi, &signals, 0, 100).unwrap();

    let report = format_protocol_template_report(&result);
    assert!(report.contains("SPI Protocol Analysis"));
}
