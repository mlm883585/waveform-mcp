//! Dependency graph loading and query utilities.
//!
//! This module handles loading `deps.yaml` files, building fan-in/fan-out
//! graph structures, and resolving signal/clock aliases.

use crate::error::{WaveAnalyzerError, WaveResult};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Dependency edge type.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepType {
    Combinational,
    Sequential,
    Memory,
    Control,
    Protocol,
    Boundary,
}

impl std::fmt::Display for DepType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DepType::Combinational => write!(f, "combinational"),
            DepType::Sequential => write!(f, "sequential"),
            DepType::Memory => write!(f, "memory"),
            DepType::Control => write!(f, "control"),
            DepType::Protocol => write!(f, "protocol"),
            DepType::Boundary => write!(f, "boundary"),
        }
    }
}

/// Boundary kind for `boundary` type edges.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    InputPort,
    Constant,
    Cdc,
    Blackbox,
    ManualStop,
}

/// Protocol kind for `protocol` type edges.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolKind {
    Handshake,
    Backpressure,
    #[serde(rename = "no_protocol")]
    NoProtocol,
}

/// Clock edge type.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockEdge {
    Posedge,
    Negedge,
}

/// Check expression for edge validation.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckExpr {
    Equal,
    NotEqual,
    GreaterThanZero,
    EqualZero,
}

impl<'de> serde::Deserialize<'de> for CheckExpr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "=" => Ok(CheckExpr::Equal),
            "!=" => Ok(CheckExpr::NotEqual),
            ">0" => Ok(CheckExpr::GreaterThanZero),
            "==0" => Ok(CheckExpr::EqualZero),
            other => Err(serde::de::Error::custom(format!(
                "Invalid check value: '{}'. Expected one of: '=', '!=', '>0', '==0'",
                other
            ))),
        }
    }
}

/// Logic type for combinational edges, used by BFS hint inference.
///
/// When specified, the BFS engine uses this to correctly classify
/// input signals as Suspect vs Context based on the logic semantics:
///
/// - **OR**: any input=1 → output=1. When output=0 but input=1, that's a
///   contradiction (Suspect). When output=1 and input=0, other inputs may
///   satisfy (Context).
/// - **AND**: all inputs=1 → output=1. When output=1 but input=0, that's a
///   contradiction (Suspect). When output=0 and input=1, other inputs may
///   drive output low (Context).
/// - **NAND**: AND followed by NOT. Inverted semantics from AND.
/// - **NOR**: OR followed by NOT. Inverted semantics from OR.
/// - **XOR**: output=1 when odd number of inputs=1. Generally ambiguous
///   for single-input analysis (Context).
/// - **MUX**: one input selected by a selector signal. The non-selected
///   inputs don't contribute (Context).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicType {
    Or,
    And,
    Nand,
    Nor,
    Xor,
    Mux,
}

impl std::fmt::Display for LogicType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogicType::Or => write!(f, "OR"),
            LogicType::And => write!(f, "AND"),
            LogicType::Nand => write!(f, "NAND"),
            LogicType::Nor => write!(f, "NOR"),
            LogicType::Xor => write!(f, "XOR"),
            LogicType::Mux => write!(f, "MUX"),
        }
    }
}

/// A single dependency edge in the graph.
#[derive(Debug, Clone, Deserialize)]
pub struct DepEdge {
    /// Upstream signal path (canonical name).
    pub signal: String,
    /// Dependency type.
    #[serde(rename = "type")]
    pub dep_type: DepType,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Logic type for combinational edges (OR/AND/NAND/NOR/XOR/MUX).
    /// Used by BFS hint inference to correctly classify input signals.
    #[serde(default)]
    pub logic_type: Option<LogicType>,
    /// Reference clock name (logical name, or null for combinational/boundary).
    #[serde(default)]
    pub clock: Option<String>,
    /// Clock edge (posedge/negedge, or null).
    #[serde(default)]
    pub edge: Option<ClockEdge>,
    /// Latency in clock cycles.
    #[serde(default)]
    pub latency_cycles: Option<u32>,
    /// Protocol kind (only for protocol type).
    #[serde(default)]
    pub protocol_kind: Option<ProtocolKind>,
    /// Boundary kind (only for boundary type).
    #[serde(default)]
    pub boundary_kind: Option<BoundaryKind>,
    /// Lightweight check expression.
    #[serde(default)]
    pub check: Option<CheckExpr>,
    /// Full condition expression for edge evaluation (LALRPOP grammar).
    /// Takes precedence over `check` when present. Falls back to `check` on evaluation error.
    #[serde(default)]
    pub condition_expression: Option<String>,
    /// Source clock domain for CDC edges.
    #[serde(default)]
    pub cdc_from_clock: Option<String>,
    /// Destination clock domain for CDC edges.
    #[serde(default)]
    pub cdc_to_clock: Option<String>,
}

/// A dependency entry for an output signal.
#[derive(Debug, Clone, Deserialize)]
pub struct DependencyEntry {
    /// Output signal path (canonical name).
    pub output: String,
    /// Signal category.
    #[serde(default)]
    pub category: Option<String>,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// List of upstream dependencies.
    pub depends_on: Vec<DepEdge>,
}

