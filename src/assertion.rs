//! Assertion log parsing utilities.
//!
//! This module handles parsing ModelSim transcript files to extract
//! assertion failure events with severity, time, scope, and source info.

use crate::error::{WaveAnalyzerError, WaveResult};
use regex::Regex;
use std::path::Path;

/// Severity level of an assertion event.
#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Failure,
}

impl Severity {
    /// Parse a severity string from transcript.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "Error" => Some(Severity::Error),
            "Warning" => Some(Severity::Warning),
            "Note" => Some(Severity::Note),
            "Failure" => Some(Severity::Failure),
            _ => None,
        }
    }

    /// Check if this severity matches a filter list.
    pub fn matches_filter(&self, filter: &[Severity]) -> bool {
        if filter.is_empty() {
            return true;
        }
        filter.contains(self)
    }
}

/// Time unit for assertion event timestamps.
#[derive(Debug, Clone, PartialEq)]
pub enum TimeUnit {
    Ps,
    Ns,
    Us,
    Ms,
    S,
}

impl TimeUnit {
    /// Parse a time unit string.
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "ps" => Some(TimeUnit::Ps),
            "ns" => Some(TimeUnit::Ns),
            "us" => Some(TimeUnit::Us),
            "ms" => Some(TimeUnit::Ms),
            "s" => Some(TimeUnit::S),
            _ => None,
        }
    }

    /// Convert to picoseconds multiplier.
    pub fn to_ps_factor(&self) -> u128 {
        match self {
            TimeUnit::Ps => 1,
            TimeUnit::Ns => 1000,
            TimeUnit::Us => 1_000_000,
            TimeUnit::Ms => 1_000_000_000,
            TimeUnit::S => 1_000_000_000_000,
        }
    }
}

/// A single assertion event extracted from a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertionEvent {
    /// Name of the assertion.
    pub assertion_name: String,
    /// Severity level.
    pub severity: Severity,
    /// Scope path where the assertion was triggered.
    pub scope_path: String,
    /// Raw time value from transcript.
    pub time_value: u64,
    /// Time unit from transcript.
    pub time_unit: TimeUnit,
    /// Time value converted to picoseconds.
    pub time_ps: u64,
    /// Source file path (if available).
    pub source_file: Option<String>,
    /// Source line number (if available).
    pub source_line: Option<u32>,
}

/// Result of parsing an assertion log.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertionParseResult {
    /// Successfully parsed assertion events.
    pub events: Vec<AssertionEvent>,
    /// Lines that could not be parsed (for debugging).
    pub unmatched_lines: Vec<String>,
}

/// Parse assertion events from a ModelSim transcript file.
///
/// # Arguments
/// * `path` - Path to the transcript file
/// * `severity_filter` - Optional filter for severity levels (empty = all)
/// * `limit` - Maximum number of events to return (-1 for unlimited)
pub fn parse_assertion_log_from_file(
    path: &Path,
    severity_filter: &[Severity],
    limit: isize,
) -> WaveResult<AssertionParseResult> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(parse_assertion_log(&content, severity_filter, limit))
}

