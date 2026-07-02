//! Condition tests
//!
//! Note: find_conditional_events now reports only state-change entry points
//! (false→true transitions), not every time point where the condition is true.

use wave_analyzer_mcp::find_conditional_events;
use wave_analyzer_mcp::parse_condition;

#[test]
fn test_find_conditional_events_lib() {
    // VCD timeline:
    // idx 0: clk=0, valid=0, ready=0
    // idx 1: clk=1, valid=0, ready=0
    // idx 2: clk=1, valid=1, ready=0
    // idx 3: clk=0, valid=1, ready=0
    // idx 4: clk=0, valid=1, ready=1
    // idx 5: clk=0, valid=0, ready=1
    // idx 6: clk=0, valid=0, ready=0
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 clk $end\n\
$var wire 1 1 valid $end\n\
$var wire 1 2 ready $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
01\n\
02\n\
#10\n\
10\n\
01\n\
02\n\
#20\n\
10\n\
11\n\
02\n\
#30\n\
00\n\
11\n\
02\n\
#40\n\
00\n\
11\n\
12\n\
#50\n\
00\n\
01\n\
12\n\
#60\n\
00\n\
01\n\
02\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // AND condition: valid && ready transitions from false→true only at idx 4
    let events = find_conditional_events(&mut waveform, "top.valid && top.ready", 0, 6, -1)
        .expect("Should find events for AND condition");
    assert!(!events.is_empty(), "Should find at least one event");
    assert!(
        events[0].contains("top.valid = 1'b1"),
        "Should show valid as 1"
    );
    assert!(
        events[0].contains("top.ready = 1'b1"),
        "Should show ready as 1"
    );
    assert!(
        events[0].contains("Time index 4 (40ns)"),
        "Event should be at time index 4 (40ns)"
    );
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition for AND condition"
    );

    // OR condition: valid||ready transitions from false→true at idx 2
    // (idx 0,1 false; idx 2 becomes true; stays true until idx 6)
    let events = find_conditional_events(&mut waveform, "top.valid || top.ready", 0, 6, -1)
        .expect("Should find events for OR condition");
    assert_eq!(events.len(), 1, "Should find 1 transition for OR condition");
    assert!(
        events[0].contains("Time index 2 (20ns)"),
        "Transition at time 2"
    );

    // NOT condition: !clk transitions from false→true at idx 0 (initial true) and idx 3
    // idx 0: !0=true (prev was false, initial→true), idx 1: !1=false, idx 3: !0=true (prev false)
    let events = find_conditional_events(&mut waveform, "!top.clk", 0, 6, -1)
        .expect("Should find events for NOT condition");
    assert_eq!(events.len(), 2, "Should find 2 transitions for !clk");
    assert!(
        events[0].contains("Time index 0 (0ns)"),
        "First transition at time 0"
    );
    assert!(
        events[1].contains("Time index 3 (30ns)"),
        "Second transition at time 3"
    );

    // Complex condition with parentheses: clk && (valid || ready)
    // idx 0: false, idx 1: clk=1 && 0=false, idx 2: clk=1 && valid=true → transition!
    // After idx 2, stays true only at idx 2. At idx 3: clk=0 → false. No more transitions.
    let events = find_conditional_events(
        &mut waveform,
        "top.clk && (top.valid || top.ready)",
        0,
        6,
        -1,
    )
    .expect("Should find events for complex condition");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition for complex condition"
    );
    assert!(
        events[0].contains("Time index 2 (20ns)"),
        "Transition at time 2"
    );

    // Test limit
    let events = find_conditional_events(&mut waveform, "!top.clk", 0, 6, 1)
        .expect("Should find events with limit");
    assert_eq!(events.len(), 1, "Should limit to 1 event");

    // Test time range
    let events = find_conditional_events(&mut waveform, "top.valid && top.ready", 3, 5, -1)
        .expect("Should find events in time range");
    assert!(!events.is_empty(), "Should find events in specified range");
    assert!(
        events[0].contains("Time index 4 (40ns)"),
        "Event should be at time index 4 (40ns)"
    );
}

