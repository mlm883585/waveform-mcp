//! Run summary parser for simulation run_summary.json files
//!
//! Handles PowerShell's ConvertTo-Json which outputs boolean fields as
//! strings ("true"/"false") as well as proper JSON booleans (true/false).

use crate::error::{WaveAnalyzerError, WaveResult};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::Path;

/// Structured representation of a simulation run summary.
///
/// Boolean fields use `deserialize_bool_or_string` to handle both
/// proper JSON booleans and PowerShell string booleans ("true"/"false").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub status: String,
    pub project_name: String,
    pub top_module: String,
    #[serde(deserialize_with = "deserialize_bool_or_string")]
    pub compile_ok: bool,
    #[serde(deserialize_with = "deserialize_bool_or_string")]
    pub elab_ok: bool,
    #[serde(deserialize_with = "deserialize_bool_or_string")]
    pub simulation_ok: bool,
    pub assertion_fail_count: u32,
    pub warning_count: u32,
    pub error_count: u32,
    pub wave_file: String,
    pub wave_format: String,
    pub transcript_file: String,
    pub simulator: String,
    pub finished_at: String,
}

/// Custom deserializer that accepts both JSON boolean and string "true"/"false".
///
/// PowerShell's ConvertTo-Json sometimes outputs boolean values as strings
/// (e.g., `"true"` instead of `true`). This deserializer handles both cases.
fn deserialize_bool_or_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    // Use a helper enum that can deserialize from either a bool or a string
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrString {
        Bool(bool),
        String(String),
    }

    match BoolOrString::deserialize(deserializer)? {
        BoolOrString::Bool(b) => Ok(b),
        BoolOrString::String(s) => match s.to_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(serde::de::Error::custom(format!(
                "Invalid boolean string: '{}'. Expected 'true' or 'false'.",
                other
            ))),
        },
    }
}

/// Returns an actionable next-step suggestion based on the run summary status.
pub fn suggest_next_step(summary: &RunSummary) -> String {
    match summary.status.as_str() {
        "passed" => "All checks passed. Proceed with waveform analysis.".to_string(),
        "compile_failed" => "Compile failed. Fix RTL/TB compilation errors before proceeding.".to_string(),
        "elab_failed" => "Elaboration failed. Check module instantiation and library paths.".to_string(),
        "simulation_failed" => "Simulation failed. Check transcript for runtime errors.".to_string(),
        "assertion_failed" => {
            "Assertion/check failures detected. Use load_assertion_log + trace_root_cause for root cause analysis.".to_string()
        }
        other => format!(
            "Unknown status '{}'. Review the run summary manually.",
            other
        ),
    }
}

