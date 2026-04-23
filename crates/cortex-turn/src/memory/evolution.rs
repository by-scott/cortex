use cortex_kernel::{MemoryGraph, MemoryStore};
use cortex_types::Payload;
use cortex_types::memory::{MemoryEntry, MemoryRelation, MemoryStatus};

/// Minimum content length for a memory to be considered for splitting.
const SPLIT_MIN_CONTENT_LEN: usize = 1500;
/// Minimum paragraph count for a memory to be considered for splitting.
const SPLIT_MIN_PARAGRAPHS: usize = 5;

// ── Memory Split ──────────────────────────────────────────────

/// Identify memories eligible for splitting.
///
/// Content must exceed 1500 chars AND have >= 5 paragraphs.
#[must_use]
pub fn split_candidates(memories: &[MemoryEntry]) -> Vec<&MemoryEntry> {
    memories
        .iter()
        .filter(|m| m.status != MemoryStatus::Deprecated)
        .filter(|m| m.content.len() > SPLIT_MIN_CONTENT_LEN)
        .filter(|m| count_paragraphs(&m.content) >= SPLIT_MIN_PARAGRAPHS)
        .collect()
}

/// Count non-empty paragraphs in text (split by double newline).
fn count_paragraphs(text: &str) -> usize {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .count()
}

/// Truncate a string to at most `max_chars` characters, at a valid UTF-8
/// boundary. Returns the full string if it has fewer than `max_chars` characters.
fn safe_truncate(text: &str, max_chars: usize) -> &str {
    text.char_indices()
        .nth(max_chars)
        .map_or(text, |(byte_idx, _)| &text[..byte_idx])
}

/// Split a memory into sub-memories, one per paragraph.
///
/// Sub-memories inherit type, kind, `instance_id` from parent.
/// Strength is divided evenly, `access_count` starts at 1.
#[must_use]
pub fn split_memory(entry: &MemoryEntry) -> Vec<MemoryEntry> {
    let paragraphs: Vec<&str> = entry
        .content
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    if paragraphs.len() < 2 {
        return vec![];
    }

    let para_count = u32::try_from(paragraphs.len()).unwrap_or(u32::MAX);
    let child_strength = (entry.strength / f64::from(para_count)).max(0.1);
    let now = chrono::Utc::now();

    paragraphs
        .into_iter()
        .map(|para| {
            let description = if para.chars().count() > 80 {
                format!("{}...", safe_truncate(para, 77))
            } else {
                para.to_string()
            };
            let mut child = MemoryEntry::new(para, &description, entry.memory_type, entry.kind);
            child.status = entry.status;
            child.strength = child_strength;
            child.created_at = entry.created_at;
            child.updated_at = now;
            child.access_count = 1;
            child.instance_id.clone_from(&entry.instance_id);
            child.source = entry.source;
            child
        })
        .collect()
}

/// Execute splits: save children, add `split_from` relations, inherit parent
/// relations, deprecate parent.
#[must_use]
pub fn execute_splits(
    candidates: &[&MemoryEntry],
    store: &MemoryStore,
    graph: &MemoryGraph,
) -> Vec<Payload> {
    let mut events = Vec::new();

    for parent in candidates {
        let children = split_memory(parent);
        if children.is_empty() {
            continue;
        }

        // Get parent's existing relations for inheritance
        let parent_relations = graph.relations_of(&parent.id).unwrap_or_default();

        let child_count = children.len();
        for child in &children {
            // Save child to store
            if store.save(child).is_err() {
                continue;
            }

            // Add split_from relation
            let _ = graph.add_relation(&MemoryRelation::new(&child.id, &parent.id, "split_from"));

            // Inherit parent's relations (excluding self-references)
            for rel in &parent_relations {
                let other_id = if rel.source_id == parent.id {
                    &rel.target_id
                } else {
                    &rel.source_id
                };
                if other_id != &parent.id {
                    let _ = graph.add_relation(&MemoryRelation::new(
                        &child.id,
                        other_id,
                        &rel.relation_type,
                    ));
                }
            }
        }

        // Deprecate parent
        let mut deprecated_parent = (*parent).clone();
        deprecated_parent.status = MemoryStatus::Deprecated;
        deprecated_parent.updated_at = chrono::Utc::now();
        let _ = store.save(&deprecated_parent);

        let reason: &'static str = if parent.content.len() > SPLIT_MIN_CONTENT_LEN * 2 {
            "oversized"
        } else {
            "multi_topic"
        };

        events.push(Payload::MemorySplit {
            parent_id: parent.id.clone(),
            child_count,
            reason: reason.to_string(),
        });
    }

    events
}

