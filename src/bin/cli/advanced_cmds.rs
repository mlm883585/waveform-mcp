use wave_analyzer_mcp::Command;

use super::CliStore;

pub(super) fn exec_analyze_cdc(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AnalyzeCdc {
        waveform_id,
        deps_id,
        simulator,
    } = cmd
    else {
        unreachable!("exec_analyze_cdc only handles AnalyzeCdc");
    };

    let sim = simulator.clone().unwrap_or_else(|| "modelsim".to_string());
    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    if let Some(id) = &deps_id {
        let dep_graph = store
            .dep_graphs
            .get(id)
            .ok_or_else(|| format!("Dep graph not found: {}", id))?;
        let result = wave_analyzer_mcp::cdc::analyze_cdc(waveform, dep_graph, &sim, true, 2)
            .map_err(|e| format!("Error analyzing CDC: {}", e))?;
        Ok(wave_analyzer_mcp::cdc::format_cdc_report(&result))
    } else {
        // BUG-R8-1 fix: auto-detect clocks from waveform when no deps_id provided
        let domains = wave_analyzer_mcp::cdc::identify_clock_domains_from_waveform(waveform)
            .map_err(|e| format!("Error auto-detecting clock domains: {}", e))?;
        let mut report_lines = vec![
            "=== CDC Analysis Report (auto-detect mode) ===".to_string(),
            String::new(),
        ];
        if domains.is_empty() {
            report_lines.push("No clock domains detected in waveform.".to_string());
            report_lines.push(
                "Tip: Provide --deps-id with clock_aliases for more accurate CDC analysis."
                    .to_string(),
            );
        } else {
            report_lines.push(format!("Clock Domains: {}", domains.len()));
            for dom in &domains {
                let period_str = if dom.period_ps > 0 {
                    format!("{}ps", dom.period_ps)
                } else {
                    "irregular".to_string()
                };
                report_lines.push(format!(
                    "  {} (period: {}, likely signals: {})",
                    dom.clock_path,
                    period_str,
                    dom.likely_signals.len()
                ));
            }
            report_lines.push(String::new());
            report_lines.push("CDC Crossings: 0 (no dependency graph — crossings require deps.yaml with cdc_crossing entries)".to_string());
            report_lines.push(String::new());
            report_lines.push(format!(
                "Summary: {} domains, 0 crossings (no deps)",
                domains.len()
            ));
        }
        Ok(report_lines.join("\n"))
    }
}

pub(super) fn exec_analyze_patterns(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AnalyzeSignalPatterns {
        waveform_id,
        signals,
        start_time_index,
        end_time_index,
        max_bins,
        idle_threshold,
    } = cmd
    else {
        unreachable!("exec_analyze_patterns only handles AnalyzeSignalPatterns");
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    let idle_thresh = idle_threshold.clone();
    let result = wave_analyzer_mcp::pattern::analyze_signal_patterns(
        waveform,
        signals,
        start_time_index.unwrap_or(0),
        end_time_index.unwrap_or(waveform.time_table().len() - 1),
        *max_bins,
        idle_thresh,
    )
    .map_err(|e| format!("Error analyzing patterns: {}", e))?;
    Ok(wave_analyzer_mcp::pattern::format_pattern_report(&result))
}

pub(super) fn exec_extract_fsm(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::ExtractFsm {
        waveform_id,
        signal_path,
        clock_signal,
        edge_type,
        start_time_index,
        end_time_index,
    } = cmd
    else {
        unreachable!("exec_extract_fsm only handles ExtractFsm");
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    let et_str = edge_type.as_deref().unwrap_or("posedge");
    let result = wave_analyzer_mcp::fsm::extract_fsm(
        waveform,
        signal_path,
        clock_signal.as_deref(),
        et_str,
        start_time_index.unwrap_or(0),
        end_time_index.unwrap_or(waveform.time_table().len() - 1),
        None,
    )
    .map_err(|e| format!("Error extracting FSM: {}", e))?;
    Ok(wave_analyzer_mcp::fsm::format_fsm_report(&result))
}

pub(super) fn exec_analyze_protocol(store: &mut CliStore, cmd: &Command) -> Result<String, String> {
    let Command::AnalyzeProtocol {
        waveform_id,
        protocol,
        signals,
        start_time_index,
        end_time_index,
    } = cmd
    else {
        unreachable!("exec_analyze_protocol only handles AnalyzeProtocol");
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    let proto = wave_analyzer_mcp::protocol_template::ProtocolTemplate::from_str_name(protocol)
        .ok_or_else(|| {
            format!(
                "Unknown protocol '{}'. Supported: spi, uart, i2c, axi_lite",
                protocol
            )
        })?;
    let signal_map: std::collections::HashMap<String, String> = signals.iter().cloned().collect();
    let start = start_time_index.unwrap_or(0);
    let end = end_time_index.unwrap_or(waveform.time_table().len() - 1);
    let result = wave_analyzer_mcp::protocol_template::analyze_protocol_template(
        waveform,
        &proto,
        &signal_map,
        start,
        end,
    )
    .map_err(|e| format!("Error analyzing protocol: {}", e))?;
    Ok(wave_analyzer_mcp::protocol_template::format_protocol_template_report(&result))
}

pub(super) fn exec_analyze_phased_array(
    store: &mut CliStore,
    cmd: &Command,
) -> Result<String, String> {
    let Command::AnalyzePhasedArray {
        waveform_id,
        channel_prefix,
        control_fsm_signal,
        coeff_signals,
        clock_signal,
        start_time_index,
        end_time_index,
    } = cmd
    else {
        unreachable!("exec_analyze_phased_array only handles AnalyzePhasedArray");
    };

    let waveform = store
        .waveforms
        .get_mut(waveform_id)
        .ok_or_else(|| format!("Waveform not found: {}", waveform_id))?;
    let coeff_refs: &[String] = match &coeff_signals {
        Some(v) => v,
        None => &[],
    };
    let result = wave_analyzer_mcp::phased_array::analyze_phased_array(
        waveform,
        channel_prefix,
        control_fsm_signal.as_deref(),
        coeff_refs,
        clock_signal,
        start_time_index.unwrap_or(0),
        end_time_index.unwrap_or(waveform.time_table().len() - 1),
    )
    .map_err(|e| format!("Error analyzing phased array: {}", e))?;
    Ok(wave_analyzer_mcp::phased_array::format_phased_array_report(
        &result,
    ))
}