/// Signal alias entry mapping canonical name to simulator-specific paths.
#[derive(Debug, Clone, Deserialize)]
pub struct SignalAliasEntry {
    /// Canonical signal path.
    pub canonical: String,
    /// ModelSim waveform path.
    #[serde(default)]
    pub modelsim: Option<String>,
    /// Optional Vivado waveform path.
    #[serde(default)]
    pub vivado: Option<String>,
}

/// Clock alias entry mapping logical clock name to simulator-specific paths.
#[derive(Debug, Clone, Deserialize)]
pub struct ClockAliasEntry {
    /// Logical clock name (must match design_spec.yaml clock_domains[].name).
    pub clock_name: String,
    /// ModelSim waveform clock path.
    #[serde(default)]
    pub modelsim: Option<String>,
    /// Optional Vivado waveform clock path.
    #[serde(default)]
    pub vivado: Option<String>,
}

/// Top-level deps.yaml file structure.
#[derive(Debug, Clone, Deserialize)]
pub struct DepsFile {
    /// Format version.
    pub format_version: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
    /// Signal alias mappings.
    /// Accepts sequence format: `[{canonical: X, modelsim: Y}]`
    /// or map format: `{X: {modelsim: Y}}` or `{X: Y}`
    #[serde(default, deserialize_with = "deserialize_signal_aliases")]
    pub signal_aliases: Vec<SignalAliasEntry>,
    /// Clock alias mappings.
    /// Accepts sequence format: `[{clock_name: X, modelsim: Y}]`
    /// or map format: `{X: {modelsim: Y}}` or `{X: Y}`
    #[serde(default, deserialize_with = "deserialize_clock_aliases")]
    pub clock_aliases: Vec<ClockAliasEntry>,
    /// Dependency entries.
    pub dependencies: Vec<DependencyEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum SignalAliasValue {
    String(String),
    Object {
        #[serde(default)]
        modelsim: Option<String>,
        #[serde(default)]
        vivado: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ClockAliasValue {
    String(String),
    Object {
        #[serde(default)]
        modelsim: Option<String>,
        #[serde(default)]
        vivado: Option<String>,
    },
}

/// Custom deserializer for signal_aliases that accepts both sequence and map formats.
fn deserialize_signal_aliases<'de, D>(deserializer: D) -> Result<Vec<SignalAliasEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    use serde_yaml::Value;

    let value = Value::deserialize(deserializer)?;

    match value {
        Value::Null => Ok(Vec::new()),
        Value::Sequence(seq) => {
            // Sequence format: [{canonical: X, modelsim: Y}, ...]
            let mut entries = Vec::new();
            for item in seq {
                match item {
                    Value::String(path) => entries.push(SignalAliasEntry {
                        canonical: path.clone(),
                        modelsim: Some(path),
                        vivado: None,
                    }),
                    other => {
                        let entry: SignalAliasEntry =
                            serde_yaml::from_value(other).map_err(de::Error::custom)?;
                        entries.push(entry);
                    }
                }
            }
            Ok(entries)
        }
        Value::Mapping(map) => {
            // Map format: {canonical: {modelsim: Y, vivado: Z}} or {canonical: "modelsim_path"}
            let mut entries = Vec::new();
            for (key, val) in map {
                let canonical = key
                    .as_str()
                    .ok_or_else(|| de::Error::custom("signal_aliases map key must be a string"))?
                    .to_string();
                let value: SignalAliasValue =
                    serde_yaml::from_value(val).map_err(de::Error::custom)?;
                let (modelsim, vivado) = match value {
                    SignalAliasValue::String(modelsim_path) => (Some(modelsim_path), None),
                    SignalAliasValue::Object { modelsim, vivado } => (modelsim, vivado),
                };
                entries.push(SignalAliasEntry {
                    canonical,
                    modelsim,
                    vivado,
                });
            }
            Ok(entries)
        }
        _ => Err(de::Error::custom(
            "signal_aliases must be a sequence or mapping",
        )),
    }
}

/// Custom deserializer for clock_aliases that accepts both sequence and map formats.
fn deserialize_clock_aliases<'de, D>(deserializer: D) -> Result<Vec<ClockAliasEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    use serde_yaml::Value;

    let value = Value::deserialize(deserializer)?;

    match value {
        Value::Null => Ok(Vec::new()),
        Value::Sequence(seq) => {
            let mut entries = Vec::new();
            for item in seq {
                match item {
                    Value::String(path) => entries.push(ClockAliasEntry {
                        clock_name: path.clone(),
                        modelsim: Some(path),
                        vivado: None,
                    }),
                    other => {
                        let entry: ClockAliasEntry =
                            serde_yaml::from_value(other).map_err(de::Error::custom)?;
                        entries.push(entry);
                    }
                }
            }
            Ok(entries)
        }
        Value::Mapping(map) => {
            let mut entries = Vec::new();
            for (key, val) in map {
                let clock_name = key
                    .as_str()
                    .ok_or_else(|| de::Error::custom("clock_aliases map key must be a string"))?
                    .to_string();
                let value: ClockAliasValue =
                    serde_yaml::from_value(val).map_err(de::Error::custom)?;
                let (modelsim, vivado) = match value {
                    ClockAliasValue::String(modelsim_path) => (Some(modelsim_path), None),
                    ClockAliasValue::Object { modelsim, vivado } => (modelsim, vivado),
                };
                entries.push(ClockAliasEntry {
                    clock_name,
                    modelsim,
                    vivado,
                });
            }
            Ok(entries)
        }
        _ => Err(de::Error::custom(
            "clock_aliases must be a sequence or mapping",
        )),
    }
}

/// Runtime dependency graph with fan-in/fan-out indices and alias resolution.
#[derive(Debug, Clone)]
pub struct DepGraph {
    /// Fan-in index: for each output signal, list of upstream edges.
    fan_in: HashMap<String, Vec<DepEdge>>,
    /// Fan-out index: for each upstream signal, list of output signals it feeds.
    fan_out: HashMap<String, Vec<String>>,
    /// Signal category mapping: output signal path -> category string.
    categories: HashMap<String, String>,
    /// Signal alias resolver: canonical -> simulator -> resolved path.
    signal_aliases: HashMap<String, HashMap<String, String>>,
    /// Clock alias resolver: clock_name -> simulator -> resolved path.
    clock_aliases: HashMap<String, HashMap<String, String>>,
    /// Metadata from the deps file.
    meta: DepsFileMeta,
}

/// Metadata extracted from deps file.
#[derive(Debug, Clone)]
pub struct DepsFileMeta {
    pub format_version: String,
    pub description: Option<String>,
    pub node_count: usize,
    pub edge_count: usize,
    pub has_cycles: bool,
    pub signal_alias_count: usize,
    pub clock_alias_count: usize,
}

/// Load a deps.yaml file and build the dependency graph.
///
/// # Arguments
/// * `path` - Path to the deps.yaml file
///
/// # Returns
/// A `DepGraph` with fan-in/fan-out indices and alias mappings.
pub fn load_deps_from_file(path: &Path) -> WaveResult<DepGraph> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: format!("Failed to read deps file: {}", e),
    })?;

    // Always preprocess YAML to handle `signal:` + `output: boolean` format.
    // serde_yaml converts `output: true` (YAML bool) to String "true", which
    // causes direct parsing to succeed with wrong signal names (e.g., "true"
    // instead of "o_power_off"). Preprocessing at the YAML level fixes this.
    let value: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| WaveAnalyzerError::DepsError {
            message: format!("Failed to parse deps YAML: {}", e),
        })?;
    let preprocessed = preprocess_deps_yaml(value).map_err(|e| WaveAnalyzerError::DepsError {
        message: format!("deps YAML preprocessing failed: {}", e),
    })?;
    let deps_file: DepsFile =
        serde_yaml::from_value(preprocessed).map_err(|e| WaveAnalyzerError::DepsError {
            message: format!("Failed to parse deps YAML: {}", e),
        })?;

    DepGraph::from_deps_file(deps_file)
}