#[test]
fn test_conditional_events_timestamps() {
    // counter: 0, 1, 2, 3, 4, 5, 6
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b0001 !\n\
#20\n\
b0010 !\n\
#30\n\
b0011 !\n\
#40\n\
b0100 !\n\
#50\n\
b0101 !\n\
#60\n\
b0110 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // counter == 2: single transition at idx 2 (prev was false at idx 1 where counter=1)
    let events = find_conditional_events(&mut waveform, "top.counter == 4'b0010", 0, 6, -1)
        .expect("Should find events");
    assert_eq!(events.len(), 1, "Should find exactly 1 transition");
    assert!(
        events[0].contains("Time index 2 (20ns)"),
        "Event should be at time 2 (20ns)"
    );

    // counter == 5: single transition at idx 5
    let events = find_conditional_events(&mut waveform, "top.counter == 4'b0101", 0, 6, -1)
        .expect("Should find events");
    assert_eq!(events.len(), 1, "Should find exactly 1 transition");
    assert!(
        events[0].contains("Time index 5 (50ns)"),
        "Event should be at time 5 (50ns)"
    );

    // counter != 0: transitions from false→true at idx 1 (counter goes from 0 to 1)
    let events = find_conditional_events(&mut waveform, "top.counter != 4'b0000", 0, 6, -1)
        .expect("Should find events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter != 0"
    );
    assert!(
        events[0].contains("Time index 1 (10ns)"),
        "Transition at time 1"
    );
}

#[test]
fn test_parse_condition() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig1 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let result = find_conditional_events(&mut waveform, "nonexistent.signal", 0, 0, -1);
    assert!(result.is_err(), "Should fail for nonexistent signal");

    let result = find_conditional_events(&mut waveform, "(top.sig1 && top.sig1", 0, 0, -1);
    assert!(result.is_err(), "Should fail for invalid syntax");
}

#[test]
fn test_comparison_operators() {
    // counter: 0, 1, 2, 3, 4, 5, 6
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b0001 !\n\
#20\n\
b0010 !\n\
#30\n\
b0011 !\n\
#40\n\
b0100 !\n\
#50\n\
b0101 !\n\
#60\n\
b0110 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // counter == 5: transition at idx 5 (prev false at idx 4)
    let events = find_conditional_events(&mut waveform, "top.counter == 4'b0101", 0, 6, -1)
        .expect("Should find events for binary literal comparison");
    assert!(!events.is_empty(), "Should find at least one event");
    assert!(
        events[0].contains("top.counter = 4'b0101"),
        "Should show counter value 5 {}",
        events[0]
    );

    // counter == 3: transition at idx 3
    let events = find_conditional_events(&mut waveform, "top.counter == 3'd3", 0, 6, -1)
        .expect("Should find events for decimal literal comparison");
    assert!(!events.is_empty(), "Should find at least one event");

    // counter == 6: transition at idx 6
    let events = find_conditional_events(&mut waveform, "top.counter == 4'h6", 0, 6, -1)
        .expect("Should find events for hex literal comparison");
    assert!(!events.is_empty(), "Should find at least one event");

    // counter != 0: transition at idx 1
    let events = find_conditional_events(&mut waveform, "top.counter != 4'b0000", 0, 6, -1)
        .expect("Should find events for inequality comparison");
    assert!(!events.is_empty(), "Should find at least one event");

    // counter == 5 || counter == 3: transitions at idx 3 and idx 5
    // At idx 2: false, idx 3: true → transition. idx 4: false, idx 5: true → transition
    let events = find_conditional_events(
        &mut waveform,
        "top.counter == 4'b0101 || top.counter == 4'b0011",
        0,
        6,
        -1,
    )
    .expect("Should find events for complex comparison");
    assert_eq!(
        events.len(),
        2,
        "Should find 2 transitions matching either condition"
    );
}

#[test]
fn test_verilog_literal_parsing() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig1 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let result = find_conditional_events(&mut waveform, "top.sig1 == invalid", 0, 0, -1);
    assert!(result.is_err(), "Should fail for invalid literal format");

    let result = find_conditional_events(&mut waveform, "top.sig1 = 1", 0, 0, -1);
    assert!(result.is_err(), "Should fail for single =");
}

