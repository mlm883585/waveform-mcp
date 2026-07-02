//! Hierarchy navigation and signal finding utilities.

use regex::Regex;
use std::borrow::Cow;
use std::collections::HashSet;
use wellen;

use crate::error::{WaveAnalyzerError, WaveResult};

/// Classify a wellen VarType into a human-readable signal type string.
///
/// Covers all 37 wellen VarType variants with semantically correct mapping:
/// - "parameter": Parameter, RealParameter, Port (compile-time constants)
/// - "supply": Supply0, Supply1 (constant supply nets)
/// - "other": String, Event, Real, ShortReal, RealTime, Time, Boolean
///   (non-numeric or time-related signals)
/// - "reg": Reg, TriReg (sequential logic outputs)
/// - "wire": Wire, Tri, Tri0, Tri1, TriAnd, TriOr, WAnd, WOr
///   (combinational logic outputs and net types)
/// - "integer": Integer, Int, ShortInt, LongInt, Byte
///   (signed/unsigned integer types)
/// - "logic": Logic, Bit, Enum (SystemVerilog logic/bit types — typically sequential)
/// - "bus": BitVector, StdLogicVector, StdULogicVector, StdLogic, StdULogic
///   (VHDL-style multi-bit types)
/// - "other": SparseArray (unusual type, rarely seen)
///
/// Previously, Integer/Logic/Bit/Int/ShortInt/LongInt/Byte all fell into
/// the default "wire" classification, causing integer signals to be misclassified
/// as "wire" and wire signals to be misclassified as "reg" when VCD uses
/// VarType::Logic instead of VarType::Wire.
pub fn classify_var_type(var_type: wellen::VarType) -> &'static str {
    match var_type {
        // Compile-time constants — not observable in waveform
        wellen::VarType::Parameter | wellen::VarType::RealParameter | wellen::VarType::Port => {
            "parameter"
        }

        // Constant supply nets — not meaningful design signals
        wellen::VarType::Supply0 | wellen::VarType::Supply1 => "supply",

        // Non-numeric or time-related — not bit-comparable
        wellen::VarType::String
        | wellen::VarType::Event
        | wellen::VarType::Real
        | wellen::VarType::ShortReal
        | wellen::VarType::RealTime
        | wellen::VarType::Time
        | wellen::VarType::Boolean => "other",

        // Sequential logic outputs
        wellen::VarType::Reg | wellen::VarType::TriReg => "reg",

        // Combinational logic outputs and net types
        wellen::VarType::Wire
        | wellen::VarType::Tri
        | wellen::VarType::Tri0
        | wellen::VarType::Tri1
        | wellen::VarType::TriAnd
        | wellen::VarType::TriOr
        | wellen::VarType::WAnd
        | wellen::VarType::WOr => "wire",

        // Signed/unsigned integer types
        wellen::VarType::Integer
        | wellen::VarType::Int
        | wellen::VarType::ShortInt
        | wellen::VarType::LongInt
        | wellen::VarType::Byte => "integer",

        // SystemVerilog logic/bit/enum — typically behave like reg
        wellen::VarType::Logic | wellen::VarType::Bit | wellen::VarType::Enum => "logic",

        // VHDL-style bus types
        wellen::VarType::BitVector
        | wellen::VarType::StdLogic
        | wellen::VarType::StdLogicVector
        | wellen::VarType::StdULogic
        | wellen::VarType::StdULogicVector => "bus",

        // Unusual type — rarely seen in practice
        wellen::VarType::SparseArray => "other",
    }
}

/// Returns true if a VarType should be excluded from observable signal lists.
///
/// Non-observable types (parameters, supply nets, ports) are not meaningful
/// for waveform analysis and should be filtered out during signal discovery
/// and hierarchy listing.
pub fn is_non_observable_var_type(var_type: wellen::VarType) -> bool {
    matches!(
        var_type,
        wellen::VarType::Parameter
            | wellen::VarType::RealParameter
            | wellen::VarType::String
            | wellen::VarType::Event
            | wellen::VarType::Port
            | wellen::VarType::Supply0
            | wellen::VarType::Supply1
    )
}