/// Preprocess deps YAML to handle format variations before serde parsing.
///
/// Handles:
/// 1. `signal:` key → rename to `output:` (the modern key)
/// 2. `output: true/false` (boolean) → remove (was an "is output" flag, not a name)
/// 3. String-typed `depends_on` entries → convert to `[{signal: ..., type: combinational}]`
/// 4. Object-typed depends_on with `signal:` key → already handled by DepEdge
///
/// This preprocessing is critical because serde_yaml converts `output: true`
/// (YAML boolean) to the String "true" when the target type is String, causing
/// `DependencyEntry.output = "true"` instead of the actual signal name.
fn preprocess_deps_yaml(value: serde_yaml::Value) -> WaveResult<serde_yaml::Value> {
    let mut root = match value {
        serde_yaml::Value::Mapping(map) => map,
        _ => {
            return Err(WaveAnalyzerError::DepsError {
                message: "top-level deps YAML must be a mapping".to_string(),
            });
        }
    };

    // Preprocess dependencies section
    let deps_key = serde_yaml::Value::String("dependencies".to_string());
    if let Some(dependencies) = root.remove(&deps_key) {
        let preprocessed_deps = preprocess_dependency_entries(dependencies)?;
        root.insert(deps_key, preprocessed_deps);
    }

    Ok(serde_yaml::Value::Mapping(root))
}

/// Preprocess a sequence of dependency entries at the YAML level.
fn preprocess_dependency_entries(deps: serde_yaml::Value) -> WaveResult<serde_yaml::Value> {
    let seq = match deps {
        serde_yaml::Value::Sequence(s) => s,
        _ => {
            return Err(WaveAnalyzerError::DepsError {
                message: "dependencies must be a sequence".to_string(),
            });
        }
    };

    let mut result = Vec::new();
    for entry in seq {
        let mapping = match entry {
            serde_yaml::Value::Mapping(m) => m,
            _ => {
                return Err(WaveAnalyzerError::DepsError {
                    message: "each dependency entry must be a mapping".to_string(),
                });
            }
        };
        result.push(preprocess_single_entry(mapping));
    }

    Ok(serde_yaml::Value::Sequence(result))
}