/// Parse a run_summary.json file into a RunSummary struct.
///
/// Reads the file at the given path and deserializes it, handling
/// both standard JSON booleans and PowerShell string booleans.
pub fn parse_run_summary_from_file(path: &Path) -> WaveResult<RunSummary> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    let summary: RunSummary =
        serde_json::from_str(&content).map_err(|e| WaveAnalyzerError::FileError {
            path: path.display().to_string(),
            message: format!("Failed to parse run_summary.json: {}", e),
        })?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_with_json_booleans() {
        let json = r#"{
            "status": "passed",
            "project_name": "test_proj",
            "top_module": "top",
            "compile_ok": true,
            "elab_ok": true,
            "simulation_ok": true,
            "assertion_fail_count": 0,
            "warning_count": 1,
            "error_count": 0,
            "wave_file": "dump.vcd",
            "wave_format": "vcd",
            "transcript_file": "transcript.log",
            "simulator": "modelsim",
            "finished_at": "2025-01-01T00:00:00"
        }"#;

        let summary: RunSummary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.status, "passed");
        assert!(summary.compile_ok);
        assert!(summary.elab_ok);
        assert!(summary.simulation_ok);
        assert_eq!(summary.assertion_fail_count, 0);
    }

    #[test]
    fn test_deserialize_with_string_booleans() {
        // PowerShell ConvertTo-Json outputs booleans as strings
        let json = r#"{
            "status": "compile_failed",
            "project_name": "test_proj",
            "top_module": "top",
            "compile_ok": "false",
            "elab_ok": "false",
            "simulation_ok": "false",
            "assertion_fail_count": 0,
            "warning_count": 5,
            "error_count": 2,
            "wave_file": "",
            "wave_format": "",
            "transcript_file": "",
            "simulator": "modelsim",
            "finished_at": "2025-01-01T00:00:00"
        }"#;

        let summary: RunSummary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.status, "compile_failed");
        assert!(!summary.compile_ok);
        assert!(!summary.elab_ok);
        assert!(!summary.simulation_ok);
    }

    #[test]
    fn test_deserialize_with_mixed_booleans() {
        // Some booleans are proper JSON, some are strings
        let json = r#"{
            "status": "assertion_failed",
            "project_name": "test_proj",
            "top_module": "top",
            "compile_ok": true,
            "elab_ok": "true",
            "simulation_ok": "true",
            "assertion_fail_count": 3,
            "warning_count": 10,
            "error_count": 1,
            "wave_file": "dump.vcd",
            "wave_format": "vcd",
            "transcript_file": "transcript.log",
            "simulator": "modelsim",
            "finished_at": "2025-01-01T00:00:00"
        }"#;

        let summary: RunSummary = serde_json::from_str(json).unwrap();
        assert!(summary.compile_ok);
        assert!(summary.elab_ok);
        assert!(summary.simulation_ok);
        assert_eq!(summary.assertion_fail_count, 3);
    }

    #[test]
    fn test_suggest_next_step_passed() {
        let summary = RunSummary {
            status: "passed".to_string(),
            project_name: "test".to_string(),
            top_module: "top".to_string(),
            compile_ok: true,
            elab_ok: true,
            simulation_ok: true,
            assertion_fail_count: 0,
            warning_count: 0,
            error_count: 0,
            wave_file: "dump.vcd".to_string(),
            wave_format: "vcd".to_string(),
            transcript_file: "trans.log".to_string(),
            simulator: "modelsim".to_string(),
            finished_at: "2025-01-01".to_string(),
        };
        assert_eq!(
            suggest_next_step(&summary),
            "All checks passed. Proceed with waveform analysis."
        );
    }

    #[test]
    fn test_suggest_next_step_compile_failed() {
        let summary = RunSummary {
            status: "compile_failed".to_string(),
            project_name: "test".to_string(),
            top_module: "top".to_string(),
            compile_ok: false,
            elab_ok: false,
            simulation_ok: false,
            assertion_fail_count: 0,
            warning_count: 0,
            error_count: 0,
            wave_file: "".to_string(),
            wave_format: "".to_string(),
            transcript_file: "".to_string(),
            simulator: "modelsim".to_string(),
            finished_at: "2025-01-01".to_string(),
        };
        assert_eq!(
            suggest_next_step(&summary),
            "Compile failed. Fix RTL/TB compilation errors before proceeding."
        );
    }

    #[test]
    fn test_suggest_next_step_assertion_failed() {
        let summary = RunSummary {
            status: "assertion_failed".to_string(),
            project_name: "test".to_string(),
            top_module: "top".to_string(),
            compile_ok: true,
            elab_ok: true,
            simulation_ok: true,
            assertion_fail_count: 2,
            warning_count: 0,
            error_count: 0,
            wave_file: "dump.vcd".to_string(),
            wave_format: "vcd".to_string(),
            transcript_file: "trans.log".to_string(),
            simulator: "modelsim".to_string(),
            finished_at: "2025-01-01".to_string(),
        };
        assert_eq!(
            suggest_next_step(&summary),
            "Assertion/check failures detected. Use load_assertion_log + trace_root_cause for root cause analysis."
        );
    }

    #[test]
    fn test_invalid_boolean_string() {
        let json = r#"{
            "status": "passed",
            "project_name": "test_proj",
            "top_module": "top",
            "compile_ok": "maybe",
            "elab_ok": true,
            "simulation_ok": true,
            "assertion_fail_count": 0,
            "warning_count": 0,
            "error_count": 0,
            "wave_file": "",
            "wave_format": "",
            "transcript_file": "",
            "simulator": "",
            "finished_at": ""
        }"#;

        let result: Result<RunSummary, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
