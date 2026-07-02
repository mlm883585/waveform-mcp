//! BFS result report generation in multiple formats.
//!
//! Provides JSON, Markdown, and HTML export for BfsResult data.

use crate::bfs::{BfsResult, NodeStatus, RootCauseCandidate};
use crate::error::{WaveAnalyzerError, WaveResult};
use crate::formatting::ReportWriter;
use crate::report_writeln;
use serde::{Deserialize, Serialize};

/// Report format options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReportFormat {
    Json,
    Markdown,
    Html,
}

/// Generate a JSON report from BfsResult.
pub fn format_bfs_report_json(result: &BfsResult) -> WaveResult<String> {
    serde_json::to_string_pretty(result)
        .map_err(|e| WaveAnalyzerError::Other(format!("JSON serialization failed: {}", e)))
}

/// Generate a Markdown report from BfsResult.
pub fn format_bfs_report_markdown(result: &BfsResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "# BFS Root Cause Trace Report");
    report_writeln!(out);
    report_writeln!(out, "**Entry signal:** `{}`", result.root_signal);
    report_writeln!(
        out,
        "**Failure time:** index {} ({:.1} ns)",
        result.root_time_index,
        result.root_time_ps as f64 / 1000.0
    );
    report_writeln!(out, "**Nodes explored:** {}", result.tree.len());
    report_writeln!(
        out,
        "**Root cause candidates:** {}",
        result.candidates.len()
    );
    report_writeln!(out);

    // Candidates table
    if !result.candidates.is_empty() {
        report_writeln!(out, "## Root Cause Candidates");
        report_writeln!(out);
        report_writeln!(out, "| # | Signal | Time | Status | Reason |");
        report_writeln!(out, "|---|--------|------|--------|--------|");
        for (i, c) in result.candidates.iter().enumerate() {
            report_writeln!(
                out,
                "| {} | `{}` | {:.1} ns | {} | {} |",
                i + 1,
                c.signal_path,
                c.time_ps as f64 / 1000.0,
                format_node_status(&c.status),
                c.reason
            );
        }
        report_writeln!(out);
    }

    // Trace tree
    report_writeln!(out, "## Trace Tree");
    report_writeln!(out);
    for node in &result.tree {
        let indent = "  ".repeat(node.depth);
        let value_str = node
            .actual_value
            .as_deref()
            .map(|v| format!(" = {}", v))
            .unwrap_or_default();
        let edge_str = node
            .edge_type
            .as_deref()
            .map(|e| format!(" [{}]", e))
            .unwrap_or_default();
        let clock_str = node
            .clock_name
            .as_deref()
            .map(|c| format!(" clk={}", c))
            .unwrap_or_default();
        let latency_str = node
            .latency_cycles
            .map(|l| format!(" lat={}", l))
            .unwrap_or_default();
        report_writeln!(
            out,
            "{}{} `{}`{}{}{}{}",
            indent,
            format_node_status(&node.status),
            node.signal_path,
            value_str,
            edge_str,
            clock_str,
            latency_str,
        );
    }

    out.finish()
}