/// Preprocess a single dependency entry mapping.
///
/// If `signal:` key is present, its value becomes the `output:` string.
/// If `output:` is a boolean (`true`/`false`), it is removed (it was a flag).
/// If `depends_on` contains bare strings, they are wrapped into objects.
fn preprocess_single_entry(mut mapping: serde_yaml::Mapping) -> serde_yaml::Value {
    let signal_key = serde_yaml::Value::String("signal".to_string());
    let output_key = serde_yaml::Value::String("output".to_string());

    // If `signal:` key exists, use its value as `output:` (the signal name)
    if let Some(signal_val) = mapping.remove(&signal_key) {
        // Remove boolean `output:` value if present (it was an "is output" flag)
        if let Some(output_val) = mapping.get(&output_key) {
            if matches!(output_val, serde_yaml::Value::Bool(_)) {
                mapping.remove(&output_key);
            }
        }
        // Set `output:` to the signal name from `signal:` key
        mapping.insert(output_key, signal_val);
    }

    // Preprocess depends_on: convert bare string entries to objects
    let depends_key = serde_yaml::Value::String("depends_on".to_string());
    if let Some(depends_on) = mapping.remove(&depends_key) {
        let preprocessed = preprocess_depends_on(depends_on);
        mapping.insert(depends_key, preprocessed);
    }

    serde_yaml::Value::Mapping(mapping)
}

/// Preprocess depends_on entries: convert bare strings to proper DepEdge objects.
fn preprocess_depends_on(depends_on: serde_yaml::Value) -> serde_yaml::Value {
    let seq = match depends_on {
        serde_yaml::Value::Sequence(s) => s,
        other => return other, // leave non-sequence values unchanged
    };

    let mut result = Vec::new();
    for item in seq {
        match item {
            // Bare string: convert to {signal: ..., type: combinational}
            serde_yaml::Value::String(s) => {
                let mut edge_map = serde_yaml::Mapping::new();
                edge_map.insert(
                    serde_yaml::Value::String("signal".to_string()),
                    serde_yaml::Value::String(s),
                );
                edge_map.insert(
                    serde_yaml::Value::String("type".to_string()),
                    serde_yaml::Value::String("combinational".to_string()),
                );
                result.push(serde_yaml::Value::Mapping(edge_map));
            }
            // Object: keep as-is (DepEdge deserializer handles `signal:` key)
            other => result.push(other),
        }
    }

    serde_yaml::Value::Sequence(result)
}

impl DepGraph {
    /// Build a DepGraph from a parsed DepsFile.
    pub fn from_deps_file(deps_file: DepsFile) -> WaveResult<Self> {
        // Build signal alias resolver
        let signal_aliases: HashMap<String, HashMap<String, String>> = deps_file
            .signal_aliases
            .iter()
            .map(|entry| {
                let mut sim_map = HashMap::new();
                if let Some(ref ms) = entry.modelsim {
                    sim_map.insert("modelsim".to_string(), ms.clone());
                }
                if let Some(ref vd) = entry.vivado {
                    sim_map.insert("vivado".to_string(), vd.clone());
                }
                (entry.canonical.clone(), sim_map)
            })
            .collect();

        // Build clock alias resolver
        let clock_aliases: HashMap<String, HashMap<String, String>> = deps_file
            .clock_aliases
            .iter()
            .map(|entry| {
                let mut sim_map = HashMap::new();
                if let Some(ref ms) = entry.modelsim {
                    sim_map.insert("modelsim".to_string(), ms.clone());
                }
                if let Some(ref vd) = entry.vivado {
                    sim_map.insert("vivado".to_string(), vd.clone());
                }
                (entry.clock_name.clone(), sim_map)
            })
            .collect();

        // Build fan-in index
        let mut fan_in: HashMap<String, Vec<DepEdge>> = HashMap::new();
        let mut total_edges = 0;
        for entry in &deps_file.dependencies {
            total_edges += entry.depends_on.len();
            fan_in.insert(entry.output.clone(), entry.depends_on.clone());
        }

        // Build fan-out index
        let mut fan_out: HashMap<String, Vec<String>> = HashMap::new();
        for entry in &deps_file.dependencies {
            for dep in &entry.depends_on {
                fan_out
                    .entry(dep.signal.clone())
                    .or_default()
                    .push(entry.output.clone());
            }
        }

        // Build categories index
        let mut categories: HashMap<String, String> = HashMap::new();
        for entry in &deps_file.dependencies {
            if let Some(ref cat) = entry.category {
                categories.insert(entry.output.clone(), cat.clone());
            }
        }

        // Detect cycles using DFS with coloring (not just self-loops)
        let has_cycles = detect_cycles(&fan_in);

        let node_count = deps_file.dependencies.len();
        let signal_alias_count = deps_file.signal_aliases.len();
        let clock_alias_count = deps_file.clock_aliases.len();

        Ok(DepGraph {
            fan_in,
            fan_out,
            categories,
            signal_aliases,
            clock_aliases,
            meta: DepsFileMeta {
                format_version: deps_file.format_version.clone(),
                description: deps_file.description.clone(),
                node_count,
                edge_count: total_edges,
                has_cycles,
                signal_alias_count,
                clock_alias_count,
            },
        })
    }

