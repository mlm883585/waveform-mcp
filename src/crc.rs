//! CRC computation and verification for waveform data.
//!
//! Computes standard CRC polynomials over data bus values and optionally
//! compares with observed CRC values from the waveform.
//!
//! Handles both proper bus signals (e.g. `i_data[7:0]` stored as one VCD
//! variable) and bit-slice decomposed signals (e.g. `o_crc` stored as 16
//! individual 1-bit wires in ModelSim VCD dumps).
//!
//! When a `data_valid_signal_path` is provided, only data values at time
//! indices where data_valid transitions from 0→1 (posedge) are processed.
//! This ensures that only valid data bytes are fed into the CRC computation,
//! matching hardware behavior where CRC is only updated when data_valid=1.

use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};
use wellen::simple::Waveform;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::{
    ReportWriter, format_time, is_signal_high, signal_value_to_biguint_strict,
};
use crate::hierarchy::{get_signal_width, resolve_signal_var_refs};
use crate::report_writeln;

/// Supported CRC polynomials.
#[derive(Debug, Clone, PartialEq)]
pub enum CrcPolynomial {
    Crc8,
    Crc16Ccitt,
    Crc32Ethernet,
}

impl CrcPolynomial {
    /// Get the polynomial coefficient (without the leading x^n term).
    pub fn poly(&self) -> u64 {
        match self {
            CrcPolynomial::Crc8 => 0x07,
            CrcPolynomial::Crc16Ccitt => 0x1021,
            CrcPolynomial::Crc32Ethernet => 0x04C11DB7,
        }
    }

    /// Get the initial value (reset value) for this polynomial.
    pub fn init_value(&self) -> u64 {
        match self {
            CrcPolynomial::Crc8 => 0x00,
            CrcPolynomial::Crc16Ccitt => 0xFFFF,
            CrcPolynomial::Crc32Ethernet => 0xFFFFFFFF,
        }
    }

    /// Get the bit width of this CRC.
    pub fn width(&self) -> u32 {
        match self {
            CrcPolynomial::Crc8 => 8,
            CrcPolynomial::Crc16Ccitt => 16,
            CrcPolynomial::Crc32Ethernet => 32,
        }
    }

    /// Compute one step of CRC: crc = update(crc, data_byte).
    pub fn update_byte(&self, crc: u64, data_byte: u8) -> u64 {
        let width = self.width();
        let poly = self.poly();
        let mut crc = crc ^ ((data_byte as u64) << (width - 8));

        for _ in 0..8 {
            if crc & (1 << (width - 1)) != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
        }

        // Mask to width bits
        let mask = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        crc & mask
    }
}

/// Parse a CRC polynomial from string.
pub fn parse_crc_polynomial(s: &str) -> WaveResult<CrcPolynomial> {
    match s.to_lowercase().as_str() {
        "crc8" | "crc_8" => Ok(CrcPolynomial::Crc8),
        "crc16" | "crc16_ccitt" | "crc_16_ccitt" => Ok(CrcPolynomial::Crc16Ccitt),
        "crc32" | "crc32_ethernet" | "crc_32_ethernet" => Ok(CrcPolynomial::Crc32Ethernet),
        _ => Err(WaveAnalyzerError::InvalidArgument {
            message: format!(
                "Unknown CRC polynomial: '{}'. Supported: crc8, crc16_ccitt, crc32_ethernet",
                s
            ),
        }),
    }
}

/// A signal that may be stored as a single bus variable or as individual
/// bit-slice wires. This struct encapsulates the resolution, loading,
/// and value-reading for both cases.
struct CompositeSignal {
    /// Corresponding SignalRefs for each VarRef.
    signal_refs: Vec<wellen::SignalRef>,
    /// Bit position for each VarRef (None for bus, Some(msb) for bit-slices).
    bit_positions: Vec<Option<u32>>,
    /// Total width of the signal in bits.
    width: u32,
    /// Whether this is a single-bus signal (reads as one packed value).
    is_bus: bool,
}

