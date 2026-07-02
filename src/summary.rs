//! Waveform summary and sampling utilities
//!
//! Provides tools for generating waveform summaries for preview cards,
//! including signal sampling and downsampling.

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::extract::BitMappingEntry;
use crate::formatting::{
    format_biguint_value, format_signal_value, format_time, is_signal_high, signal_value_to_biguint,
};
use crate::hierarchy::{find_var_by_path, resolve_signal_with_width};
use num_traits::Zero;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wellen::simple::Waveform;

/// A single sampled point from a signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplePoint {
    /// Time index in the waveform
    pub time_index: u64,
    /// Signal value as string (e.g., "8'h0A", "8'hFF")
    pub value: String,
}

/// Summary of a single signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalSummary {
    /// Hierarchical path to the signal
    pub path: String,
    /// Signal width in bits
    pub width: u32,
    /// Signal type (wire, reg, clock, etc.)
    pub signal_type: String,
    /// Sampled values at reduced resolution
    pub samples: Vec<SamplePoint>,
}

/// Full waveform summary response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveformSummary {
    /// Waveform ID (alias or filename)
    pub waveform_id: String,
    /// Original filename
    pub filename: String,
    /// Summary of each requested signal
    pub signals: Vec<SignalSummary>,
    /// Time range (start, end) in time indices
    pub time_range: (u64, u64),
    /// BUG-7 fix: physical time range formatted strings
    pub time_range_formatted: (String, String),
    /// Timescale string (e.g., "1ns")
    pub timescale: String,
    /// Total number of samples per signal
    pub sample_count: usize,
}

/// Request parameters for get_waveform_summary
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WaveformSummaryRequest {
    /// Path to the waveform file
    pub file_path: String,
    /// List of signal paths to summarize (empty = auto-detect top signals)
    pub signals: Vec<String>,
    /// Maximum number of samples per signal (default: 100)
    pub max_samples: Option<usize>,
}

/// Request parameters for export_waveform_svg
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportSvgRequest {
    /// Waveform ID (from open_waveform)
    pub waveform_id: String,
    /// List of signal paths to include
    pub signals: Vec<String>,
    /// Time range to export (start, end) in time indices
    pub time_range: Option<(u64, u64)>,
    /// Output image width in pixels (default: 800)
    pub width: Option<u32>,
    /// Output image height in pixels (default: 600)
    pub height: Option<u32>,
}

/// Response for export_waveform_svg
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportSvgResponse {
    /// SVG content as string
    pub svg_content: String,
    /// Path to saved SVG file (if saved to disk)
    pub file_path: String,
}

/// Detect signal type from signal name and hierarchy variable type.
/// BUG-3/24/VarType fix: uses both name pattern matching and the centralized
/// `classify_var_type` function to correctly classify signals, avoiding
/// misclassifying integer/logic/bit types as "wire" and ensuring all 37
/// wellen VarType variants are handled.
fn detect_signal_type(name: &str, var_type: wellen::VarType) -> String {
    let lower = name.to_lowercase();
    if lower.contains("clk") || lower.contains("clock") {
        "clock".to_string()
    } else if lower.contains("rst")
        || lower.contains("reset")
        || lower.contains("arst")
        || lower.contains("srst")
        || lower.contains("nrst")
    {
        "reset".to_string()
    } else {
        crate::hierarchy::classify_var_type(var_type).to_string()
    }
}

