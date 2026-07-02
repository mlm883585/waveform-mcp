//! Tests for BFS report generation module.

use wave_analyzer_mcp::bfs::{BfsNode, BfsResult, NodeStatus, RootCauseCandidate};
use wave_analyzer_mcp::report::{
    BatchBfsReport, BfsTraceEntry, format_batch_bfs_report_html, format_batch_bfs_report_markdown,
    format_bfs_report_html, format_bfs_report_json, format_bfs_report_markdown,
};

fn make_test_bfs_result() -> BfsResult {
    BfsResult {
        root_signal: "TOP.data_out".to_string(),
        root_time_index: 10,
        root_time_ps: 1750,
        tree: vec![
            BfsNode {
                signal_path: "TOP.data_out".to_string(),
                resolved_signal_path: "TOP.data_out".to_string(),
                time_index: 10,
                time_ps: 1750,
                depth: 0,
                status: NodeStatus::Suspect,
                actual_value: Some("0".to_string()),
                expected_hint: None,
                edge_type: None,
                clock_name: None,
                latency_cycles: None,
                note: Some("value mismatch".to_string()),
                parent_id: None,
                node_id: "root".to_string(),
                source_clock_domain: None,
                dest_clock_domain: None,
                synchronizer_info: None,
            },
            BfsNode {
                signal_path: "TOP.ctrl.enable".to_string(),
                resolved_signal_path: "TOP.ctrl.enable".to_string(),
                time_index: 9,
                time_ps: 1700,
                depth: 1,
                status: NodeStatus::RootCauseCandidate,
                actual_value: Some("1".to_string()),
                expected_hint: None,
                edge_type: Some("sequential".to_string()),
                clock_name: Some("clk_sys".to_string()),
                latency_cycles: Some(1),
                note: Some("changed at clock edge".to_string()),
                parent_id: Some("root".to_string()),
                node_id: "n1".to_string(),
                source_clock_domain: None,
                dest_clock_domain: None,
                synchronizer_info: None,
            },
            BfsNode {
                signal_path: "TOP.input_a".to_string(),
                resolved_signal_path: "TOP.input_a".to_string(),
                time_index: 9,
                time_ps: 1700,
                depth: 1,
                status: NodeStatus::Boundary,
                actual_value: Some("8'h5A".to_string()),
                expected_hint: None,
                edge_type: None,
                clock_name: None,
                latency_cycles: None,
                note: Some("input port".to_string()),
                parent_id: Some("root".to_string()),
                node_id: "n2".to_string(),
                source_clock_domain: None,
                dest_clock_domain: None,
                synchronizer_info: None,
            },
        ],
        candidates: vec![RootCauseCandidate {
            signal_path: "TOP.ctrl.enable".to_string(),
            time_ps: 1700,
            time_index: 9,
            status: NodeStatus::RootCauseCandidate,
            reason: "changed at clock edge".to_string(),
        }],
        summary: "BFS trace complete. 3 nodes explored.".to_string(),
    }
}

#[test]
fn test_format_bfs_report_json_roundtrip() {
    let result = make_test_bfs_result();
    let json_str = format_bfs_report_json(&result).expect("JSON serialization should succeed");
    // Verify it can be deserialized back
    let deserialized: BfsResult =
        serde_json::from_str(&json_str).expect("JSON deserialization should succeed");
    assert_eq!(deserialized.root_signal, result.root_signal);
    assert_eq!(deserialized.root_time_index, result.root_time_index);
    assert_eq!(deserialized.tree.len(), result.tree.len());
    assert_eq!(deserialized.candidates.len(), result.candidates.len());
}

#[test]
fn test_format_bfs_report_markdown_no_duplicate_status() {
    let result = make_test_bfs_result();
    let md = format_bfs_report_markdown(&result);
    // After the fix, trace tree lines should NOT have "—" followed by a status label
    for line in md.lines() {
        // Only check trace tree lines (indented lines with signal paths)
        if line.starts_with("  ") && line.contains("`TOP.") {
            // "—" should not appear in trace tree lines after the fix
            assert!(
                !line.contains(" — "),
                "Duplicate status found in line: {}",
                line
            );
        }
    }
    // Verify key content is present
    assert!(md.contains("TOP.data_out"));
    assert!(md.contains("Suspect"));
    assert!(md.contains("RootCause"));
    assert!(md.contains("Boundary"));
}