    /// Get fan-in edges for a signal (upstream dependencies).
    pub fn fan_in(&self, signal: &str) -> Option<&Vec<DepEdge>> {
        self.fan_in.get(signal)
    }

    /// Get fan-out signals for a signal (downstream dependents).
    pub fn fan_out(&self, signal: &str) -> Option<&Vec<String>> {
        self.fan_out.get(signal)
    }

    /// Resolve a canonical signal name to a simulator-specific waveform path.
    ///
    /// # Arguments
    /// * `canonical` - Canonical signal path from deps.yaml
    /// * `simulator` - Simulator identifier (e.g., "modelsim", "vivado")
    ///
    /// # Returns
    /// `Some(resolved_path)` if an alias mapping exists, `None` if no alias
    /// mapping is found for this canonical name and simulator.
    pub fn resolve_signal(&self, canonical: &str, simulator: &str) -> Option<String> {
        self.signal_aliases
            .get(canonical)
            .and_then(|sim_map| sim_map.get(simulator).cloned())
    }

    /// Resolve a canonical signal name with fuzzy leaf-name fallback.
    ///
    /// First tries exact alias lookup. If that fails, extracts the leaf name
    /// (last segment after '.') and searches all alias entries whose canonical
    /// or simulator-specific path's leaf name matches. This handles cases where
    /// deps.yaml uses short names like `o_power_off` but the waveform has
    /// full paths like `protect_ctrl_top_tb.u_dut.o_power_off`.
    pub fn resolve_signal_fuzzy(&self, canonical: &str, simulator: &str) -> Option<String> {
        // Try exact match first
        if let Some(resolved) = self.resolve_signal(canonical, simulator) {
            return Some(resolved);
        }

        // Fuzzy: match by leaf name
        let leaf = canonical.rsplit('.').next().unwrap_or(canonical);

        // Check if any alias entry has a canonical that ends with this leaf name
        for (alias_canonical, sim_map) in &self.signal_aliases {
            let alias_leaf = alias_canonical
                .rsplit('.')
                .next()
                .unwrap_or(alias_canonical);
            if alias_leaf == leaf {
                if let Some(resolved) = sim_map.get(simulator) {
                    return Some(resolved.clone());
                }
                // If no simulator-specific path, return the canonical itself
                return Some(alias_canonical.clone());
            }
            // Also check simulator-specific path leaf names
            if let Some(sim_path) = sim_map.get(simulator) {
                let sim_leaf = sim_path.rsplit('.').next().unwrap_or(sim_path);
                if sim_leaf == leaf {
                    return Some(sim_path.clone());
                }
            }
        }

        None
    }

    /// Resolve a simulator-specific path back to its canonical signal name.
    pub fn canonicalize_signal(&self, path: &str, simulator: &str) -> Option<String> {
        self.signal_aliases.iter().find_map(|(canonical, sim_map)| {
            sim_map
                .get(simulator)
                .filter(|resolved| resolved.as_str() == path)
                .map(|_| canonical.clone())
        })
    }

    /// Resolve a simulator-specific path back to canonical with fuzzy leaf-name fallback.
    ///
    /// Resolution order:
    /// 1. Exact reverse lookup in signal_aliases
    /// 2. Leaf-name match in signal_aliases (case-sensitive then case-insensitive)
    /// 3. Leaf-name match in fan_in keys (output nodes with traceable dependencies)
    /// 4. Leaf-name match in fan_out keys (upstream/boundary signals)
    ///
    /// This handles waveform paths like `tb.u_dut.o_power_off` mapping to
    /// canonical `o_power_off`, even when signal_aliases are empty or
    /// the deps.yaml uses different naming conventions.
    pub fn canonicalize_signal_fuzzy(&self, path: &str, simulator: &str) -> Option<String> {
        // Try exact match first
        if let Some(canonical) = self.canonicalize_signal(path, simulator) {
            return Some(canonical);
        }

        // Fuzzy: match by leaf name (case-sensitive then case-insensitive)
        let leaf = path.rsplit('.').next().unwrap_or(path);
        let leaf_lower = leaf.to_lowercase();

        // Search signal aliases by leaf name
        for (canonical, sim_map) in &self.signal_aliases {
            // Check if canonical leaf matches path leaf
            let canonical_leaf = canonical.rsplit('.').next().unwrap_or(canonical);
            if canonical_leaf == leaf {
                return Some(canonical.clone());
            }
            if canonical_leaf.to_lowercase() == leaf_lower {
                return Some(canonical.clone());
            }
            // Also check simulator path leaf
            if let Some(sim_path) = sim_map.get(simulator) {
                let sim_leaf = sim_path.rsplit('.').next().unwrap_or(sim_path);
                if sim_leaf == leaf {
                    return Some(canonical.clone());
                }
                if sim_leaf.to_lowercase() == leaf_lower {
                    return Some(canonical.clone());
                }
            }
        }

        // Search fan_in keys (output nodes) by leaf name — these are
        // the primary targets for BFS tracing and entry signal suggestion
        for canonical in self.fan_in.keys() {
            let canonical_leaf = canonical.rsplit('.').next().unwrap_or(canonical);
            if canonical_leaf == leaf || canonical_leaf.to_lowercase() == leaf_lower {
                return Some(canonical.clone());
            }
        }

        // Search fan_out keys (upstream/boundary signals) by leaf name
        for canonical in self.fan_out.keys() {
            let canonical_leaf = canonical.rsplit('.').next().unwrap_or(canonical);
            if canonical_leaf == leaf || canonical_leaf.to_lowercase() == leaf_lower {
                return Some(canonical.clone());
            }
        }

        None
    }

