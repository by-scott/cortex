use super::decay::{freshness, should_deprecate};
use super::recall::cosine_similarity;
use cortex_types::{MemoryEntry, MemoryKind, MemoryStatus, MemoryType};

/// Result of a consolidation pass.
pub struct ConsolidateResult {
    pub upgraded: usize,
    pub deprecated: usize,
}

/// Result of LLM-driven smart consolidation.
pub struct SmartConsolidateResult {
    /// Number of groups successfully merged.
    pub merged: usize,
    /// Number of source memories removed after merge.
    pub removed: usize,
    /// Number of groups where LLM call failed (skipped).
    pub skipped: usize,
}

/// Combined result of status-migration + smart consolidation.
pub struct FullConsolidateResult {
    pub upgraded: usize,
    pub deprecated: usize,
    pub merged: usize,
    pub removed: usize,
    pub skipped: usize,
}

/// Consolidate memories: upgrade status based on access patterns.
///
/// - Captured to Materialized: `access_count` >= 2
/// - Materialized to Stabilized: `access_count` >= 5 and strength > 0.5
/// - Stabilized memories in a reconsolidation window are downgraded to
///   Materialized (Nader 2000) so they can be updated with new evidence.
///
/// Modifies entries in-place.
pub fn consolidate_memories(memories: &mut [MemoryEntry]) -> ConsolidateResult {
    let mut upgraded = 0;
    let now = chrono::Utc::now();

    for m in memories.iter_mut() {
        // Reconsolidation: Stabilized memory in active window -> downgrade
        if m.status == MemoryStatus::Stabilized
            && m.reconsolidation_until.is_some_and(|until| until > now)
        {
            m.status = MemoryStatus::Materialized;
            continue;
        }

        let should_upgrade = match m.status {
            MemoryStatus::Captured => m.access_count >= 2,
            MemoryStatus::Materialized => m.access_count >= 5 && m.strength > 0.5,
            _ => false,
        };
        if should_upgrade && let Ok(new_status) = m.status.try_advance() {
            m.status = new_status;
            upgraded += 1;
        }
    }

    ConsolidateResult {
        upgraded,
        deprecated: 0,
    }
}

/// Upgrade episodic memories to semantic when a description appears >= 3 times
/// among episodic entries and at least one is stabilized.
///
/// Returns `(id, description)` pairs for each upgraded memory.
pub fn upgrade_episodic_to_semantic(memories: &mut [MemoryEntry]) -> Vec<(String, String)> {
    use std::collections::HashMap;

    // Group indices of Episodic memories by description
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, m) in memories.iter().enumerate() {
        if m.kind == MemoryKind::Episodic {
            groups.entry(m.description.clone()).or_default().push(i);
        }
    }

    let mut upgraded = Vec::new();
    for (desc, indices) in &groups {
        if indices.len() < 3 {
            continue;
        }
        let has_stabilized = indices
            .iter()
            .any(|&i| memories[i].status == MemoryStatus::Stabilized);
        if !has_stabilized {
            continue;
        }
        for &i in indices {
            if memories[i].kind == MemoryKind::Episodic {
                memories[i].kind = MemoryKind::Semantic;
                upgraded.push((memories[i].id.clone(), desc.clone()));
            }
        }
    }
    upgraded
}

/// Apply decay to all memories.
///
/// Reduce strength and deprecate stale entries.
/// Returns the number of deprecated memories.
pub fn apply_decay(
    memories: &mut [MemoryEntry],
    decay_rate: f64,
    current_time: chrono::DateTime<chrono::Utc>,
) -> usize {
    let mut deprecated_count = 0;

    for m in memories.iter_mut() {
        if m.status == MemoryStatus::Deprecated {
            continue;
        }

        let secs =
            u32::try_from((current_time - m.updated_at).num_seconds().max(0)).unwrap_or(u32::MAX);
        let hours = f64::from(secs) / 3600.0;
        m.strength = freshness(hours, m.access_count, decay_rate);

        if should_deprecate(hours, m.access_count, decay_rate) {
            m.status = m.status.deprecate();
            deprecated_count += 1;
        }
    }

    deprecated_count
}

// ── Smart consolidation: embedding grouping + LLM merge ─────

const SIMILARITY_THRESHOLD: f64 = 0.85;
const MAX_GROUP_SIZE: usize = 5;

