//! SPI protocol template analysis.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::hierarchy::find_signal_by_path;
use crate::protocol::{ClockMeasurement, PulseMeasurement, measure_clock, measure_pulses};
use crate::protocol_template::shared::{
    bits_to_hex, collect_changes, collect_negedge_indices, collect_posedge_indices,
    count_changes_in_window, find_cs_low_windows, read_bit_at_time,
};

/// A single SPI transaction (CS-low window).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SpiTransaction {
    pub start_time_index: u64,
    pub end_time_index: u64,
    pub mosi_data: Option<String>,
    pub miso_data: Option<String>,
    pub bit_count: usize,
}

/// SPI protocol analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SpiAnalysisResult {
    pub clock_measurement: ClockMeasurement,
    pub cs_pulse_measurement: PulseMeasurement,
    pub transaction_count: usize,
    pub clock_polarity: String,
    pub clock_phase: String,
    pub mosi_change_count: usize,
    pub miso_change_count: usize,
    pub transactions: Vec<SpiTransaction>,
}

/// Capture edge type determined by CPOL and CPHA.
enum CaptureEdge {
    Posedge,
    Negedge,
}

fn determine_capture_edge(cpol: &str, cpha: &str) -> CaptureEdge {
    match (cpol, cpha) {
        ("CPOL0", "CPHA0") | ("CPOL1", "CPHA1") => CaptureEdge::Posedge,
        ("CPOL0", "CPHA1") | ("CPOL1", "CPHA0") => CaptureEdge::Negedge,
        _ => CaptureEdge::Posedge,
    }
}

/// Detect SPI clock polarity (CPOL) by sampling SCLK's idle level.
///
/// CPOL is the SCLK level when the bus is idle (CS deasserted). The previous
/// implementation inferred CPOL from duty cycle, which is unsound — both
/// CPOL=0 and CPOL=1 clocks routinely run at ~50% duty. This helper samples
/// SCLK at a moment when CS is high:
///
/// - If `cs_changes` has a first falling edge (CS going low), sample SCLK
///   at the time index immediately *before* that edge. At that moment CS is
///   still high, so SCLK is at its idle level.
/// - If the bus starts with CS already low (unusual) or there are no CS
///   changes at all, fall back to sampling SCLK at `start_idx`.
///
/// Returns `"CPOL1"` if the idle level is high, `"CPOL0"` otherwise.
fn detect_cpol(
    waveform: &mut wellen::simple::Waveform,
    sclk_ref: wellen::SignalRef,
    cs_changes: &[(u32, bool)],
    start_idx: usize,
) -> &'static str {
    // Find the first CS falling edge (transition high → low) within the
    // observed change stream. Track the prior value so we don't miss a
    // falling edge that crosses the start boundary.
    let mut prev_high: Option<bool> = None;
    let sample_time: u32 = if let Some((fall_t, _)) = cs_changes.iter().find(|(_, is_high)| {
        let falling = prev_high == Some(true) && !*is_high;
        prev_high = Some(*is_high);
        falling
    }) {
        // Sample one time-step before the first CS-low edge. Saturating at 0
        // is fine — if the very first recorded change is a CS falling edge,
        // the bus was idle from t=0, so t=0 is the most "idle-like" sample
        // we can get from the recorded stream.
        fall_t.saturating_sub(1)
    } else {
        // No CS falling edge in the window — bus is idle throughout. Sample
        // at start_idx to read the recorded SCLK level there.
        start_idx as u32
    };

    match read_bit_at_time(waveform, sclk_ref, sample_time) {
        Some(true) => "CPOL1",
        Some(false) => "CPOL0",
        // No recorded SCLK value at the sample point: default to CPOL0
        // (the most common case in standard SPI peripherals). This is a
        // best-effort fallback; callers can override by post-processing.
        None => "CPOL0",
    }
}

/// Detect CPHA by checking whether MOSI data changes happen near posedge or negedge of SCLK.
fn detect_cpha(
    mosi_ref: Option<wellen::SignalRef>,
    waveform: &mut wellen::simple::Waveform,
    sclk_changes: &[(u32, bool)],
    cs_windows: &[(u32, u32)],
) -> String {
    if mosi_ref.is_none() {
        return "CPHA0".to_string();
    }

    let mosi_ref = mosi_ref.unwrap();
    let mosi_signal = match waveform.get_signal(mosi_ref) {
        Some(sig) => sig,
        None => return "CPHA0".to_string(),
    };

    let posedge_indices: Vec<u32> = sclk_changes
        .iter()
        .filter(|&(_, is_high)| *is_high)
        .map(|&(t, _)| t)
        .collect();

    let negedge_indices: Vec<u32> = sclk_changes
        .iter()
        .filter(|&(_, is_high)| !*is_high)
        .map(|&(t, _)| t)
        .collect();

    let mut posedge_aligned = 0usize;
    let mut negedge_aligned = 0usize;

    for (time_idx, _) in mosi_signal.iter_changes() {
        let idx = time_idx as usize;
        let in_window = cs_windows
            .iter()
            .any(|&(s, e)| idx >= s as usize && idx <= e as usize);
        if !in_window {
            continue;
        }
        if posedge_indices.contains(&time_idx) {
            posedge_aligned += 1;
        }
        if negedge_indices.contains(&time_idx) {
            negedge_aligned += 1;
        }
    }

    if posedge_aligned > negedge_aligned {
        "CPHA1".to_string()
    } else {
        "CPHA0".to_string()
    }
}