// ── Graph Health Assessment ───────────────────────────────────

/// Assess graph health based on topology metrics.
///
/// Returns a `MemoryGraphHealthAssessed` payload.
#[must_use]
pub fn assess_graph_health(store: &MemoryStore, graph: &MemoryGraph) -> Payload {
    // Get all memory IDs from store
    let all_memories = store.list_all().unwrap_or_default();
    let memory_ids: std::collections::HashSet<String> =
        all_memories.iter().map(|m| m.id.clone()).collect();
    let total_memories = u32::try_from(memory_ids.len()).unwrap_or(u32::MAX);

    if total_memories == 0 {
        return Payload::MemoryGraphHealthAssessed {
            score: 1.0,
            orphan_ratio: 0.0,
            avg_degree: 0.0,
            largest_component_ratio: 1.0,
            dead_link_count: 0,
        };
    }

    // Graph node IDs (only those appearing in relations)
    let graph_nodes = graph.all_node_ids().unwrap_or_default();
    let degree_map = graph.degree_map().unwrap_or_default();

    // Orphan ratio: memories not in any relation / total memories
    let connected_memories =
        u32::try_from(memory_ids.intersection(&graph_nodes).count()).unwrap_or(u32::MAX);
    let orphan_count = total_memories.saturating_sub(connected_memories);
    let orphan_ratio = f64::from(orphan_count) / f64::from(total_memories);

    // Average degree (only for connected nodes)
    let avg_degree = if connected_memories > 0 {
        let total_degree = u32::try_from(
            memory_ids
                .iter()
                .filter_map(|id| degree_map.get(id))
                .sum::<usize>(),
        )
        .unwrap_or(u32::MAX);
        f64::from(total_degree) / f64::from(connected_memories)
    } else {
        0.0
    };

    // Largest connected component ratio
    let components = graph.connected_components(&memory_ids).unwrap_or_default();
    let largest_component_size = u32::try_from(
        components
            .iter()
            .map(std::collections::HashSet::len)
            .max()
            .unwrap_or(0),
    )
    .unwrap_or(u32::MAX);
    let largest_component_ratio = f64::from(largest_component_size) / f64::from(total_memories);

    // Dead links: relations where source or target is not in store or is deprecated
    let deprecated_ids: std::collections::HashSet<&String> = all_memories
        .iter()
        .filter(|m| m.status == MemoryStatus::Deprecated)
        .map(|m| &m.id)
        .collect();
    let all_rels = graph.all_relations().unwrap_or_default();
    let dead_link_count = all_rels
        .iter()
        .filter(|r| {
            !memory_ids.contains(&r.source_id)
                || !memory_ids.contains(&r.target_id)
                || deprecated_ids.contains(&r.source_id)
                || deprecated_ids.contains(&r.target_id)
        })
        .count();

    // Aggregate score: weighted combination
    let orphan_score = 1.0 - orphan_ratio;
    let degree_score = (avg_degree / 5.0).min(1.0);
    let component_score = largest_component_ratio;
    let dead_links = u32::try_from(dead_link_count).unwrap_or(u32::MAX);
    let total_rels = u32::try_from(all_rels.len()).unwrap_or(u32::MAX);
    let dead_link_penalty = if all_rels.is_empty() {
        1.0
    } else {
        1.0 - (f64::from(dead_links) / f64::from(total_rels)).min(1.0)
    };

    let health_score = orphan_score.mul_add(
        0.3,
        degree_score.mul_add(0.2, component_score.mul_add(0.3, dead_link_penalty * 0.2)),
    );

    Payload::MemoryGraphHealthAssessed {
        score: health_score,
        orphan_ratio,
        avg_degree,
        largest_component_ratio,
        dead_link_count,
    }
}

// ── Relation Reorganization ───────────────────────────────────