/// Group memories by embedding cosine similarity using greedy clustering.
///
/// Each memory is compared to existing group centroids (average embeddings).
/// If similarity exceeds the threshold, it joins that group (up to
/// `MAX_GROUP_SIZE`). Otherwise it starts a new group.
/// Returns groups of indices into the embeddings / memories slice.
#[must_use]
pub fn group_by_similarity(embeddings: &[Vec<f64>]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut centroids: Vec<Vec<f64>> = Vec::new();

    for (i, emb) in embeddings.iter().enumerate() {
        let mut best_group: Option<usize> = None;
        let mut best_sim: f64 = 0.0;

        for (g, centroid) in centroids.iter().enumerate() {
            if groups[g].len() >= MAX_GROUP_SIZE {
                continue;
            }
            let sim = cosine_similarity(emb, centroid);
            if sim > SIMILARITY_THRESHOLD && sim > best_sim {
                best_sim = sim;
                best_group = Some(g);
            }
        }

        if let Some(g) = best_group {
            groups[g].push(i);
            // Update centroid as running average
            let n = f64::from(u32::try_from(groups[g].len()).unwrap_or(u32::MAX));
            for (j, val) in emb.iter().enumerate() {
                if j < centroids[g].len() {
                    centroids[g][j] = centroids[g][j].mul_add((n - 1.0) / n, val / n);
                }
            }
        } else {
            groups.push(vec![i]);
            centroids.push(emb.clone());
        }
    }

    groups
}

const PH_MEMORIES: &str = "{memories}";

/// Build the consolidation prompt by filling the template with memory contents.
#[must_use]
pub fn build_consolidate_prompt(template: &str, memories: &[&MemoryEntry]) -> String {
    let memories_text = memories
        .iter()
        .enumerate()
        .map(|(i, m)| {
            format!(
                "Memory {}: [{:?}/{:?}] {}\n{}",
                i + 1,
                m.memory_type,
                m.kind,
                m.description,
                m.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    template.replace(PH_MEMORIES, &memories_text)
}

/// Parse the LLM consolidation response as JSON with `summary` and `description` fields.
#[must_use]
pub fn parse_consolidate_response(response: &str) -> Option<(String, String)> {
    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .and_then(|s| s.rsplit_once("```"))
            .map_or(trimmed, |(content, _)| content.trim())
    } else {
        trimmed
    };

    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let summary = parsed.get("summary")?.as_str()?;
    let description = parsed.get("description")?.as_str()?;
    Some((summary.to_string(), description.to_string()))
}

/// Compute merged attributes from a group of memories.
#[must_use]
pub fn merge_group_attributes(group: &[&MemoryEntry]) -> MergedAttributes {
    let status = group
        .iter()
        .map(|m| m.status)
        .max_by_key(|s| match s {
            MemoryStatus::Stabilized => 3,
            MemoryStatus::Materialized => 2,
            MemoryStatus::Captured => 1,
            MemoryStatus::Deprecated => 0,
        })
        .unwrap_or(MemoryStatus::Captured);

    let access_count: u32 = group.iter().map(|m| m.access_count).sum();

    let strength = group.iter().map(|m| m.strength).fold(0.0_f64, f64::max);

    let created_at = group
        .iter()
        .map(|m| m.created_at)
        .min()
        .unwrap_or_else(chrono::Utc::now);

    // Most frequent memory_type, kind, source (groups are always non-empty)
    let memory_type =
        most_frequent(group.iter().map(|m| m.memory_type)).unwrap_or(MemoryType::User);
    let kind = most_frequent(group.iter().map(|m| m.kind)).unwrap_or(MemoryKind::Episodic);
    let source = most_frequent(group.iter().map(|m| m.source)).unwrap_or_default();

    MergedAttributes {
        status,
        access_count,
        strength,
        created_at,
        memory_type,
        kind,
        source,
    }
}

/// Attributes computed from merging a group of memories.
pub struct MergedAttributes {
    pub status: MemoryStatus,
    pub access_count: u32,
    pub strength: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub memory_type: MemoryType,
    pub kind: MemoryKind,
    pub source: cortex_types::MemorySource,
}

/// Find the most frequent value in an iterator (ties broken by first occurrence).
fn most_frequent<T: Eq + Copy>(items: impl Iterator<Item = T>) -> Option<T> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for item in items {
        if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == item) {
            entry.1 += 1;
        } else {
            counts.push((item, 1));
        }
    }
    counts.into_iter().max_by_key(|(_, c)| *c).map(|(v, _)| v)
}

