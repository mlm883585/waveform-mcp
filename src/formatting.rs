//! Formatting utilities for time and signal values.
//!
//! Also provides [`ReportWriter`] and the [`report_writeln!`] / [`report_write!`]
//! macros — helpers for building multi-line text reports without the `.unwrap()`
//! noise that `writeln!` on a `String` would require. Because writing to a
//! `String` is infallible, the macros return `()` instead of `fmt::Result`,
//! eliminating the "unused Result" warnings that `writeln!` produces.

use crate::error::{WaveAnalyzerError, WaveResult};
use num_bigint::BigUint;
use num_traits::Zero;
use wellen;

/// A helper for building multi-line text reports.
///
/// Use [`report_writeln!`] and [`report_write!`] macros to write formatted text
/// into a `ReportWriter`. These macros return `()` (not `fmt::Result`) because
/// writing to a `String` never fails, which eliminates the `.unwrap()` calls
/// and `unused_must_use` warnings that `writeln!` would produce.
///
/// # Example
///
/// ```ignore
/// let mut w = ReportWriter::new();
/// report_writeln!(w, "BFS Trace Report");
/// report_writeln!(w, "Entry signal: {}", signal);
/// report_writeln!(w, "Candidates: {} found", count);
/// w.finish()  // → returns the accumulated String
/// ```
pub struct ReportWriter {
    output: String,
}

impl ReportWriter {
    /// Create a new, empty report writer.
    pub fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    /// Create a new report writer with a pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            output: String::with_capacity(capacity),
        }
    }

    /// Consume the writer and return the accumulated output string.
    pub fn finish(self) -> String {
        self.output
    }

    /// Append a raw string slice to the output (no newline).
    pub fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    /// Append a single character to the output.
    pub fn push(&mut self, ch: char) {
        self.output.push(ch);
    }

    /// Append formatted text to the output (no newline, no Result).
    ///
    /// This is the infallible counterpart of `fmt::Write::write_fmt`.
    /// Use via the [`report_write!`] macro for ergonomic format syntax.
    pub fn write_fmt_args(&mut self, args: std::fmt::Arguments<'_>) {
        if let Some(s) = args.as_str() {
            // Fast path: literal string with no formatting
            self.output.push_str(s);
        } else {
            // Slow path: needs formatting — delegate to fmt::Write impl
            // (which always succeeds for String-backed ReportWriter)
            use std::fmt::Write;
            let _ = self.write_fmt(args);
        }
    }

    /// Append formatted text + newline to the output (no Result).
    ///
    /// Use via the [`report_writeln!`] macro for ergonomic format syntax.
    pub fn writeln_fmt_args(&mut self, args: std::fmt::Arguments<'_>) {
        self.write_fmt_args(args);
        self.output.push('\n');
    }

    /// Return the current length of the accumulated output.
    pub fn len(&self) -> usize {
        self.output.len()
    }

    /// Return whether the accumulated output is empty.
    pub fn is_empty(&self) -> bool {
        self.output.is_empty()
    }
}

impl std::fmt::Write for ReportWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.output.push_str(s);
        Ok(())
    }
}

impl Default for ReportWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Write formatted text into a [`ReportWriter`] (no newline, returns `()`).
///
/// Infallible — writing to a `String` never fails, so no `.unwrap()` needed.
///
/// ```ignore
/// let mut w = ReportWriter::new();
/// report_write!(w, "no newline here");
/// ```
#[macro_export]
macro_rules! report_write {
    ($dst:expr, $($arg:tt)*) => {
        $dst.write_fmt_args(format_args!($($arg)*))
    };
}

/// Write formatted text + newline into a [`ReportWriter`] (returns `()`).
///
/// Infallible — writing to a `String` never fails, so no `.unwrap()` needed.
/// This is the primary macro for building multi-line reports.
///
/// ```ignore
/// let mut w = ReportWriter::new();
/// report_writeln!(w, "BFS Trace Report");
/// report_writeln!(w, "Candidates: {} found", count);
/// ```
#[macro_export]
macro_rules! report_writeln {
    ($dst:expr) => {
        $dst.writeln_fmt_args(format_args!(""))
    };
    ($dst:expr, $($arg:tt)*) => {
        $dst.writeln_fmt_args(format_args!($($arg)*))
    };
}