struct HierarchyRenderer {
    lines: Vec<String>,
    limit: Option<usize>,
    truncated: bool,
}

impl HierarchyRenderer {
    fn new(limit: Option<isize>) -> Self {
        Self {
            lines: Vec::new(),
            limit: limit.and_then(|value| (value >= 0).then_some(value as usize)),
            truncated: false,
        }
    }

    fn is_full(&self) -> bool {
        self.limit.is_some_and(|limit| self.lines.len() >= limit)
    }

    fn push_line(&mut self, line: String) -> bool {
        if self.is_full() {
            self.truncated = true;
            return false;
        }

        self.lines.push(line);
        true
    }

    fn finish(mut self) -> Vec<String> {
        if self.truncated
            && let Some(limit) = self.limit
        {
            self.lines
                .push(format!("... truncated after {} items", limit));
        }

        self.lines
    }
}

/// Strip bracket notation from a variable name for lookup.
/// e.g. "counter[7:0]" → "counter", "status[1]" → "status"
fn split_index_suffix(name: &str) -> (&str, Option<wellen::VarIndex>) {
    let Some(open) = name.find('[') else {
        return (name, None);
    };
    let Some(close) = name[open..].find(']') else {
        return (name, None);
    };
    let inner = &name[open + 1..open + close];
    // Support both range notation "[7:0]" and single-bit notation "[1]"
    let Some((msb, lsb)) = inner.split_once(':') else {
        // Single-bit index: "signal[0]" → "signal", VarIndex(0,0)
        let Ok(bit) = inner.trim().parse::<i64>() else {
            return (name, None);
        };
        return (&name[..open], Some(wellen::VarIndex::new(bit, bit)));
    };
    let Ok(msb_val) = msb.trim().parse::<i64>() else {
        return (name, None);
    };
    let Ok(lsb_val) = lsb.trim().parse::<i64>() else {
        return (name, None);
    };
    (&name[..open], Some(wellen::VarIndex::new(msb_val, lsb_val)))
}

fn normalized_var_name<'a>(var: &'a wellen::Var, hierarchy: &'a wellen::Hierarchy) -> Cow<'a, str> {
    match var.index() {
        Some(idx) if var.length().unwrap_or(1) > 1 => Cow::Owned(format!(
            "{}[{}:{}]",
            var.name(hierarchy),
            idx.msb(),
            idx.lsb()
        )),
        _ => Cow::Borrowed(var.name(hierarchy)),
    }
}

pub fn find_var_ref_by_path(hierarchy: &wellen::Hierarchy, path: &str) -> Option<wellen::VarRef> {
    resolve_signal_var_refs(hierarchy, path).and_then(|vars| vars.first().copied())
}

/// Resolve a signal path to one or more variable refs.
///
/// Returns a single bus var if the waveform stores the signal as a true vector,
/// or all bit-slice vars when the waveform stores each bit separately.
pub fn resolve_signal_var_refs(
    hierarchy: &wellen::Hierarchy,
    path: &str,
) -> Option<Vec<wellen::VarRef>> {
    let parts: Vec<&str> = path.split('.').collect();
    let (path_parts, name) = if parts.len() > 1 {
        (&parts[..parts.len() - 1], parts[parts.len() - 1])
    } else {
        (&[][..], path)
    };
    let (base_name, requested_index) = split_index_suffix(name);

    let scope = if path_parts.is_empty() {
        None
    } else {
        Some(hierarchy.lookup_scope(path_parts)?)
    };

    let vars: Box<dyn Iterator<Item = wellen::VarRef> + '_> = match scope {
        Some(scope_ref) => Box::new(hierarchy[scope_ref].vars(hierarchy)),
        None => Box::new(hierarchy.vars()),
    };

    let mut matches = Vec::new();
    for var_ref in vars {
        let var = &hierarchy[var_ref];
        if var.name(hierarchy) != base_name {
            continue;
        }

        if let Some(index) = requested_index {
            // If requesting a single-bit index (msb == lsb), match the full bus
            // and extract the bit later, since wellen stores bus variables as a whole
            // (e.g. o_state[2:0] not separate o_state[0], o_state[1], o_state[2])
            if index.msb() == index.lsb() {
                // Single-bit request: find the full bus that contains this bit
                if let Some(var_index) = var.index() {
                    if var_index.lsb() <= index.lsb() && var_index.msb() >= index.lsb() {
                        return Some(vec![var_ref]);
                    }
                } else if var.length().unwrap_or(1) == 1 {
                    // Single-bit variable with no index — exact match
                    return Some(vec![var_ref]);
                }
                continue;
            }
            // Multi-bit range request: exact match required
            if var.index() == Some(index) {
                return Some(vec![var_ref]);
            }
            continue;
        }

        matches.push(var_ref);
    }

    if matches.is_empty() {
        return None;
    }

    if let Some(bus_ref) = matches
        .iter()
        .copied()
        .find(|var_ref| hierarchy[*var_ref].length().unwrap_or(1) > 1)
    {
        return Some(vec![bus_ref]);
    }

    if matches.len() > 1 {
        matches.sort_by_key(|var_ref| {
            hierarchy[*var_ref]
                .index()
                .map(|idx| idx.msb())
                .unwrap_or(i64::MIN)
        });
        matches.reverse();
    }

    Some(matches)
}