#[test]
fn test_past_function() {
    // signal: 0→1→0→1→0
    // Rising edges at idx 1, 3. Falling edges at idx 2, 4.
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 signal $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n\
#20\n\
00\n\
#30\n\
10\n\
#40\n\
00\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // Rising edge: !$past(signal) && signal
    // idx 0: !$past(0=false/no_past) && 0 = false, idx 1: !0 && 1 = true → transition!
    // After idx 1 stays true? No - at idx 2: !$past(1) && 0 = !1&&0 = false. So only one transition.
    // Wait - this condition is a "pulse" check, not a sustained condition.
    // idx 0: false, idx 1: true (transition!), idx 2: false, idx 3: true (transition!), idx 4: false
    let events =
        find_conditional_events(&mut waveform, "!$past(top.signal) && top.signal", 0, 4, -1)
            .expect("Should find events for rising edge");
    assert_eq!(events.len(), 2, "Should find 2 rising edge transitions");
    assert!(
        events[0].contains("Time index 1 (10ns)"),
        "First rising edge at time 1"
    );
    assert!(
        events[1].contains("Time index 3 (30ns)"),
        "Second rising edge at time 3"
    );

    // Falling edge: $past(signal) && !signal
    // idx 0: false, idx 1: 0&&!1=false, idx 2: 1&&!0=true → transition!, idx 3: 0&&!1=false, idx 4: 1&&!0=true → transition!
    let events =
        find_conditional_events(&mut waveform, "$past(top.signal) && !top.signal", 0, 4, -1)
            .expect("Should find events for falling edge");
    assert_eq!(events.len(), 2, "Should find 2 falling edge transitions");
    assert!(
        events[0].contains("Time index 2 (20ns)"),
        "First falling edge at time 2"
    );
    assert!(
        events[1].contains("Time index 4 (40ns)"),
        "Second falling edge at time 4"
    );

    // $past(signal) || signal: sustained condition with transitions
    // idx 0: 0||0=0=false, idx 1: 0||1=1=true (transition!), idx 2: 1||0=1=true (stays true),
    // idx 3: 0||1=1=true (stays true), idx 4: 1||0=1=true (stays true)
    // Only 1 transition at idx 1
    let events =
        find_conditional_events(&mut waveform, "$past(top.signal) || top.signal", 0, 4, -1)
            .expect("Should find events for OR with $past");
    assert_eq!(events.len(), 1, "Should find 1 transition");
}

#[test]
fn test_past_edge_case() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 signal $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // $past at idx 0 = false (no past). At idx 1: past=0=false.
    let events = find_conditional_events(&mut waveform, "$past(top.signal)", 0, 1, -1)
        .expect("Should handle $past at time 0");
    assert_eq!(events.len(), 0, "Should find 0 events");
}

#[test]
fn test_past_with_and_expression() {
    // idx 0: sig1=0, sig2=1 → $past(0&&1) at idx 1 = false (past of idx 0 = 0&&1=false)
    // idx 1: sig1=1, sig2=1 → $past(1&&1) at idx 2 = true (past of idx 1 = 1&&1=true)
    // idx 2: sig1=0, sig2=1 → $past(0&&1) at idx 3 = false (past of idx 2 = 0&&1=false)
    // idx 3: sig1=1, sig2=1 → $past(1&&1) at idx 4 = true (past of idx 3 = 1&&1=true)
    // So $past(sig1&&sig2) transitions: idx 0=false, idx 1=false, idx 2=true → transition!, idx 3=false, idx 4=true → transition!
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 signal1 $end\n\
$var wire 1 1 signal2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
11\n\
#10\n\
10\n\
#20\n\
00\n\
#30\n\
10\n\
11\n\
#40\n\
00\n\
01\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events =
        find_conditional_events(&mut waveform, "$past(top.signal1 && top.signal2)", 0, 4, -1)
            .expect("Should find events for $past with AND expression");
    assert_eq!(events.len(), 2, "Should find 2 transitions");
    assert!(
        events[0].contains("Time index 2 (20ns)"),
        "First transition at time 2"
    );
    assert!(
        events[1].contains("Time index 4 (40ns)"),
        "Second transition at time 4"
    );
}

#[test]
fn test_nested_past() {
    // signal: 0→1→0→1→0→1
    // $past($past(signal)):
    // idx 0: 0 (no past), idx 1: 0 (past=0), idx 2: 0 ($past at idx1=0),
    // idx 3: 1 ($past at idx2=1), idx 4: 0, idx 5: 1
    // Transitions: idx 3 (false→true), idx 5 (false→true)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 signal $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n\
#20\n\
00\n\
#30\n\
10\n\
#40\n\
00\n\
#50\n\
10\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "$past($past(top.signal))", 0, 5, -1)
        .expect("Should find events for nested $past");
    assert_eq!(events.len(), 2, "Should find 2 transitions");
    assert!(
        events[0].contains("Time index 3 (30ns)"),
        "First transition at time 3"
    );
    assert!(
        events[1].contains("Time index 5 (50ns)"),
        "Second transition at time 5"
    );
}

