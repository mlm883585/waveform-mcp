//! Condition parsing and evaluation for conditional event search.

use super::{
    formatting::{format_biguint_verilog, format_time, signal_value_to_biguint_strict},
    hierarchy::{find_var_by_path, get_signal_width, resolve_signal_var_refs},
};
use crate::error::{WaveAnalyzerError, WaveResult};
use lalrpop_util::lalrpop_mod;
use num_bigint::BigUint;
use num_traits::Zero;
use std::ops::{BitAnd, BitOr, BitXor};
use wellen;

// Import generated parser
lalrpop_mod!(condition);

/// Literal value for signal comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Binary(Vec<bool>, u32), // bits, bit width
    Decimal(u64, u32),      // value, bit width
    Hexadecimal(u64, u32),  // value, bit width
}

/// Entry in the signal cache that supports both single-bus and
/// bit-slice decomposed multi-bit signals.
///
/// For a proper bus variable (length > 1), `var_refs` has one element
/// and `bit_positions` is `[None]`. For a bit-slice decomposed signal
/// (e.g. o_state stored as o_state[2], o_state[1], o_state[0]),
/// `var_refs` has all VarRefs and `bit_positions` gives each bit's
/// position in the composite value.
#[derive(Debug, Clone)]
pub struct SignalCacheEntry {
    /// All VarRefs for this signal (1 for bus, N for bit-slice).
    pub var_refs: Vec<wellen::VarRef>,
    /// Corresponding SignalRefs (same length as var_refs).
    pub signal_refs: Vec<wellen::SignalRef>,
    /// Bit position in the composite value (None for a full bus).
    pub bit_positions: Vec<Option<u32>>,
    /// Total width in bits (bus width, or MSB+1 for bit-slice).
    pub width: u32,
}

/// Condition for finding events based on signal values.
#[derive(Debug, Clone)]
pub enum Condition {
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
    Not(Box<Condition>),
    BitwiseNot(Box<Condition>),
    Signal(String),
    BitExtract(String, Option<u32>, Option<u32>), // signal, msb (optional), lsb (optional)
    Eq(Box<Condition>, Box<Condition>),
    Neq(Box<Condition>, Box<Condition>),
    Lt(Box<Condition>, Box<Condition>),
    Le(Box<Condition>, Box<Condition>),
    Gt(Box<Condition>, Box<Condition>),
    Ge(Box<Condition>, Box<Condition>),
    Add(Box<Condition>, Box<Condition>),
    Sub(Box<Condition>, Box<Condition>),
    BitwiseAnd(Box<Condition>, Box<Condition>),
    BitwiseOr(Box<Condition>, Box<Condition>),
    BitwiseXor(Box<Condition>, Box<Condition>),
    Literal(Literal),
    Past(Box<Condition>),
    PastN(Box<Condition>, u32),
    Rose(String),
    Fell(String),
    Stable(String),
}

/// Parse a simple condition string into a Condition AST.
///
/// Supports:
/// - Signal paths (e.g., "TOP.signal")
/// - Bit extraction: `signal[bit]` for single bit, `signal[msb:lsb]` for range
/// - `&&` for logical AND
/// - `||` for logical OR
/// - `!` for logical NOT
/// - `~` for bitwise NOT
/// - `&` for bitwise AND
/// - `|` for bitwise OR
/// - `^` for bitwise XOR
/// - `==` for equality comparison
/// - `!=` for inequality comparison
/// - `<`, `<=`, `>`, `>=` for magnitude comparison
/// - `+`, `-` for arithmetic (addition and subtraction)
/// - `$past(expr)` to read signal value from previous time index
/// - `$past(expr, N)` to read signal value from N cycles back
/// - `$rose(signal)` to detect 0→1 transition
/// - `$fell(signal)` to detect 1→0 transition
/// - `$stable(signal)` to detect unchanged value
/// - Parentheses for grouping
/// - Verilog-style literals: 4'b0101, 3'd2, 5'h1A
/// - Bare decimal literals: 0, 1, 42
///
/// Uses lalrpop-generated parser.
pub fn parse_condition(condition: &str) -> WaveResult<Condition> {
    let parser = crate::condition::condition::ExprParser::new();
    parser
        .parse(condition)
        .map_err(|e| WaveAnalyzerError::ConditionParseError {
            message: e.to_string(),
        })
}