impl CompositeSignal {
    /// Resolve a signal path into a CompositeSignal, handling both
    /// proper bus variables and bit-slice decomposed signals.
    fn resolve(hierarchy: &wellen::Hierarchy, path: &str) -> WaveResult<Self> {
        let var_refs = resolve_signal_var_refs(hierarchy, path).ok_or_else(|| {
            WaveAnalyzerError::SignalNotFound {
                path: path.to_string(),
            }
        })?;
        let width = get_signal_width(hierarchy, path);

        let is_bus = var_refs.len() == 1;

        let mut signal_refs = Vec::with_capacity(var_refs.len());
        let mut bit_positions = Vec::with_capacity(var_refs.len());

        if is_bus {
            // Single bus variable
            let vr = var_refs[0];
            signal_refs.push(hierarchy[vr].signal_ref());
            bit_positions.push(None);
        } else {
            // Bit-slice decomposed — sort by MSB descending for consistent reconstruction
            let mut sorted_refs = var_refs.clone();
            sorted_refs.sort_by_key(|vr| hierarchy[*vr].index().map(|idx| idx.msb()).unwrap_or(0));
            sorted_refs.reverse(); // MSB first

            for vr in &sorted_refs {
                let var = &hierarchy[*vr];
                signal_refs.push(var.signal_ref());
                bit_positions.push(var.index().map(|idx| idx.msb() as u32));
            }
        }

        Ok(Self {
            signal_refs,
            bit_positions,
            width,
            is_bus,
        })
    }

    /// Load all signal refs into the waveform.
    fn load(&self, waveform: &mut Waveform) {
        waveform.load_signals(&self.signal_refs);
    }

    /// Read the composite signal value at a time table index.
    ///
    /// For bus signals: reads the packed value directly.
    /// For decomposed signals: reads each individual bit and reconstructs
    /// the composite value by setting bit positions.
    fn read_at(
        &self,
        waveform: &Waveform,
        time_table_idx: wellen::TimeTableIdx,
    ) -> WaveResult<BigUint> {
        if self.is_bus {
            // Single bus: read directly
            let signal = waveform.get_signal(self.signal_refs[0]).ok_or_else(|| {
                WaveAnalyzerError::Other("Bus signal not found after loading".to_string())
            })?;
            let offset = signal.get_offset(time_table_idx).ok_or_else(|| {
                WaveAnalyzerError::Other("No data at time index for bus signal".to_string())
            })?;
            let signal_value = signal.get_value_at(&offset, 0);
            // Use strict conversion with width masking
            let raw = signal_value_to_biguint_strict(signal_value)?;
            // Mask to declared width (packed bytes may exceed signal bit width)
            if self.width > 0 && self.width < 8192 {
                let mask = (BigUint::from(1u32) << self.width) - BigUint::from(1u32);
                Ok(raw & mask)
            } else {
                Ok(raw)
            }
        } else {
            // Bit-slice decomposed: reconstruct composite from individual bits
            let mut composite = BigUint::zero();

            for (sig_ref, bit_pos) in self.signal_refs.iter().zip(self.bit_positions.iter()) {
                let signal = waveform.get_signal(*sig_ref).ok_or_else(|| {
                    WaveAnalyzerError::Other("Bit-slice signal not found after loading".to_string())
                })?;

                if let Some(offset) = signal.get_offset(time_table_idx) {
                    let value = signal.get_value_at(&offset, 0);
                    if is_signal_high(&value) {
                        if let Some(bp) = bit_pos {
                            composite.set_bit(*bp as u64, true);
                        }
                    }
                }
            }

            // Mask to declared width
            if self.width > 0 && self.width < 8192 {
                let mask = (BigUint::from(1u32) << self.width) - BigUint::from(1u32);
                Ok(composite & mask)
            } else {
                Ok(composite)
            }
        }
    }

