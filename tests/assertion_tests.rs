//! Tests for assertion log parsing.

use wave_analyzer_mcp::assertion::{Severity, TimeUnit, parse_assertion_log};

// Standard two-line format transcript
const STANDARD_TRANSCRIPT: &str = r#"
# ** Error: (vsim-10142) TOP.tb_top.assert_data_transfer:
#    Time: 30 ns  Scope: tb_top File: tb/tb_top.sv Line: 42
# run -all
"#;

// Multiple events transcript
const MULTI_EVENT_TRANSCRIPT: &str = r#"
# ** Error: (vsim-10142) TOP.tb_top.assert_coeff_valid_latency:
#    Time: 1750 ns  Scope: tb_top File: tb/tb_top.sv Line: 55
# ** Warning: (vsim-10142) TOP.tb_top.assert_data_stable:
#    Time: 500 ns  Scope: tb_top File: tb/tb_top.sv Line: 30
# ** Note: TOP.tb_top.assert_reset_done:
#    Time: 100 ps  Scope: tb_top
"#;

// Short format transcript
const SHORT_FORMAT_TRANSCRIPT: &str = r#"
# ** Error: assert_xxx [30 ns] : TOP.tb_top
"#;

// Failure format (vsim-10143)
const FAILURE_TRANSCRIPT: &str = r#"
# ** Failure: (vsim-10143) TOP.tb_top.assert_xxx:
#    Time: 1750 ns  Scope: tb_top
"#;

#[test]
fn test_parse_standard_format() {
    let result = parse_assertion_log(STANDARD_TRANSCRIPT, &[], -1);

    assert_eq!(result.events.len(), 1);
    let event = &result.events[0];
    assert_eq!(event.assertion_name, "assert_data_transfer");
    assert_eq!(event.severity, Severity::Error);
    assert_eq!(event.time_value, 30);
    assert_eq!(event.time_unit, TimeUnit::Ns);
    assert_eq!(event.time_ps, 30000);
    assert_eq!(event.scope_path, "tb_top");
    assert_eq!(event.source_file, Some("tb/tb_top.sv".to_string()));
    assert_eq!(event.source_line, Some(42));
}

#[test]
fn test_parse_multi_event() {
    let result = parse_assertion_log(MULTI_EVENT_TRANSCRIPT, &[], -1);

    assert_eq!(result.events.len(), 3);

    assert_eq!(result.events[0].severity, Severity::Error);
    assert_eq!(result.events[0].time_ps, 1750000);

    assert_eq!(result.events[1].severity, Severity::Warning);
    assert_eq!(result.events[1].time_ps, 500000);

    assert_eq!(result.events[2].severity, Severity::Note);
    assert_eq!(result.events[2].time_ps, 100);
}

#[test]
fn test_severity_filter() {
    let result = parse_assertion_log(MULTI_EVENT_TRANSCRIPT, &[Severity::Error], -1);

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].severity, Severity::Error);
}

#[test]
fn test_time_unit_conversion() {
    let result = parse_assertion_log(MULTI_EVENT_TRANSCRIPT, &[], -1);

    // ns -> ps: 1750 * 1000 = 1750000
    assert_eq!(result.events[0].time_ps, 1750000);
    // ps -> ps: 100 * 1 = 100
    assert_eq!(result.events[2].time_ps, 100);
}

#[test]
fn test_parse_short_format() {
    let result = parse_assertion_log(SHORT_FORMAT_TRANSCRIPT, &[], -1);

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].assertion_name, "assert_xxx");
    assert_eq!(result.events[0].severity, Severity::Error);
    assert_eq!(result.events[0].time_value, 30);
    assert_eq!(result.events[0].time_unit, TimeUnit::Ns);
    assert_eq!(result.events[0].scope_path, "TOP.tb_top");
}

#[test]
fn test_parse_failure_format() {
    let result = parse_assertion_log(FAILURE_TRANSCRIPT, &[], -1);

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].severity, Severity::Failure);
    assert_eq!(result.events[0].time_value, 1750);
}

#[test]
fn test_limit() {
    let result = parse_assertion_log(MULTI_EVENT_TRANSCRIPT, &[], 2);
    assert_eq!(result.events.len(), 2);
}

#[test]
fn test_bad_format_tolerance() {
    let bad_content = r#"
# Some random line
# ** Error: something that doesn't match any pattern
# Another random line
"#;
    let result = parse_assertion_log(bad_content, &[], -1);
    assert_eq!(result.events.len(), 0);
}

#[test]
fn test_empty_transcript() {
    let result = parse_assertion_log("", &[], -1);
    assert_eq!(result.events.len(), 0);
    assert_eq!(result.unmatched_lines.len(), 0);
}
