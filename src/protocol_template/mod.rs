//! Standard protocol template library.
//!
//! Provides parameterized protocol analysis for SPI, UART, I2C, and AXI-Lite.
//! Each template defines signal roles and delegates to existing measurement
//! functions for protocol-specific analysis.

mod axi;
mod i2c;
mod shared;
mod spi;
mod uart;

#[allow(unused_imports)]
// WaveAnalyzerError imported for type documentation; WaveResult used directly
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::ReportWriter;
use crate::{report_write, report_writeln};
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wellen::simple::Waveform;

// Re-export protocol-specific result types
pub use axi::{AxiLiteAnalysisResult, AxiViolation};
pub use i2c::I2cAnalysisResult;
pub use spi::{SpiAnalysisResult, SpiTransaction};
pub use uart::UartAnalysisResult;

// === Data Structures ===

/// Supported protocol template types.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ProtocolTemplate {
    Spi,
    Uart,
    I2c,
    AxiLite,
}

impl ProtocolTemplate {
    pub fn from_str_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "spi" => Some(ProtocolTemplate::Spi),
            "uart" => Some(ProtocolTemplate::Uart),
            "i2c" => Some(ProtocolTemplate::I2c),
            "axi_lite" | "axilite" | "axi4-lite" => Some(ProtocolTemplate::AxiLite),
            _ => None,
        }
    }
}

/// Combined protocol template analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProtocolAnalysisResult {
    pub protocol: ProtocolTemplate,
    pub signal_mapping: HashMap<String, String>,
    pub spi_result: Option<SpiAnalysisResult>,
    pub uart_result: Option<UartAnalysisResult>,
    pub i2c_result: Option<I2cAnalysisResult>,
    pub axi_lite_result: Option<AxiLiteAnalysisResult>,
}

// === Core Functions ===