#[test]
fn test_bitwise_and() {
    // signal1: 5, 3, 10, 6; signal2: 3, 6, 5, 4
    // Bitwise AND results: 1, 2, 0, 4
    // Transitions (non-zero): idx 0 (false→true, initial), idx 2→idx 3 (0→4=true)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! signal1 $end\n\
$var wire 4 0 signal2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
b0011 0\n\
#10\n\
b0011 !\n\
b0110 0\n\
#20\n\
b1010 !\n\
b0101 0\n\
#30\n\
b0110 !\n\
b0100 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.signal1 & top.signal2", 0, 3, -1)
        .expect("Should find events for bitwise AND");
    // idx 0: non-zero (initial true → transition), idx 1: non-zero (stays true),
    // idx 2: zero (false), idx 3: non-zero (false→true → transition)
    assert_eq!(
        events.len(),
        2,
        "Should find 2 transitions where bitwise AND is non-zero"
    );
    assert!(
        events[0].contains("Time index 0 (0ns)"),
        "First transition at time 0"
    );
    assert!(
        events[1].contains("Time index 3 (30ns)"),
        "Second transition at time 3"
    );
}

#[test]
fn test_bitwise_or() {
    // signal1: 5, 2; signal2: 3, 4
    // OR: 7, 6 → both non-zero
    // idx 0: true (transition!), idx 1: stays true → only 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! signal1 $end\n\
$var wire 4 0 signal2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
b0011 0\n\
#10\n\
b0010 !\n\
b0100 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.signal1 | top.signal2", 0, 1, -1)
        .expect("Should find events for bitwise OR");
    assert_eq!(events.len(), 1, "Should find 1 transition (initial true)");
}

#[test]
fn test_bitwise_xor() {
    // XOR: 6, 3, 15 → all non-zero
    // idx 0: true (transition!), stays true → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! signal1 $end\n\
$var wire 4 0 signal2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
b0011 0\n\
#10\n\
b0110 !\n\
b0101 0\n\
#20\n\
b1111 !\n\
b0000 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.signal1 ^ top.signal2", 0, 2, -1)
        .expect("Should find events for bitwise XOR");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition (initial true, stays true)"
    );
}

#[test]
fn test_bitwise_mixed_operations() {
    // Both time indices have non-zero result for (signal1 & signal2) | signal3
    // idx 0: true (transition!), idx 1: stays true → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! signal1 $end\n\
$var wire 4 0 signal2 $end\n\
$var wire 4 1 signal3 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
b0011 0\n\
b0110 1\n\
#10\n\
b0011 !\n\
b0110 0\n\
b0100 1\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(
        &mut waveform,
        "(top.signal1 & top.signal2) | top.signal3",
        0,
        1,
        -1,
    )
    .expect("Should find events for mixed bitwise operations");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition for mixed bitwise operations"
    );
}

#[test]
fn test_bitwise_with_logical_operations() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig1 $end\n\
$var wire 1 1 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
11\n\
#10\n\
01\n\
10\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events =
        find_conditional_events(&mut waveform, "top.sig1 & top.sig2 && top.sig2", 0, 1, -1)
            .expect("Should find events for bitwise and logical operations");
    assert_eq!(events.len(), 0, "Should find 0 events");
}

#[test]
fn test_bitwise_zero_result() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig1 $end\n\
$var wire 1 1 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
10\n\
01\n\
#10\n\
00\n\
00\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.sig1 & top.sig2", 0, 1, -1)
        .expect("Should find events for bitwise AND");
    assert_eq!(
        events.len(),
        0,
        "Should find 0 events where bitwise AND is zero"
    );
}

#[test]
fn test_single_bit_extraction() {
    // counter: 5(0101), 3(0011), 10(1010), 6(0110)
    // counter[0]: 1, 1, 0, 0
    // counter[0] == 1 transitions: idx 0 true (transition!), idx 1 stays true
    // Then idx 2 false, idx 3 false → only 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
#10\n\
b0011 !\n\
#20\n\
b1010 !\n\
#30\n\
b0110 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.counter[0] == 1'b1", 0, 3, -1)
        .expect("Should find events");
    assert_eq!(events.len(), 1, "Should find 1 transition where LSB is 1");
    assert!(
        events[0].contains("Time index 0 (0ns)"),
        "Transition at time 0"
    );
}