    /// Infer signal aliases from waveform hierarchy by leaf-name matching.
    ///
    /// Scans all canonical names in the deps graph and matches them against
    /// waveform signal leaf names. Only populates aliases for canonical names
    /// that don't already have an alias for the given simulator.
    ///
    /// This is critical for deps.yaml files that use short names (e.g., `led`,
    /// `counter`) while the waveform has full hierarchical paths (e.g.,
    /// `top.dut.led`, `top.dut.counter`).
    pub fn infer_aliases_from_waveform(&mut self, hierarchy: &wellen::Hierarchy, simulator: &str) {
        // Build leaf-name → full-path map from waveform hierarchy FIRST,
        // so we can check whether existing aliases point to valid signals
        let mut leaf_to_paths: HashMap<String, Vec<String>> = HashMap::new();
        for var in hierarchy.iter_vars() {
            let full_path = var.full_name(hierarchy);
            let leaf = full_path.rsplit('.').next().unwrap_or(&full_path);
            leaf_to_paths
                .entry(leaf.to_string())
                .or_default()
                .push(full_path.to_string());
        }

        // Also build a set of all full paths for quick existence checking
        let all_paths: HashSet<String> = leaf_to_paths
            .values()
            .flat_map(|paths| paths.iter().cloned())
            .collect();

        // Collect canonical names that need alias resolution.
        // Skip canonicals whose existing alias points to a signal that
        // actually exists in this waveform hierarchy. Re-infer canonicals
        // whose alias points to a non-existent signal (e.g., wrong testbench name).
        let canonical_names: HashSet<String> = {
            let mut names = HashSet::new();
            for output in self.fan_in.keys() {
                if let Some(alias_map) = self.signal_aliases.get(output) {
                    if let Some(existing_alias) = alias_map.get(simulator) {
                        if all_paths.contains(existing_alias) {
                            continue; // Existing alias points to a valid signal
                        }
                        // Existing alias points to a non-existent signal — needs re-inference
                    }
                }
                names.insert(output.clone());
            }
            for edges in self.fan_in.values() {
                for edge in edges {
                    if let Some(alias_map) = self.signal_aliases.get(&edge.signal) {
                        if let Some(existing_alias) = alias_map.get(simulator) {
                            if all_paths.contains(existing_alias) {
                                continue; // Existing alias points to a valid signal
                            }
                            // Existing alias points to non-existent signal
                        }
                    }
                    names.insert(edge.signal.clone());
                }
            }
            names
        };

        if canonical_names.is_empty() {
            return; // All canonical names have valid aliases
        }

        // Match canonical names to waveform paths by leaf name
        let mut new_aliases: Vec<(String, String)> = Vec::new();
        let mut unresolved: HashSet<String> = HashSet::new();
        for canonical in &canonical_names {
            let leaf = canonical.rsplit('.').next().unwrap_or(canonical);
            if let Some(paths) = leaf_to_paths.get(leaf) {
                // Prefer shortest path (closest to DUT), or first match if ambiguous
                let best_path = paths.iter().min_by_key(|p| (p.len(), p.as_str())).unwrap();
                new_aliases.push((canonical.clone(), best_path.clone()));
            } else {
                unresolved.insert(canonical.clone());
            }
        }

        // Second pass: suffix-path matching for unresolved canonical names.
        // For canonical names whose leaf didn't match any waveform leaf, try
        // matching the canonical name's path segments against waveform paths'
        // suffix segments. This handles partial hierarchical paths and
        // generate-block naming mismatches.
        if !unresolved.is_empty() {
            // Build full-path → segments map for suffix matching
            let mut path_suffix_map: Vec<(String, Vec<&str>)> = Vec::new();
            for full_path in &all_paths {
                let segments: Vec<&str> = full_path.split('.').collect();
                path_suffix_map.push((full_path.clone(), segments));
            }

            for canonical in &unresolved {
                let canon_segments: Vec<&str> = canonical.split('.').collect();
                // Try matching with decreasing suffix length (longest suffix first)
                // Skip single-segment match (that's leaf-name matching, already failed)
                let min_suffix_len = 2.min(canon_segments.len());
                for suffix_len in min_suffix_len..=canon_segments.len() {
                    let suffix = &canon_segments[canon_segments.len() - suffix_len..];
                    let mut matches: Vec<&String> = Vec::new();
                    for (full_path, segments) in &path_suffix_map {
                        if segments.len() >= suffix_len {
                            let wave_suffix = &segments[segments.len() - suffix_len..];
                            if wave_suffix == suffix {
                                matches.push(full_path);
                            }
                        }
                    }
                    if !matches.is_empty() {
                        // Prefer shortest path (closest to DUT)
                        let best_path = matches
                            .into_iter()
                            .min_by_key(|p| (p.len(), p.as_str()))
                            .unwrap();
                        new_aliases.push((canonical.clone(), best_path.clone()));
                        break; // Found match, skip shorter suffix attempts
                    }
                }
            }
        }

        // Apply inferred aliases
        for (canonical, resolved_path) in new_aliases {
            self.signal_aliases
                .entry(canonical)
                .or_default()
                .insert(simulator.to_string(), resolved_path);
        }

        // Update meta signal_alias_count
        self.meta.signal_alias_count = self.signal_aliases.len();
    }