/// Build a SignalCacheEntry for a signal path, resolving both bus
/// and bit-slice decomposed signals.
///
/// For a proper bus variable (e.g. o_state[2:0]), returns a single
/// VarRef with `bit_positions = [None]`. For bit-slice decomposed
/// signals (e.g. o_state stored as o_state[0], o_state[1], o_state[2]),
/// returns all VarRefs with their respective bit positions.
pub fn build_signal_cache_entry(
    hierarchy: &wellen::Hierarchy,
    path: &str,
) -> WaveResult<SignalCacheEntry> {
    // Try exact resolution first (handles bus variables, bit-slice, bracket notation)
    if let Some(var_refs) = resolve_signal_var_refs(hierarchy, path) {
        let width = get_signal_width(hierarchy, path);

        let mut signal_refs = Vec::with_capacity(var_refs.len());
        let mut bit_positions = Vec::with_capacity(var_refs.len());

        if var_refs.len() == 1 {
            // Single bus variable
            let vr = var_refs[0];
            signal_refs.push(hierarchy[vr].signal_ref());
            bit_positions.push(None);
        } else {
            // Bit-slice decomposed — sort by MSB descending
            let mut sorted_refs = var_refs.clone();
            sorted_refs.sort_by_key(|vr| hierarchy[*vr].index().map(|idx| idx.msb()).unwrap_or(0));
            sorted_refs.reverse(); // MSB first

            for vr in &sorted_refs {
                let var = &hierarchy[*vr];
                signal_refs.push(var.signal_ref());
                bit_positions.push(var.index().map(|idx| idx.msb() as u32));
            }
        }

        return Ok(SignalCacheEntry {
            var_refs,
            signal_refs,
            bit_positions,
            width,
        });
    }

    // Fallback: use find_var_by_path which includes leaf-name and suffix-path matching.
    // This handles short signal names (e.g. "o_power_off") and partial paths
    // (e.g. "u_dut.o_power_off") that resolve_signal_var_refs cannot find.
    if let Some(vr) = find_var_by_path(hierarchy, path) {
        let var = &hierarchy[vr];
        let width = var.length().unwrap_or(1);

        return Ok(SignalCacheEntry {
            var_refs: vec![vr],
            signal_refs: vec![var.signal_ref()],
            bit_positions: vec![None],
            width,
        });
    }

    Err(WaveAnalyzerError::SignalNotFound {
        path: path.to_string(),
    })
}

/// Read a signal's composite BigUint value at a given time index,
/// handling both single-bus and bit-slice decomposed signals.
///
/// For a bus signal, reads the full packed value directly.
/// For bit-slice signals, reads each individual bit and reconstructs
/// the composite value by setting the appropriate bit positions.
fn read_signal_composite(
    entry: &SignalCacheEntry,
    waveform: &wellen::simple::Waveform,
    time_idx: usize,
) -> WaveResult<BigUint> {
    if entry.var_refs.len() == 1 {
        // Single bus: read directly, then mask to declared width
        let signal = waveform.get_signal(entry.signal_refs[0]).ok_or_else(|| {
            WaveAnalyzerError::SignalNotFound {
                path: "waveform signal".to_string(),
            }
        })?;
        let time_table_idx: wellen::TimeTableIdx =
            time_idx
                .try_into()
                .map_err(|_| WaveAnalyzerError::ConditionParseError {
                    message: format!("Time index {} too large", time_idx),
                })?;
        let offset = signal.get_offset(time_table_idx).ok_or_else(|| {
            WaveAnalyzerError::ConditionParseError {
                message: format!("No data for signal at time index {}", time_idx),
            }
        })?;
        let signal_value = signal.get_value_at(&offset, 0);
        let raw = signal_value_to_biguint_strict(signal_value)?;
        // Mask to declared width — VCD initial values can have excess bits
        let mask = (BigUint::from(1u32) << entry.width) - BigUint::from(1u32);
        Ok(raw & mask)
    } else {
        // Bit-slice: reconstruct composite value from individual bits
        let time_table_idx: wellen::TimeTableIdx =
            time_idx
                .try_into()
                .map_err(|_| WaveAnalyzerError::ConditionParseError {
                    message: format!("Time index {} too large", time_idx),
                })?;
        let mut composite = BigUint::zero();
        for (sig_ref, bit_pos) in entry.signal_refs.iter().zip(entry.bit_positions.iter()) {
            let signal =
                waveform
                    .get_signal(*sig_ref)
                    .ok_or_else(|| WaveAnalyzerError::SignalNotFound {
                        path: "bit-slice waveform signal".to_string(),
                    })?;
            if let Some(offset) = signal.get_offset(time_table_idx) {
                let value = signal.get_value_at(&offset, 0);
                let bit_val = signal_value_to_biguint_strict(value)?;
                if let Some(bp) = bit_pos {
                    if !bit_val.is_zero() {
                        composite.set_bit(*bp as u64, true);
                    }
                }
            }
        }
        Ok(composite)
    }
}