/// Find a variable (VarRef) by its hierarchical path in waveform hierarchy.
///
/// Resolution order:
/// 1. Exact full-path lookup via `resolve_signal_var_refs`.
/// 2. Leaf-name fallback: if path has no dots (bare leaf name), search all vars.
/// 3. Suffix-path fallback: search all vars for any whose full hierarchy path
///    ends with the given dot-separated path segments. This handles partial
///    hierarchical paths like `u_dut.o_power_off` matching
///    `protect_ctrl_top_tb.u_dut.o_power_off`.
pub fn find_var_by_path(hierarchy: &wellen::Hierarchy, path: &str) -> Option<wellen::VarRef> {
    // 1. Exact full-path lookup first
    if let Some(vr) = find_var_ref_by_path(hierarchy, path) {
        return Some(vr);
    }
    // 2. Leaf-name fallback (for bare leaf names, no dots)
    if !path.contains('.') {
        if let Some(vr) = find_var_by_leaf_name(hierarchy, path) {
            return Some(vr);
        }
    }
    // 3. Suffix-path fallback (for partial hierarchical paths)
    find_var_by_suffix_path(hierarchy, path)
}

/// Find a variable (VarRef) by leaf name when full path is unavailable.
///
/// Searches all scopes recursively for a variable whose name matches the
/// leaf name exactly. If multiple matches exist, returns the first one found.
fn find_var_by_leaf_name(hierarchy: &wellen::Hierarchy, leaf_name: &str) -> Option<wellen::VarRef> {
    for var in hierarchy.iter_vars() {
        if var.name(hierarchy) == leaf_name {
            // We need VarRef, not SignalRef. iter_vars returns Var objects.
            // Get the VarRef by looking up the variable in its scope.
            let full_name = var.full_name(hierarchy);
            let parts: Vec<&str> = full_name.split('.').collect();
            if parts.len() > 1 {
                if let Some(scope_ref) = hierarchy.lookup_scope(&parts[..parts.len() - 1]) {
                    for var_ref in hierarchy[scope_ref].vars(hierarchy) {
                        if hierarchy[var_ref].name(hierarchy) == leaf_name {
                            return Some(var_ref);
                        }
                    }
                }
            } else {
                // Root-level variable
                for var_ref in hierarchy.vars() {
                    if hierarchy[var_ref].name(hierarchy) == leaf_name {
                        return Some(var_ref);
                    }
                }
            }
        }
    }
    None
}