#[test]
fn test_bit_range_extraction() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 8 ! data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b10101010 !\n\
#10\n\
b11001100 !\n\
#20\n\
b11110000 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // data[7:4] == 0b1010: true only at idx 0 → 1 transition
    let events = find_conditional_events(&mut waveform, "top.data[7:4] == 4'b1010", 0, 2, -1)
        .expect("Should find events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where upper nibble is 0b1010"
    );
    assert!(events[0].contains("Time index 0 (0ns)"), "Event at time 0");

    // data[3:0] == 0b1100: true only at idx 1 → 1 transition
    let events = find_conditional_events(&mut waveform, "top.data[3:0] == 4'b1100", 0, 2, -1)
        .expect("Should find events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where lower nibble is 0b1100"
    );
    assert!(events[0].contains("Time index 1 (10ns)"), "Event at time 1");
}

#[test]
fn test_bit_extraction_with_bitwise() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 8 ! sig1 $end\n\
$var wire 8 0 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b10101010 !\n\
b11111111 0\n\
#10\n\
b11001100 !\n\
b00000000 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // sig1[3:0] & sig2[3:0]: idx 0: 10&15=10 (true→transition), idx 1: 12&0=0 (false)
    let events = find_conditional_events(&mut waveform, "top.sig1[3:0] & top.sig2[3:0]", 0, 1, -1)
        .expect("Should find events");
    assert_eq!(events.len(), 1, "Should find 1 transition");
    assert!(events[0].contains("Time index 0 (0ns)"), "Event at time 0");
}

#[test]
fn test_bit_extraction_equals_zero() {
    // counter: 5(0101), 0(0000), 8(1000)
    // counter[2]: 1, 0, 0
    // counter[2] == 0 transitions: idx 0 false, idx 1 true (transition!), idx 2 stays true → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
#10\n\
b0000 !\n\
#20\n\
b1000 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.counter[2] == 1'b0", 0, 2, -1)
        .expect("Should find events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where extracted bit equals 0"
    );
    assert!(
        events[0].contains("Time index 1 (10ns)"),
        "Transition at time 1"
    );
}

#[test]
fn test_bitwise_not() {
    // ~4'b0101 = 10 (non-zero), ~4'b0000 = 15 (non-zero), ~4'b1010 = 5 (non-zero)
    // All non-zero → true at idx 0 (transition!), stays true → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! sig1 $end\n\
$var wire 4 0 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
b0011 0\n\
#10\n\
b0000 !\n\
b1111 0\n\
#20\n\
b1010 !\n\
b1100 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "~4'b0101", 0, 2, -1)
        .expect("Should find events for bitwise NOT");
    // ~5 = 10 always, independent of VCD → constant true → 1 transition at idx 0
    assert_eq!(events.len(), 1, "Should find 1 transition (constant true)");
    assert!(
        events[0].contains("Time index 0 (0ns)"),
        "Transition at time 0"
    );
}

#[test]
fn test_bitwise_not_with_signal() {
    // data: 0, 15, 10 → ~data: 15, 0, 5
    // Transitions: idx 0 true (transition!), idx 1 false, idx 2 true (transition!)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b1111 !\n\
#20\n\
b1010 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "~top.data", 0, 2, -1)
        .expect("Should find events for bitwise NOT on signal");
    assert_eq!(
        events.len(),
        2,
        "Should find 2 transitions for ~data non-zero"
    );
    assert!(
        events[0].contains("Time index 0 (0ns)"),
        "First transition at time 0"
    );
    assert!(
        events[1].contains("Time index 2 (20ns)"),
        "Second transition at time 2"
    );
}

#[test]
fn test_bitwise_not_with_bit_extract() {
    // ~data[7:0]: always non-zero for both idx → 1 transition (initial true, stays true)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 8 ! data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b00000001 !\n\
#10\n\
b00000010 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "~top.data[7:0]", 0, 1, -1)
        .expect("Should find events for bitwise NOT on bit extraction");
    assert_eq!(events.len(), 1, "Should find 1 transition");
}

