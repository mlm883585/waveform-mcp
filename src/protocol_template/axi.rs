//! AXI-Lite protocol template analysis.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::hierarchy::find_signal_by_path;
use crate::protocol::HandshakeReport;
use crate::protocol::analyze_handshake;
use crate::protocol_template::shared::collect_changes;

/// An AXI-Lite protocol violation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AxiViolation {
    /// Violation kind (e.g., "ready_without_valid", "valid_stall_timeout", "data_without_handshake").
    pub kind: String,
    /// Channel where violation occurred ("read" or "write").
    pub channel: String,
    /// Time index where violation was detected.
    pub time_index: u64,
    /// Human-readable description of the violation.
    pub description: String,
}

/// AXI-Lite protocol analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AxiLiteAnalysisResult {
    pub read_handshakes: HandshakeReport,
    pub write_handshakes: HandshakeReport,
    pub violations: Vec<AxiViolation>,
}

enum SignalKind {
    Valid,
    Ready,
}

/// Detect READY asserted while VALID is low.
fn detect_ready_without_valid(
    valid_changes: &[(u32, bool)],
    ready_changes: &[(u32, bool)],
    channel: &str,
    violations: &mut Vec<AxiViolation>,
) {
    let mut merged: Vec<(u32, SignalKind, bool)> = Vec::new();
    for &(t, is_high) in valid_changes {
        merged.push((t, SignalKind::Valid, is_high));
    }
    for &(t, is_high) in ready_changes {
        merged.push((t, SignalKind::Ready, is_high));
    }
    merged.sort_by_key(|(t, _, _)| *t);

    let mut valid_high = false;
    for (time, kind, is_high) in &merged {
        match kind {
            SignalKind::Valid => valid_high = *is_high,
            SignalKind::Ready => {
                if *is_high && !valid_high {
                    violations.push(AxiViolation {
                        kind: "ready_without_valid".to_string(),
                        channel: channel.to_string(),
                        time_index: *time as u64,
                        description: format!(
                            "READY asserted on {} channel at time {} while VALID is low",
                            channel, time
                        ),
                    });
                }
            }
        }
    }
}

/// Detect VALID asserted for an extended period without READY.
/// Threshold: 16 clock cycles of VALID without READY completing handshake.
fn detect_valid_stall_timeout(
    valid_changes: &[(u32, bool)],
    ready_changes: &[(u32, bool)],
    channel: &str,
    violations: &mut Vec<AxiViolation>,
    end_idx: usize,
) {
    let mut merged: Vec<(u32, SignalKind, bool)> = Vec::new();
    for &(t, is_high) in valid_changes {
        merged.push((t, SignalKind::Valid, is_high));
    }
    for &(t, is_high) in ready_changes {
        merged.push((t, SignalKind::Ready, is_high));
    }
    merged.sort_by_key(|(t, _, _)| *t);

    let mut valid_high = false;
    let mut valid_assert_time: Option<u32> = None;
    let mut stall_count = 0u32;
    const STALL_THRESHOLD: u32 = 16;

    for (time, kind, is_high) in &merged {
        match kind {
            SignalKind::Valid => {
                if *is_high && !valid_high {
                    valid_assert_time = Some(*time);
                    stall_count = 0;
                    valid_high = true;
                } else if !*is_high && valid_high {
                    if stall_count >= STALL_THRESHOLD {
                        let assert_time = valid_assert_time.unwrap_or(*time);
                        violations.push(AxiViolation {
                            kind: "valid_stall_timeout".to_string(),
                            channel: channel.to_string(),
                            time_index: assert_time as u64,
                            description: format!(
                                "VALID on {} channel stalled for {} cycles starting at time {} without READY",
                                channel, stall_count, assert_time
                            ),
                        });
                    }
                    valid_high = false;
                    valid_assert_time = None;
                    stall_count = 0;
                }
            }
            SignalKind::Ready => {
                if valid_high && !*is_high {
                    stall_count += 1;
                } else if valid_high && *is_high {
                    stall_count = 0;
                }
            }
        }
    }

    if valid_high && stall_count >= STALL_THRESHOLD {
        let assert_time = valid_assert_time.unwrap_or(end_idx as u32);
        violations.push(AxiViolation {
            kind: "valid_stall_timeout".to_string(),
            channel: channel.to_string(),
            time_index: assert_time as u64,
            description: format!(
                "VALID on {} channel stalled for {} cycles starting at time {} without READY",
                channel, stall_count, assert_time
            ),
        });
    }
}