/// Downsample signal changes while preserving value transitions
pub fn downsample_signal_changes(
    changes: &[(u32, wellen::SignalValue<'_>)],
    max_samples: usize,
) -> Vec<SamplePoint> {
    if changes.is_empty() || max_samples == 0 {
        return Vec::new();
    }

    if changes.len() <= max_samples {
        return changes
            .iter()
            .map(|(time, value)| SamplePoint {
                time_index: *time as u64,
                value: format_signal_value(*value),
            })
            .collect();
    }

    // Sample across change points while avoiding phase-locking on perfectly
    // alternating signals such as clocks. If the first two values differ and
    // the computed stride is even, bump it to an odd stride so both phases
    // remain visible in the preview.
    let mut result: Vec<SamplePoint> = Vec::with_capacity(max_samples.min(changes.len()));
    let mut step = changes.len().div_ceil(max_samples);
    if changes.len() >= 2
        && format_signal_value(changes[0].1) != format_signal_value(changes[1].1)
        && step % 2 == 0
    {
        step += 1;
    }

    for idx in (0..changes.len()).step_by(step) {
        let (time, value) = changes[idx];
        result.push(SamplePoint {
            time_index: time as u64,
            value: format_signal_value(value),
        });
    }

    if result
        .last()
        .is_none_or(|sample| sample.time_index != changes.last().unwrap().0 as u64)
    {
        let (time, value) = changes[changes.len() - 1];
        result.push(SamplePoint {
            time_index: time as u64,
            value: format_signal_value(value),
        });
    }

    result
}

/// Generate a summary for a waveform file
pub fn generate_waveform_summary(
    waveform: &mut Waveform,
    waveform_id: &str,
    signal_paths: &[String],
    max_samples: Option<usize>,
) -> WaveResult<WaveformSummary> {
    let max_samples = max_samples.unwrap_or(100);
    if max_samples == 0 {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "max_samples must be greater than 0".into(),
        });
    }
    let hierarchy = waveform.hierarchy();
    let time_table_len = waveform.time_table().len();

    if time_table_len == 0 {
        return Err(WaveAnalyzerError::Other(
            "Waveform has no time entries".into(),
        ));
    }

    // Get timescale from hierarchy
    let timescale = hierarchy
        .timescale()
        .map(|ts| crate::formatting::format_timescale(&ts))
        .unwrap_or_else(|| "1ns".to_string());

    // Find signals and collect SignalRefs
    let mut signal_refs: Vec<wellen::SignalRef> = Vec::new();
    let mut signal_infos = Vec::new();

    for path in signal_paths {
        let (signal_ref, width) = resolve_signal_with_width(hierarchy, path)?;

        signal_refs.push(signal_ref);
        // BUG-3/24 fix: use variable type from hierarchy for classification
        let var_type = if let Some(vr) = crate::hierarchy::find_var_by_path(hierarchy, path) {
            hierarchy[vr].var_type()
        } else {
            wellen::VarType::Wire // default fallback
        };
        let signal_type = detect_signal_type(path, var_type);
        signal_infos.push((path.clone(), width, signal_type));
    }

    // Load signals into waveform
    waveform.load_signals(&signal_refs);

    // Sample each signal
    let mut signals = Vec::new();

    for (i, signal_ref) in signal_refs.iter().enumerate() {
        let (path, width, signal_type) = &signal_infos[i];

        // Get signal and iterate through changes
        let signal = waveform
            .get_signal(*signal_ref)
            .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: path.clone() })?;

        let changes: Vec<_> = signal.iter_changes().collect();

        // Downsample
        let samples = downsample_signal_changes(&changes, max_samples);

        signals.push(SignalSummary {
            path: path.clone(),
            width: *width,
            signal_type: signal_type.clone(),
            samples,
        });
    }

    let time_range = (0u64, (time_table_len - 1) as u64);
    let wt: Vec<wellen::Time> = waveform.time_table().to_vec();
    let ts_opt = waveform.hierarchy().timescale();
    let start_formatted = format_time(wt[0], ts_opt.as_ref());
    let end_formatted = format_time(wt[time_table_len - 1], ts_opt.as_ref());
    let time_range_formatted = (start_formatted, end_formatted);

    Ok(WaveformSummary {
        waveform_id: waveform_id.to_string(),
        filename: waveform_id.to_string(),
        signals,
        time_range,
        time_range_formatted,
        timescale,
        sample_count: max_samples,
    })
}

