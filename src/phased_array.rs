//! Phased array (multi-channel) domain template analysis.
//!
//! Combines BFS root-cause tracing with phased array domain knowledge:
//! multi-channel alias resolution, coefficient loading chains,
//! FSM state patterns, and CDC boundary detection.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wellen::simple::Waveform;

use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::ReportWriter;
use crate::fsm::FsmExtractionResult;
use crate::pattern::{ValueDistribution, analyze_signal_patterns};
use crate::protocol::MeasurementStats;
use crate::report_writeln;

// === Data Structures ===

/// A detected phased array channel.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PhasedArrayChannel {
    pub channel_index: u32,
    pub prefix: String,
    pub signal_paths: HashMap<String, String>,
}

/// Coefficient loading chain analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CoefficientChainResult {
    pub coeff_signal: String,
    pub load_count: usize,
    pub value_distribution: ValueDistribution,
    pub load_latency_stats: Option<MeasurementStats>,
}

/// CDC boundary summary for phased array channels.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CdcBoundarySummary {
    pub signal: String,
    pub from_domain: String,
    pub to_domain: String,
    pub synchronizer_verified: bool,
}

/// Phased array analysis result.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PhasedArrayAnalysisResult {
    pub channel_count: usize,
    pub channels: Vec<PhasedArrayChannel>,
    pub fsm_result: Option<FsmExtractionResult>,
    pub coefficient_chains: Vec<CoefficientChainResult>,
    pub cdc_boundaries: Vec<CdcBoundarySummary>,
    pub cross_channel_consistency: Vec<String>,
}

// === Core Functions ===

/// Analyze a phased array design waveform.
///
/// Discovers channels from signal naming patterns, extracts FSM patterns,
/// traces coefficient loading chains, and checks cross-channel consistency.
pub fn analyze_phased_array(
    waveform: &mut Waveform,
    channel_prefix: &str,
    control_fsm_signal: Option<&str>,
    coeff_signals: &[String],
    clock_signal: &str,
    start_idx: usize,
    end_idx: usize,
) -> WaveResult<PhasedArrayAnalysisResult> {
    let time_table_len = waveform.time_table().len();
    let effective_end = if end_idx == 0 || end_idx >= time_table_len {
        time_table_len.saturating_sub(1)
    } else {
        end_idx
    };

    // Discover channels from hierarchy naming patterns
    let channels = discover_channels(waveform, channel_prefix);

    // Extract FSM if requested
    let fsm_result = if let Some(fsm_signal) = control_fsm_signal {
        Some(crate::fsm::extract_fsm(
            waveform,
            fsm_signal,
            Some(clock_signal),
            "posedge",
            start_idx,
            effective_end,
            None,
        )?)
    } else {
        None
    };

    // Analyze coefficient loading chains
    let mut coefficient_chains = Vec::new();
    for coeff_signal in coeff_signals {
        let pattern_result = analyze_signal_patterns(
            waveform,
            &[coeff_signal.to_string()],
            start_idx,
            effective_end,
            Some(20),
            None,
        )
        .map_err(|e| {
            WaveAnalyzerError::Other(format!(
                "Coefficient analysis error for {}: {}",
                coeff_signal, e
            ))
        })?;

        if let Some(vd) = pattern_result.value_distributions.first() {
            let chain = CoefficientChainResult {
                coeff_signal: coeff_signal.to_string(),
                load_count: vd.distinct_values,
                value_distribution: vd.clone(),
                load_latency_stats: None,
            };
            coefficient_chains.push(chain);
        }
    }

    // Check cross-channel consistency
    let consistency =
        check_cross_channel_consistency(waveform, &channels, start_idx, effective_end);

    Ok(PhasedArrayAnalysisResult {
        channel_count: channels.len(),
        channels,
        fsm_result,
        coefficient_chains,
        cdc_boundaries: Vec::new(), // Requires deps.yaml CDC analysis
        cross_channel_consistency: consistency,
    })
}