/// Analyze a waveform using a standard protocol template.
pub fn analyze_protocol_template(
    waveform: &mut Waveform,
    protocol: &ProtocolTemplate,
    signal_mapping: &HashMap<String, String>,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<ProtocolAnalysisResult> {
    let result = match protocol {
        ProtocolTemplate::Spi => spi::analyze_spi(waveform, signal_mapping, start_idx, end_idx)?,
        ProtocolTemplate::Uart => uart::analyze_uart(waveform, signal_mapping, start_idx, end_idx)?,
        ProtocolTemplate::I2c => i2c::analyze_i2c(waveform, signal_mapping, start_idx, end_idx)?,
        ProtocolTemplate::AxiLite => {
            axi::analyze_axi_lite(waveform, signal_mapping, start_idx, end_idx)?
        }
    };

    Ok(result)
}

// === Report Formatting ===

/// Format a protocol template analysis result as human-readable text.
pub fn format_protocol_template_report(result: &ProtocolAnalysisResult) -> String {
    let mut out = ReportWriter::new();

    match result.protocol {
        ProtocolTemplate::Spi => {
            report_writeln!(out, "=== SPI Protocol Analysis ===");
            if let Some(ref spi) = result.spi_result {
                report_writeln!(out, "Clock (SCLK):");
                report_writeln!(
                    out,
                    "  Period: avg={:.2}, min={}, max={}",
                    spi.clock_measurement.period.avg,
                    spi.clock_measurement.period.min,
                    spi.clock_measurement.period.max
                );
                if let Some(ref freq) = spi.clock_measurement.frequency_hz {
                    report_writeln!(out, "  Frequency: {:.0} Hz", freq);
                }
                report_writeln!(out, "  Polarity: {}", spi.clock_polarity);
                report_writeln!(out, "  Phase: {}", spi.clock_phase);
                report_writeln!(
                    out,
                    "CS: {} low pulses detected (transactions)",
                    spi.transaction_count
                );
                report_writeln!(out, "MOSI changes: {}", spi.mosi_change_count);
                report_writeln!(out, "MISO changes: {}", spi.miso_change_count);
                if !spi.transactions.is_empty() {
                    report_writeln!(out, "\nTransactions ({}):", spi.transactions.len());
                    for (i, tx) in spi.transactions.iter().enumerate() {
                        report_write!(
                            out,
                            "  #{}: CS-low [{}, {}], {} bits",
                            i + 1,
                            tx.start_time_index,
                            tx.end_time_index,
                            tx.bit_count
                        );
                        if let Some(ref mosi) = tx.mosi_data {
                            report_write!(out, ", MOSI={}", mosi);
                        }
                        if let Some(ref miso) = tx.miso_data {
                            report_write!(out, ", MISO={}", miso);
                        }
                        report_writeln!(out);
                    }
                }
            }
        }
        ProtocolTemplate::Uart => {
            report_writeln!(out, "=== UART Protocol Analysis ===");
            if let Some(ref uart) = result.uart_result {
                report_writeln!(out, "Baud rate measurement:");
                report_writeln!(
                    out,
                    "  Bit period: avg={:.2}",
                    uart.baud_rate_measurement.period.avg
                );
                if let Some(ref freq) = uart.baud_rate_measurement.frequency_hz {
                    report_writeln!(out, "  Estimated baud rate: {:.0} bps", freq);
                }
                report_writeln!(out, "Frames: {}", uart.frame_count);
                if uart.start_bit_width_stats.count > 0 {
                    report_writeln!(
                        out,
                        "Start bit width: avg={:.2}, min={:.0}, max={:.0}",
                        uart.start_bit_width_stats.avg,
                        uart.start_bit_width_stats.min,
                        uart.start_bit_width_stats.max
                    );
                }
                if uart.stop_bit_width_stats.count > 0 {
                    report_writeln!(
                        out,
                        "Stop bit width: avg={:.2}, min={:.0}, max={:.0}",
                        uart.stop_bit_width_stats.avg,
                        uart.stop_bit_width_stats.min,
                        uart.stop_bit_width_stats.max
                    );
                }
                report_writeln!(out, "Parity errors: {}", uart.parity_errors);
                report_writeln!(out, "Framing errors: {}", uart.framing_errors);
            }
        }
        ProtocolTemplate::I2c => {
            report_writeln!(out, "=== I2C Protocol Analysis ===");
            if let Some(ref i2c) = result.i2c_result {
                report_writeln!(out, "SCL clock:");
                report_writeln!(out, "  Period: avg={:.2}", i2c.scl_measurement.period.avg);
                if let Some(ref freq) = i2c.scl_measurement.frequency_hz {
                    report_writeln!(out, "  Frequency: {:.0} Hz", freq);
                }
                report_writeln!(out, "Transactions: {}", i2c.transaction_count);
                report_writeln!(out, "Start conditions: {}", i2c.start_condition_count);
                report_writeln!(out, "Stop conditions: {}", i2c.stop_condition_count);
                report_writeln!(out, "ACK count: {}", i2c.ack_count);
                report_writeln!(out, "NACK count: {}", i2c.nack_count);
            }
        }
        ProtocolTemplate::AxiLite => {
            report_writeln!(out, "=== AXI-Lite Protocol Analysis ===");
            if let Some(ref axi) = result.axi_lite_result {
                report_writeln!(out, "Read channel (ARVALID/ARREADY):");
                report_writeln!(
                    out,
                    "  Handshakes: {}",
                    axi.read_handshakes.summary.total_handshakes
                );
                report_writeln!(
                    out,
                    "  Avg latency: {:.2}",
                    axi.read_handshakes.summary.avg_latency
                );
                report_writeln!(out, "Write channel (AWVALID/AWREADY):");
                report_writeln!(
                    out,
                    "  Handshakes: {}",
                    axi.write_handshakes.summary.total_handshakes
                );
                report_writeln!(
                    out,
                    "  Avg latency: {:.2}",
                    axi.write_handshakes.summary.avg_latency
                );
                if !axi.violations.is_empty() {
                    report_writeln!(out, "\nViolations ({}):", axi.violations.len());
                    for v in &axi.violations {
                        report_writeln!(
                            out,
                            "  [{}] {} channel @ {}: {}",
                            v.kind,
                            v.channel,
                            v.time_index,
                            v.description
                        );
                    }
                }
            }
        }
    }

    // Signal mapping
    report_writeln!(out, "\nSignal mapping:");
    for (role, path) in &result.signal_mapping {
        report_writeln!(out, "  {} -> {}", role, path);
    }

    out.finish()
}