/// Parse assertion events from transcript content string.
///
/// Supports ModelSim 20.1.1.720 output formats:
///
/// **Format 1: Standard two-line format (vsim-10142/10143)**
/// ```text
/// # ** Error: (vsim-10142) TOP.tb_top.assert_data_transfer:
/// #    Time: 30 ns  Scope: tb_top File: tb/tb_top.sv Line: 42
/// ```
///
/// **Format 2: Short single-line format**
/// ```text
/// # ** Error: assert_xxx [30 ns] : TOP.tb_top
/// ```
///
/// **Format 3: Failure format (vsim-10143)**
/// ```text
/// # ** Failure: (vsim-10143) TOP.tb_top.assert_xxx:
/// #    Time: 1750 ns  Scope: tb_top
/// ```
///
/// **Format 4: Note without vsim code**
/// ```text
/// # ** Note: TOP.tb_top.assert_reset_done:
/// #    Time: 100 ps  Scope: tb_top
/// ```
pub fn parse_assertion_log(
    content: &str,
    severity_filter: &[Severity],
    limit: isize,
) -> AssertionParseResult {
    let mut events = Vec::new();
    let mut unmatched_lines = Vec::new();

    // Pattern for first line of two-line format
    // # ** Error: (vsim-10142) assertion_path:  (with vsim code)
    // # ** Note: TOP.tb_top.assert_reset_done:  (without vsim code)
    let line1_re =
        Regex::new(r"^#\s+\*\*\s+(Error|Warning|Note|Failure):\s+(?:\(vsim-\d+\)\s+)?(.+?):\s*$")
            .unwrap();

    // Pattern for second line of two-line format (with File and Line)
    // #    Time: 30 ns  Scope: tb_top File: tb/tb_top.sv Line: 42
    let line2_full_re = Regex::new(
        r"^#\s+Time:\s+(\d+)\s+(ps|ns|us|ms)\s+Scope:\s+(\S+)\s+File:\s+(\S+)\s+Line:\s+(\d+)\s*$",
    )
    .unwrap();

    // Pattern for second line of two-line format (without File and Line)
    // #    Time: 1750 ns  Scope: tb_top
    let line2_short_re =
        Regex::new(r"^#\s+Time:\s+(\d+)\s+(ps|ns|us|ms)\s+Scope:\s+(\S+)\s*$").unwrap();

    // Pattern for single-line format
    // # ** Error: assert_xxx [30 ns] : TOP.tb_top
    let single_line_re = Regex::new(
        r"^#\s+\*\*\s+(Error|Warning|Note|Failure):\s+(\S+)\s+\[(\d+)\s+(ps|ns|us|ms)\]\s+:\s+(\S+)\s*$"
    ).unwrap();

    // Pattern for custom assertion format (no # prefix):
    // Assertion Error at 15510 ns: assert_v_protect_triggered (v_protect == 0, expected 1 after debounce)
    // Also supports plain format without # prefix:
    // Error: assert_name at 15510 ns in scope TOP
    let custom_format_re = Regex::new(
        r"^(?:#\s+)?Assertion\s+(Error|warning|note|failure)\s+at\s+(\d+)\s+(ps|ns|us|ms)\s*:\s*(\S+)(?:\s+\((.+)\))?\s*$"
    ).unwrap();

    // Pattern for plain severity:time:name format (common in verification reports):
    // Error at 15510 ns: assert_name
    let plain_severity_time_re = Regex::new(
        r"^(?:#\s+)?(Error|Warning|Note|Failure)\s+at\s+(\d+)\s+(ps|ns|us|ms)\s*:\s*(\S+)\s*$",
    )
    .unwrap();

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        // Try custom "Assertion Error at N ns:" format first
        if let Some(caps) = custom_format_re.captures(lines[i]) {
            let severity_str = caps.get(1).unwrap().as_str();
            // Capitalize first letter for consistent matching
            let severity_normalized = {
                let mut s = severity_str.to_lowercase();
                if let Some(first) = s.chars().next() {
                    s = first.to_uppercase().to_string() + &s[1..];
                }
                s
            };
            let time_value: u64 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            let time_unit_str = caps.get(3).unwrap().as_str();
            let assertion_name = caps.get(4).unwrap().as_str().to_string();

            if let (Some(severity), Some(time_unit)) = (
                Severity::from_str_name(&severity_normalized),
                TimeUnit::try_from_str(time_unit_str),
            ) && severity.matches_filter(severity_filter)
            {
                let time_ps: u128 = time_value as u128 * time_unit.to_ps_factor();
                events.push(AssertionEvent {
                    assertion_name,
                    severity,
                    scope_path: String::new(), // No scope in custom format
                    time_value,
                    time_unit,
                    time_ps: time_ps as u64,
                    source_file: None,
                    source_line: None,
                });
            }
            i += 1;
            continue;
        }

        // Try plain "Error at N ns: name" format
        if let Some(caps) = plain_severity_time_re.captures(lines[i]) {
            let severity_str = caps.get(1).unwrap().as_str();
            let time_value: u64 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            let time_unit_str = caps.get(3).unwrap().as_str();
            let assertion_name = caps.get(4).unwrap().as_str().to_string();

            if let (Some(severity), Some(time_unit)) = (
                Severity::from_str_name(severity_str),
                TimeUnit::try_from_str(time_unit_str),
            ) && severity.matches_filter(severity_filter)
            {
                let time_ps: u128 = time_value as u128 * time_unit.to_ps_factor();
                events.push(AssertionEvent {
                    assertion_name,
                    severity,
                    scope_path: String::new(),
                    time_value,
                    time_unit,
                    time_ps: time_ps as u64,
                    source_file: None,
                    source_line: None,
                });
            }
            i += 1;
            continue;
        }

        // Try single-line format
        if let Some(caps) = single_line_re.captures(lines[i]) {
            let severity_str = caps.get(1).unwrap().as_str();
            let assertion_name = caps.get(2).unwrap().as_str().to_string();
            let time_value: u64 = caps.get(3).unwrap().as_str().parse().unwrap_or(0);
            let time_unit_str = caps.get(4).unwrap().as_str();
            let scope_path = caps.get(5).unwrap().as_str().to_string();

            if let (Some(severity), Some(time_unit)) = (
                Severity::from_str_name(severity_str),
                TimeUnit::try_from_str(time_unit_str),
            ) && severity.matches_filter(severity_filter)
            {
                let time_ps: u128 = time_value as u128 * time_unit.to_ps_factor();
                events.push(AssertionEvent {
                    assertion_name,
                    severity,
                    scope_path,
                    time_value,
                    time_unit,
                    time_ps: time_ps as u64,
                    source_file: None,
                    source_line: None,
                });
            }
            i += 1;
            continue;
        }

        // Try two-line format
        if let Some(caps) = line1_re.captures(lines[i]) {
            let severity_str = caps.get(1).unwrap().as_str();
            let assertion_path = caps.get(2).unwrap().as_str().to_string();

            // Extract short assertion name from full path
            let assertion_name = assertion_path
                .rsplit('.')
                .next()
                .unwrap_or(&assertion_path)
                .to_string();

            if let Some(severity) = Severity::from_str_name(severity_str) {
                // Look for second line
                if i + 1 < lines.len() {
                    // Try full format (with File and Line)
                    if let Some(caps2) = line2_full_re.captures(lines[i + 1]) {
                        let time_value: u64 = caps2.get(1).unwrap().as_str().parse().unwrap_or(0);
                        let time_unit_str = caps2.get(2).unwrap().as_str();
                        let scope_path = caps2.get(3).unwrap().as_str().to_string();
                        let source_file = caps2.get(4).unwrap().as_str().to_string();
                        let source_line: u32 = caps2.get(5).unwrap().as_str().parse().unwrap_or(0);

                        if let Some(time_unit) = TimeUnit::try_from_str(time_unit_str)
                            && severity.matches_filter(severity_filter)
                        {
                            let time_ps: u128 = time_value as u128 * time_unit.to_ps_factor();
                            events.push(AssertionEvent {
                                assertion_name,
                                severity,
                                scope_path,
                                time_value,
                                time_unit,
                                time_ps: time_ps as u64,
                                source_file: Some(source_file),
                                source_line: Some(source_line),
                            });
                        }
                        i += 2;
                        continue;
                    }

                    // Try short format (without File and Line)
                    if let Some(caps2) = line2_short_re.captures(lines[i + 1]) {
                        let time_value: u64 = caps2.get(1).unwrap().as_str().parse().unwrap_or(0);
                        let time_unit_str = caps2.get(2).unwrap().as_str();
                        let scope_path = caps2.get(3).unwrap().as_str().to_string();

                        if let Some(time_unit) = TimeUnit::try_from_str(time_unit_str)
                            && severity.matches_filter(severity_filter)
                        {
                            let time_ps: u128 = time_value as u128 * time_unit.to_ps_factor();
                            events.push(AssertionEvent {
                                assertion_name,
                                severity,
                                scope_path,
                                time_value,
                                time_unit,
                                time_ps: time_ps as u64,
                                source_file: None,
                                source_line: None,
                            });
                        }
                        i += 2;
                        continue;
                    }
                }

                // Second line did not match - record unmatched first line
                unmatched_lines.push(lines[i].to_string());
                i += 1;
                continue;
            }
        }

        // No match found
        if lines[i].starts_with("#") && lines[i].contains("**") {
            unmatched_lines.push(lines[i].to_string());
        }
        i += 1;
    }

    // Apply limit
    if limit >= 0 && events.len() > limit as usize {
        events.truncate(limit as usize);
    }

    AssertionParseResult {
        events,
        unmatched_lines,
    }
}
