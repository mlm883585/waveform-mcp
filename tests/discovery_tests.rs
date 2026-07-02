//! Tests for signal auto-discovery module.

use wave_analyzer_mcp::{auto_discover_signals, format_discovery_report};

const VCD: &str = r#"$date test $end
$version 1.0 $end
$timescale 1ns $end
$scope module top $end
$var reg 1 ! clk $end
$var reg 1 " rst_n $end
$var reg 1 # crc_0 $end
$var reg 1 $ crc_1 $end
$var reg 1 % crc_2 $end
$var reg 1 & crc_3 $end
$upscope $end
$enddefinitions $end
#0
0!
1"
0#
0$
0%
0&
#10
1!
1"
1#
0$
1%
0&
#20
0!
1"
0#
1$
0%
1&
#30
1!
1"
1#
1$
1%
1&
#40
0!
1"
0#
0$
0%
0&
#50
1!
1"
1#
0$
1%
0&
#60
0!
1"
0#
1$
0%
1&
#70
1!
1"
1#
1$
1%
1&
"#;

fn create_test_waveform() -> wellen::simple::Waveform {
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), VCD).unwrap();
    wellen::simple::read(temp.path()).unwrap()
}

#[test]
fn test_discover_bus_slices() {
    let mut waveform = create_test_waveform();
    let result = auto_discover_signals(&mut waveform, "bus_slices", None, None, None).unwrap();
    assert!(!result.bus_groups.is_empty());
}

#[test]
fn test_discover_clock_signals() {
    let mut waveform = create_test_waveform();
    let result = auto_discover_signals(&mut waveform, "clocks", None, None, None).unwrap();
    assert!(!result.clock_signals.is_empty());
}

#[test]
fn test_discover_reset_signals() {
    let mut waveform = create_test_waveform();
    let result = auto_discover_signals(&mut waveform, "all", None, None, None).unwrap();
    assert!(!result.reset_signals.is_empty());
}

#[test]
fn test_discover_with_scope_filter() {
    let mut waveform = create_test_waveform();
    let result = auto_discover_signals(&mut waveform, "all", Some("top"), None, None).unwrap();
    assert!(!result.clock_signals.is_empty() || !result.reset_signals.is_empty());
}

#[test]
fn test_discover_deep_dut_internal_bus_slices() {
    let vcd_content = "\
$date test $end
$version deep dut discovery test $end
$timescale 1ns $end
$scope module top $end
$scope module u_dut $end
$scope module u_crc $end
$var wire 1 ! crc_out_0 $end
$var wire 1 \" crc_out_1 $end
$upscope $end
$upscope $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
0\"
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let result = auto_discover_signals(&mut waveform, "bus_slices", None, None, None).unwrap();

    assert!(result.bus_groups.iter().any(|group| {
        group.name == "crc_out"
            && group.width == 2
            && group
                .signals
                .iter()
                .any(|signal| signal.path == "top.u_dut.u_crc.crc_out_0")
    }));
}

#[test]
fn test_format_discovery_report() {
    let mut waveform = create_test_waveform();
    let result = auto_discover_signals(&mut waveform, "groups", None, None, None).unwrap();
    let report = format_discovery_report(&result);
    assert!(report.contains("Signal Discovery Report"));
    assert!(report.contains("Bus Groups"));
    assert!(report.contains("Clock Signals"));
    assert!(report.contains("Reset Signals"));
}

#[test]
fn test_discover_clock_signals_deduplicates_aliases() {
    let vcd_content = "\
$date test $end
$version alias clock test $end
$timescale 1ns $end
$scope module top $end
$var reg 1 ! clk $end
$var reg 1 ! clock_alias $end
$var reg 1 \" rst_n $end
$upscope $end
$enddefinitions $end
#0
0!
1\"
#10
1!
1\"
#20
0!
1\"
#30
1!
1\"
#40
0!
1\"
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let result = auto_discover_signals(&mut waveform, "clocks", None, None, None).unwrap();

    assert_eq!(result.clock_signals.len(), 1);
    assert_eq!(result.clock_signals[0], "top.clk");
}

#[test]
fn test_discover_clock_signals_deduplicates_hierarchical_mirrors() {
    let vcd_content = "\
$date test $end
$version mirrored clock test $end
$timescale 1ns $end
$scope module top $end
$var reg 1 ! clk $end
$scope module u_dut $end
$var wire 1 \" clk $end
$upscope $end
$upscope $end
$enddefinitions $end
#0
0!
0\"
#10
1!
1\"
#20
0!
0\"
#30
1!
1\"
#40
0!
0\"
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let result = auto_discover_signals(&mut waveform, "clocks", None, None, None).unwrap();

    assert_eq!(result.clock_signals, vec!["top.clk".to_string()]);
}