/// Export waveform to SVG format
///
/// Export waveform visualization as SVG with actual waveform traces.
///
/// Renders:
/// - 1-bit signals: square wave transitions (high/low levels)
/// - Multi-bit signals: bus transitions with hex value labels
/// - Time axis ruler at bottom
/// - Signal name labels on left
pub fn export_waveform_to_svg(
    waveform: &mut Waveform,
    signal_paths: &[String],
    time_range: Option<(u64, u64)>,
    width: Option<u32>,
    height: Option<u32>,
) -> WaveResult<ExportSvgResponse> {
    let img_width = width.unwrap_or(800);
    let img_height = height.unwrap_or(600);
    let time_table_len = waveform.time_table().len();
    if time_table_len == 0 {
        return Err(WaveAnalyzerError::Other(
            "Waveform has no time entries".into(),
        ));
    }

    let (start_idx, end_idx) = match time_range {
        Some((s, e)) => {
            let si = s as usize;
            let ei = e as usize;
            if si >= time_table_len {
                return Err(WaveAnalyzerError::Other(format!(
                    "time-range start ({}) is out of bounds (max: {})",
                    si,
                    time_table_len - 1
                )));
            }
            let ei_clamped = ei.min(time_table_len - 1);
            if si > ei_clamped {
                return Err(WaveAnalyzerError::Other(format!(
                    "time-range start ({}) must be <= end ({})",
                    si, ei_clamped
                )));
            }
            (si, ei_clamped)
        }
        None => (0, time_table_len.saturating_sub(1)),
    };

    let timescale = waveform.hierarchy().timescale();
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();

    // Layout constants
    let left_margin = 140; // Space for signal names
    let bottom_margin = 40; // Space for time axis
    let top_margin = 30; // Header space
    let row_height_single = 30; // Row height for 1-bit signals
    let row_height_bus = 40; // Row height for multi-bit signals
    let trace_area_width = img_width - left_margin;
    let _trace_area_height = img_height - top_margin - bottom_margin;

    // Load all signals
    let mut signal_refs: Vec<wellen::SignalRef> = Vec::new();
    let mut signal_info: Vec<(String, u32, wellen::SignalRef)> = Vec::new();

    for path in signal_paths {
        let (sr, width) = resolve_signal_with_width(waveform.hierarchy(), path)?;
        signal_refs.push(sr);
        signal_info.push((path.clone(), width, sr));
    }
    waveform.load_signals(&signal_refs);

    // Calculate start/end time values
    let start_time = time_table[start_idx];
    let end_time = time_table[end_idx.min(time_table_len - 1)];
    let time_span = if (end_time - start_time) as f64 > 0.0 {
        (end_time - start_time) as f64
    } else {
        1.0 // Avoid division by zero for single-point waveforms
    };

    // Build SVG
    let mut svg = Vec::new();

    // SVG header
    svg.push(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
        img_width, img_height
    ));

    // Background
    svg.push(format!(
        "<rect width=\"{}\" height=\"{}\" fill=\"#1e1e2e\"/>",
        img_width, img_height
    ));

    // Header text
    svg.push(format!(
        "<text x=\"{}\" y=\"18\" fill=\"#cdd6f4\" font-size=\"13\" font-family=\"monospace\">Waveform</text>",
        left_margin
    ));
    svg.push(format!(
        "<text x=\"{}\" y=\"18\" fill=\"#6c7086\" font-size=\"10\" font-family=\"monospace\">{} - {}</text>",
        left_margin + 80,
        format_time(start_time, timescale.as_ref()),
        format_time(end_time, timescale.as_ref())
    ));

    // Vertical grid lines (time markers)
    let grid_count = 10;
    for i in 0..=grid_count {
        let x = left_margin + (trace_area_width as f64 * i as f64 / grid_count as f64) as u32;
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#313244\" stroke-width=\"1\"/>",
            x,
            top_margin,
            x,
            img_height - bottom_margin
        ));
        // Time label at bottom
        let fraction = i as f64 / grid_count as f64;
        let t = start_time + (fraction * time_span) as wellen::Time;
        svg.push(format!(
            "<text x=\"{}\" y=\"{}\" fill=\"#6c7086\" font-size=\"9\" font-family=\"monospace\" text-anchor=\"middle\">{}</text>",
            x,
            img_height - bottom_margin + 15,
            format_time(t, timescale.as_ref())
        ));
    }

    // Left separator line
    svg.push(format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#45475a\" stroke-width=\"1\"/>",
        left_margin,
        top_margin,
        left_margin,
        img_height - bottom_margin
    ));

    // Draw each signal trace
    let mut y_offset = top_margin;

    for (path, sig_width, sig_ref) in &signal_info {
        let row_height = if *sig_width <= 1 {
            row_height_single
        } else {
            row_height_bus
        };
        if y_offset + row_height > img_height - bottom_margin {
            break; // No more space
        }

        let signal = waveform
            .get_signal(*sig_ref)
            .ok_or_else(|| WaveAnalyzerError::SignalNotFound { path: path.clone() })?;

        // Signal name label (shortened if too long)
        let display_name = if path.len() > 20 {
            path.split('.').next_back().unwrap_or(path).to_string()
        } else {
            path.clone()
        };
        svg.push(format!(
            "<text x=\"5\" y=\"{}\" fill=\"#cdd6f4\" font-size=\"10\" font-family=\"monospace\" clip-path=\"url(#nameClip)\">{}</text>",
            y_offset + row_height / 2 + 4,
            display_name
        ));

        if *sig_width <= 1 {
            // 1-bit signal: draw square wave
            draw_single_bit_trace(
                &mut svg,
                signal,
                &time_table,
                start_idx,
                end_idx,
                left_margin,
                y_offset,
                row_height,
                trace_area_width,
                start_time,
                time_span,
            );
        } else {
            // Multi-bit signal: draw bus with hex labels
            draw_bus_trace(
                &mut svg,
                signal,
                &time_table,
                start_idx,
                end_idx,
                left_margin,
                y_offset,
                row_height,
                trace_area_width,
                start_time,
                time_span,
                *sig_width,
                timescale.as_ref(),
            );
        }

        // Row separator line
        svg.push(format!(
            "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#313244\" stroke-width=\"0.5\"/>",
            y_offset + row_height, img_width, y_offset + row_height
        ));

        y_offset += row_height;
    }

    // Time axis bottom line
    svg.push(format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#45475a\" stroke-width=\"1\"/>",
        left_margin,
        img_height - bottom_margin,
        img_width,
        img_height - bottom_margin
    ));

    svg.push("</svg>".to_string());

    let svg_content = svg.join("\n");

    Ok(ExportSvgResponse {
        svg_content,
        file_path: String::new(),
    })
}