    /// Collect all change time indices for this signal within [start, end].
    ///
    /// For bus signals: iterates changes of the single signal.
    /// For decomposed signals: merges change times from all bit-slice signals.
    fn collect_change_times(
        &self,
        waveform: &Waveform,
        start: usize,
        end: usize,
    ) -> std::collections::BTreeSet<usize> {
        let mut change_times = std::collections::BTreeSet::new();

        if self.is_bus {
            // Single bus: collect changes from the one signal
            if let Some(signal) = waveform.get_signal(self.signal_refs[0]) {
                for (time_idx, _) in signal.iter_changes() {
                    let idx = time_idx as usize;
                    if idx >= start && idx <= end {
                        change_times.insert(idx);
                    }
                }
            }
        } else {
            // Bit-slice decomposed: merge change times from all bit signals
            for sig_ref in &self.signal_refs {
                if let Some(signal) = waveform.get_signal(*sig_ref) {
                    for (time_idx, _) in signal.iter_changes() {
                        let idx = time_idx as usize;
                        if idx >= start && idx <= end {
                            change_times.insert(idx);
                        }
                    }
                }
            }
        }

        change_times
    }

    /// Collect time indices where this 1-bit signal transitions from 0→1 (posedge).
    /// Used for data_valid signals to determine when data bytes are valid for CRC.
    fn collect_posedge_times(
        &self,
        waveform: &Waveform,
        start: usize,
        end: usize,
    ) -> std::collections::BTreeSet<usize> {
        let mut posedge_times = std::collections::BTreeSet::new();

        // Only works for 1-bit signals (or the first signal in a decomposed group)
        let sig_ref = self.signal_refs[0];
        if let Some(signal) = waveform.get_signal(sig_ref) {
            let mut prev_high = false;
            for (time_idx, value) in signal.iter_changes() {
                let idx = time_idx as usize;
                if idx >= start && idx <= end {
                    let current_high = is_signal_high(&value);
                    if current_high && !prev_high {
                        // 0→1 transition: posedge
                        posedge_times.insert(idx);
                    }
                    prev_high = current_high;
                } else if idx > end {
                    break;
                } else {
                    // Track state before start
                    prev_high = is_signal_high(&value);
                }
            }
        }

        posedge_times
    }

    /// Collect all time indices where this 1-bit signal is high (1).
    /// Unlike collect_posedge_times which only records 0→1 transitions,
    /// this method iterates over every time table index in [start, end],
    /// using get_offset()+get_value_at() to sample the signal value at each
    /// index. This correctly handles burst valid signals that stay asserted
    /// across multiple consecutive clock cycles — every index where valid=1
    /// is collected, not just the transition point.
    fn collect_high_times(
        &self,
        waveform: &Waveform,
        start: usize,
        end: usize,
    ) -> std::collections::BTreeSet<usize> {
        let mut high_times = std::collections::BTreeSet::new();

        let sig_ref = self.signal_refs[0];
        if let Some(signal) = waveform.get_signal(sig_ref) {
            let time_table_len = waveform.time_table().len();
            let effective_end = end.min(time_table_len.saturating_sub(1));

            for idx in start..=effective_end {
                let time_table_idx: wellen::TimeTableIdx = idx as u32;
                if let Some(offset) = signal.get_offset(time_table_idx) {
                    let value = signal.get_value_at(&offset, 0);
                    if is_signal_high(&value) {
                        high_times.insert(idx);
                    }
                }
            }
        }

        high_times
    }
}

/// A single data point in the CRC computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrcDataPoint {
    /// Time index of this data point.
    pub time_index: u64,
    /// Formatted time string.
    pub time_formatted: String,
    /// Data value at this time.
    pub data_value: String,
    /// Computed CRC value after processing this data.
    pub computed_crc: String,
    /// Observed CRC value from waveform (if available).
    pub observed_crc: Option<String>,
    /// Whether computed and observed CRC match.
    pub crc_match: Option<bool>,
}