#[test]
fn test_bitwise_not_with_bitwise_ops() {
    // ~data & data: always 0 → no events
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! data $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0101 !\n\
#10\n\
b1010 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "~top.data & top.data", 0, 1, -1)
        .expect("Should find events for bitwise NOT and AND");
    assert_eq!(
        events.len(),
        0,
        "Should find 0 events where (~data & data) is zero"
    );
}

#[test]
fn test_magnitude_comparison_lt() {
    // counter: 0, 1, 2, 3, 4, 5, 6
    // counter < 3: true at idx 0, 1, 2; false at idx 3+
    // Transition: idx 0 (initial true → transition), stays true until idx 3 → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b0001 !\n\
#20\n\
b0010 !\n\
#30\n\
b0011 !\n\
#40\n\
b0100 !\n\
#50\n\
b0101 !\n\
#60\n\
b0110 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.counter < 4'd3", 0, 6, -1)
        .expect("Should find events for < comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition (counter<3 stays true from idx 0-2)"
    );
    assert!(events[0].contains("Time index 0"), "Transition at idx 0");
}

#[test]
fn test_magnitude_comparison_le_ge_gt() {
    // counter: 0, 3, 5, 8
    // counter <= 3: true at idx 0, 1 → 1 transition at idx 0
    // counter >= 5: true at idx 2, 3 → 1 transition at idx 2
    // counter > 3: true at idx 2, 3 → 1 transition at idx 2
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b0011 !\n\
#20\n\
b0101 !\n\
#30\n\
b1000 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.counter <= 4'd3", 0, 3, -1)
        .expect("Should find events for <= comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter <= 3"
    );

    let events = find_conditional_events(&mut waveform, "top.counter >= 4'd5", 0, 3, -1)
        .expect("Should find events for >= comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter >= 5"
    );

    let events = find_conditional_events(&mut waveform, "top.counter > 4'd3", 0, 3, -1)
        .expect("Should find events for > comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter > 3"
    );
}

#[test]
fn test_arithmetic_add_sub() {
    // sig1: 2, 5; sig2: 3, 1
    // sig1+sig2==5: true at idx 0 → 1 transition
    // sig1-sig2==4: true at idx 1 → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! sig1 $end\n\
$var wire 4 0 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0010 !\n\
b0011 0\n\
#10\n\
b0101 !\n\
b0001 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.sig1 + top.sig2 == 4'd5", 0, 1, -1)
        .expect("Should find events for add comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where sig1+sig2==5"
    );

    let events = find_conditional_events(&mut waveform, "top.sig1 - top.sig2 == 4'd4", 0, 1, -1)
        .expect("Should find events for sub comparison");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where sig1-sig2==4"
    );
}

#[test]
fn test_arithmetic_sub_underflow() {
    // 1-3 underflow → 0, 0==0 → true at idx 0 → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! sig1 $end\n\
$var wire 4 0 sig2 $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0001 !\n\
b0011 0\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "top.sig1 - top.sig2 == 4'd0", 0, 0, -1)
        .expect("Should handle underflow");
    assert_eq!(events.len(), 1, "Underflow should result in 0");
}

#[test]
fn test_rose_fell_stable() {
    // signal: 0→1→0→1→0
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 signal $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
00\n\
#10\n\
10\n\
#20\n\
00\n\
#30\n\
10\n\
#40\n\
00\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    // $rose(signal): idx 0=0(no past), idx 1=1(0→1), idx 2=0, idx 3=1(0→1), idx 4=0
    // Transitions: idx 1 (false→true), idx 3 (false→true)
    let events = find_conditional_events(&mut waveform, "$rose(top.signal)", 0, 4, -1)
        .expect("Should find $rose events");
    assert_eq!(events.len(), 2, "Should find 2 rising edges");
    assert!(events[0].contains("Time index 1"), "First rising at idx 1");
    assert!(events[1].contains("Time index 3"), "Second rising at idx 3");

    // $fell(signal): idx 0=0, idx 1=0(0→1=false), idx 2=1(1→0=true), idx 3=0, idx 4=1(1→0=true)
    // Transitions: idx 2, idx 4
    let events = find_conditional_events(&mut waveform, "$fell(top.signal)", 0, 4, -1)
        .expect("Should find $fell events");
    assert_eq!(events.len(), 2, "Should find 2 falling edges");
    assert!(events[0].contains("Time index 2"), "First falling at idx 2");
    assert!(
        events[1].contains("Time index 4"),
        "Second falling at idx 4"
    );

    // $stable(signal): idx 0=0(no past), idx 1=0(0≠1=false), idx 2=0(1≠0=false), etc. → 0 events
    let events = find_conditional_events(&mut waveform, "$stable(top.signal)", 0, 4, -1)
        .expect("Should find $stable events");
    assert_eq!(events.len(), 0, "No stable periods in toggling signal");
}