    /// Resolve a logical clock name to a simulator-specific waveform path.
    ///
    /// # Arguments
    /// * `clock_name` - Logical clock name from deps.yaml
    /// * `simulator` - Simulator identifier (e.g., "modelsim", "vivado")
    ///
    /// # Returns
    /// The resolved clock waveform path, or an error if no alias exists.
    pub fn resolve_clock(&self, clock_name: &str, simulator: &str) -> WaveResult<String> {
        self.clock_aliases
            .get(clock_name)
            .and_then(|sim_map| sim_map.get(simulator).cloned())
            .ok_or_else(|| WaveAnalyzerError::DepsError {
                message: format!(
                    "CLOCK_NOT_FOUND: No alias for clock '{}' in simulator '{}'. Add a clock_aliases entry.",
                    clock_name, simulator
                ),
            })
    }

    /// Get metadata about the loaded dependency graph.
    pub fn meta(&self) -> &DepsFileMeta {
        &self.meta
    }

    /// Check if a signal has any dependencies in the graph.
    pub fn has_signal(&self, signal: &str) -> bool {
        self.fan_in.contains_key(signal) || self.fan_out.contains_key(signal)
    }

    /// Get all output signal paths (signals with fan-in dependencies defined).
    pub fn output_signals(&self) -> Vec<String> {
        self.fan_in.keys().cloned().collect()
    }

    /// Check if a signal is an output node (has fan-in edges defined).
    pub fn is_output_node(&self, signal: &str) -> bool {
        self.fan_in.contains_key(signal)
    }

    /// Get the category for an output signal.
    pub fn get_category(&self, signal: &str) -> Option<&str> {
        self.categories.get(signal).map(|s| s.as_str())
    }

    /// Get all signal paths that appear as upstream sources in the deps graph.
    /// These are keys of the fan-out index — signals that feed into other signals.
    pub fn fan_out_keys(&self) -> Vec<String> {
        self.fan_out.keys().cloned().collect()
    }

    /// Find all CDC boundary edges in the dependency graph.
    /// Returns (output_signal, DepEdge) pairs where boundary_kind == Cdc.
    pub fn find_cdc_edges(&self) -> Vec<(String, &DepEdge)> {
        self.fan_in
            .iter()
            .flat_map(|(output, edges)| {
                edges
                    .iter()
                    .filter(|e| e.boundary_kind == Some(BoundaryKind::Cdc))
                    .map(|e| (output.clone(), e))
            })
            .collect()
    }

    /// Find all boundary edges by a specific BoundaryKind.
    pub fn find_boundary_edges_by_kind(&self, kind: &BoundaryKind) -> Vec<(String, &DepEdge)> {
        self.fan_in
            .iter()
            .flat_map(|(output, edges)| {
                edges
                    .iter()
                    .filter(|e| e.boundary_kind.as_ref() == Some(kind))
                    .map(|e| (output.clone(), e))
            })
            .collect()
    }

    /// Get all distinct clock names referenced in the dependency graph.
    /// Includes clocks from sequential/control/memory/protocol edges and
    /// cdc_from_clock/cdc_to_clock from CDC edges.
    pub fn get_all_clock_names(&self) -> Vec<String> {
        let mut clocks: HashSet<String> = HashSet::new();
        // Collect clocks from dep_edges
        for edges in self.fan_in.values() {
            for e in edges {
                if let Some(ref clk) = e.clock {
                    clocks.insert(clk.clone());
                }
                if let Some(ref clk) = e.cdc_from_clock {
                    clocks.insert(clk.clone());
                }
                if let Some(ref clk) = e.cdc_to_clock {
                    clocks.insert(clk.clone());
                }
            }
        }
        // Also collect clocks from clock_aliases (BUG-R8-6 fix)
        for clock_name in self.clock_aliases.keys() {
            clocks.insert(clock_name.clone());
        }
        clocks.into_iter().collect()
    }

    /// Get all output signals driven by a specific clock domain.
    pub fn get_signals_by_clock_domain(&self, clock_name: &str) -> Vec<String> {
        self.fan_in
            .iter()
            .filter(|(_, edges)| edges.iter().any(|e| e.clock.as_deref() == Some(clock_name)))
            .map(|(output, _)| output.clone())
            .collect()
    }