/// Generate an HTML report from BfsResult (dark theme, matching SVG style).
pub fn format_bfs_report_html(result: &BfsResult) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "<!DOCTYPE html>");
    report_writeln!(out, "<html><head>");
    report_writeln!(out, "<meta charset='utf-8'>");
    report_writeln!(out, "<title>BFS Root Cause Report</title>");
    report_writeln!(out, "<style>");
    report_writeln!(
        out,
        "body {{ background: #1e1e2e; color: #cdd6f4; font-family: monospace; margin: 20px; }}"
    );
    report_writeln!(out, "h1 {{ color: #89b4fa; }} h2 {{ color: #a6e3a1; }}");
    report_writeln!(
        out,
        "table {{ border-collapse: collapse; width: 100%; margin: 10px 0; }}"
    );
    report_writeln!(
        out,
        "th {{ background: #313244; color: #89b4fa; padding: 8px; text-align: left; }}"
    );
    report_writeln!(
        out,
        "td {{ padding: 6px 8px; border-bottom: 1px solid #45475a; }}"
    );
    report_writeln!(
        out,
        ".suspect {{ color: #f9e2af; }} .ok {{ color: #a6e3a1; }} .boundary {{ color: #89b4fa; }}"
    );
    report_writeln!(
        out,
        ".root-cause {{ color: #f38ba8; font-weight: bold; }} .stopped {{ color: #6c7086; }}"
    );
    report_writeln!(
        out,
        ".truncated {{ color: #6c7086; }} .cyclic {{ color: #cba6f7; }} .context {{ color: #9399b2; }}"
    );
    report_writeln!(
        out,
        "pre {{ background: #313244; padding: 12px; border-radius: 4px; overflow-x: auto; }}"
    );
    report_writeln!(out, "</style></head><body>");

    // Header
    report_writeln!(out, "<h1>BFS Root Cause Trace Report</h1>");
    report_writeln!(
        out,
        "<p><b>Entry signal:</b> {}<br/><b>Failure time:</b> index {} ({:.1} ns)<br/><b>Nodes explored:</b> {}<br/><b>Candidates:</b> {}</p>",
        html_escape(&result.root_signal),
        result.root_time_index,
        result.root_time_ps as f64 / 1000.0,
        result.tree.len(),
        result.candidates.len()
    );

    // Candidates table
    if !result.candidates.is_empty() {
        report_writeln!(out, "<h2>Root Cause Candidates</h2>");
        report_writeln!(
            out,
            "<table><tr><th>#</th><th>Signal</th><th>Time</th><th>Status</th><th>Reason</th></tr>"
        );
        for (i, c) in result.candidates.iter().enumerate() {
            report_writeln!(
                out,
                "<tr><td>{}</td><td>{}</td><td>{:.1} ns</td><td class='{}'>{}</td><td>{}</td></tr>",
                i + 1,
                html_escape(&c.signal_path),
                c.time_ps as f64 / 1000.0,
                status_css_class(&c.status),
                format_node_status(&c.status),
                html_escape(&c.reason)
            );
        }
        report_writeln!(out, "</table>");
    }

    // Trace tree
    report_writeln!(out, "<h2>Trace Tree</h2>");
    report_writeln!(out, "<pre>");
    for node in &result.tree {
        let indent = "  ".repeat(node.depth);
        let value_str = node
            .actual_value
            .as_deref()
            .map(|v| format!(" = {}", v))
            .unwrap_or_default();
        let edge_str = node
            .edge_type
            .as_deref()
            .map(|e| format!(" [{}]", e))
            .unwrap_or_default();
        report_writeln!(
            out,
            "{}<span class='{}'>{}</span> {}{}{}",
            indent,
            status_css_class(&node.status),
            format_node_status(&node.status),
            html_escape(&node.signal_path),
            html_escape(&value_str),
            html_escape(&edge_str)
        );
    }
    report_writeln!(out, "</pre>");

    report_writeln!(out, "</body></html>");

    out.finish()
}

/// Format a NodeStatus as a human-readable string.
fn format_node_status(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Suspect => "Suspect",
        NodeStatus::Ok => "Ok",
        NodeStatus::Boundary => "Boundary",
        NodeStatus::Stopped => "Stopped",
        NodeStatus::Truncated => "Truncated",
        NodeStatus::Cyclic => "Cyclic",
        NodeStatus::Context => "Context",
        NodeStatus::RootCauseCandidate => "RootCause",
        NodeStatus::CdcBoundary => "CDC-Boundary",
        NodeStatus::CdcPenetrated => "CDC-Penetrated",
        NodeStatus::CdcSynchronizer => "CDC-Sync",
        NodeStatus::Unresolved => "Unresolved",
    }
}

/// Get CSS class name for a NodeStatus.
fn status_css_class(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Suspect => "suspect",
        NodeStatus::Ok => "ok",
        NodeStatus::Boundary => "boundary",
        NodeStatus::Stopped => "stopped",
        NodeStatus::Truncated => "truncated",
        NodeStatus::Cyclic => "cyclic",
        NodeStatus::Context => "context",
        NodeStatus::RootCauseCandidate => "root-cause",
        NodeStatus::CdcBoundary => "cdc-boundary",
        NodeStatus::CdcPenetrated => "cdc-penetrated",
        NodeStatus::CdcSynchronizer => "cdc-sync",
        NodeStatus::Unresolved => "unresolved",
    }
}