/// Remove dead links (pointing to deprecated or non-existent memories) from the graph.
#[must_use]
pub fn reorganize_relations(store: &MemoryStore, graph: &MemoryGraph) -> Payload {
    let all_memories = store.list_all().unwrap_or_default();
    let active_ids: std::collections::HashSet<String> = all_memories
        .iter()
        .filter(|m| m.status != MemoryStatus::Deprecated)
        .map(|m| m.id.clone())
        .collect();

    let all_rels = graph.all_relations().unwrap_or_default();
    let mut dead_links_removed = 0;

    for rel in &all_rels {
        if (!active_ids.contains(&rel.source_id) || !active_ids.contains(&rel.target_id))
            && graph
                .remove_relation(&rel.source_id, &rel.target_id, &rel.relation_type)
                .unwrap_or(false)
        {
            dead_links_removed += 1;
        }
    }

    Payload::MemoryRelationReorganized {
        dead_links_removed,
        duplicate_relations_found: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::memory::{MemoryKind, MemoryType};

    fn make_memory(id: &str, content: &str, status: MemoryStatus) -> MemoryEntry {
        let mut entry =
            MemoryEntry::new(content, "desc", MemoryType::Project, MemoryKind::Semantic);
        entry.id = id.to_string();
        entry.status = status;
        entry.strength = 0.8;
        entry
    }

    fn long_multi_topic_content() -> String {
        let para = "This is a substantial paragraph about a specific topic that has enough content to justify its own memory entry in the system. It includes durable context, constraints, consequences, and enough supporting details that a later consolidation pass can decide whether it should remain independent or be merged with related material.";
        format!("{para}\n\n{para}\n\n{para}\n\n{para}\n\n{para}")
    }

    // ── Split tests ──

    #[test]
    fn split_candidates_detects_long_multi_paragraph() {
        let content = long_multi_topic_content();
        let memories = [make_memory("m1", &content, MemoryStatus::Materialized)];
        let candidates = split_candidates(&memories);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn split_candidates_skips_short_content() {
        let memories = [make_memory("m1", "short", MemoryStatus::Materialized)];
        let candidates = split_candidates(&memories);
        assert!(candidates.is_empty());
    }

    #[test]
    fn split_candidates_skips_deprecated() {
        let content = long_multi_topic_content();
        let memories = [make_memory("m1", &content, MemoryStatus::Deprecated)];
        let candidates = split_candidates(&memories);
        assert!(candidates.is_empty());
    }

    #[test]
    fn split_memory_produces_children() {
        let content = long_multi_topic_content();
        let m = make_memory("m1", &content, MemoryStatus::Materialized);
        let children = split_memory(&m);
        assert_eq!(children.len(), 5);
        assert!(
            children
                .iter()
                .all(|c| c.memory_type == MemoryType::Project)
        );
        assert!(
            children
                .iter()
                .all(|c| c.status == MemoryStatus::Materialized)
        );
        assert!(children.iter().all(|c| c.access_count == 1));
    }

    #[test]
    fn split_memory_single_paragraph_returns_empty() {
        let m = make_memory("m1", "just one paragraph", MemoryStatus::Materialized);
        let children = split_memory(&m);
        assert!(children.is_empty());
    }

    #[test]
    fn split_memory_safe_utf8_truncation() {
        // Build a paragraph with multi-byte characters exceeding 80 chars
        let multibyte_para = "a]b]c]d]e]f]g]h]i]j]k]l]m]n]o]p]q]r]s]t]u]v]w]x]y]z]A]B]C]D]E]F]G]H]I]J]K]L]M]N]O]P]Q]R]S]T]U]V]W]X]Y]Z]"
            .replace(']', "\u{00e9}"); // e-acute, 2 bytes each
        let content = format!("{multibyte_para}\n\n{multibyte_para}\n\n{multibyte_para}");
        let mut entry = MemoryEntry::new(&content, "desc", MemoryType::User, MemoryKind::Semantic);
        entry.strength = 1.0;
        let children = split_memory(&entry);
        // Should not panic — every description must be valid UTF-8
        assert_eq!(children.len(), 3);
        for child in &children {
            assert!(child.description.ends_with("..."));
            // Verify description is within bounds (77 chars + "...")
            assert!(child.description.chars().count() <= 80);
        }
    }

    #[test]
    fn execute_splits_with_store_and_graph() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        let content = long_multi_topic_content();
        let m = make_memory("m1", &content, MemoryStatus::Materialized);
        store.save(&m).unwrap();

        graph
            .add_relation(&MemoryRelation::new("m1", "m2", "relates_to"))
            .unwrap();

        let memories = [m];
        let candidates = split_candidates(&memories);
        let refs: Vec<&MemoryEntry> = candidates.into_iter().collect();
        let events = execute_splits(&refs, &store, &graph);

        assert_eq!(events.len(), 1);
        match &events[0] {
            Payload::MemorySplit {
                parent_id,
                child_count,
                ..
            } => {
                assert_eq!(parent_id, "m1");
                assert_eq!(*child_count, 5);
            }
            _ => panic!("expected MemorySplit"),
        }

        let parent = store.load("m1").unwrap();
        assert_eq!(parent.status, MemoryStatus::Deprecated);

        let all = store.list_all().unwrap();
        let children: Vec<_> = all.iter().filter(|m| m.id != "m1").collect();
        assert_eq!(children.len(), 5);

        for child in &children {
            let child_rels = graph.relations_of(&child.id).unwrap();
            assert!(child_rels.iter().any(|r| r.relation_type == "split_from"));
            assert!(child_rels.iter().any(|r| r.relation_type == "relates_to"));
        }
    }

    // ── Graph health tests ──

    #[test]
    fn graph_health_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        match assess_graph_health(&store, &graph) {
            Payload::MemoryGraphHealthAssessed { score, .. } => {
                assert!((score - 1.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected MemoryGraphHealthAssessed"),
        }
    }

    #[test]
    fn graph_health_connected_graph() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        for id in ["a", "b", "c"] {
            store
                .save(&make_memory(id, "content", MemoryStatus::Materialized))
                .unwrap();
        }
        graph
            .add_relation(&MemoryRelation::new("a", "b", "x"))
            .unwrap();
        graph
            .add_relation(&MemoryRelation::new("b", "c", "x"))
            .unwrap();

        match assess_graph_health(&store, &graph) {
            Payload::MemoryGraphHealthAssessed {
                score,
                orphan_ratio,
                largest_component_ratio,
                dead_link_count,
                ..
            } => {
                assert!(score > 0.5, "healthy graph: {score}");
                assert!(orphan_ratio < 0.01);
                assert!((largest_component_ratio - 1.0).abs() < f64::EPSILON);
                assert_eq!(dead_link_count, 0);
            }
            _ => panic!("expected MemoryGraphHealthAssessed"),
        }
    }

    #[test]
    fn graph_health_fragmented() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        for id in ["a", "b", "c", "d", "e"] {
            store
                .save(&make_memory(id, "content", MemoryStatus::Materialized))
                .unwrap();
        }

        match assess_graph_health(&store, &graph) {
            Payload::MemoryGraphHealthAssessed {
                score,
                orphan_ratio,
                ..
            } => {
                assert!(score < 0.5, "fragmented graph: {score}");
                assert!((orphan_ratio - 1.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected MemoryGraphHealthAssessed"),
        }
    }

    // ── Relation reorg tests ──

    #[test]
    fn reorganize_removes_dead_links() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        store
            .save(&make_memory("a", "content", MemoryStatus::Materialized))
            .unwrap();
        store
            .save(&make_memory("b", "old", MemoryStatus::Deprecated))
            .unwrap();

        graph
            .add_relation(&MemoryRelation::new("a", "b", "x"))
            .unwrap();
        graph
            .add_relation(&MemoryRelation::new("a", "nonexistent", "y"))
            .unwrap();

        match reorganize_relations(&store, &graph) {
            Payload::MemoryRelationReorganized {
                dead_links_removed, ..
            } => {
                assert_eq!(dead_links_removed, 2);
            }
            _ => panic!("expected MemoryRelationReorganized"),
        }

        let rels = graph.all_relations().unwrap();
        assert!(rels.is_empty());
    }

    #[test]
    fn reorganize_keeps_healthy_relations() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let graph = MemoryGraph::in_memory().unwrap();

        store
            .save(&make_memory("a", "content", MemoryStatus::Materialized))
            .unwrap();
        store
            .save(&make_memory("b", "content", MemoryStatus::Materialized))
            .unwrap();
        graph
            .add_relation(&MemoryRelation::new("a", "b", "x"))
            .unwrap();

        match reorganize_relations(&store, &graph) {
            Payload::MemoryRelationReorganized {
                dead_links_removed, ..
            } => {
                assert_eq!(dead_links_removed, 0);
            }
            _ => panic!("expected MemoryRelationReorganized"),
        }

        let rels = graph.all_relations().unwrap();
        assert_eq!(rels.len(), 1);
    }
}