/// Find a variable (VarRef) by suffix-path matching.
///
/// Searches all variables for one whose full hierarchy path ends with the
/// given dot-separated path segments. Bracket notation in the leaf name is
/// stripped before comparison.
///
/// Example: searching `u_dut.o_power_off` would match a variable at
/// `protect_ctrl_top_tb.u_dut.o_power_off` because the last 2 segments
/// match.
///
/// Returns the first (shortest-path) match to prefer signals closer to the
/// DUT rather than deep inside testbench infrastructure.
fn find_var_by_suffix_path(hierarchy: &wellen::Hierarchy, path: &str) -> Option<wellen::VarRef> {
    let parts: Vec<&str> = path.split('.').collect();
    let (leaf_base, _requested_index) = split_index_suffix(parts[parts.len() - 1]);

    // Build expected suffix segments: scope parts + bracket-stripped leaf name
    let expected_suffix: Vec<&str> = if parts.len() > 1 {
        let mut v: Vec<&str> = parts[..parts.len() - 1].to_vec();
        v.push(leaf_base);
        v
    } else {
        vec![leaf_base]
    };

    let suffix_len = expected_suffix.len();

    let mut best_match: Option<(wellen::VarRef, String)> = None;

    for var in hierarchy.iter_vars() {
        let full_name = var.full_name(hierarchy);
        let full_parts: Vec<&str> = full_name.split('.').collect();

        if full_parts.len() < suffix_len {
            continue;
        }

        let start = full_parts.len() - suffix_len;
        if full_parts[start..] == expected_suffix[..] {
            // Found a suffix match. Reconstruct VarRef via scope lookup.
            let scope_segments = &full_parts[..full_parts.len() - 1];
            let var_ref = if scope_segments.is_empty() {
                hierarchy
                    .vars()
                    .find(|vr| hierarchy[*vr].name(hierarchy) == leaf_base)
            } else if let Some(scope_ref) = hierarchy.lookup_scope(scope_segments) {
                hierarchy[scope_ref]
                    .vars(hierarchy)
                    .find(|vr| hierarchy[*vr].name(hierarchy) == leaf_base)
            } else {
                continue;
            };

            if let Some(vr) = var_ref {
                // Prefer shortest full_name (closest to DUT)
                let is_shorter = best_match
                    .as_ref()
                    .is_none_or(|(_, existing)| full_name.len() < existing.len());
                if is_shorter {
                    best_match = Some((vr, full_name.to_string()));
                }
            }
        }
    }

    best_match.map(|(vr, _)| vr)
}

/// Find a signal by its hierarchical path in the waveform hierarchy.
///
/// Strips bracket notation before lookup, then maps VarRef → SignalRef.
/// Resolution order mirrors `find_var_by_path`:
/// 1. Exact full-path lookup.
/// 2. Leaf-name fallback (bare leaf names, no dots).
/// 3. Suffix-path fallback (partial hierarchical paths).
pub fn find_signal_by_path(hierarchy: &wellen::Hierarchy, path: &str) -> Option<wellen::SignalRef> {
    // 1. Exact full-path lookup first
    if let Some(vr) = find_var_ref_by_path(hierarchy, path) {
        return Some(hierarchy[vr].signal_ref());
    }
    // 2. Leaf-name fallback
    if !path.contains('.') {
        if let Some(vr) = find_var_by_leaf_name(hierarchy, path) {
            return Some(hierarchy[vr].signal_ref());
        }
    }
    // 3. Suffix-path fallback
    find_var_by_suffix_path(hierarchy, path).map(|vr| hierarchy[vr].signal_ref())
}

/// Resolve a signal by hierarchical path and return (SignalRef, declared_width).
///
/// Combines `find_signal_by_path` and `get_signal_width` into a single call,
/// producing a structured error message when the signal is not found.
/// Resolution order mirrors `find_signal_by_path`:
/// 1. Exact full-path lookup.
/// 2. Leaf-name fallback (bare leaf names, no dots).
/// 3. Suffix-path fallback (partial hierarchical paths).
pub fn resolve_signal_with_width(
    hierarchy: &wellen::Hierarchy,
    path: &str,
) -> WaveResult<(wellen::SignalRef, u32)> {
    let signal_ref =
        find_signal_by_path(hierarchy, path).ok_or_else(|| WaveAnalyzerError::SignalNotFound {
            path: path.to_string(),
        })?;
    let width = get_signal_width(hierarchy, path);
    Ok((signal_ref, width))
}