/// Format a time value with its timescale into a human-readable string.
///
/// # Arguments
/// * `time_value` - The raw time value from waveform
/// * `timescale` - Optional timescale information for proper formatting
///
/// # Examples
/// ```
/// use wave_analyzer_mcp::formatting::format_time;
///
/// let timescale = wellen::Timescale {
///     factor: 1,
///     unit: wellen::TimescaleUnit::NanoSeconds,
/// };
/// assert_eq!(format_time(10, Some(&timescale)), "10ns");
/// ```
pub fn format_time(time_value: u64, timescale: Option<&wellen::Timescale>) -> String {
    match timescale {
        Some(ts) => {
            let unit = match ts.unit {
                wellen::TimescaleUnit::ZeptoSeconds => "zs",
                wellen::TimescaleUnit::AttoSeconds => "as",
                wellen::TimescaleUnit::FemtoSeconds => "fs",
                wellen::TimescaleUnit::PicoSeconds => "ps",
                wellen::TimescaleUnit::NanoSeconds => "ns",
                wellen::TimescaleUnit::MicroSeconds => "us",
                wellen::TimescaleUnit::MilliSeconds => "ms",
                wellen::TimescaleUnit::Seconds => "s",
                wellen::TimescaleUnit::Unknown => "unknown",
            };
            format!("{}{}", time_value * ts.factor as u64, unit)
        }
        None => format!("{} (unknown timescale)", time_value),
    }
}

/// Format a timescale into a human-readable string like "1ns", "10ps", etc.
pub fn format_timescale(ts: &wellen::Timescale) -> String {
    let unit = match ts.unit {
        wellen::TimescaleUnit::ZeptoSeconds => "zs",
        wellen::TimescaleUnit::AttoSeconds => "as",
        wellen::TimescaleUnit::FemtoSeconds => "fs",
        wellen::TimescaleUnit::PicoSeconds => "ps",
        wellen::TimescaleUnit::NanoSeconds => "ns",
        wellen::TimescaleUnit::MicroSeconds => "us",
        wellen::TimescaleUnit::MilliSeconds => "ms",
        wellen::TimescaleUnit::Seconds => "s",
        wellen::TimescaleUnit::Unknown => "unknown",
    };
    format!("{}{}", ts.factor, unit)
}

/// Format a signal value into a human-readable string.
///
/// # Arguments
/// * `signal_value` - The signal value to format
///
/// # Returns
/// A string representation of signal value.
///
/// Format mimics Verilog representation:
/// - Short signals (<= 4 bits): binary format like `3'b101`
/// - Longer signals: hex format like `16'h1a2b`
pub fn format_signal_value(signal_value: wellen::SignalValue) -> String {
    format_signal_value_with_width(signal_value, None)
}

/// Format a signal value with an optional declared width override.
///
/// When `declared_width` is provided and > 0, it overrides the `bits` reported
/// by wellen, which may be inaccurate for multi-bit signals read via iter_changes().
pub fn format_signal_value_with_width(
    signal_value: wellen::SignalValue,
    declared_width: Option<u32>,
) -> String {
    match signal_value {
        wellen::SignalValue::Event => "Event".to_string(),
        wellen::SignalValue::Binary(_, bits)
        | wellen::SignalValue::FourValue(_, bits)
        | wellen::SignalValue::NineValue(_, bits) => {
            let w = if declared_width.unwrap_or(0) > 0 {
                declared_width.unwrap()
            } else {
                bits
            };
            format_biguint_verilog_inner(signal_value, w)
        }
        wellen::SignalValue::String(s) => s.to_string(),
        wellen::SignalValue::Real(r) => format!("{}", r),
    }
}

/// Format binary data as a Verilog-style literal string.
/// Internal helper that delegates to `format_biguint_verilog`.
fn format_biguint_verilog_inner(signal_value: wellen::SignalValue, bits: u32) -> String {
    let value = signal_value_to_biguint(signal_value, Some(bits));
    format_biguint_verilog(&value, bits)
}

