//! I2C protocol template analysis.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::hierarchy::find_signal_by_path;
use crate::protocol::{ClockMeasurement, measure_clock};
use crate::protocol_template::shared::{
    collect_changes, collect_posedge_indices, read_bit_at_time_from_changes,
};

/// I2C protocol analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct I2cAnalysisResult {
    pub transaction_count: usize,
    pub start_condition_count: usize,
    pub stop_condition_count: usize,
    pub ack_count: usize,
    pub nack_count: usize,
    pub scl_measurement: ClockMeasurement,
}

enum SclSdaChange {
    Scl(bool),
    Sda(bool),
}

/// Detect I2C START and STOP conditions.
/// START: SDA falls (down) while SCL is high
/// STOP:  SDA rises (up) while SCL is high
fn detect_i2c_start_stop(
    scl_changes: &[(u32, bool)],
    sda_changes: &[(u32, bool)],
) -> (Vec<u32>, Vec<u32>) {
    let mut starts: Vec<u32> = Vec::new();
    let mut stops: Vec<u32> = Vec::new();

    let mut merged: Vec<(u32, SclSdaChange)> = Vec::new();
    for &(t, is_high) in scl_changes {
        merged.push((t, SclSdaChange::Scl(is_high)));
    }
    for &(t, is_high) in sda_changes {
        merged.push((t, SclSdaChange::Sda(is_high)));
    }
    merged.sort_by_key(|(t, _)| *t);

    let mut scl_state = true;
    let mut sda_state = true;

    for (time, change) in &merged {
        match change {
            SclSdaChange::Scl(high) => scl_state = *high,
            SclSdaChange::Sda(high) => {
                if scl_state {
                    let prev_sda = sda_state;
                    sda_state = *high;
                    if prev_sda && !*high {
                        starts.push(*time);
                    } else if !prev_sda && *high {
                        stops.push(*time);
                    }
                } else {
                    sda_state = *high;
                }
            }
        }
    }

    (starts, stops)
}

/// Count I2C transactions: each START followed by a STOP = 1 transaction.
fn count_i2c_transactions(starts: &[u32], stops: &[u32]) -> usize {
    let mut count = 0;
    for &start_time in starts {
        if stops.iter().any(|&stop_time| stop_time > start_time) {
            count += 1;
        }
    }
    count
}

/// Detect ACK/NACK on I2C bus.
fn detect_i2c_ack_nack(scl_changes: &[(u32, bool)], sda_changes: &[(u32, bool)]) -> (usize, usize) {
    let mut ack_count = 0usize;
    let mut nack_count = 0usize;

    let posedges = collect_posedge_indices(scl_changes);

    for chunk in posedges.chunks(9) {
        if chunk.len() == 9 {
            let ninth_posedge = chunk[8];
            let sda_at_ninth = read_bit_at_time_from_changes(sda_changes, ninth_posedge as usize);
            if let Some(sda_high) = sda_at_ninth {
                if !sda_high {
                    ack_count += 1;
                } else {
                    nack_count += 1;
                }
            }
        }
    }

    (ack_count, nack_count)
}

pub(super) fn analyze_i2c(
    waveform: &mut wellen::simple::Waveform,
    signals: &HashMap<String, String>,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<crate::protocol_template::ProtocolAnalysisResult> {
    let scl_path = signals
        .get("scl")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "I2C template requires 'scl' signal".to_string(),
        })?;
    let sda_path = signals
        .get("sda")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "I2C template requires 'sda' signal".to_string(),
        })?;

    let hierarchy = waveform.hierarchy();
    let scl_ref = find_signal_by_path(hierarchy, scl_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: scl_path.to_string(),
        }
    })?;
    let sda_ref = find_signal_by_path(hierarchy, sda_path).ok_or_else(|| {
        WaveAnalyzerError::SignalNotFound {
            path: sda_path.to_string(),
        }
    })?;
    waveform.load_signals(&[scl_ref, sda_ref]);

    let scl_measurement = measure_clock(waveform, scl_path, "posedge", start_idx, end_idx)?;

    let scl_signal = waveform
        .get_signal(scl_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Failed to get SCL signal".to_string(),
        ))?;
    let sda_signal = waveform
        .get_signal(sda_ref)
        .ok_or(WaveAnalyzerError::Other(
            "Failed to get SDA signal".to_string(),
        ))?;
    let scl_changes = collect_changes(scl_signal, start_idx, end_idx);
    let sda_changes = collect_changes(sda_signal, start_idx, end_idx);

    let (start_conditions, stop_conditions) = detect_i2c_start_stop(&scl_changes, &sda_changes);
    let transaction_count = count_i2c_transactions(&start_conditions, &stop_conditions);
    let (ack_count, nack_count) = detect_i2c_ack_nack(&scl_changes, &sda_changes);

    Ok(crate::protocol_template::ProtocolAnalysisResult {
        protocol: crate::protocol_template::ProtocolTemplate::I2c,
        signal_mapping: signals.clone(),
        spi_result: None,
        uart_result: None,
        i2c_result: Some(I2cAnalysisResult {
            transaction_count,
            start_condition_count: start_conditions.len(),
            stop_condition_count: stop_conditions.len(),
            ack_count,
            nack_count,
            scl_measurement,
        }),
        axi_lite_result: None,
    })
}