/// Decode data bits from a signal at the given capture edges, return as hex string.
fn decode_bits_at_edges(
    waveform: &mut wellen::simple::Waveform,
    signal_ref: wellen::SignalRef,
    edges: &[u32],
) -> Option<String> {
    if edges.is_empty() {
        return None;
    }

    let mut bits: Vec<bool> = Vec::new();
    for &edge_time in edges {
        if let Some(bit) = read_bit_at_time(waveform, signal_ref, edge_time) {
            bits.push(bit);
        }
    }

    if bits.is_empty() {
        return None;
    }

    Some(bits_to_hex(&bits))
}

pub(super) fn analyze_spi(
    waveform: &mut wellen::simple::Waveform,
    signals: &HashMap<String, String>,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<crate::protocol_template::ProtocolAnalysisResult> {
    let sclk_path = signals
        .get("sclk")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "SPI template requires 'sclk' signal".to_string(),
        })?;
    let cs_path = signals
        .get("cs")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "SPI template requires 'cs' signal".to_string(),
        })?;
    let mosi_path = signals.get("mosi");
    let miso_path = signals.get("miso");

    let clock_measurement = measure_clock(waveform, sclk_path, "posedge", start_idx, end_idx)?;
    let cs_pulse_measurement = measure_pulses(waveform, cs_path, start_idx, end_idx)?;

    let hierarchy = waveform.hierarchy();
    let sclk_ref = find_signal_by_path(hierarchy, sclk_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: sclk_path.to_string(),
        }
    })?;
    let cs_ref = find_signal_by_path(hierarchy, cs_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: cs_path.to_string(),
        }
    })?;

    let mosi_ref = mosi_path
        .as_ref()
        .and_then(|p| find_signal_by_path(hierarchy, p));
    let miso_ref = miso_path
        .as_ref()
        .and_then(|p| find_signal_by_path(hierarchy, p));

    let mut refs_to_load = vec![sclk_ref, cs_ref];
    if let Some(mr) = mosi_ref {
        refs_to_load.push(mr);
    }
    if let Some(mr) = miso_ref {
        refs_to_load.push(mr);
    }
    waveform.load_signals(&refs_to_load);

    let cs_signal = waveform.get_signal(cs_ref).ok_or(WaveAnalyzerError::Other(
        "Failed to get CS signal".to_string(),
    ))?;
    let cs_changes = collect_changes(cs_signal, start_idx, end_idx);
    let cs_windows = find_cs_low_windows(&cs_changes, start_idx, end_idx);

    let sclk_signal = waveform
        .get_signal(sclk_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Failed to get SCLK signal".to_string(),
        ))?;
    let sclk_posedge_changes = collect_changes(sclk_signal, start_idx, end_idx);

    // CPOL detection: CPOL is defined as the SCLK idle level (level when CS is
    // deasserted / bus is inactive). The previous implementation used duty-cycle
    // (`> 50%` → CPOL1) which is incorrect — CPOL0/CPOL1 clocks both commonly run
    // at ~50% duty cycle. The correct method: sample SCLK at a moment when CS is
    // high (i.e., just before the first CS-low falling edge, or at start_idx if
    // the bus starts idle).
    let cpol = detect_cpol(waveform, sclk_ref, &cs_changes, start_idx);

    let cpha = detect_cpha(mosi_ref, waveform, &sclk_posedge_changes, &cs_windows);
    let capture_edge = determine_capture_edge(cpol, &cpha);

    let capture_edges = match capture_edge {
        CaptureEdge::Posedge => collect_posedge_indices(&sclk_posedge_changes),
        CaptureEdge::Negedge => collect_negedge_indices(&sclk_posedge_changes),
    };

    let mut transactions: Vec<SpiTransaction> = Vec::new();
    let mut mosi_change_count = 0usize;
    let mut miso_change_count = 0usize;

    for (cs_start, cs_end) in &cs_windows {
        let edges_in_window: Vec<u32> = capture_edges
            .iter()
            .filter(|&e| *e >= *cs_start && *e <= *cs_end)
            .copied()
            .collect();

        if let Some(mr) = mosi_ref
            && let Some(sig) = waveform.get_signal(mr)
        {
            mosi_change_count += count_changes_in_window(sig, *cs_start, *cs_end);
        }
        if let Some(mr) = miso_ref
            && let Some(sig) = waveform.get_signal(mr)
        {
            miso_change_count += count_changes_in_window(sig, *cs_start, *cs_end);
        }

        let mosi_data = if let Some(mr) = mosi_ref {
            decode_bits_at_edges(waveform, mr, &edges_in_window)
        } else {
            None
        };
        let miso_data = if let Some(mr) = miso_ref {
            decode_bits_at_edges(waveform, mr, &edges_in_window)
        } else {
            None
        };

        let bit_count = edges_in_window.len();

        transactions.push(SpiTransaction {
            start_time_index: *cs_start as u64,
            end_time_index: *cs_end as u64,
            mosi_data,
            miso_data,
            bit_count,
        });
    }

    let transaction_count = cs_pulse_measurement.low_pulse_count;

    Ok(crate::protocol_template::ProtocolAnalysisResult {
        protocol: crate::protocol_template::ProtocolTemplate::Spi,
        signal_mapping: signals.clone(),
        spi_result: Some(SpiAnalysisResult {
            clock_measurement,
            cs_pulse_measurement,
            transaction_count,
            clock_polarity: cpol.to_string(),
            clock_phase: cpha,
            mosi_change_count,
            miso_change_count,
            transactions,
        }),
        uart_result: None,
        i2c_result: None,
        axi_lite_result: None,
    })
}