/// Get signal width by its hierarchical path.
///
/// Strips bracket notation before lookup so multi-bit bus signals
/// (e.g. `counter[7:0]`) are correctly resolved to their full width.
/// Falls back to leaf-name search when full-path lookup fails for bare
/// leaf names (no dots), consistent with `find_signal_by_path`.
pub fn get_signal_width(hierarchy: &wellen::Hierarchy, path: &str) -> u32 {
    let var_refs = resolve_signal_var_refs(hierarchy, path)
        .or_else(|| {
            // Fallback chain: leaf-name → suffix-path
            if !path.contains('.') {
                find_var_by_leaf_name(hierarchy, path).map(|vr| vec![vr])
            } else {
                None
            }
        })
        .or_else(|| {
            // Suffix-path fallback for partial hierarchical paths
            find_var_by_suffix_path(hierarchy, path).map(|vr| vec![vr])
        });
    var_refs
        .map(|var_refs| {
            if var_refs.len() == 1 {
                hierarchy[var_refs[0]].length().unwrap_or(1)
            } else {
                let mut msb = None;
                let mut lsb = None;
                for var_ref in var_refs {
                    if let Some(index) = hierarchy[var_ref].index() {
                        msb =
                            Some(msb.map_or(index.msb(), |current: i64| current.max(index.msb())));
                        lsb =
                            Some(lsb.map_or(index.lsb(), |current: i64| current.min(index.lsb())));
                    }
                }
                match (msb, lsb) {
                    (Some(msb), Some(lsb)) if msb >= lsb => (msb - lsb + 1) as u32,
                    _ => 1,
                }
            }
        })
        .unwrap_or(1)
}

/// Find a scope by its hierarchical path in waveform hierarchy.
///
/// # Arguments
/// * `hierarchy` - The waveform hierarchy to search
/// * `path` - The hierarchical path to scope (e.g., "top.module")
///
/// # Returns
/// `Some(ScopeRef)` if scope is found, `None` otherwise.
pub fn find_scope_by_path(hierarchy: &wellen::Hierarchy, path: &str) -> Option<wellen::ScopeRef> {
    for scope_ref in hierarchy.scopes() {
        let scope = &hierarchy[scope_ref];
        let scope_path = scope.full_name(hierarchy);
        if scope_path == path {
            return Some(scope_ref);
        }
        // Recursively check child scopes
        if let Some(child_ref) = find_scope_by_path_recursive(hierarchy, scope_ref, path) {
            return Some(child_ref);
        }
    }
    None
}

fn find_scope_by_path_recursive(
    hierarchy: &wellen::Hierarchy,
    parent_ref: wellen::ScopeRef,
    target_path: &str,
) -> Option<wellen::ScopeRef> {
    let parent = &hierarchy[parent_ref];
    for child_ref in parent.scopes(hierarchy) {
        let child = &hierarchy[child_ref];
        let child_path = child.full_name(hierarchy);
        if child_path == target_path {
            return Some(child_ref);
        }
        // Recursively check child scopes
        if let Some(found) = find_scope_by_path_recursive(hierarchy, child_ref, target_path) {
            return Some(found);
        }
    }
    None
}

/// Read the waveform module hierarchy as an indented tree.
///
/// # Arguments
/// * `hierarchy` - The waveform hierarchy to read
/// * `scope_path` - Optional root scope path to start from
/// * `recursive` - If true, include all descendant modules; if false, include only one level below the selected scope
/// * `limit` - Optional maximum number of modules to return. Use -1 for unlimited.
///
/// # Returns
/// A vector of formatted module hierarchy lines, or an error if the scope path is invalid.
pub fn read_hierarchy(
    hierarchy: &wellen::Hierarchy,
    scope_path: Option<&str>,
    recursive: bool,
    limit: Option<isize>,
) -> WaveResult<Vec<String>> {
    let mut renderer = HierarchyRenderer::new(limit);
    let mut seen_keys = HashSet::new();

    match scope_path {
        Some(path) => {
            let scope_ref = find_scope_by_path(hierarchy, path).ok_or_else(|| {
                WaveAnalyzerError::InvalidArgument {
                    message: format!("Scope not found: {}", path),
                }
            })?;
            let child_depth = if recursive { usize::MAX } else { 1 };
            render_scope(
                hierarchy,
                scope_ref,
                0,
                child_depth,
                true,
                &mut renderer,
                &mut seen_keys,
                &HashSet::new(),
            );
        }
        None => {
            let child_depth = if recursive { usize::MAX } else { 0 };
            for item in hierarchy.items() {
                if renderer.is_full() {
                    renderer.truncated = true;
                    break;
                }

                render_item(
                    hierarchy,
                    item,
                    0,
                    child_depth,
                    true,
                    &mut renderer,
                    &mut seen_keys,
                    &HashSet::new(),
                );
            }
        }
    }

    Ok(renderer.finish())
}