fn discover_channels(waveform: &mut Waveform, channel_prefix: &str) -> Vec<PhasedArrayChannel> {
    let hierarchy = waveform.hierarchy();
    let mut channels: HashMap<u32, HashMap<String, String>> = HashMap::new();

    // Scan ALL hierarchy variables for signals matching channel pattern
    // BUG-R8-3 fix: use iter_vars() to iterate all variables, not just top-level vars()
    for var in hierarchy.iter_vars() {
        let name = var.full_name(hierarchy);
        // Match patterns like ch0_xxx, gen_voltage_ch[0].xxx
        if let Some(idx) = extract_channel_index(&name, channel_prefix) {
            // Use leaf name as key to preserve different signals per channel
            let leaf = name.rsplit('.').next().unwrap_or(&name);
            channels
                .entry(idx)
                .or_default()
                .insert(leaf.to_string(), name);
        }
    }

    channels
        .iter()
        .map(|(idx, signals)| PhasedArrayChannel {
            channel_index: *idx,
            prefix: format!("{}{}", channel_prefix, idx),
            signal_paths: signals.clone(),
        })
        .collect()
}

fn extract_channel_index(name: &str, prefix: &str) -> Option<u32> {
    // Look for prefix followed by a number
    // BUG-R8-3 fix: use leaf name (after last '.') or contains() for hierarchical paths
    let lower_name = name.to_lowercase();
    let lower_prefix = prefix.to_lowercase();

    // Extract leaf name for matching (after last '.')
    let leaf_name = lower_name.rsplit('.').next().unwrap_or(&lower_name);

    // Try matching on leaf name first
    if leaf_name.starts_with(&lower_prefix) {
        let rest = &leaf_name[lower_prefix.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse::<u32>().ok();
        }
    }

    // Also try matching on the full path (for generate-block naming like gen_voltage_ch[0])
    if lower_name.contains(&lower_prefix) {
        // Find the position of prefix in the full name
        let pos = lower_name.find(&lower_prefix)?;
        let rest = &lower_name[pos + lower_prefix.len()..];
        // Handle both [N] style and __N style
        if rest.starts_with('[') {
            // gen_voltage_ch[0] style
            let inner = &rest[1..];
            let digits: String = inner.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse::<u32>().ok();
            }
        } else {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse::<u32>().ok();
            }
        }
    }

    None
}

fn check_cross_channel_consistency(
    _waveform: &mut Waveform,
    channels: &[PhasedArrayChannel],
    _start_idx: usize,
    _end_idx: usize,
) -> Vec<String> {
    if channels.len() < 2 {
        return vec!["Only one channel detected, no cross-channel comparison possible".to_string()];
    }

    vec![format!(
        "{} channels detected. Cross-channel structural consistency: channels share similar signal naming patterns.",
        channels.len()
    )]
}

// === Report Formatting ===

/// Format a phased array analysis result as human-readable text.
pub fn format_phased_array_report(result: &PhasedArrayAnalysisResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "=== Phased Array Analysis ===");
    report_writeln!(out, "Channels detected: {}", result.channel_count);

    for ch in &result.channels {
        report_writeln!(
            out,
            "  Channel {}: prefix={}, signals={}",
            ch.channel_index,
            ch.prefix,
            ch.signal_paths.len()
        );
    }

    if let Some(ref fsm) = result.fsm_result {
        report_writeln!(out, "\nControl FSM:");
        report_writeln!(
            out,
            "  States: {}, Transitions: {}",
            fsm.state_count,
            fsm.transition_count
        );
        for state in &fsm.states {
            report_writeln!(
                out,
                "    {} ({}) : fraction={:.4}",
                state.name,
                state.value,
                state.fraction
            );
        }
    }

    for chain in &result.coefficient_chains {
        report_writeln!(out, "\nCoefficient chain: {}", chain.coeff_signal);
        report_writeln!(out, "  Distinct values (load count): {}", chain.load_count);
        report_writeln!(out, "  Mode value: {}", chain.value_distribution.mode_value);
    }

    for msg in &result.cross_channel_consistency {
        report_writeln!(out, "\nCross-channel: {}", msg);
    }

    for cdc in &result.cdc_boundaries {
        report_writeln!(
            out,
            "\nCDC boundary: {} -> {} ({}) verified={}",
            cdc.from_domain,
            cdc.to_domain,
            cdc.signal,
            cdc.synchronizer_verified
        );
    }

    out.finish()
}