/// Result of CRC computation and verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrcResult {
    /// Polynomial name.
    pub polynomial: String,
    /// Initial CRC value (hex).
    pub initial_value: String,
    /// Final computed CRC value (hex).
    pub final_computed_crc: String,
    /// Final observed CRC value (hex), if available.
    pub final_observed_crc: Option<String>,
    /// Whether final CRCs match.
    pub crc_match: Option<bool>,
    /// Total data points processed.
    pub data_points: usize,
    /// Individual data points with CRC values.
    pub points: Vec<CrcDataPoint>,
    /// Warning about sampling method (e.g., event-only without clock/data_valid).
    #[serde(default)]
    pub warning: Option<String>,
}

/// Compute CRC over a data bus signal and optionally verify against observed CRC.
///
/// When `data_valid_signal_path` is provided, only data values at time indices
/// where the data_valid signal transitions from 0→1 (posedge) are processed.
/// This matches hardware behavior where CRC is only updated when data_valid=1,
/// avoiding processing of reset/initialization values that aren't valid data.
///
/// When `clear_signal_path` is provided, the computed CRC is reset to the init
/// value whenever i_clear is high at a data_valid posedge (clear has priority
/// over data in typical CRC RTL: `if (clear) crc <= INIT; else if (valid)...`).
/// Also detects clear pulses between data_valid events and resets CRC.
///
/// When `data_valid_signal_path` is not provided, all data signal changes
/// within the time range are processed. This may include invalid data if the
/// data bus changes when data_valid=0 (e.g., initial reset values).
///
/// CRC comparison: the computed CRC after processing each data byte is compared
/// with the observed CRC at the same time index. For combinational CRC outputs
/// (common in CRC modules: `o_crc = r_crc ^ next_byte_crc`), this works directly.
/// For registered CRC outputs, use the internal register signal (e.g., `r_crc`)
/// and note that comparison may be off by one clock cycle.
#[allow(clippy::too_many_arguments)]
pub fn compute_and_verify_crc(
    waveform: &mut Waveform,
    data_signal_path: &str,
    crc_signal_path: Option<&str>,
    data_valid_signal_path: Option<&str>,
    clear_signal_path: Option<&str>,
    clock_signal_path: Option<&str>,
    crc_polynomial: &str,
    initial_value: Option<u64>,
    start_idx: usize,
    end_idx: usize,
    limit: Option<isize>,
) -> WaveResult<CrcResult> {
    let poly = parse_crc_polynomial(crc_polynomial)?;
    let crc_width = poly.width();
    let init = initial_value.unwrap_or(poly.init_value());

    let time_table_len = waveform.time_table().len();
    let start = start_idx.min(time_table_len.saturating_sub(1));
    let end = end_idx.min(time_table_len.saturating_sub(1));
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale = waveform.hierarchy().timescale();

    // Resolve data signal (handles both bus and bit-slice decomposed)
    let data_signal = CompositeSignal::resolve(waveform.hierarchy(), data_signal_path)?;

    // Resolve data_valid signal (optional, for filtering valid data changes)
    let data_valid_signal = data_valid_signal_path
        .map(|p| CompositeSignal::resolve(waveform.hierarchy(), p))
        .transpose()?;

    // Resolve clear signal (optional, for CRC reset detection)
    let clear_signal = clear_signal_path
        .map(|p| CompositeSignal::resolve(waveform.hierarchy(), p))
        .transpose()?;

    // Resolve CRC signal (optional, handles both bus and bit-slice decomposed)
    let crc_signal = crc_signal_path
        .map(|p| CompositeSignal::resolve(waveform.hierarchy(), p))
        .transpose()?;

    // Load all signals
    data_signal.load(waveform);
    if let Some(ref dv_sig) = data_valid_signal {
        dv_sig.load(waveform);
    }
    if let Some(ref clr_sig) = clear_signal {
        clr_sig.load(waveform);
    }
    if let Some(ref crc_sig) = crc_signal {
        crc_sig.load(waveform);
    }

    // Build the ordered list of events to process.
    // When data_valid is provided: collect data signal change times where
    // data_valid is currently high. This avoids double-sampling at both
    // posedge and negedge clock — only processes data at actual change
    // points while valid=1.
    // When data_valid is not provided: use all data signal change points.
    let mut events: std::collections::BTreeSet<(usize, CrcEvent)> =
        std::collections::BTreeSet::new();

    if let Some(ref dv_sig) = data_valid_signal {
        // Collect data signal change times, filtered by valid=high at each point
        let valid_high_times = dv_sig.collect_high_times(waveform, start, end);
        for idx in data_signal.collect_change_times(waveform, start, end) {
            if valid_high_times.contains(&idx) {
                events.insert((idx, CrcEvent::DataValid));
            }
        }
        // Also sample data at valid posedge if data hasn't changed yet
        for idx in dv_sig.collect_posedge_times(waveform, start, end) {
            if !events.contains(&(idx, CrcEvent::DataValid)) {
                events.insert((idx, CrcEvent::DataValid));
            }
        }
        // Collect clear posedge events (if provided)
        if let Some(ref clr_sig) = clear_signal {
            for idx in clr_sig.collect_posedge_times(waveform, start, end) {
                events.insert((idx, CrcEvent::Clear));
            }
        }
    } else {
        // No data_valid signal provided.
        // BUG-27 fix: when clock_signal is provided, sample data at every
        // clock posedge for per-cycle CRC computation. This correctly handles
        // cases where data stays stable across multiple cycles.
        // When neither data_valid nor clock is provided, use data change
        // events (may miss stable data held across cycles).
        if let Some(clk_path) = clock_signal_path {
            let clock_signal = CompositeSignal::resolve(waveform.hierarchy(), clk_path)?;
            clock_signal.load(waveform);

            // Sample data at every clock posedge
            let clock_posedge_times = clock_signal.collect_posedge_times(waveform, start, end);
            for idx in clock_posedge_times {
                events.insert((idx, CrcEvent::DataValid));
            }

            // Also collect clear events if provided
            if let Some(ref clr_sig) = clear_signal {
                for idx in clr_sig.collect_posedge_times(waveform, start, end) {
                    events.insert((idx, CrcEvent::Clear));
                }
            }
        } else {
            // No clock either: collect data signal changes only
            // This may miss stable data held across cycles — add warning
            for idx in data_signal.collect_change_times(waveform, start, end) {
                events.insert((idx, CrcEvent::DataValid));
            }
            // Also collect clear events if provided
            if let Some(ref clr_sig) = clear_signal {
                for idx in clr_sig.collect_posedge_times(waveform, start, end) {
                    events.insert((idx, CrcEvent::Clear));
                }
            }
        }
    }

    // If no events found, return empty result
    if events.is_empty() {
        return Ok(CrcResult {
            polynomial: crc_polynomial.to_string(),
            initial_value: format!("0x{:X}", init),
            final_computed_crc: format_crc_value(&BigUint::from(init), crc_width),
            final_observed_crc: None,
            crc_match: None,
            data_points: 0,
            points: Vec::new(),
            warning: None,
        });
    }

    // Compute CRC incrementally and compare with observed CRC at each point.
    let mut crc = init;
    let mut points: Vec<CrcDataPoint> = Vec::new();
    let mut last_observed_crc: Option<String> = None;
    let mut last_crc_match: Option<bool> = None;

    for (time_idx, event_type) in events {
        if time_idx >= time_table_len {
            break;
        }

        let time_table_idx: wellen::TimeTableIdx = time_idx
            .try_into()
            .map_err(|_| WaveAnalyzerError::Other(format!("Time index {} too large", time_idx)))?;

        match event_type {
            CrcEvent::Clear => {
                // Clear event: reset CRC to init value
                // In RTL: `if (i_clear) r_crc <= CRC_INIT;` has priority over data_valid.
                // Also check if clear is asserted at the same time as data_valid
                // — but since both events are in the BTreeSet, the clear event
                // will be processed first (same time_idx, Clear < DataValid in ordering).
                // If a DataValid event also exists at this time_idx, it will be
                // processed next, but since CRC was just reset, it will compute
                // from the fresh init state — matching hardware behavior where
                // clear overrides data_valid.
                crc = init;
                // Read observed CRC at this time (shows CRC after reset)
                let computed_str = format_crc_value(&BigUint::from(crc), crc_width);
                let observed_crc_str: Option<String> = if let Some(ref crc_sig) = crc_signal {
                    match crc_sig.read_at(waveform, time_table_idx) {
                        Ok(val) => Some(format_crc_value(&val, crc_width)),
                        Err(_) => None,
                    }
                } else {
                    None
                };
                let crc_match = observed_crc_str.as_ref().map(|obs| obs == &computed_str);
                if let Some(ref s) = observed_crc_str {
                    last_observed_crc = Some(s.clone());
                }
                last_crc_match = crc_match;
                // Don't add a point for clear-only events (they're intermediate resets)
                // Only add data points to the output for clarity
            }
            CrcEvent::DataValid => {
                // Check if clear is also asserted at this time (clear has priority)
                let clear_is_active = if let Some(ref clr_sig) = clear_signal {
                    match clr_sig.read_at(waveform, time_table_idx) {
                        Ok(val) => !val.is_zero(),
                        Err(_) => false,
                    }
                } else {
                    false
                };

                if clear_is_active {
                    // Clear overrides data_valid — reset CRC, don't process data
                    crc = init;
                } else {
                    // Read data value and update CRC
                    let data_value = data_signal.read_at(waveform, time_table_idx)?;

                    // Handle zero-value: BigUint::to_bytes_be() returns empty for zero
                    let byte_count = ((data_signal.width as usize) / 8).max(1);
                    let data_bytes = if data_value.is_zero() {
                        vec![0u8; byte_count]
                    } else {
                        let raw = data_value.to_bytes_be();
                        if raw.len() < byte_count {
                            let mut padded = vec![0u8; byte_count - raw.len()];
                            padded.extend_from_slice(&raw);
                            padded
                        } else {
                            raw
                        }
                    };
                    for &byte in &data_bytes {
                        crc = poly.update_byte(crc, byte);
                    }
                }

                // Computed CRC after this event
                let computed_str = format_crc_value(&BigUint::from(crc), crc_width);

                // Read observed CRC at this time
                let observed_crc_str: Option<String> = if let Some(ref crc_sig) = crc_signal {
                    match crc_sig.read_at(waveform, time_table_idx) {
                        Ok(val) => Some(format_crc_value(&val, crc_width)),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                let crc_match = observed_crc_str.as_ref().map(|obs| obs == &computed_str);

                if let Some(ref s) = observed_crc_str {
                    last_observed_crc = Some(s.clone());
                }
                last_crc_match = crc_match;

                let formatted_time = format_time(time_table[time_idx], timescale.as_ref());

                // For clear-active case, show the reset event
                let data_value_str = if clear_is_active {
                    "CLEAR (reset)".to_string()
                } else {
                    let data_value = data_signal.read_at(waveform, time_table_idx)?;
                    format_value_hex(&data_value, data_signal.width)
                };

                points.push(CrcDataPoint {
                    time_index: time_idx as u64,
                    time_formatted: formatted_time,
                    data_value: data_value_str,
                    computed_crc: computed_str,
                    observed_crc: observed_crc_str,
                    crc_match,
                });

                // Apply limit
                if let Some(lim) = limit
                    && lim > 0
                    && points.len() >= lim as usize
                {
                    break;
                }
            }
        }
    }

    let final_computed = format_crc_value(&BigUint::from(crc), crc_width);

    let warning = if data_valid_signal_path.is_none()
        && clock_signal_path.is_none()
        && !points.is_empty()
    {
        Some("CRC computed from data change events only (no data_valid or clock signal provided). \
              Data held stable across multiple cycles will be processed only once at its change point. \
              Provide --valid or --clock for per-cycle sampling.".to_string())
    } else {
        None
    };

    Ok(CrcResult {
        polynomial: crc_polynomial.to_string(),
        initial_value: format!("0x{:X}", init),
        final_computed_crc: final_computed,
        final_observed_crc: last_observed_crc,
        crc_match: last_crc_match,
        data_points: points.len(),
        points,
        warning,
    })
}

/// Event type in CRC computation timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CrcEvent {
    /// CRC clear/reset event (i_clear posedge).
    Clear,
    /// Data valid event (i_data_valid posedge or data bus change).
    DataValid,
}

/// Format a BigUint as a CRC value with appropriate width.
fn format_crc_value(value: &BigUint, width: u32) -> String {
    format_value_hex(value, width)
}

/// Format a BigUint as hex with width-based padding.
fn format_value_hex(value: &BigUint, width: u32) -> String {
    if value.is_zero() {
        let hex_chars = ((width as usize).div_ceil(4)).max(1);
        return format!("0x{}", "0".repeat(hex_chars));
    }

    let bytes = value.to_bytes_be();
    let hex_str: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let hex_chars = ((width as usize).div_ceil(4)).max(1);
    let padded = if hex_str.len() < hex_chars {
        format!("{:0>width$}", hex_str, width = hex_chars)
    } else if hex_str.len() > hex_chars {
        hex_str[hex_str.len() - hex_chars..].to_string()
    } else {
        hex_str
    };
    format!("0x{}", padded)
}

/// Format a CRC result as human-readable text.
pub fn format_crc_report(result: &CrcResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "=== CRC Computation Report ===");
    report_writeln!(out, "Polynomial: {}", result.polynomial);
    report_writeln!(out, "Initial Value: {}", result.initial_value);
    report_writeln!(out, "Data points processed: {}", result.data_points);
    report_writeln!(out, "Final computed CRC: {}", result.final_computed_crc);

    if let Some(ref obs) = result.final_observed_crc {
        report_writeln!(out, "Final observed CRC: {}", obs);
        match result.crc_match {
            Some(true) => report_writeln!(out, "Result: CRC MATCH"),
            Some(false) => report_writeln!(out, "Result: CRC MISMATCH"),
            None => report_writeln!(out, "Result: Could not determine"),
        };
    }

    if let Some(ref warning) = result.warning {
        report_writeln!(out, "Warning: {}", warning);
    }

    if !result.points.is_empty() {
        report_writeln!(out, "{}", "-".repeat(80));
        report_writeln!(
            out,
            "{:<10} {:<15} {:<20} {:<15} {:<15} {:<8}",
            "TimeIdx",
            "Data",
            "Computed CRC",
            "Observed CRC",
            "Time",
            "Match"
        );
        report_writeln!(out, "{}", "-".repeat(80));

        for point in &result.points {
            let obs = point.observed_crc.as_deref().unwrap_or("-");
            let match_str = point
                .crc_match
                .map(|m| if m { "OK" } else { "MISMATCH" })
                .unwrap_or("-");
            report_writeln!(
                out,
                "{:<10} {:<20} {:<20} {:<15} {:<15} {:<8}",
                point.time_index,
                point.data_value,
                point.computed_crc,
                obs,
                point.time_formatted,
                match_str
            );
        }
    }

    out.finish()
}