/// Draw a single-bit signal trace as a square wave in SVG.
#[allow(clippy::too_many_arguments)]
fn draw_single_bit_trace(
    svg: &mut Vec<String>,
    signal: &wellen::Signal,
    time_table: &[wellen::Time],
    start_idx: usize,
    end_idx: usize,
    left_margin: u32,
    y_offset: u32,
    row_height: u32,
    trace_width: u32,
    start_time: wellen::Time,
    time_span: f64,
) {
    let high_y = y_offset + 4;
    let low_y = y_offset + row_height - 4;

    // Collect change events within time range
    let mut changes: Vec<(usize, bool)> = Vec::new();
    for (time_idx, value) in signal.iter_changes() {
        let idx = time_idx as usize;
        if idx < start_idx || idx > end_idx {
            continue;
        }
        let is_high = is_signal_high(&value);
        changes.push((idx, is_high));
    }

    if changes.is_empty() {
        // No changes - draw flat line at low level
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#a6e3a1\" stroke-width=\"1.5\"/>",
            left_margin, low_y, left_margin + trace_width, low_y
        ));
        return;
    }

    // Build SVG path for square wave
    let mut path_parts: Vec<String> = Vec::new();

    // Initial state: move to first change point
    let (first_idx, first_high) = changes[0];
    let first_x = time_to_x(
        first_idx,
        time_table,
        start_time,
        time_span,
        left_margin,
        trace_width,
    );
    let _first_y = if first_high { high_y } else { low_y };

    // Draw initial level from left_margin to first change
    let initial_high = first_high;
    let initial_y = if initial_high { high_y } else { low_y };
    path_parts.push(format!("M {} {}", left_margin, initial_y));
    path_parts.push(format!("L {} {}", first_x, initial_y));

    // Process transitions
    for i in 0..changes.len() {
        let (idx, is_high) = changes[i];
        let x = time_to_x(
            idx,
            time_table,
            start_time,
            time_span,
            left_margin,
            trace_width,
        );
        let y = if is_high { high_y } else { low_y };

        // Vertical transition line
        path_parts.push(format!("L {} {}", x, y));

        // Horizontal level until next change or end
        let next_x = if i + 1 < changes.len() {
            time_to_x(
                changes[i + 1].0,
                time_table,
                start_time,
                time_span,
                left_margin,
                trace_width,
            )
        } else {
            left_margin + trace_width
        };
        path_parts.push(format!("L {} {}", next_x, y));
    }

    svg.push(format!(
        "<path d=\"{}\" stroke=\"#a6e3a1\" stroke-width=\"1.5\" fill=\"none\"/>",
        path_parts.join(" ")
    ));
}