/// Detect AXI-Lite protocol violations.
fn detect_axi_violations(
    waveform: &mut wellen::simple::Waveform,
    arvalid_path: &str,
    arready_path: &str,
    awvalid_path: &str,
    awready_path: &str,
    start_idx: usize,
    end_idx: usize,
) -> Vec<AxiViolation> {
    let mut violations: Vec<AxiViolation> = Vec::new();

    let hierarchy = waveform.hierarchy();

    let arvalid_ref = find_signal_by_path(hierarchy, arvalid_path);
    let arready_ref = find_signal_by_path(hierarchy, arready_path);
    let awvalid_ref = find_signal_by_path(hierarchy, awvalid_path);
    let awready_ref = find_signal_by_path(hierarchy, awready_path);

    // Check read channel violations
    if let (Some(vr), Some(rr)) = (arvalid_ref, arready_ref) {
        waveform.load_signals(&[vr, rr]);
        let valid_sig = waveform.get_signal(vr);
        let ready_sig = waveform.get_signal(rr);

        if let (Some(vs), Some(rs)) = (valid_sig, ready_sig) {
            let valid_changes = collect_changes(vs, start_idx, end_idx);
            let ready_changes = collect_changes(rs, start_idx, end_idx);

            detect_ready_without_valid(&valid_changes, &ready_changes, "read", &mut violations);
            detect_valid_stall_timeout(
                &valid_changes,
                &ready_changes,
                "read",
                &mut violations,
                end_idx,
            );
        }
    }

    // Check write channel violations
    if let (Some(vr), Some(rr)) = (awvalid_ref, awready_ref) {
        waveform.load_signals(&[vr, rr]);
        let valid_sig = waveform.get_signal(vr);
        let ready_sig = waveform.get_signal(rr);

        if let (Some(vs), Some(rs)) = (valid_sig, ready_sig) {
            let valid_changes = collect_changes(vs, start_idx, end_idx);
            let ready_changes = collect_changes(rs, start_idx, end_idx);

            detect_ready_without_valid(&valid_changes, &ready_changes, "write", &mut violations);
            detect_valid_stall_timeout(
                &valid_changes,
                &ready_changes,
                "write",
                &mut violations,
                end_idx,
            );
        }
    }

    violations
}

pub(super) fn analyze_axi_lite(
    waveform: &mut wellen::simple::Waveform,
    signals: &HashMap<String, String>,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<crate::protocol_template::ProtocolAnalysisResult> {
    let arvalid = signals
        .get("arvalid")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "AXI-Lite template requires 'arvalid' signal".to_string(),
        })?;
    let arready = signals
        .get("arready")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "AXI-Lite template requires 'arready' signal".to_string(),
        })?;
    let awvalid = signals
        .get("awvalid")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "AXI-Lite template requires 'awvalid' signal".to_string(),
        })?;
    let awready = signals
        .get("awready")
        .ok_or_else(|| WaveAnalyzerError::InvalidArgument {
            message: "AXI-Lite template requires 'awready' signal".to_string(),
        })?;

    let rdata = signals.get("rdata");
    let wdata = signals.get("wdata");

    let read_handshakes = analyze_handshake(
        waveform,
        arvalid,
        arready,
        rdata.map(|s| s.as_str()),
        start_idx,
        end_idx,
        None,
        "summary",
        false,
    )?;

    let write_handshakes = analyze_handshake(
        waveform,
        awvalid,
        awready,
        wdata.map(|s| s.as_str()),
        start_idx,
        end_idx,
        None,
        "summary",
        false,
    )?;

    let violations = detect_axi_violations(
        waveform, arvalid, arready, awvalid, awready, start_idx, end_idx,
    );

    Ok(crate::protocol_template::ProtocolAnalysisResult {
        protocol: crate::protocol_template::ProtocolTemplate::AxiLite,
        signal_mapping: signals.clone(),
        spi_result: None,
        uart_result: None,
        i2c_result: None,
        axi_lite_result: Some(AxiLiteAnalysisResult {
            read_handshakes,
            write_handshakes,
            violations,
        }),
    })
}