/// Check if a 1-bit signal value is high (1), using BigUint conversion
/// to correctly handle FourValue/NineValue encoding.
pub fn is_signal_high(signal_value: &wellen::SignalValue) -> bool {
    match signal_value {
        wellen::SignalValue::Binary(_, _)
        | wellen::SignalValue::FourValue(_, _)
        | wellen::SignalValue::NineValue(_, _) => {
            let value = signal_value_to_biguint(*signal_value, None);
            !value.is_zero()
        }
        wellen::SignalValue::String(s) => *s == "1" || s.eq_ignore_ascii_case("true"),
        wellen::SignalValue::Real(r) => *r != 0.0,
        _ => false,
    }
}

/// Apply width mask to a BigUint value.
fn apply_mask(mut value: BigUint, mask_width: Option<u32>) -> BigUint {
    if let Some(w) = mask_width {
        if w > 0 && w < 8192 {
            let mask = (BigUint::from(1u32) << w) - BigUint::from(1u32);
            value &= mask;
        }
    }
    value
}

/// Convert `wellen::SignalValue` to `BigUint`, correctly handling
/// FourValue/NineValue encoding via wellen's Display trait.
///
/// BUG-11 fix: For FourValue/NineValue, uses wellen's `Display` impl
/// to correctly decode multi-bit-per-position encoded data, then parses
/// the resulting bit string. X/Z/H/U/W/L/D states are treated as 0
/// (BigUint cannot represent them).
/// For Binary, uses `packed_bytes_to_biguint` (MSB-first byte packing).
/// For String/Real/Event, uses lenient parsing.
///
/// `mask_width` masks the result to the declared signal width (if > 0).
pub fn signal_value_to_biguint(
    signal_value: wellen::SignalValue,
    mask_width: Option<u32>,
) -> BigUint {
    match &signal_value {
        wellen::SignalValue::Binary(data, bits) => {
            let w = mask_width.unwrap_or(*bits);
            packed_bytes_to_biguint(data, Some(w))
        }
        wellen::SignalValue::FourValue(_, _) | wellen::SignalValue::NineValue(_, _) => {
            // Use wellen's Display trait to correctly decode the multi-state
            // encoding, then parse the bit string to extract 0/1 values.
            let bit_string = format!("{}", signal_value);
            let mut value = BigUint::zero();
            for (i, ch) in bit_string.chars().rev().enumerate() {
                if ch == '1' {
                    value.set_bit(i as u64, true);
                }
            }
            apply_mask(value, mask_width)
        }
        wellen::SignalValue::String(s) => {
            if *s == "1" || s.eq_ignore_ascii_case("true") {
                BigUint::from(1u32)
            } else if *s == "0" || s.eq_ignore_ascii_case("false") {
                BigUint::zero()
            } else {
                s.parse::<u64>()
                    .map(BigUint::from)
                    .unwrap_or(BigUint::zero())
            }
        }
        wellen::SignalValue::Real(r) => BigUint::from(*r as u64),
        wellen::SignalValue::Event => BigUint::from(1u32),
    }
}

/// Convert MSB-first packed bytes to BigUint.
///
/// wellen stores multi-bit signal values as MSB-first packed byte arrays.
/// If `mask_width` is `Some(width)`, the result is masked to `width` bits
/// (useful for bus signals where the byte representation may exceed the
/// declared bit width). If `None`, no masking is applied.
///
/// **IMPORTANT**: This function only works correctly for Binary (2-state)
/// signal data. For FourValue/NineValue data, use `signal_value_to_biguint`
/// instead, which correctly decodes the multi-state encoding.
pub fn packed_bytes_to_biguint(data: &[u8], mask_width: Option<u32>) -> BigUint {
    let mut value = BigUint::from(0u32);
    for (i, &byte) in data.iter().enumerate() {
        value |= BigUint::from(byte) << ((data.len() - 1 - i) * 8);
    }
    if let Some(width) = mask_width {
        if width > 0 && width < 8192 {
            let mask = (BigUint::from(1u32) << width) - BigUint::from(1u32);
            value &= mask;
        }
    }
    value
}