#[test]
fn test_format_bfs_report_html_escapes_special_chars() {
    let result = BfsResult {
        root_signal: "TOP.<script>alert('xss')</script>".to_string(),
        root_time_index: 5,
        root_time_ps: 500,
        tree: vec![BfsNode {
            signal_path: "TOP.data&out".to_string(),
            resolved_signal_path: "TOP.data&out".to_string(),
            time_index: 5,
            time_ps: 500,
            depth: 0,
            status: NodeStatus::Suspect,
            actual_value: Some(" = val".to_string()),
            expected_hint: None,
            edge_type: Some(" [edge<type>]".to_string()),
            clock_name: None,
            latency_cycles: None,
            note: Some("reason\"quote".to_string()),
            parent_id: None,
            node_id: "n0".to_string(),
            source_clock_domain: None,
            dest_clock_domain: None,
            synchronizer_info: None,
        }],
        candidates: vec![RootCauseCandidate {
            signal_path: "TOP.sig<1>".to_string(),
            time_ps: 500,
            time_index: 5,
            status: NodeStatus::Suspect,
            reason: "test&reason".to_string(),
        }],
        summary: "test".to_string(),
    };

    let html = format_bfs_report_html(&result);
    // Verify XSS characters are escaped
    assert!(!html.contains("<script>"), "Unescaped <script> tag found");
    assert!(
        html.contains("&lt;script&gt;"),
        "script tag should be escaped"
    );
    assert!(html.contains("&amp;"), "& should be escaped");
    assert!(html.contains("&lt;"), "< should be escaped");
    assert!(html.contains("&gt;"), "> should be escaped");
    // The reason field in candidates table should escape quotes
    // Note: html_escape replaces " with &quot; only in candidate reason and signal_path
    // Check that the candidate reason with a quote was escaped
    let has_escaped_quote = html.contains("&quot;") || html.contains("&#39;");
    assert!(
        has_escaped_quote,
        "quotes in candidate fields should be escaped"
    );
}

#[test]
fn test_format_bfs_report_html_css_classes() {
    let result = make_test_bfs_result();
    let html = format_bfs_report_html(&result);
    // Verify CSS classes match status
    assert!(html.contains("class='suspect'"));
    assert!(html.contains("class='root-cause'"));
    assert!(html.contains("class='boundary'"));
}

#[test]
fn test_format_batch_bfs_report_markdown() {
    let result = make_test_bfs_result();
    let batch = BatchBfsReport {
        traces: vec![BfsTraceEntry {
            event_name: "ASSERT_PASSED".to_string(),
            entry_signal: "TOP.data_out".to_string(),
            fail_time_ps: 1750,
            result: result.clone(),
        }],
        aggregated_candidates: result.candidates.clone(),
        summary: "Batch trace complete.".to_string(),
    };

    let md = format_batch_bfs_report_markdown(&batch);
    assert!(md.contains("Batch BFS Root Cause Analysis Report"));
    assert!(md.contains("ASSERT_PASSED"));
    assert!(md.contains("Aggregated Root Cause Candidates"));
}

#[test]
fn test_format_batch_bfs_report_html_escapes() {
    let result = BfsResult {
        root_signal: "TOP.sig".to_string(),
        root_time_index: 1,
        root_time_ps: 100,
        tree: vec![],
        candidates: vec![],
        summary: "empty".to_string(),
    };
    let batch = BatchBfsReport {
        traces: vec![BfsTraceEntry {
            event_name: "ASSERT<script>".to_string(),
            entry_signal: "TOP.data&out".to_string(),
            fail_time_ps: 100,
            result,
        }],
        aggregated_candidates: vec![],
        summary: "test".to_string(),
    };

    let html = format_batch_bfs_report_html(&batch);
    assert!(
        !html.contains("<script>"),
        "Unescaped <script> in batch HTML"
    );
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&amp;"));
}