/// Escape special HTML characters to prevent XSS.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Batch BFS result for report generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchBfsReport {
    /// Individual trace results keyed by assertion name.
    pub traces: Vec<BfsTraceEntry>,
    /// Aggregated candidates from all traces.
    pub aggregated_candidates: Vec<RootCauseCandidate>,
    /// Summary text.
    pub summary: String,
}

/// A single trace entry in the batch report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BfsTraceEntry {
    /// Event name that triggered this trace.
    pub event_name: String,
    /// Entry signal used.
    pub entry_signal: String,
    /// Failure time in picoseconds.
    pub fail_time_ps: u64,
    /// The BFS result.
    pub result: BfsResult,
}

/// Generate a Markdown report from a BatchBfsReport.
pub fn format_batch_bfs_report_markdown(batch: &BatchBfsReport) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "# Batch BFS Root Cause Analysis Report");
    report_writeln!(out);
    report_writeln!(out, "**Events traced:** {}", batch.traces.len());
    report_writeln!(
        out,
        "**Total candidates:** {}",
        batch.aggregated_candidates.len()
    );
    report_writeln!(out);

    for entry in &batch.traces {
        report_writeln!(out, "## Event: `{}`", entry.event_name);
        report_writeln!(
            out,
            "Entry signal: `{}`, Failure time: {:.1} ns",
            entry.entry_signal,
            entry.fail_time_ps as f64 / 1000.0
        );
        report_writeln!(out);
        out.push_str(&format_bfs_report_markdown(&entry.result));
        report_writeln!(out);
    }

    // Aggregated candidates
    if !batch.aggregated_candidates.is_empty() {
        report_writeln!(out, "## Aggregated Root Cause Candidates");
        report_writeln!(out);
        report_writeln!(out, "| # | Signal | Time | Status | Reason |");
        report_writeln!(out, "|---|--------|------|--------|--------|");
        for (i, c) in batch.aggregated_candidates.iter().enumerate() {
            report_writeln!(
                out,
                "| {} | `{}` | {:.1} ns | {} | {} |",
                i + 1,
                c.signal_path,
                c.time_ps as f64 / 1000.0,
                format_node_status(&c.status),
                c.reason
            );
        }
    }

    out.finish()
}

/// Generate an HTML report from a BatchBfsReport.
pub fn format_batch_bfs_report_html(batch: &BatchBfsReport) -> String {
    let mut out = ReportWriter::new();

    report_writeln!(out, "<!DOCTYPE html><html><head>");
    report_writeln!(out, "<meta charset='utf-8'><title>Batch BFS Report</title>");
    report_writeln!(out, "<style>");
    report_writeln!(
        out,
        "body {{ background: #1e1e2e; color: #cdd6f4; font-family: monospace; margin: 20px; }}"
    );
    report_writeln!(
        out,
        "h1 {{ color: #89b4fa; }} h2 {{ color: #a6e3a1; }} h3 {{ color: #f9e2af; }}"
    );
    report_writeln!(out, "table {{ border-collapse: collapse; width: 100%; }}");
    report_writeln!(
        out,
        "th {{ background: #313244; color: #89b4fa; padding: 8px; }}"
    );
    report_writeln!(
        out,
        "td {{ padding: 6px 8px; border-bottom: 1px solid #45475a; }}"
    );
    report_writeln!(
        out,
        ".root-cause {{ color: #f38ba8; font-weight: bold; }} .suspect {{ color: #f9e2af; }} .ok {{ color: #a6e3a1; }} .boundary {{ color: #89b4fa; }}"
    );
    report_writeln!(out, "</style></head><body>");

    report_writeln!(out, "<h1>Batch BFS Root Cause Analysis</h1>");
    report_writeln!(
        out,
        "<p><b>Events traced:</b> {}<br/><b>Total candidates:</b> {}</p>",
        batch.traces.len(),
        batch.aggregated_candidates.len()
    );

    for entry in &batch.traces {
        report_writeln!(out, "<h2>Event: {}</h2>", html_escape(&entry.event_name));
        report_writeln!(
            out,
            "<p>Entry: {} @ {:.1} ns | Nodes: {} | Candidates: {}</p>",
            html_escape(&entry.entry_signal),
            entry.fail_time_ps as f64 / 1000.0,
            entry.result.tree.len(),
            entry.result.candidates.len()
        );
        out.push_str(&format_bfs_report_html(&entry.result));
    }

    report_writeln!(out, "</body></html>");

    out.finish()
}