/// Strict conversion of `wellen::SignalValue` to `BigUint`.
///
/// Returns `Result`: errors on `Event` signals and unparseable strings.
/// For `Binary/FourValue/NineValue`, uses `signal_value_to_biguint` (with masking).
/// For `String`, tries `"1"/"true"` → 1, `"0"/"false"` → 0, then `u64` parse.
/// For `Real`, truncates to `u64`.
///
/// Use this when correctness matters (condition evaluation, clock edge detection).
pub fn signal_value_to_biguint_strict(signal_value: wellen::SignalValue) -> WaveResult<BigUint> {
    match signal_value {
        wellen::SignalValue::Binary(_, _) => Ok(signal_value_to_biguint(signal_value, None)),
        wellen::SignalValue::FourValue(_, _) => Ok(signal_value_to_biguint(signal_value, None)),
        wellen::SignalValue::NineValue(_, _) => Ok(signal_value_to_biguint(signal_value, None)),
        wellen::SignalValue::String(s) => {
            if s == "1" || s.eq_ignore_ascii_case("true") {
                Ok(BigUint::from(1u32))
            } else if s == "0" || s.eq_ignore_ascii_case("false") {
                Ok(BigUint::from(0u32))
            } else {
                s.parse::<u64>().map(BigUint::from).map_err(|_| {
                    WaveAnalyzerError::Other(format!("Cannot convert string '{}' to integer", s))
                })
            }
        }
        wellen::SignalValue::Real(r) => Ok(BigUint::from(r as u64)),
        wellen::SignalValue::Event => Err(WaveAnalyzerError::Other(
            "Event signal cannot be compared".to_string(),
        )),
    }
}

/// Lenient conversion of `wellen::SignalValue` to `BigUint`.
///
/// Never fails: `Event` → 1, unparseable strings → 0, X/Z values treated as 0.
/// For `Binary/FourValue/NineValue`, uses `signal_value_to_biguint` (with masking).
/// For `String`, tries `0x` hex prefix, then `"1"/"true"` → 1, `"0"/"false"` → 0,
/// then `u64` decimal parse — all failures fall back to 0.
/// For `Real`, truncates to `u64`; zero `Real` → 0.
///
/// Use this when robustness matters more than precision (FSM, pattern, extraction).
pub fn signal_value_to_biguint_lenient(signal_value: wellen::SignalValue) -> BigUint {
    match signal_value {
        wellen::SignalValue::Binary(_, _) => signal_value_to_biguint(signal_value, None),
        wellen::SignalValue::FourValue(_, _) => signal_value_to_biguint(signal_value, None),
        wellen::SignalValue::NineValue(_, _) => signal_value_to_biguint(signal_value, None),
        wellen::SignalValue::String(s) => {
            // Try 0x hex prefix first (common in FSM state strings)
            if let Some(hex) = s.strip_prefix("0x") {
                if let Ok(val) = u64::from_str_radix(hex, 16) {
                    return BigUint::from(val);
                }
            }
            // Try boolean-like strings
            if s == "1" || s.eq_ignore_ascii_case("true") {
                return BigUint::from(1u32);
            }
            if s == "0" || s.eq_ignore_ascii_case("false") {
                return BigUint::from(0u32);
            }
            // Try decimal parse
            if let Ok(val) = s.parse::<u64>() {
                return BigUint::from(val);
            }
            // Fallback: 0
            BigUint::zero()
        }
        wellen::SignalValue::Real(r) => BigUint::from(r as u64),
        wellen::SignalValue::Event => BigUint::from(1u32),
    }
}

/// Format a `BigUint` as a Verilog-style literal.
///
/// Short signals (≤ 4 bits): binary format like `3'b101`.
/// Longer signals: hex format like `16'h1a2b`.
/// Zero values are padded correctly.
///
/// This is the canonical Verilog formatter. Modules that previously had
/// local `format_biguint_verilog`, `format_biguint`, etc. should use this.
pub fn format_biguint_verilog(value: &BigUint, width: u32) -> String {
    // Zero case: padded format
    if value.is_zero() {
        if width <= 4 {
            return format!("{}'b{}", width, "0".repeat(width as usize));
        } else {
            let hex_chars = (width as usize).div_ceil(4);
            return format!("{}'h{}", width, "0".repeat(hex_chars));
        }
    }

    // Non-zero case
    if width <= 4 {
        // Binary format for short signals
        let bin_str = format!("{:b}", value);
        let padded = if bin_str.len() < width as usize {
            format!("{:0>width$}", bin_str, width = width as usize)
        } else if bin_str.len() > width as usize {
            bin_str[bin_str.len() - width as usize..].to_string()
        } else {
            bin_str
        };
        format!("{}'b{}", width, padded)
    } else {
        // Hex format for longer signals
        let hex_chars = (width as usize).div_ceil(4);
        let hex_str = format!("{:x}", value);
        let padded = if hex_str.len() < hex_chars {
            format!("{:0>width$}", hex_str, width = hex_chars)
        } else if hex_str.len() > hex_chars {
            hex_str[hex_str.len() - hex_chars..].to_string()
        } else {
            hex_str
        };
        format!("{}'h{}", width, padded)
    }
}

