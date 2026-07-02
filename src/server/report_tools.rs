//! Helper functions for report/export tools (export_bfs, summary, svg, run_summary, analyze_run).

use rmcp::{ErrorData as McpError, model::*};
use std::path::PathBuf;
use wave_analyzer_mcp::WaveAnalyzerError;
use wave_analyzer_mcp::bfs::BfsResult;
use wave_analyzer_mcp::run_summary::{parse_run_summary_from_file, suggest_next_step};

use super::args::*;
use super::*;

pub async fn handle_export_bfs_report(
    bfs_results: &BfsResultStore,
    args: &ExportBfsReportArgs,
) -> Result<CallToolResult, McpError> {
    let format = args.format.as_deref().unwrap_or("markdown");

    // Try in-memory store first, then fall back to disk cache
    // (supports cross-process retrieval when MCP server was restarted)
    let bfs_results = bfs_results.read().await;
    let result: BfsResult = bfs_results
        .get(&args.trace_id)
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| wave_analyzer_mcp::bfs::load_bfs_result_from_cache(&args.trace_id))
        .map_err(|e| McpError::invalid_params(e, None))?;

    #[allow(clippy::wildcard_in_or_patterns)]
    match format {
        "json" => {
            let json_str = wave_analyzer_mcp::report::format_bfs_report_json(&result)
                .map_err(|e| McpError::internal_error(e, None))?;
            Ok(CallToolResult::success(vec![Content::text(json_str)]))
        }
        "html" => {
            let html_str = wave_analyzer_mcp::report::format_bfs_report_html(&result);
            Ok(CallToolResult::success(vec![Content::text(html_str)]))
        }
        "markdown" | _ => {
            let md_str = wave_analyzer_mcp::report::format_bfs_report_markdown(&result);
            Ok(CallToolResult::success(vec![Content::text(md_str)]))
        }
    }
}

pub async fn handle_load_run_summary(
    run_summaries: &RunSummaryStore,
    args: &LoadRunSummaryArgs,
) -> Result<CallToolResult, McpError> {
    let path = PathBuf::from(&args.file_path);

    if !path.exists() {
        return Err(McpError::invalid_params(
            WaveAnalyzerError::FileError {
                path: args.file_path.clone(),
                message: "not found".into(),
            },
            None,
        ));
    }

    let run_summary = match parse_run_summary_from_file(&path) {
        Ok(s) => s,
        Err(e) => {
            return Err(McpError::invalid_params(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("Failed to parse run_summary.json: {}", e),
                },
                None,
            ));
        }
    };

    let alias = args.alias.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let next_step = suggest_next_step(&run_summary);

    let summary_text = format!(
        "Run summary loaded with alias: {}\n\
        Status: {}\n\
        Project: {}, Top module: {}\n\
        Compile OK: {}, Elab OK: {}, Simulation OK: {}\n\
        Assertion failures: {}, Warnings: {}, Errors: {}\n\
        Wave file: {} ({})\n\
        Transcript: {}\n\
        Simulator: {}\n\
        Finished at: {}\n\
        Suggested next step: {}",
        alias,
        run_summary.status,
        run_summary.project_name,
        run_summary.top_module,
        run_summary.compile_ok,
        run_summary.elab_ok,
        run_summary.simulation_ok,
        run_summary.assertion_fail_count,
        run_summary.warning_count,
        run_summary.error_count,
        run_summary.wave_file,
        run_summary.wave_format,
        run_summary.transcript_file,
        run_summary.simulator,
        run_summary.finished_at,
        next_step,
    );

    // Store in run_summaries map
    {
        let mut run_summaries = run_summaries.write().await;
        run_summaries.insert(alias.clone(), run_summary.clone());
    }

    Ok(CallToolResult::success(vec![
        Content::text(summary_text),
        Content::json(&run_summary).map_err(|e| {
            McpError::internal_error(
                WaveAnalyzerError::InvalidArgument {
                    message: format!("JSON serialization failed: {}", e),
                },
                None,
            )
        })?,
    ]))
}