#[test]
fn test_stable_with_constant_signal() {
    // sig stays at 1 for all time indices
    // $stable(sig): idx 0=0(no past), idx 1=1(1=1=true → transition!), idx 2=1(stays true)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
10\n\
#10\n\
10\n\
#20\n\
10\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "$stable(top.sig)", 0, 2, -1)
        .expect("Should find $stable events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition (at idx 1, past=1 matches current=1)"
    );
}

#[test]
fn test_pastn_multi_cycle_lookback() {
    // counter: 0, 1, 2, 3, 4, 5, 6
    // $past(counter, 2) == 1: true at idx 3 (counter@1=1)
    // Transitions: idx 0 false (past returns 0, 0≠1), ..., idx 3 true → 1 transition
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! counter $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0000 !\n\
#10\n\
b0001 !\n\
#20\n\
b0010 !\n\
#30\n\
b0011 !\n\
#40\n\
b0100 !\n\
#50\n\
b0101 !\n\
#60\n\
b0110 !\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "$past(top.counter, 2) == 4'd1", 3, 6, -1)
        .expect("Should find $pastN events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter 2 cycles back is 1"
    );
    assert!(events[0].contains("Time index 3"), "Transition at idx 3");

    let events = find_conditional_events(&mut waveform, "$past(top.counter, 3) == 4'd0", 3, 6, -1)
        .expect("Should find $pastN events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where counter 3 cycles back is 0"
    );
}

#[test]
fn test_sva_at_time_zero() {
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 0 sig $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
10\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(&mut waveform, "$rose(top.sig)", 0, 0, -1)
        .expect("Should handle $rose at time 0");
    assert_eq!(events.len(), 0, "No rose event at time 0");

    let events = find_conditional_events(&mut waveform, "$past(top.sig, 1) == 1'b1", 0, 0, -1)
        .expect("Should handle $pastN at time 0");
    assert_eq!(events.len(), 0, "No $pastN event at time 0");
}

#[test]
fn test_parse_new_operators() {
    assert!(parse_condition("top.counter >= 4'd10").is_ok());
    assert!(parse_condition("top.counter <= 4'd5").is_ok());
    assert!(parse_condition("top.counter > 4'd0").is_ok());
    assert!(parse_condition("top.counter < 4'd8").is_ok());
    assert!(parse_condition("top.addr + 4'd4 == top.target").is_ok());
    assert!(parse_condition("top.count - 8'd1 >= 8'd0").is_ok());
    assert!(parse_condition("$rose(top.valid)").is_ok());
    assert!(parse_condition("$fell(top.ready)").is_ok());
    assert!(parse_condition("$stable(top.clk)").is_ok());
    assert!(parse_condition("$past(top.signal, 2)").is_ok());
    assert!(parse_condition("$past(top.signal, 5)").is_ok());
    assert!(parse_condition("top.addr + 4'd4 >= top.threshold && $rose(top.valid)").is_ok());
    assert!(parse_condition("top.counter > 4'd3 || top.counter < 4'd1").is_ok());
}

#[test]
fn test_comparison_with_arithmetic() {
    // addr=4, offset=3, threshold=7 → addr+offset=7 >= 7 → true
    // Only 1 idx → 1 transition (initial true)
    let vcd_content = "\
$date 2024-01-01 $end\n\
$version Test VCD file $end\n\
$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 4 ! addr $end\n\
$var wire 4 0 offset $end\n\
$var wire 4 1 threshold $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n\
b0100 !\n\
b0011 0\n\
b0111 1\n\
";

    let temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    std::fs::write(temp_file.path(), vcd_content).expect("Failed to write VCD file");
    let mut waveform = wellen::simple::read(temp_file.path()).expect("Failed to read VCD file");

    let events = find_conditional_events(
        &mut waveform,
        "top.addr + top.offset >= top.threshold",
        0,
        0,
        -1,
    )
    .expect("Should find events");
    assert_eq!(
        events.len(),
        1,
        "Should find 1 transition where addr+offset >= threshold"
    );
}