/// Collect signals from a scope and optionally its children recursively.
///
/// If `name_pattern` is provided, it is interpreted as a regular expression.
/// Use `.*` to match any sequence, `^` / `$` for anchors, etc.
/// Returns an error if the regex pattern is invalid.
///
/// BUG-3/24/5/8 fix: Filters out non-observable variable types (Parameter,
/// RealParameter, String, Event, Port, Supply0, Supply1) that are not
/// meaningful design signals for waveform analysis.
pub fn collect_signals_from_scope(
    hierarchy: &wellen::Hierarchy,
    scope_ref: wellen::ScopeRef,
    recursive: bool,
    name_pattern: Option<&str>,
) -> WaveResult<Vec<String>> {
    // Compile regex pattern if provided
    let pattern_re = if let Some(pat) = name_pattern {
        Some(
            Regex::new(pat).map_err(|e| WaveAnalyzerError::InvalidArgument {
                message: format!("Invalid pattern regex: {}", e),
            })?,
        )
    } else {
        None
    };

    let mut seen = std::collections::HashSet::new();
    let mut signals = Vec::new();
    let scope = &hierarchy[scope_ref];

    // Collect variables directly in this scope
    for var_ref in scope.vars(hierarchy) {
        let var = &hierarchy[var_ref];

        // BUG-3/24/5/8 fix: filter out non-observable variable types
        if is_non_observable_var_type(var.var_type()) {
            continue;
        }

        let path = var.full_name(hierarchy);

        // Apply name pattern filter if provided (regex matching)
        if let Some(ref re) = pattern_re
            && !re.is_match(&path)
        {
            continue;
        }

        // Skip bit-slice variables whose parent bus is already in the list
        // A bit-slice has length=1 and its name matches parent_name[N] pattern
        let var_length = var.length().unwrap_or(1);
        if var_length == 1 {
            let var_name = var.name(hierarchy);
            if let Some(pos) = var_name.find('[') {
                let base_name = &var_name[..pos];
                // Check if a parent bus variable with this base name exists in scope
                let has_parent_bus = scope.vars(hierarchy).any(|vr| {
                    let v = &hierarchy[vr];
                    v.name(hierarchy) == base_name && v.length().unwrap_or(1) > 1
                });
                if has_parent_bus {
                    continue; // Skip bit-slice; the bus parent will represent it
                }
            }
        }

        if seen.insert(path.clone()) {
            signals.push(path);
        }
    }

    // If recursive, also collect from child scopes
    if recursive {
        for child_ref in scope.scopes(hierarchy) {
            let child_signals =
                collect_signals_from_scope(hierarchy, child_ref, true, name_pattern)?;
            for path in child_signals {
                if seen.insert(path.clone()) {
                    signals.push(path);
                }
            }
        }
    }

    Ok(signals)
}

fn render_item(
    hierarchy: &wellen::Hierarchy,
    item: wellen::ScopeOrVarRef,
    depth: usize,
    child_depth: usize,
    show_full_name: bool,
    renderer: &mut HierarchyRenderer,
    seen_keys: &mut HashSet<String>,
    wide_base_names: &HashSet<String>,
) {
    match item {
        wellen::ScopeOrVarRef::Scope(scope_ref) => {
            render_scope(
                hierarchy,
                scope_ref,
                depth,
                child_depth,
                show_full_name,
                renderer,
                seen_keys,
                wide_base_names,
            );
        }
        wellen::ScopeOrVarRef::Var(var_ref) => {
            if child_depth > 0 {
                render_var(
                    hierarchy,
                    var_ref,
                    depth,
                    show_full_name,
                    renderer,
                    seen_keys,
                    wide_base_names,
                );
            }
        }
    }
}