/// Run LLM-driven smart consolidation on a set of memories.
///
/// Flow: embed all memories, group by similarity, LLM merge each group,
/// save merged, delete originals. Falls back gracefully: if embeddings or LLM
/// are unavailable, returns a zero result (caller should still run
/// `consolidate_memories` for status migration).
pub async fn smart_consolidate(
    memories: &[MemoryEntry],
    store: &cortex_kernel::MemoryStore,
    embedding_client: &cortex_kernel::EmbeddingClient,
    embedding_store: &cortex_kernel::EmbeddingStore,
    llm: &dyn crate::llm::client::LlmClient,
    template: &str,
    max_tokens: usize,
) -> SmartConsolidateResult {
    // Step 1: Embed all non-deprecated memories
    let active: Vec<&MemoryEntry> = memories
        .iter()
        .filter(|m| m.status != MemoryStatus::Deprecated)
        .collect();

    if active.len() < 2 {
        return SmartConsolidateResult {
            merged: 0,
            removed: 0,
            skipped: 0,
        };
    }

    let mut embeddings: Vec<Vec<f64>> = Vec::with_capacity(active.len());
    for mem in &active {
        let text = format!("{} {}", mem.description, mem.content);
        let hash = cortex_kernel::embedding_store::content_hash(&text);

        let emb = match embedding_store.get(&hash) {
            Some(cached) => cached,
            None => match embedding_client.embed(&text).await {
                Ok(emb) => {
                    let _ = embedding_store.put(&hash, "default", &emb);
                    let _ = embedding_store.ensure_vector_table(emb.len());
                    let _ = embedding_store.upsert_vector(&mem.id, &emb);
                    emb
                }
                Err(_) => {
                    return SmartConsolidateResult {
                        merged: 0,
                        removed: 0,
                        skipped: 0,
                    };
                }
            },
        };
        embeddings.push(emb);
    }

    // Step 2: Group by similarity
    let groups = group_by_similarity(&embeddings);

    let mut result = SmartConsolidateResult {
        merged: 0,
        removed: 0,
        skipped: 0,
    };

    // Step 3: For each multi-member group, call LLM to merge
    for group_indices in &groups {
        if group_indices.len() < 2 {
            continue;
        }

        let group_mems: Vec<&MemoryEntry> = group_indices.iter().map(|&i| active[i]).collect();
        let prompt = build_consolidate_prompt(template, &group_mems);

        let messages = vec![cortex_types::Message::user(prompt)];

        let request = crate::llm::types::LlmRequest {
            system: None,
            messages: &messages,
            tools: None,
            max_tokens,
            transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
            on_text: None,
        };

        match llm.complete(request).await {
            Ok(resp) => {
                let text = resp.text.unwrap_or_default();
                if let Some((summary, description)) = parse_consolidate_response(&text) {
                    let attrs = merge_group_attributes(&group_mems);

                    let mut merged =
                        MemoryEntry::new(&description, &summary, attrs.memory_type, attrs.kind);
                    merged.status = attrs.status;
                    merged.access_count = attrs.access_count;
                    merged.strength = attrs.strength;
                    merged.created_at = attrs.created_at;
                    merged.updated_at = chrono::Utc::now();
                    merged.source = attrs.source;

                    if store.save(&merged).is_ok() {
                        let mut deleted = 0;
                        for mem in &group_mems {
                            if store.delete(&mem.id).is_ok() {
                                deleted += 1;
                            }
                        }
                        result.merged += 1;
                        result.removed += deleted;
                    } else {
                        result.skipped += 1;
                    }
                } else {
                    result.skipped += 1;
                }
            }
            Err(_) => {
                result.skipped += 1;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cortex_types::{MemoryKind, MemoryType};

    fn make_mem(status: MemoryStatus, access_count: u32, strength: f64) -> MemoryEntry {
        let mut m = MemoryEntry::new("content", "desc", MemoryType::User, MemoryKind::Semantic);
        m.status = status;
        m.access_count = access_count;
        m.strength = strength;
        m
    }

    #[test]
    fn consolidate_captured_to_materialized() {
        let mut mems = vec![make_mem(MemoryStatus::Captured, 3, 1.0)];
        let result = consolidate_memories(&mut mems);
        assert_eq!(result.upgraded, 1);
        assert_eq!(mems[0].status, MemoryStatus::Materialized);
    }

    #[test]
    fn consolidate_materialized_to_stabilized() {
        let mut mems = vec![make_mem(MemoryStatus::Materialized, 6, 0.8)];
        let result = consolidate_memories(&mut mems);
        assert_eq!(result.upgraded, 1);
        assert_eq!(mems[0].status, MemoryStatus::Stabilized);
    }

    #[test]
    fn consolidate_no_upgrade_insufficient_access() {
        let mut mems = vec![make_mem(MemoryStatus::Captured, 1, 1.0)];
        let result = consolidate_memories(&mut mems);
        assert_eq!(result.upgraded, 0);
        assert_eq!(mems[0].status, MemoryStatus::Captured);
    }

    #[test]
    fn consolidate_no_upgrade_low_strength() {
        let mut mems = vec![make_mem(MemoryStatus::Materialized, 10, 0.3)];
        let result = consolidate_memories(&mut mems);
        assert_eq!(result.upgraded, 0);
        assert_eq!(mems[0].status, MemoryStatus::Materialized);
    }

    #[test]
    fn apply_decay_deprecates_old() {
        let mut mems = vec![make_mem(MemoryStatus::Captured, 0, 1.0)];
        mems[0].updated_at = Utc::now() - chrono::Duration::hours(200);
        let deprecated = apply_decay(&mut mems, 0.05, Utc::now());
        assert_eq!(deprecated, 1);
        assert_eq!(mems[0].status, MemoryStatus::Deprecated);
    }

    #[test]
    fn apply_decay_preserves_recent() {
        let mut mems = vec![make_mem(MemoryStatus::Captured, 0, 1.0)];
        let deprecated = apply_decay(&mut mems, 0.05, Utc::now());
        assert_eq!(deprecated, 0);
        assert_eq!(mems[0].status, MemoryStatus::Captured);
    }

    #[test]
    fn apply_decay_skips_deprecated() {
        let mut mems = vec![make_mem(MemoryStatus::Deprecated, 0, 0.01)];
        mems[0].updated_at = Utc::now() - chrono::Duration::hours(200);
        let deprecated = apply_decay(&mut mems, 0.05, Utc::now());
        assert_eq!(deprecated, 0);
    }

    // ── Smart consolidation tests ──

    #[test]
    fn group_by_similarity_identical_vectors() {
        let embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.99, 0.01, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let groups = group_by_similarity(&embs);
        let group_of_0 = groups.iter().find(|g| g.contains(&0)).unwrap();
        assert!(
            group_of_0.contains(&1),
            "similar vectors should group together"
        );
        let group_of_2 = groups.iter().find(|g| g.contains(&2)).unwrap();
        assert!(
            !group_of_2.contains(&0),
            "dissimilar vector should be separate"
        );
    }

    #[test]
    fn group_by_similarity_all_different() {
        let embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let groups = group_by_similarity(&embs);
        assert_eq!(groups.len(), 3, "all different vectors = separate groups");
    }

    #[test]
    fn group_by_similarity_respects_max_size() {
        let embs: Vec<Vec<f64>> = (0..7).map(|_| vec![1.0, 0.0, 0.0]).collect();
        let groups = group_by_similarity(&embs);
        for g in &groups {
            assert!(g.len() <= MAX_GROUP_SIZE, "group exceeds max size");
        }
        let total: usize = groups.iter().map(Vec::len).sum();
        assert_eq!(total, 7);
    }

    #[test]
    fn build_consolidate_prompt_fills_template() {
        let template = "Consolidate:\n{memories}\nDone.";
        let m1 = MemoryEntry::new("content1", "desc1", MemoryType::User, MemoryKind::Semantic);
        let m2 = MemoryEntry::new(
            "content2",
            "desc2",
            MemoryType::Project,
            MemoryKind::Episodic,
        );
        let result = build_consolidate_prompt(template, &[&m1, &m2]);
        assert!(result.contains("Memory 1:"));
        assert!(result.contains("Memory 2:"));
        assert!(result.contains("desc1"));
        assert!(result.contains("desc2"));
        assert!(!result.contains("{memories}"));
    }

    #[test]
    fn parse_consolidate_response_valid() {
        let json = r#"{"summary": "user prefers concise", "description": "detailed info"}"#;
        let result = parse_consolidate_response(json);
        assert!(result.is_some());
        let (s, d) = result.unwrap();
        assert_eq!(s, "user prefers concise");
        assert_eq!(d, "detailed info");
    }

    #[test]
    fn parse_consolidate_response_with_fences() {
        let json = "```json\n{\"summary\": \"a\", \"description\": \"b\"}\n```";
        let result = parse_consolidate_response(json);
        assert!(result.is_some());
    }

    #[test]
    fn parse_consolidate_response_invalid() {
        assert!(parse_consolidate_response("not json").is_none());
        assert!(parse_consolidate_response("{\"summary\": \"a\"}").is_none());
    }

    #[test]
    fn merge_group_attributes_picks_highest_status() {
        let m1 = make_mem(MemoryStatus::Captured, 3, 0.5);
        let m2 = make_mem(MemoryStatus::Stabilized, 5, 0.9);
        let attrs = merge_group_attributes(&[&m1, &m2]);
        assert_eq!(attrs.status, MemoryStatus::Stabilized);
        assert_eq!(attrs.access_count, 8);
        assert!((attrs.strength - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_group_attributes_earliest_created() {
        let mut m1 = make_mem(MemoryStatus::Captured, 1, 1.0);
        let m2 = make_mem(MemoryStatus::Captured, 1, 1.0);
        m1.created_at = Utc::now() - chrono::Duration::hours(48);
        let attrs = merge_group_attributes(&[&m1, &m2]);
        assert_eq!(attrs.created_at, m1.created_at);
    }

    #[test]
    fn merge_group_attributes_most_frequent_type() {
        let m1 = MemoryEntry::new("c1", "d1", MemoryType::User, MemoryKind::Semantic);
        let m2 = MemoryEntry::new("c2", "d2", MemoryType::User, MemoryKind::Episodic);
        let m3 = MemoryEntry::new("c3", "d3", MemoryType::Project, MemoryKind::Semantic);
        let attrs = merge_group_attributes(&[&m1, &m2, &m3]);
        assert_eq!(attrs.memory_type, MemoryType::User);
        assert_eq!(attrs.kind, MemoryKind::Semantic);
    }

    #[test]
    fn upgrade_episodic_three_same_desc_with_stabilized() {
        let m1 = MemoryEntry::new(
            "c1",
            "user prefers short answers",
            MemoryType::User,
            MemoryKind::Episodic,
        );
        let m2 = MemoryEntry::new(
            "c2",
            "user prefers short answers",
            MemoryType::User,
            MemoryKind::Episodic,
        );
        let mut m3 = MemoryEntry::new(
            "c3",
            "user prefers short answers",
            MemoryType::User,
            MemoryKind::Episodic,
        );
        m3.status = MemoryStatus::Stabilized;

        let mut memories = vec![m1, m2, m3];
        let upgraded = upgrade_episodic_to_semantic(&mut memories);
        assert_eq!(upgraded.len(), 3);
        for m in &memories {
            assert_eq!(m.kind, MemoryKind::Semantic);
        }
    }

    #[test]
    fn upgrade_episodic_two_same_desc_no_upgrade() {
        let m1 = MemoryEntry::new("c1", "desc", MemoryType::User, MemoryKind::Episodic);
        let mut m2 = MemoryEntry::new("c2", "desc", MemoryType::User, MemoryKind::Episodic);
        m2.status = MemoryStatus::Stabilized;

        let mut memories = vec![m1, m2];
        let upgraded = upgrade_episodic_to_semantic(&mut memories);
        assert!(upgraded.is_empty());
    }

    #[test]
    fn upgrade_episodic_already_semantic_no_change() {
        let m1 = MemoryEntry::new("c1", "desc", MemoryType::User, MemoryKind::Semantic);
        let m2 = MemoryEntry::new("c2", "desc", MemoryType::User, MemoryKind::Semantic);
        let mut m3 = MemoryEntry::new("c3", "desc", MemoryType::User, MemoryKind::Semantic);
        m3.status = MemoryStatus::Stabilized;

        let mut memories = vec![m1, m2, m3];
        let upgraded = upgrade_episodic_to_semantic(&mut memories);
        assert!(upgraded.is_empty());
    }

    #[test]
    fn upgrade_episodic_no_stabilized_no_upgrade() {
        let m1 = MemoryEntry::new("c1", "desc", MemoryType::User, MemoryKind::Episodic);
        let m2 = MemoryEntry::new("c2", "desc", MemoryType::User, MemoryKind::Episodic);
        let m3 = MemoryEntry::new("c3", "desc", MemoryType::User, MemoryKind::Episodic);

        let mut memories = vec![m1, m2, m3];
        let upgraded = upgrade_episodic_to_semantic(&mut memories);
        assert!(upgraded.is_empty());
    }
}