/// Evaluate a condition at a specific time index.
///
/// # Arguments
/// * `condition` - The condition to evaluate
/// * `waveform` - The waveform to read from
/// * `signal_cache` - Cache of variable references
/// * `time_idx` - The time index to evaluate at
///
/// # Returns
/// A BigUint value where 0 = false and any non-zero value = true.
pub fn evaluate_condition(
    condition: &Condition,
    waveform: &mut wellen::simple::Waveform,
    signal_cache: &std::collections::HashMap<String, SignalCacheEntry>,
    time_idx: usize,
) -> WaveResult<BigUint> {
    let (value, _width) =
        evaluate_condition_with_width(condition, waveform, signal_cache, time_idx)?;
    Ok(value)
}

/// Evaluate a condition at a specific time index, returning both value and bit width.
///
/// # Returns
/// A tuple of (value, bit_width) where bit_width is the bit width of the value.
fn evaluate_condition_with_width(
    condition: &Condition,
    waveform: &mut wellen::simple::Waveform,
    signal_cache: &std::collections::HashMap<String, SignalCacheEntry>,
    time_idx: usize,
) -> WaveResult<(BigUint, u32)> {
    match condition {
        Condition::And(left, right) => {
            let left_val = evaluate_condition(left, waveform, signal_cache, time_idx)?;
            let right_val = evaluate_condition(right, waveform, signal_cache, time_idx)?;
            Ok((
                if !left_val.is_zero() && !right_val.is_zero() {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1, // Logical operations return 1-bit result
            ))
        }
        Condition::Or(left, right) => {
            let left_val = evaluate_condition(left, waveform, signal_cache, time_idx)?;
            let right_val = evaluate_condition(right, waveform, signal_cache, time_idx)?;
            Ok((
                if !left_val.is_zero() || !right_val.is_zero() {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1, // Logical operations return 1-bit result
            ))
        }
        Condition::BitwiseAnd(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let width = left_width.max(right_width);
            Ok((left_val.bitand(right_val), width))
        }
        Condition::BitwiseOr(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let width = left_width.max(right_width);
            Ok((left_val.bitor(right_val), width))
        }
        Condition::BitwiseXor(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let width = left_width.max(right_width);
            Ok((left_val.bitxor(right_val), width))
        }
        Condition::Not(expr) => {
            let val = evaluate_condition(expr, waveform, signal_cache, time_idx)?;
            Ok((
                if !val.is_zero() {
                    BigUint::from(0u32)
                } else {
                    BigUint::from(1u32)
                },
                1, // Logical NOT returns 1-bit result
            ))
        }
        Condition::BitwiseNot(expr) => {
            let (val, width) =
                evaluate_condition_with_width(expr, waveform, signal_cache, time_idx)?;
            // Create a mask with all bits set for the given width
            let mask = (BigUint::from(1u32) << width) - BigUint::from(1u32);
            // Bitwise NOT is value XOR mask
            Ok((val.bitxor(mask), width))
        }
        Condition::Signal(path) => {
            // Get signal entry from cache
            let entry = signal_cache
                .get(path)
                .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: path.clone() })?;

            // Read composite value (handles both bus and bit-slice)
            let value = read_signal_composite(entry, waveform, time_idx)?;
            Ok((value, entry.width))
        }
        Condition::BitExtract(path, msb, lsb) => {
            // Get signal entry from cache
            let entry = signal_cache
                .get(path)
                .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: path.clone() })?;

            let full_width = entry.width;

            // Read composite value (handles both bus and bit-slice)
            let full_value = read_signal_composite(entry, waveform, time_idx)?;

            // Extract the specified bits and determine the result width
            let (result, width) = match (msb, lsb) {
                (Some(msb), Some(lsb)) => {
                    if msb < lsb {
                        return Err(WaveAnalyzerError::ConditionParseError {
                            message: format!(
                                "Invalid bit range [{}:{}] - msb must be >= lsb",
                                msb, lsb
                            ),
                        });
                    }
                    // Extract bits [msb:lsb] by:
                    // 1. Shift right by lsb positions
                    // 2. Create a mask with (msb - lsb + 1) bits set
                    let num_bits = msb - lsb + 1;
                    let shifted = full_value >> lsb;
                    let mask = (BigUint::from(1u32) << num_bits) - BigUint::from(1u32);
                    (shifted & mask, num_bits)
                }
                _ => (full_value, full_width), // No bit extraction needed
            };
            Ok((result, width))
        }
        Condition::Eq(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked == right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1, // Comparison operations return 1-bit result
            ))
        }
        Condition::Neq(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked != right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Lt(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked < right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Le(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked <= right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Gt(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked > right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Ge(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let min_width = left_width.min(right_width);
            let mask = (BigUint::from(1u32) << min_width) - BigUint::from(1u32);
            let left_masked = left_val & &mask;
            let right_masked = right_val & mask;
            Ok((
                if left_masked >= right_masked {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Add(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let width = left_width.max(right_width) + 1;
            Ok((left_val + right_val, width))
        }
        Condition::Sub(left, right) => {
            let (left_val, left_width) =
                evaluate_condition_with_width(left, waveform, signal_cache, time_idx)?;
            let (right_val, right_width) =
                evaluate_condition_with_width(right, waveform, signal_cache, time_idx)?;
            let width = left_width.max(right_width);
            Ok((
                if left_val >= right_val {
                    left_val - right_val
                } else {
                    BigUint::from(0u32)
                },
                width,
            ))
        }
        Condition::Rose(path) => {
            if time_idx == 0 {
                return Ok((BigUint::from(0u32), 1));
            }
            let current = evaluate_signal_at(path, waveform, signal_cache, time_idx)?;
            let past = evaluate_signal_at(path, waveform, signal_cache, time_idx - 1)?;
            Ok((
                if past.is_zero() && !current.is_zero() {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Fell(path) => {
            if time_idx == 0 {
                return Ok((BigUint::from(0u32), 1));
            }
            let current = evaluate_signal_at(path, waveform, signal_cache, time_idx)?;
            let past = evaluate_signal_at(path, waveform, signal_cache, time_idx - 1)?;
            Ok((
                if !past.is_zero() && current.is_zero() {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Stable(path) => {
            if time_idx == 0 {
                return Ok((BigUint::from(0u32), 1));
            }
            let current = evaluate_signal_at(path, waveform, signal_cache, time_idx)?;
            let past = evaluate_signal_at(path, waveform, signal_cache, time_idx - 1)?;
            Ok((
                if current == past {
                    BigUint::from(1u32)
                } else {
                    BigUint::from(0u32)
                },
                1,
            ))
        }
        Condition::Literal(literal) => literal_to_biguint(literal),
        Condition::Past(expr) => {
            if time_idx == 0 {
                return Ok((BigUint::from(0u32), 1));
            }
            evaluate_condition_with_width(expr, waveform, signal_cache, time_idx - 1)
        }
        Condition::PastN(expr, n) => {
            let n_usize = *n as usize;
            if time_idx < n_usize {
                return Ok((BigUint::from(0u32), 1));
            }
            evaluate_condition_with_width(expr, waveform, signal_cache, time_idx - n_usize)
        }
    }
}

/// Evaluate a signal's BigUint value at a given time index.
/// Used by $rose, $fell, $stable operators.
fn evaluate_signal_at(
    path: &str,
    waveform: &mut wellen::simple::Waveform,
    signal_cache: &std::collections::HashMap<String, SignalCacheEntry>,
    time_idx: usize,
) -> WaveResult<BigUint> {
    let entry = signal_cache
        .get(path)
        .ok_or_else(|| WaveAnalyzerError::SignalNotFound {
            path: path.to_string(),
        })?;
    read_signal_composite(entry, waveform, time_idx)
}

/// Convert a literal to BigUint for comparison.
/// Also returns the bit width.
fn literal_to_biguint(literal: &Literal) -> WaveResult<(BigUint, u32)> {
    match literal {
        Literal::Binary(bits, width) => {
            let mut value = BigUint::from(0u32);
            for (i, &bit) in bits.iter().rev().enumerate() {
                if bit {
                    value.set_bit(i as u64, true);
                }
            }
            Ok((value, *width))
        }
        Literal::Decimal(v, width) => Ok((BigUint::from(*v), *width)),
        Literal::Hexadecimal(v, width) => Ok((BigUint::from(*v), *width)),
    }
}

/// Parse a binary literal (e.g., "4'b0101") from the condition grammar.
///
/// This function is called by the lalrpop-generated parser.
pub(super) fn parse_binary_literal(s: &str) -> WaveResult<Literal> {
    let lower = s.to_lowercase();
    let parts: Vec<&str> = lower.split('\'').collect();
    if parts.len() != 2 {
        return Err(WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid binary literal: {}", s),
        });
    }

    let width: u32 = parts[0]
        .parse()
        .map_err(|_| WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid bit width in binary literal: {}", s),
        })?;
    let value_str = parts[1].trim_start_matches('b').replace('_', "");
    let mut bits = Vec::new();
    for c in value_str.chars() {
        match c {
            '0' => bits.push(false),
            '1' => bits.push(true),
            _ => {
                return Err(WaveAnalyzerError::ConditionParseError {
                    message: format!("Invalid binary digit '{}' in literal: {}", c, s),
                });
            }
        }
    }
    Ok(Literal::Binary(bits, width))
}

/// Parse a decimal literal (e.g., "3'd2") from the condition grammar.
///
/// This function is called by the lalrpop-generated parser.
pub(super) fn parse_decimal_literal(s: &str) -> WaveResult<Literal> {
    let lower = s.to_lowercase();
    let parts: Vec<&str> = lower.split('\'').collect();
    if parts.len() != 2 {
        return Err(WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid decimal literal: {}", s),
        });
    }

    let width: u32 = parts[0]
        .parse()
        .map_err(|_| WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid bit width in decimal literal: {}", s),
        })?;
    let value_str = parts[1].trim_start_matches('d').replace('_', "");
    let value: u64 = value_str
        .parse()
        .map_err(|_| WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid decimal value in literal: {}", s),
        })?;
    Ok(Literal::Decimal(value, width))
}

/// Parse a bare decimal literal (e.g., "0", "1", "42") from the condition grammar.
///
/// Per Verilog convention, unsized decimal literals are treated as at least 32-bit,
/// which avoids width mismatch issues when comparing against multi-bit signals.
/// The comparison operators mask both sides to `min(left_width, right_width)`,
/// so a 32-bit literal compared against a 3-bit signal is masked to 3 bits,
/// giving the correct result.
pub(super) fn parse_unsized_decimal_literal(s: &str) -> WaveResult<Literal> {
    let value: u64 = s
        .parse()
        .map_err(|_| WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid decimal value in literal: {}", s),
        })?;
    // Verilog convention: unsized literals default to 32-bit width
    let width: u32 = 32;
    Ok(Literal::Decimal(value, width))
}

/// Parse a hex literal (e.g., "5'h1A") from the condition grammar.
///
/// This function is called by the lalrpop-generated parser.
pub(super) fn parse_hex_literal(s: &str) -> WaveResult<Literal> {
    let lower = s.to_lowercase();
    let parts: Vec<&str> = lower.split('\'').collect();
    if parts.len() != 2 {
        return Err(WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid hex literal: {}", s),
        });
    }

    let width: u32 = parts[0]
        .parse()
        .map_err(|_| WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid bit width in hex literal: {}", s),
        })?;
    let value_str = parts[1].trim_start_matches('h').replace('_', "");
    let value = u64::from_str_radix(&value_str, 16).map_err(|_| {
        WaveAnalyzerError::ConditionParseError {
            message: format!("Invalid hex value in literal: {}", s),
        }
    })?;
    Ok(Literal::Hexadecimal(value, width))
}

/// Find events where a condition is satisfied.
///
/// # Arguments
/// * `waveform` - The waveform to read from (must have signals loaded)
/// * `condition` - The condition to evaluate (e.g., "TOP.signal1 && TOP.signal2")
/// * `start_idx` - Starting time index (inclusive)
/// * `end_idx` - Ending time index (inclusive)
/// * `limit` - Maximum number of events to return. Use -1 for unlimited.
///
/// # Returns
/// A vector of formatted event strings, or an error if the operation fails.
pub fn find_conditional_events(
    waveform: &mut wellen::simple::Waveform,
    condition: &str,
    start_idx: usize,
    end_idx: usize,
    limit: isize,
) -> WaveResult<Vec<String>> {
    // Get timescale before any mutable operations
    let timescale = waveform.hierarchy().timescale();

    // Parse condition
    let condition_ast = parse_condition(condition)?;

    // Extract all signal names from condition
    let signal_names = extract_signal_names(&condition_ast);

    // Find and load all signals
    let mut signal_cache = std::collections::HashMap::new();
    let hierarchy = waveform.hierarchy();

    // Collect all signal_refs first (including bit-slice components)
    let mut all_signal_refs = Vec::new();
    for signal_name in &signal_names {
        let entry = build_signal_cache_entry(hierarchy, signal_name)?;
        all_signal_refs.extend_from_slice(&entry.signal_refs);
        signal_cache.insert(signal_name.clone(), entry);
    }

    // Load all signals at once
    waveform.load_signals(&all_signal_refs);

    // Get time table after loading signals
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();

    let mut events = Vec::new();
    let mut prev_result = num_bigint::BigUint::zero();

    // Scan through time indices, only report state-change entry points
    let end = end_idx.min(time_table.len().saturating_sub(1));
    for (idx, &time_value) in time_table[start_idx..=end].iter().enumerate() {
        let time_idx = start_idx + idx;
        let result = evaluate_condition(&condition_ast, waveform, &signal_cache, time_idx)?;
        // Only report when condition transitions from false (0) to true (non-0)
        if prev_result.is_zero() && !result.is_zero() {
            let formatted_time = format_time(time_value, timescale.as_ref());

            // Build event description with signal values
            let mut signal_values = Vec::new();
            for signal_name in &signal_names {
                if let Some(entry) = signal_cache.get(signal_name) {
                    if let Ok(value) = read_signal_composite(entry, waveform, time_idx) {
                        let value_str = format_biguint_verilog(&value, entry.width);
                        signal_values.push(format!("{} = {}", signal_name, value_str));
                    }
                }
            }

            events.push(format!(
                "Time index {} ({}): {}",
                time_idx,
                formatted_time,
                signal_values.join(", ")
            ));
        }

        // Check limit
        if limit >= 0 && events.len() >= limit as usize {
            break;
        }

        prev_result = result;
    }

    Ok(events)
}

/// Extract all signal names from a condition AST.
pub fn extract_signal_names(condition: &Condition) -> Vec<String> {
    let mut names = Vec::new();
    extract_signal_names_recursive(condition, &mut names);
    names
}

fn extract_signal_names_recursive(condition: &Condition, names: &mut Vec<String>) {
    match condition {
        Condition::And(left, right)
        | Condition::Or(left, right)
        | Condition::Eq(left, right)
        | Condition::Neq(left, right)
        | Condition::Lt(left, right)
        | Condition::Le(left, right)
        | Condition::Gt(left, right)
        | Condition::Ge(left, right)
        | Condition::Add(left, right)
        | Condition::Sub(left, right)
        | Condition::BitwiseAnd(left, right)
        | Condition::BitwiseOr(left, right)
        | Condition::BitwiseXor(left, right) => {
            extract_signal_names_recursive(left, names);
            extract_signal_names_recursive(right, names);
        }
        Condition::Not(expr)
        | Condition::BitwiseNot(expr)
        | Condition::Past(expr)
        | Condition::PastN(expr, _) => {
            extract_signal_names_recursive(expr, names);
        }
        Condition::Signal(path)
        | Condition::BitExtract(path, _, _)
        | Condition::Rose(path)
        | Condition::Fell(path)
        | Condition::Stable(path) => {
            if !names.contains(path) {
                names.push(path.clone());
            }
        }
        Condition::Literal(_) => {}
    }
}
