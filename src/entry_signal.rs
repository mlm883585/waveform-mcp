//! Entry signal suggestion for BFS root-cause tracing.
//!
//! When no design_spec.yaml is available, this module provides deterministic
//! ranking of candidate entry signals based on waveform hierarchy and dependency
//! graph information.

use crate::deps::DepGraph;
use crate::hierarchy::{collect_signals_from_scope, find_scope_by_path, find_signal_by_path};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A candidate entry signal for BFS, with ranking information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntrySignalCandidate {
    /// Signal path (canonical name from deps, or raw waveform path).
    pub signal_path: String,
    /// Resolved waveform path after alias resolution (if applicable).
    pub resolved_path: Option<String>,
    /// Ranking tier: 1 = deps output node, 2 = deps boundary node, 3 = not in deps.
    pub tier: u8,
    /// Whether the signal name matches assertion name tokens.
    pub matches_assertion: bool,
    /// Number of fan-in edges (if signal is a deps output node).
    pub fan_in_count: Option<usize>,
    /// Dep edge types feeding into this signal.
    pub dep_types: Vec<String>,
    /// Category from deps (if applicable).
    pub category: Option<String>,
    /// Human-readable reason for this candidate.
    pub reason: String,
}

/// Extract search tokens from an assertion name.
///
/// Splits on '_', strips common prefixes like "assert", "check", "verify".
pub fn extract_assertion_tokens(assertion_name: &str) -> Vec<String> {
    let lower = assertion_name.to_lowercase();
    lower
        .split('_')
        .filter(|token| {
            !matches!(
                *token,
                "assert" | "check" | "verify" | "p0" | "p1" | "p2" | ""
            )
        })
        .filter(|token| token.len() >= 2)
        .map(|t| t.to_string())
        .collect()
}

/// Check if a signal's local name matches any assertion token.
pub fn signal_matches_assertion(signal_path: &str, tokens: &[String]) -> bool {
    assertion_match_score(signal_path, tokens) > 0
}

fn assertion_match_score(signal_path: &str, tokens: &[String]) -> usize {
    if tokens.is_empty() {
        return 0;
    }

    let full_path = signal_path.to_lowercase();
    let local_name = signal_path
        .rsplit('.')
        .next()
        .unwrap_or(signal_path)
        .to_lowercase();

    tokens
        .iter()
        .filter(|token| local_name.contains(token.as_str()) || full_path.contains(token.as_str()))
        .count()
}

/// Collect all signals from all top-level scopes in the hierarchy.
fn collect_all_signals(hierarchy: &wellen::Hierarchy) -> Vec<String> {
    let mut signals = Vec::new();
    let mut seen = HashSet::new();
    for scope_ref in hierarchy.scopes() {
        for signal in
            collect_signals_from_scope(hierarchy, scope_ref, true, None).unwrap_or_default()
        {
            if seen.insert(signal.clone()) {
                signals.push(signal);
            }
        }
    }
    signals
}

/// Try to resolve a waveform signal path to a deps canonical name.
///
/// Uses dep_graph.canonicalize_signal_fuzzy which searches:
/// 1. Exact alias reverse lookup
/// 2. Leaf-name match in signal_aliases (case-sensitive then case-insensitive)
/// 3. Leaf-name match in fan_in/fan_out keys
///
/// Falls back to exact has_signal check for paths that happen to be canonical names.
fn resolve_to_deps_canonical(
    waveform_path: &str,
    dep_graph: &DepGraph,
    simulator: &str,
) -> Option<String> {
    dep_graph
        .canonicalize_signal_fuzzy(waveform_path, simulator)
        .or_else(|| {
            if dep_graph.is_output_node(waveform_path) || dep_graph.has_signal(waveform_path) {
                return Some(waveform_path.to_string());
            }
            None
        })
}