fn scope_key_for_var(hierarchy: &wellen::Hierarchy, var_ref: wellen::VarRef) -> String {
    let full_name = hierarchy[var_ref].full_name(hierarchy);
    full_name
        .rsplit_once('.')
        .map(|(scope, _)| scope.to_string())
        .unwrap_or_else(|| "<root>".to_string())
}

fn scope_bus_base_names(
    hierarchy: &wellen::Hierarchy,
    scope_ref: wellen::ScopeRef,
) -> HashSet<String> {
    let scope = &hierarchy[scope_ref];
    let mut bases = HashSet::new();

    for var_ref in scope.vars(hierarchy) {
        let var = &hierarchy[var_ref];
        if var.length().unwrap_or(1) <= 1 {
            continue;
        }

        let name = var.name(hierarchy);
        let base = split_index_suffix(name).0.to_string();
        bases.insert(base);
    }

    bases
}

fn should_skip_bit_slice_var(
    hierarchy: &wellen::Hierarchy,
    var_ref: wellen::VarRef,
    wide_base_names: &HashSet<String>,
) -> bool {
    let var = &hierarchy[var_ref];
    if var.length().unwrap_or(1) > 1 {
        return false;
    }

    let name = var.name(hierarchy);
    let base_name = split_index_suffix(name).0;
    wide_base_names.contains(base_name)
}

fn render_scope(
    hierarchy: &wellen::Hierarchy,
    scope_ref: wellen::ScopeRef,
    depth: usize,
    child_depth: usize,
    show_full_name: bool,
    renderer: &mut HierarchyRenderer,
    seen_keys: &mut HashSet<String>,
    _parent_wide_base_names: &HashSet<String>,
) {
    let scope = &hierarchy[scope_ref];
    let is_module = matches!(scope.scope_type(), wellen::ScopeType::Module);
    let scope_key = scope.full_name(hierarchy);
    let wide_base_names = scope_bus_base_names(hierarchy, scope_ref);

    let mut next_depth = depth;
    let mut next_child_depth = child_depth;
    let mut next_show_full_name = show_full_name;

    if is_module {
        let scope_name = if show_full_name {
            scope_key.clone()
        } else {
            scope.name(hierarchy).to_string()
        };

        if !seen_keys.insert(format!("scope::{}", scope_key)) {
            return;
        }
        if !renderer.push_line(format!("{}{}", "  ".repeat(depth), scope_name)) {
            return;
        }

        if child_depth == 0 {
            return;
        }

        next_depth += 1;
        next_child_depth = child_depth.saturating_sub(1);
        next_show_full_name = false;
    }
    for item in scope.items(hierarchy) {
        if renderer.is_full() {
            renderer.truncated = true;
            break;
        }

        render_item(
            hierarchy,
            item,
            next_depth,
            next_child_depth,
            next_show_full_name,
            renderer,
            seen_keys,
            &wide_base_names,
        );
    }
}

fn render_var(
    hierarchy: &wellen::Hierarchy,
    var_ref: wellen::VarRef,
    depth: usize,
    show_full_name: bool,
    renderer: &mut HierarchyRenderer,
    seen_keys: &mut HashSet<String>,
    wide_base_names: &HashSet<String>,
) {
    let var = &hierarchy[var_ref];
    let scope_key = scope_key_for_var(hierarchy, var_ref);

    if is_non_observable_var_type(var.var_type()) {
        return;
    }

    if should_skip_bit_slice_var(hierarchy, var_ref, &wide_base_names) {
        return;
    }

    let var_name = if show_full_name {
        var.full_name(hierarchy).to_string()
    } else {
        normalized_var_name(var, hierarchy).into_owned()
    };

    if !seen_keys.insert(format!("var::{}::{}", scope_key, var_name)) {
        return;
    }
    let _ = renderer.push_line(format!("{}{}", "  ".repeat(depth), var_name));
}