/// Draw a multi-bit bus signal trace with hex value labels in SVG.
#[allow(clippy::too_many_arguments)]
fn draw_bus_trace(
    svg: &mut Vec<String>,
    signal: &wellen::Signal,
    time_table: &[wellen::Time],
    start_idx: usize,
    end_idx: usize,
    left_margin: u32,
    y_offset: u32,
    row_height: u32,
    trace_width: u32,
    start_time: wellen::Time,
    time_span: f64,
    sig_width: u32,
    _timescale: Option<&wellen::Timescale>,
) {
    let mid_y = y_offset + row_height / 2;

    // Collect change events
    let mut changes: Vec<(usize, String)> = Vec::new();
    for (time_idx, value) in signal.iter_changes() {
        let idx = time_idx as usize;
        if idx < start_idx || idx > end_idx {
            continue;
        }
        let val_str = format_signal_value(value);
        changes.push((idx, val_str));
    }

    if changes.is_empty() {
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
            left_margin,
            mid_y,
            left_margin + trace_width,
            mid_y
        ));
        return;
    }

    // Draw bus trace: X transitions at change points, hex labels in stable regions
    for i in 0..changes.len() {
        let (idx, val) = &changes[i];
        let x = time_to_x(
            *idx,
            time_table,
            start_time,
            time_span,
            left_margin,
            trace_width,
        );

        let next_x = if i + 1 < changes.len() {
            time_to_x(
                changes[i + 1].0,
                time_table,
                start_time,
                time_span,
                left_margin,
                trace_width,
            )
        } else {
            left_margin + trace_width
        };

        let segment_width = next_x - x;

        // Draw X transition pattern (diagonal cross at change point)
        if i > 0 {
            let prev_val = &changes[i - 1].1;
            if prev_val != val {
                // Diagonal cross showing bus transition
                let cross_width = 6.min(segment_width / 2);
                svg.push(format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
                    x, y_offset + 2, x + cross_width, y_offset + row_height - 2
                ));
                svg.push(format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
                    x, y_offset + row_height - 2, x + cross_width, y_offset + 2
                ));
            }
        }

        // Hex value label if segment is wide enough
        let label_x = x + if i > 0 { 8 } else { 2 };
        if segment_width > 30 {
            // Simplify value for display
            let display_val = simplify_bus_value(val, sig_width);
            svg.push(format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#89b4fa\" font-size=\"9\" font-family=\"monospace\">{}</text>",
                label_x, mid_y + 4, display_val
            ));
        }

        // Stable region line (top and bottom boundaries)
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
            label_x,
            y_offset + 2,
            next_x,
            y_offset + 2
        ));
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
            label_x,
            y_offset + row_height - 2,
            next_x,
            y_offset + row_height - 2
        ));
    }

    // Initial stable region before first change
    if !changes.is_empty() {
        let (first_idx, first_val) = &changes[0];
        let first_x = time_to_x(
            *first_idx,
            time_table,
            start_time,
            time_span,
            left_margin,
            trace_width,
        );
        let display_val = simplify_bus_value(first_val, sig_width);
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
            left_margin,
            y_offset + 2,
            first_x,
            y_offset + 2
        ));
        svg.push(format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#89b4fa\" stroke-width=\"1\"/>",
            left_margin,
            y_offset + row_height - 2,
            first_x,
            y_offset + row_height - 2
        ));
        if first_x - left_margin > 30 {
            svg.push(format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#89b4fa\" font-size=\"9\" font-family=\"monospace\">{}</text>",
                left_margin + 4, mid_y + 4, display_val
            ));
        }
    }
}