/// Suggest candidate entry signals for BFS tracing.
///
/// # Arguments
/// * `hierarchy` - Waveform hierarchy
/// * `dep_graph` - Loaded dependency graph
/// * `assertion_name` - Optional assertion name for substring matching
/// * `scope_path` - Optional scope path from assertion event (limits search scope)
/// * `simulator` - Simulator identifier for alias resolution
/// * `limit` - Maximum candidates to return (-1 for unlimited)
pub fn suggest_entry_signals(
    hierarchy: &wellen::Hierarchy,
    dep_graph: &DepGraph,
    assertion_name: Option<&str>,
    scope_path: Option<&str>,
    simulator: &str,
    limit: isize,
) -> Vec<EntrySignalCandidate> {
    let tokens = assertion_name
        .map(extract_assertion_tokens)
        .unwrap_or_default();

    // Step 1: Collect waveform signals, preferring deps-first fast path
    let waveform_signals = if scope_path.is_some() {
        // When scope_path is specified, collect from that scope
        if let Some(sp) = scope_path {
            if let Some(scope_ref) = find_scope_by_path(hierarchy, sp) {
                collect_signals_from_scope(hierarchy, scope_ref, true, None).unwrap_or_default()
            } else {
                collect_all_signals(hierarchy)
            }
        } else {
            unreachable!()
        }
    } else {
        // No scope specified: use deps-first fast path
        // First check dep_graph output signals exist in waveform
        let mut signals = Vec::new();
        for canonical in dep_graph.output_signals() {
            // Try resolved alias path first, then canonical directly
            let resolved = dep_graph.resolve_signal(&canonical, simulator);
            let lookup_path = resolved.as_deref().unwrap_or(&canonical);
            if find_signal_by_path(hierarchy, lookup_path).is_some()
                || find_signal_by_path(hierarchy, &canonical).is_some()
            {
                signals.push(canonical.clone());
            }
        }
        // Also include waveform signals not yet in deps (for Tier 3 candidates)
        let all_hierarchy_signals = collect_all_signals(hierarchy);
        let deps_known: std::collections::HashSet<String> = dep_graph
            .output_signals()
            .into_iter()
            .chain(dep_graph.fan_out_keys())
            .collect();
        for ws in &all_hierarchy_signals {
            if !deps_known.contains(ws) {
                if resolve_to_deps_canonical(ws, dep_graph, simulator).is_none() {
                    signals.push(ws.clone());
                }
            }
        }
        signals
    };

    // Step 2: Classify each signal against dep_graph
    let mut candidates: Vec<EntrySignalCandidate> = Vec::new();

    for signal_path in &waveform_signals {
        // Resolve to deps canonical name (handles case-insensitive matching)
        let deps_canonical = resolve_to_deps_canonical(signal_path, dep_graph, simulator);
        let lookup_path = deps_canonical.as_deref().unwrap_or(signal_path);

        let resolved = dep_graph.resolve_signal(lookup_path, simulator);
        let match_score = assertion_match_score(signal_path, &tokens);
        let matches = match_score > 0;

        if let Some(canonical) = &deps_canonical {
            if dep_graph.is_output_node(canonical) {
                // Tier 1: deps output node
                // is_output_node guarantees fan_in contains this key
                let fan_in_edges = dep_graph.fan_in(canonical).unwrap();
                let dep_types: Vec<String> = fan_in_edges
                    .iter()
                    .map(|e| e.dep_type.to_string())
                    .collect();

                candidates.push(EntrySignalCandidate {
                    signal_path: canonical.clone(),
                    resolved_path: resolved,
                    tier: 1,
                    matches_assertion: matches,
                    fan_in_count: Some(fan_in_edges.len()),
                    dep_types,
                    category: dep_graph.get_category(canonical).map(|s| s.to_string()),
                    reason: if matches {
                        format!(
                            "Output node in deps (matches '{}'), {} fan-in edges",
                            assertion_name.unwrap_or("?"),
                            fan_in_edges.len()
                        )
                    } else {
                        format!("Output node in deps, {} fan-in edges", fan_in_edges.len())
                    },
                });
            } else {
                // Tier 2: signal in deps but only as boundary/upstream source
                candidates.push(EntrySignalCandidate {
                    signal_path: canonical.clone(),
                    resolved_path: resolved,
                    tier: 2,
                    matches_assertion: matches,
                    fan_in_count: None,
                    dep_types: Vec::new(),
                    category: None,
                    reason: if matches {
                        format!(
                            "Boundary in deps (matches '{}'), no upstream traceable",
                            assertion_name.unwrap_or("?")
                        )
                    } else {
                        "Boundary in deps, no upstream traceable".to_string()
                    },
                });
            }
        } else {
            // Tier 3: signal not in deps at all
            candidates.push(EntrySignalCandidate {
                signal_path: signal_path.clone(),
                resolved_path: None,
                tier: 3,
                matches_assertion: matches,
                fan_in_count: None,
                dep_types: Vec::new(),
                category: None,
                reason: if matches {
                    format!(
                        "Not in deps (matches '{}'), add to deps.yaml to make traceable",
                        assertion_name.unwrap_or("?")
                    )
                } else {
                    "Not in deps, add to deps.yaml to make traceable".to_string()
                },
            });
        }
    }

    // Step 3: Sort by composite priority
    candidates.sort_by(|a, b| {
        match a.tier.cmp(&b.tier) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        let a_score = assertion_match_score(&a.signal_path, &tokens);
        let b_score = assertion_match_score(&b.signal_path, &tokens);
        match b_score.cmp(&a_score) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        let a_count = a.fan_in_count.unwrap_or(0);
        let b_count = b.fan_in_count.unwrap_or(0);
        b_count.cmp(&a_count)
    });

    if !tokens.is_empty() {
        let best_score = candidates
            .iter()
            .map(|candidate| assertion_match_score(&candidate.signal_path, &tokens))
            .max()
            .unwrap_or(0);
        if best_score > 0 {
            let matched: Vec<EntrySignalCandidate> = candidates
                .iter()
                .filter(|candidate| {
                    assertion_match_score(&candidate.signal_path, &tokens) == best_score
                })
                .cloned()
                .collect();
            candidates = matched;
        }
    }

    // Step 4: Apply limit
    if limit >= 0 && candidates.len() > limit as usize {
        candidates.truncate(limit as usize);
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::{assertion_match_score, extract_assertion_tokens, signal_matches_assertion};

    #[test]
    fn assertion_tokens_drop_noise_and_short_fragments() {
        let tokens = extract_assertion_tokens("assert_o_byte_valid_p0");
        assert_eq!(tokens, vec!["byte".to_string(), "valid".to_string()]);
    }

    #[test]
    fn signal_match_checks_full_path_and_local_name() {
        let tokens = extract_assertion_tokens("assert_o_byte_valid");
        assert!(signal_matches_assertion("tb_uart_rx.o_byte_valid", &tokens));
        assert!(signal_matches_assertion(
            "tb_uart_rx.u_dut.control_o_byte_valid",
            &tokens
        ));
        assert!(signal_matches_assertion("tb_uart_rx.o_byte_data", &tokens));
        assert_eq!(
            assertion_match_score("tb_uart_rx.u_dut.control_o_byte_valid", &tokens),
            2
        );
        assert_eq!(assertion_match_score("tb_uart_rx.o_byte_data", &tokens), 1);
    }
}