    /// Check if a signal is a CDC boundary signal.
    pub fn is_cdc_boundary(&self, signal: &str) -> bool {
        self.fan_in.get(signal).is_some_and(|edges| {
            edges
                .iter()
                .any(|e| e.boundary_kind == Some(BoundaryKind::Cdc))
        })
    }

    /// Get the CDC clock domains for a signal: returns (from_clock, to_clock).
    pub fn get_cdc_clock_domains(&self, signal: &str) -> Option<(String, String)> {
        self.fan_in.get(signal).and_then(|edges| {
            edges
                .iter()
                .find(|e| e.boundary_kind == Some(BoundaryKind::Cdc))
                .and_then(|e| {
                    e.cdc_from_clock
                        .as_ref()
                        .zip(e.cdc_to_clock.as_ref())
                        .map(|(f, t)| (f.clone(), t.clone()))
                })
        })
    }
}

/// Detect cycles in the dependency graph using DFS with coloring.
/// Returns true if any cycle (including self-loops and multi-node cycles) exists.
fn detect_cycles(fan_in: &HashMap<String, Vec<DepEdge>>) -> bool {
    use std::collections::HashMap;

    // 0 = white (unvisited), 1 = gray (in progress), 2 = black (done)
    let mut color: HashMap<&str, u8> = HashMap::new();

    for node in fan_in.keys() {
        color.insert(node.as_str(), 0);
    }

    fn dfs<'a>(
        node: &'a str,
        fan_in: &'a HashMap<String, Vec<DepEdge>>,
        color: &mut HashMap<&'a str, u8>,
    ) -> bool {
        color.insert(node, 1); // gray
        if let Some(edges) = fan_in.get(node) {
            for edge in edges {
                let neighbor = edge.signal.as_str();
                let neighbor_color = color.get(neighbor).copied().unwrap_or(0);
                if neighbor_color == 1 {
                    // Found a back edge 鈫?cycle
                    return true;
                }
                if neighbor_color == 0 && dfs(neighbor, fan_in, color) {
                    return true;
                }
            }
        }
        color.insert(node, 2); // black
        false
    }

    for node in fan_in.keys() {
        if color.get(node.as_str()).copied().unwrap_or(0) == 0
            && dfs(node.as_str(), fan_in, &mut color)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_legacy_deps_yaml_formats() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let deps_path = temp_dir.path().join("legacy_deps.yaml");
        std::fs::write(
            &deps_path,
            r#"
format_version: "1.0"
signal_aliases:
  - tb_top.sig_a
  - tb_top.sig_b
clock_aliases:
  - tb_top.clk
dependencies:
  - signal: tb_top.sig_b
    depends_on:
      - tb_top.sig_a
"#,
        )
        .expect("write deps file");

        let dep_graph = load_deps_from_file(&deps_path).expect("legacy deps should load");

        assert!(dep_graph.has_signal("tb_top.sig_b"));
        assert_eq!(dep_graph.output_signals(), vec!["tb_top.sig_b".to_string()]);
        assert_eq!(
            dep_graph
                .fan_in("tb_top.sig_b")
                .expect("fan-in should exist")[0]
                .dep_type,
            DepType::Combinational
        );
        assert_eq!(
            dep_graph.resolve_signal("tb_top.sig_a", "modelsim"),
            Some("tb_top.sig_a".to_string())
        );
        assert_eq!(
            dep_graph
                .resolve_clock("tb_top.clk", "modelsim")
                .expect("clock alias"),
            "tb_top.clk".to_string()
        );
    }

    #[test]
    fn load_protect_style_deps_yaml_with_signal_and_output_bool() {
        // This format uses `signal:` for the output name and `output: true/false`
        // as a boolean flag — exactly like protect_ctrl_top's deps.yaml.
        // serde_yaml converts `output: true` (YAML bool) to String "true",
        // so preprocessing must rename `signal:` → `output:` and remove the
        // boolean `output:` before serde parsing.
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let deps_path = temp_dir.path().join("protect_deps.yaml");
        std::fs::write(
            &deps_path,
            r#"
format_version: "1.0"
dependencies:
  - signal: o_power_off
    output: true
    depends_on:
      - signal: v_protect
        type: combinational
      - signal: c_protect
        type: combinational
  - signal: v_protect
    output: false
    depends_on:
      - signal: v_ch0_state
        type: combinational
"#,
        )
        .expect("write deps file");

        let dep_graph = load_deps_from_file(&deps_path).expect("protect-style deps should load");

        assert!(dep_graph.has_signal("o_power_off"));
        assert!(dep_graph.has_signal("v_protect"));
        assert!(dep_graph.is_output_node("o_power_off"));

        let edges = dep_graph
            .fan_in("o_power_off")
            .expect("fan-in for o_power_off");
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].signal, "v_protect");
        assert_eq!(edges[1].signal, "c_protect");

        let v_edges = dep_graph.fan_in("v_protect").expect("fan-in for v_protect");
        assert_eq!(v_edges.len(), 1);
        assert_eq!(v_edges[0].signal, "v_ch0_state");

        assert_eq!(dep_graph.meta().node_count, 2);
        assert_eq!(dep_graph.meta().edge_count, 3);
    }
}
