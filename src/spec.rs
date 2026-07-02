//! Design spec lookup utilities.
//!
//! This module handles loading `design_spec.yaml` files and provides
//! minimal lookup for mapping assertion names to entry signals.

use crate::error::{WaveAnalyzerError, WaveResult};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// An assertion entry in the design spec.
#[derive(Debug, Clone, Deserialize)]
pub struct AssertionEntry {
    /// Assertion name.
    pub name: String,
    /// Optional requirement IDs.
    #[serde(default)]
    pub requirement_ids: Vec<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Reference clock.
    #[serde(default)]
    pub clock: Option<String>,
    /// Severity level.
    #[serde(default)]
    pub severity: Option<String>,
    /// Signals to observe when this assertion fails.
    #[serde(default)]
    pub observe_signals: Vec<String>,
    /// Optional SVA code.
    #[serde(default)]
    pub sva: Option<String>,
}

/// A behavior entry in the design spec.
#[derive(Debug, Clone, Deserialize)]
pub struct BehaviorEntry {
    /// Behavior ID.
    pub id: String,
    /// Optional requirement IDs.
    #[serde(default)]
    pub requirement_ids: Vec<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Behavior kind (invariant, event, latency, protocol).
    #[serde(default)]
    pub kind: Option<String>,
    /// Check engine (wave_analyzer_mcp or sva).
    #[serde(default)]
    pub check_engine: Option<String>,
    /// Check expression (for wave_analyzer_mcp engine).
    #[serde(default)]
    pub check: Option<String>,
    /// Signals that serve as BFS entry when this behavior fails.
    #[serde(default)]
    pub fail_entry_signals: Vec<String>,
    /// Reference clock.
    #[serde(default)]
    pub related_clock: Option<String>,
}

/// A debug hint entry in the design spec.
#[derive(Debug, Clone, Deserialize)]
pub struct DebugEntryPoint {
    /// Signal path.
    pub signal: String,
    /// Reason why this is a good debugging entry point.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Debug hints section.
#[derive(Debug, Clone, Deserialize)]
pub struct DebugHints {
    /// Preferred debugging entry points.
    #[serde(default)]
    pub entry_points: Vec<DebugEntryPoint>,
    /// Signals where BFS can stop.
    #[serde(default)]
    pub stop_signals: Vec<String>,
}

/// Top-level design_spec.yaml file structure (minimal subset).
#[derive(Debug, Clone, Deserialize)]
pub struct SpecFile {
    /// Spec version.
    pub spec_version: String,
    /// Module name.
    #[serde(default)]
    pub module_name: Option<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Assertions defined in the spec.
    #[serde(default)]
    pub assertions: Vec<AssertionEntry>,
    /// Behaviors defined in the spec.
    #[serde(default)]
    pub behaviors: Vec<BehaviorEntry>,
    /// Debug hints.
    #[serde(default)]
    pub debug_hints: Option<DebugHints>,
}

/// Runtime lookup structure for design spec.
#[derive(Debug, Clone)]
pub struct SpecLookup {
    /// Map from assertion name to its entry.
    assertions: HashMap<String, AssertionEntry>,
    /// List of behaviors.
    behaviors: Vec<BehaviorEntry>,
    /// Debug hints (if available).
    debug_hints: Option<DebugHints>,
}

/// Load a design_spec.yaml file and build the lookup structure.
///
/// # Arguments
/// * `path` - Path to the design_spec.yaml file
pub fn load_spec_from_file(path: &Path) -> WaveResult<SpecLookup> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    let spec_file: SpecFile =
        serde_yaml::from_str(&content).map_err(|e| WaveAnalyzerError::FileError {
            path: path.display().to_string(),
            message: format!("Failed to parse spec YAML: {}", e),
        })?;

    Ok(SpecLookup::from_spec_file(spec_file)?)
}

impl SpecLookup {
    /// Build a SpecLookup from a parsed SpecFile.
    pub fn from_spec_file(spec_file: SpecFile) -> WaveResult<Self> {
        let assertions: HashMap<String, AssertionEntry> = spec_file
            .assertions
            .into_iter()
            .map(|entry| (entry.name.clone(), entry))
            .collect();

        Ok(SpecLookup {
            assertions,
            behaviors: spec_file.behaviors,
            debug_hints: spec_file.debug_hints,
        })
    }

    /// Find entry signals for a given assertion name.
    ///
    /// Returns the `observe_signals` from the assertion entry.
    /// If the assertion name is not found, returns an empty list.
    pub fn find_entry_signals_by_assertion(&self, assertion_name: &str) -> Vec<String> {
        self.assertions
            .get(assertion_name)
            .map(|entry| entry.observe_signals.clone())
            .unwrap_or_default()
    }

    /// Find entry signals for a given behavior ID.
    ///
    /// Returns the `fail_entry_signals` from the behavior entry.
    /// If the behavior ID is not found, returns an empty list.
    pub fn find_entry_signals_by_behavior(&self, behavior_id: &str) -> Vec<String> {
        self.behaviors
            .iter()
            .find(|b| b.id == behavior_id)
            .map(|b| b.fail_entry_signals.clone())
            .unwrap_or_default()
    }

    /// Find all fail_entry_signals from all behaviors matching a requirement ID.
    pub fn find_entry_signals_by_requirement(&self, requirement_id: &str) -> Vec<String> {
        self.behaviors
            .iter()
            .filter(|b| b.requirement_ids.contains(&requirement_id.to_string()))
            .flat_map(|b| b.fail_entry_signals.clone())
            .collect()
    }

    /// Get debug entry points (if available).
    pub fn find_debug_entry_points(&self) -> Vec<DebugEntryPoint> {
        self.debug_hints
            .as_ref()
            .map(|hints| hints.entry_points.clone())
            .unwrap_or_default()
    }

    /// Get stop signals (if available).
    pub fn find_stop_signals(&self) -> Vec<String> {
        self.debug_hints
            .as_ref()
            .map(|hints| hints.stop_signals.clone())
            .unwrap_or_default()
    }

    /// Check if an assertion exists in the spec.
    pub fn has_assertion(&self, assertion_name: &str) -> bool {
        self.assertions.contains_key(assertion_name)
    }
}