/// Format a `BigUint` value according to a user-specified format string.
///
/// - `"hex"` → Verilog hex literal like `16'h0A3F`
/// - `"binary"` → Verilog binary literal like `8'b10101010`
/// - `"decimal"` → plain decimal number like `42`
///
/// This is the canonical format-parameterized formatter. Modules that
/// previously had local `format_value_for_compare`, `format_value`,
/// `format_value_biguint`, etc. should use this.
pub fn format_biguint_value(value: &BigUint, width: u32, format: &str) -> String {
    // Zero case
    if value.is_zero() {
        return match format {
            "hex" => format!("{}'h{}", width, "0".repeat((width as usize).div_ceil(4))),
            "binary" => format!("{}'b{}", width, "0".repeat(width as usize)),
            _ => "0".to_string(),
        };
    }

    match format {
        "hex" => {
            let hex_chars = (width as usize).div_ceil(4);
            let bytes = value.to_bytes_be();
            let hex_str: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let padded = if hex_str.len() < hex_chars {
                format!("{:0>width$}", hex_str, width = hex_chars)
            } else if hex_str.len() > hex_chars {
                hex_str[hex_str.len() - hex_chars..].to_string()
            } else {
                hex_str
            };
            format!("{}'h{}", width, padded)
        }
        "binary" => {
            let bin_str = format!("{:b}", value);
            let padded = if bin_str.len() < width as usize {
                format!("{:0>width$}", bin_str, width = width as usize)
            } else {
                bin_str
            };
            format!("{}'b{}", width, padded)
        }
        "decimal" => format!("{}", value),
        _ => format!("{}'h{:x}", width, value),
    }
}

/// Parse a Verilog-style value string (like `"8'h5A"`, `"1'b0"`, `"3'd5"`)
/// or a plain decimal string into a `BigUint`.
///
/// This is the canonical Verilog literal parser. Modules that previously
/// had local `parse_value_string_to_biguint`, `threshold_to_biguint`,
/// `parse_threshold` should use this.
pub fn parse_verilog_literal(value_str: &str) -> BigUint {
    let s = value_str.trim();

    // Verilog-style literal: N'bXXX, N'hXXX, N'dXXX
    if let Some(pos) = s.find('\'') {
        let _width_str = &s[..pos];
        let rest = &s[pos + 1..];
        let base_char = rest.chars().next().unwrap_or('d');
        let digits = &rest[1..];

        return match base_char {
            'b' | 'B' => {
                let mut val = 0u64;
                for c in digits.chars() {
                    match c {
                        '0' => val = val << 1,
                        '1' => val = (val << 1) | 1,
                        '_' => {} // skip underscores
                        _ => {}   // skip X/Z etc.
                    }
                }
                BigUint::from(val)
            }
            'h' | 'H' => {
                let clean = digits.replace('_', "");
                u64::from_str_radix(&clean, 16)
                    .map(BigUint::from)
                    .unwrap_or(BigUint::zero())
            }
            'd' | 'D' => {
                let clean = digits.replace('_', "");
                clean
                    .parse::<u64>()
                    .map(BigUint::from)
                    .unwrap_or(BigUint::zero())
            }
            _ => BigUint::zero(),
        };
    }

    // C-style hex prefix: 0xXXX
    if let Some(hex) = s.strip_prefix("0x") {
        return u64::from_str_radix(hex, 16)
            .map(BigUint::from)
            .unwrap_or(BigUint::zero());
    }

    // Plain decimal
    s.parse::<u64>()
        .map(BigUint::from)
        .unwrap_or(BigUint::zero())
}