/// Convert a time index to an x pixel coordinate.
fn time_to_x(
    time_idx: usize,
    time_table: &[wellen::Time],
    start_time: wellen::Time,
    time_span: f64,
    left_margin: u32,
    trace_width: u32,
) -> u32 {
    if time_idx >= time_table.len() {
        return left_margin + trace_width;
    }
    let t = time_table[time_idx];
    let fraction = ((t - start_time) as f64) / time_span;
    left_margin + (trace_width as f64 * fraction) as u32
}

/// Simplify a bus value string for SVG display.
fn simplify_bus_value(val: &str, width: u32) -> String {
    // If it looks like a Verilog-style value (e.g., "8'h5A"), extract the hex part
    if let Some(pos) = val.find('\'') {
        let after = &val[pos + 1..];
        if after.len() >= 2 {
            let specifier = &after[0..1];
            let digits = &after[1..];
            match specifier {
                "h" | "H" => {
                    // For hex, show compact form
                    if width <= 8 {
                        format!("0x{}", digits)
                    } else {
                        val.to_string() // Keep full Verilog format for wider buses
                    }
                }
                _ => val.to_string(),
            }
        } else {
            val.to_string()
        }
    } else {
        // Plain number - show compact hex
        if let Ok(n) = val.parse::<u64>() {
            let hex_chars = (width as usize).div_ceil(4);
            format!("0x{:0>width$}", format!("{:x}", n), width = hex_chars)
        } else {
            val.to_string()
        }
    }
}

/// Entry describing a signal in the timeline, with optional alias.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignalEntry {
    /// Signal path in the waveform hierarchy.
    pub signal_path: Option<String>,
    /// Bit-to-signal mapping for reconstructed signals.
    #[serde(default)]
    pub bit_mapping: Vec<BitMappingEntry>,
    /// Optional display alias for this signal.
    #[serde(default)]
    pub alias: Option<String>,
}

/// A single row in the unified timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineRow {
    /// Time index in the waveform.
    pub time_index: u64,
    /// Formatted time string (e.g., "10ns").
    pub time_formatted: String,
    /// Map from signal alias to value string at this time index.
    pub values: HashMap<String, String>,
}

/// Result of multi-signal timeline extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResult {
    /// Signal entries with their resolved aliases and widths.
    pub signals: Vec<(String, u32)>, // (alias, width)
    /// Total number of timeline rows.
    pub total_rows: usize,
    /// Timeline rows.
    pub rows: Vec<TimelineRow>,
}

