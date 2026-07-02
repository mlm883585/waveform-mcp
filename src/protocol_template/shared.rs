//! Cross-protocol helper functions shared by SPI, UART, I2C, and AXI-Lite analysis.

use crate::formatting::is_signal_high;

/// Collect signal changes as (time_index, is_high) pairs within a range.
pub(super) fn collect_changes(
    signal: &wellen::Signal,
    start_idx: usize,
    end_idx: usize,
) -> Vec<(u32, bool)> {
    let mut changes: Vec<(u32, bool)> = Vec::new();
    for (time_idx, value) in signal.iter_changes() {
        let idx = time_idx as usize;
        if idx >= start_idx && idx <= end_idx {
            changes.push((time_idx, is_signal_high(&value)));
        }
    }
    changes
}

/// Find CS-low windows: each falling edge to next rising edge.
/// Returns (cs_fall_time, cs_rise_time) pairs as inclusive boundaries.
pub(super) fn find_cs_low_windows(
    cs_changes: &[(u32, bool)],
    _start_idx: usize,
    end_idx: usize,
) -> Vec<(u32, u32)> {
    let mut windows: Vec<(u32, u32)> = Vec::new();
    let mut in_low = false;
    let mut low_start: u32 = 0;

    for &(time_idx, is_high) in cs_changes {
        if !in_low && !is_high {
            low_start = time_idx;
            in_low = true;
        } else if in_low && is_high {
            windows.push((low_start, time_idx));
            in_low = false;
        }
    }

    if in_low {
        windows.push((low_start, end_idx as u32));
    }

    windows
}

/// Collect posedge time indices from a change stream.
pub(super) fn collect_posedge_indices(changes: &[(u32, bool)]) -> Vec<u32> {
    let mut prev_high: Option<bool> = None;
    let mut indices: Vec<u32> = Vec::new();
    for &(t, is_high) in changes {
        if prev_high == Some(false) && is_high {
            indices.push(t);
        }
        prev_high = Some(is_high);
    }
    indices
}

/// Collect negedge time indices from a change stream.
pub(super) fn collect_negedge_indices(changes: &[(u32, bool)]) -> Vec<u32> {
    let mut prev_high: Option<bool> = None;
    let mut indices: Vec<u32> = Vec::new();
    for &(t, is_high) in changes {
        if prev_high == Some(true) && !is_high {
            indices.push(t);
        }
        prev_high = Some(is_high);
    }
    indices
}

/// Count signal changes within a time window.
pub(super) fn count_changes_in_window(
    signal: &wellen::Signal,
    window_start: u32,
    window_end: u32,
) -> usize {
    let mut count = 0;
    for (time_idx, _) in signal.iter_changes() {
        if time_idx >= window_start && time_idx <= window_end {
            count += 1;
        }
    }
    count
}

/// Read a 1-bit signal value at a specific time index, return as bool.
pub(super) fn read_bit_at_time(
    waveform: &mut wellen::simple::Waveform,
    signal_ref: wellen::SignalRef,
    time_idx: u32,
) -> Option<bool> {
    let signal = waveform.get_signal(signal_ref)?;
    let time_table_idx: wellen::TimeTableIdx = time_idx;
    let offset = signal.get_offset(time_table_idx)?;
    Some(is_signal_high(&signal.get_value_at(&offset, 0)))
}

/// Convert a bit vector to hex string (MSB-first, padded to byte boundary).
pub(super) fn bits_to_hex(bits: &[bool]) -> String {
    let bit_count = bits.len();
    let padded_len = bit_count.div_ceil(8) * 8;
    let mut padded_bits: Vec<bool> = bits.to_vec();
    while padded_bits.len() < padded_len {
        padded_bits.insert(0, false);
    }

    let mut hex_chars = String::new();
    for chunk in padded_bits.chunks(8) {
        let byte_val: u8 = chunk
            .iter()
            .fold(0u8, |acc, &b| (acc << 1) | if b { 1 } else { 0 });
        hex_chars.push_str(&format!("{:02x}", byte_val));
    }

    format!("{}'h{}", bit_count, hex_chars)
}

/// Read a bit value from a change stream at a given time index.
/// Finds the last change before or at the specified time.
pub(super) fn read_bit_at_time_from_changes(
    changes: &[(u32, bool)],
    time_idx: usize,
) -> Option<bool> {
    let mut result: Option<bool> = None;
    for &(t, is_high) in changes {
        if t as usize <= time_idx {
            result = Some(is_high);
        } else {
            break;
        }
    }
    result
}

/// Find the next rising edge after a given time in a change stream.
pub(super) fn find_next_rising_after(changes: &[(u32, bool)], after_time: u32) -> Option<u32> {
    let mut prev_high: Option<bool> = None;
    for &(t, is_high) in changes {
        if t > after_time && prev_high == Some(false) && is_high {
            return Some(t);
        }
        prev_high = Some(is_high);
    }
    None
}

/// Find the next falling edge after a given time in a change stream.
pub(super) fn find_next_falling_after(changes: &[(u32, bool)], after_time: u32) -> Option<u32> {
    let mut prev_high: Option<bool> = None;
    for &(t, is_high) in changes {
        if t > after_time && prev_high == Some(true) && !is_high {
            return Some(t);
        }
        prev_high = Some(is_high);
    }
    None
}