/// Build a unified timeline of multiple signals.
///
/// Merge modes:
/// - `"union"`: Include time indices where ANY signal changes.
/// - `"intersection"`: Include only time indices where ALL signals change simultaneously.
pub fn build_multi_signal_timeline(
    waveform: &mut Waveform,
    signals: &[SignalEntry],
    start_idx: usize,
    end_idx: usize,
    merge_mode: &str,
    value_format: &str,
    limit: Option<isize>,
) -> WaveResult<TimelineResult> {
    let time_table_len = waveform.time_table().len();
    let time_table: Vec<wellen::Time> = waveform.time_table().to_vec();
    let timescale = waveform.hierarchy().timescale();

    if signals.is_empty() {
        return Err(WaveAnalyzerError::InvalidArgument {
            message: "At least one signal must be provided".into(),
        });
    }

    // Resolve signals: (SignalRef, width, alias)
    let mut resolved: Vec<(wellen::SignalRef, u32, String)> = Vec::new();
    for (i, entry) in signals.iter().enumerate() {
        let alias = entry.alias.clone().unwrap_or_else(|| {
            if let Some(ref path) = entry.signal_path {
                path.clone()
            } else {
                format!("reconstructed_{}", i)
            }
        });

        if let Some(ref path) = entry.signal_path {
            let (sr, width) = resolve_signal_with_width(waveform.hierarchy(), path)?;
            resolved.push((sr, width, alias));
        } else if !entry.bit_mapping.is_empty() {
            let max_bit = entry
                .bit_mapping
                .iter()
                .map(|e| e.bit_position)
                .max()
                .unwrap_or(0);
            let width = max_bit + 1;
            // Use first bit signal as representative (we'll read all bits separately)
            let first_path = &entry.bit_mapping[0].signal_path;
            let (sr, _) =
                resolve_signal_with_width(waveform.hierarchy(), first_path).map_err(|e| {
                    WaveAnalyzerError::Other(format!("Signal not found for bit 0: {}", e))
                })?;
            resolved.push((sr, width, alias));
        } else {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!("Signal entry {} has neither signal_path nor bit_mapping", i),
            });
        }
    }

    // Load all signals
    let sig_refs: Vec<wellen::SignalRef> = resolved.iter().map(|(sr, _, _)| *sr).collect();
    waveform.load_signals(&sig_refs);

    // Collect change time indices per signal
    let start = start_idx.min(time_table_len.saturating_sub(1));
    let end = end_idx.min(time_table_len.saturating_sub(1));

    let mut per_signal_times: Vec<std::collections::BTreeSet<usize>> = Vec::new();
    for (sig_ref, _, _) in &resolved {
        let signal = waveform
            .get_signal(*sig_ref)
            .ok_or(WaveAnalyzerError::Other(
                "Signal not found after loading".into(),
            ))?;

        let mut times = std::collections::BTreeSet::new();
        times.insert(start);
        for (time_idx, _) in signal.iter_changes() {
            let idx = time_idx as usize;
            if idx >= start && idx <= end {
                times.insert(idx);
            }
        }
        per_signal_times.push(times);
    }

    // Build merged time indices
    let merged_times: Vec<usize> = match merge_mode {
        "intersection" => {
            // Intersection of all sets
            let mut common = per_signal_times[0].clone();
            for set in &per_signal_times[1..] {
                common = common.intersection(set).cloned().collect();
            }
            common.into_iter().collect()
        }
        _ => {
            // Union of all sets
            let mut union = std::collections::BTreeSet::new();
            for set in &per_signal_times {
                union.extend(set.iter().cloned());
            }
            union.into_iter().collect()
        }
    };

    // Apply limit
    let limited_times: Vec<usize> = if let Some(lim) = limit {
        if lim > 0 {
            merged_times.iter().take(lim as usize).cloned().collect()
        } else {
            merged_times
        }
    } else {
        merged_times
    };

    // Build rows
    let mut rows: Vec<TimelineRow> = Vec::new();

    for time_idx in &limited_times {
        if *time_idx >= time_table_len {
            break;
        }

        let time_table_idx: wellen::TimeTableIdx = (*time_idx)
            .try_into()
            .map_err(|_| WaveAnalyzerError::Other(format!("Time index {} too large", time_idx)))?;

        let mut values = HashMap::new();

        for (j, (sig_ref, width, alias)) in resolved.iter().enumerate() {
            let signal = waveform
                .get_signal(*sig_ref)
                .ok_or(WaveAnalyzerError::Other(
                    "Signal not found after loading".into(),
                ))?;

            let value = if signals[j].bit_mapping.is_empty() {
                // Single signal
                if let Some(offset) = signal.get_offset(time_table_idx) {
                    let sv = signal.get_value_at(&offset, 0);
                    format_value_timeline(&sv, *width, value_format)
                } else {
                    "N/A".to_string()
                }
            } else {
                // Reconstructed from bit mapping
                read_reconstructed_value_at(
                    waveform,
                    &signals[j].bit_mapping,
                    time_table_idx,
                    *width,
                    value_format,
                )
            };

            values.insert(alias.clone(), value);
        }

        let formatted_time = format_time(time_table[*time_idx], timescale.as_ref());

        rows.push(TimelineRow {
            time_index: *time_idx as u64,
            time_formatted: formatted_time,
            values,
        });
    }

    let signal_info: Vec<(String, u32)> =
        resolved.iter().map(|(_, w, a)| (a.clone(), *w)).collect();

    Ok(TimelineResult {
        signals: signal_info,
        total_rows: rows.len(),
        rows,
    })
}

/// Read reconstructed value from bit mapping at a time index.
fn read_reconstructed_value_at(
    waveform: &Waveform,
    bit_mapping: &[BitMappingEntry],
    time_table_idx: wellen::TimeTableIdx,
    width: u32,
    value_format: &str,
) -> String {
    let mut composite = num_bigint::BigUint::zero();

    for entry in bit_mapping {
        let hierarchy = waveform.hierarchy();
        let var_ref = match find_var_by_path(hierarchy, &entry.signal_path) {
            Some(v) => v,
            None => continue,
        };
        let signal = match waveform.get_signal(hierarchy[var_ref].signal_ref()) {
            Some(s) => s,
            None => continue,
        };

        if let Some(offset) = signal.get_offset(time_table_idx) {
            let sv = signal.get_value_at(&offset, 0);
            let bit_set = is_signal_high(&sv);
            if bit_set {
                composite.set_bit(entry.bit_position as u64, true);
            }
        }
    }

    format_biguint_value(&composite, width, value_format)
}

/// Format a signal value.
fn format_value_timeline(signal_value: &wellen::SignalValue, width: u32, format: &str) -> String {
    let value = signal_value_to_biguint(*signal_value, Some(width));
    format_biguint_value(&value, width, format)
}

/// Format a timeline result as human-readable text.
pub fn format_timeline_report(result: &TimelineResult) -> String {
    let mut lines = Vec::new();

    // Header
    let header_parts: Vec<&str> = result
        .signals
        .iter()
        .map(|(alias, _)| alias.as_str())
        .collect();
    lines.push(format!(
        "=== Multi-Signal Timeline ({} signals) ===",
        result.signals.len()
    ));
    lines.push(format!("Signals: {}", header_parts.join(", ")));
    lines.push(format!("Total rows: {}", result.total_rows));
    lines.push("-".repeat(80));

    // Build column widths
    let mut time_col_width = 12;
    let mut col_widths: Vec<usize> = result
        .signals
        .iter()
        .map(|(alias, _)| alias.len().max(4))
        .collect();

    for row in &result.rows {
        let ts = format!("{}", row.time_index);
        if ts.len() > time_col_width {
            time_col_width = ts.len();
        }
        for (i, _) in result.signals.iter().enumerate() {
            if let Some(val) = row.values.get(&result.signals[i].0) {
                let len = val.len();
                if len > col_widths[i] {
                    col_widths[i] = len;
                }
            }
        }
    }

    // Print header
    let mut header = format!("{:<width$}", "Time", width = time_col_width);
    for (i, (alias, _)) in result.signals.iter().enumerate() {
        header.push_str(&format!(" | {:<width$}", alias, width = col_widths[i]));
    }
    lines.push(header);
    lines.push("-".repeat(80));

    // Print rows
    for row in &result.rows {
        let mut line = format!("{:<width$}", row.time_index, width = time_col_width);
        for (i, _) in result.signals.iter().enumerate() {
            let val = row
                .values
                .get(&result.signals[i].0)
                .cloned()
                .unwrap_or_else(|| "N/A".to_string());
            line.push_str(&format!(" | {:<width$}", val, width = col_widths[i]));
        }
        lines.push(line);
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wellen::SignalValue;

    #[test]
    fn test_detect_signal_type() {
        assert_eq!(
            detect_signal_type("clk_sys", wellen::VarType::Wire),
            "clock"
        );
        assert_eq!(
            detect_signal_type("reset_n", wellen::VarType::Wire),
            "reset"
        );
        assert_eq!(
            detect_signal_type("data_out", wellen::VarType::Wire),
            "wire"
        );
        // BUG-3/24: parameters should not be classified as "wire"
        assert_eq!(
            detect_signal_type("DATA_WIDTH", wellen::VarType::Parameter),
            "parameter"
        );
        assert_eq!(detect_signal_type("data_reg", wellen::VarType::Reg), "reg");
    }

    #[test]
    fn test_downsample_signal_changes_preserves_toggling_clock() {
        let values = [0u8, 1u8, 0u8, 1u8, 0u8, 1u8];
        let changes: Vec<(u32, SignalValue<'_>)> = values
            .iter()
            .enumerate()
            .map(|(i, v)| (i as u32, SignalValue::Binary(std::slice::from_ref(v), 1)))
            .collect();

        let samples = downsample_signal_changes(&changes, 4);
        let sampled_values: Vec<&str> = samples.iter().map(|s| s.value.as_str()).collect();

        assert!(sampled_values.contains(&"1'b0"));
        assert!(sampled_values.contains(&"1'b1"));
    }
}
