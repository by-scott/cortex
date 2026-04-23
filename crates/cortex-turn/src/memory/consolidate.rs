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

/// Dependencies and thresholds for LLM-driven smart consolidation.
pub struct SmartConsolidateOptions<'a> {
    pub store: &'a cortex_kernel::MemoryStore,
    pub embedding_client: &'a cortex_kernel::EmbeddingClient,
    pub embedding_store: &'a cortex_kernel::EmbeddingStore,
    pub llm: &'a dyn crate::llm::client::LlmClient,
    pub template: &'a str,
    pub max_tokens: usize,
    pub similarity_threshold: f64,
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
/// among semantically similar episodic entries and at least one is stabilized.
///
/// Returns `(id, description)` pairs for each upgraded memory.
pub fn upgrade_episodic_to_semantic(
    memories: &mut [MemoryEntry],
    description_embeddings: &[Option<Vec<f64>>],
    similarity_threshold: f64,
) -> Vec<(String, String)> {
    let episodic_indices: Vec<usize> = memories
        .iter()
        .enumerate()
        .filter_map(|(idx, memory)| (memory.kind == MemoryKind::Episodic).then_some(idx))
        .collect();
    if episodic_indices.is_empty() {
        return Vec::new();
    }

    let descriptions: Vec<String> = episodic_indices
        .iter()
        .map(|&idx| memories[idx].description.clone())
        .collect();
    let embeddings = align_embeddings(description_embeddings, memories.len(), &episodic_indices);
    let groups = group_descriptions_by_embedding(&descriptions, &embeddings, similarity_threshold);

    let mut upgraded = Vec::new();
    for group in &groups {
        if group.len() < 3 {
            continue;
        }
        let indices: Vec<usize> = group.iter().map(|&idx| episodic_indices[idx]).collect();
        let has_stabilized = indices
            .iter()
            .any(|&i| memories[i].status == MemoryStatus::Stabilized);
        if !has_stabilized {
            continue;
        }
        for &i in &indices {
            if memories[i].kind == MemoryKind::Episodic {
                memories[i].kind = MemoryKind::Semantic;
                upgraded.push((memories[i].id.clone(), memories[i].description.clone()));
            }
        }
    }
    upgraded
}

fn align_embeddings(
    description_embeddings: &[Option<Vec<f64>>],
    memory_count: usize,
    episodic_indices: &[usize],
) -> Vec<Option<Vec<f64>>> {
    if description_embeddings.len() == memory_count {
        episodic_indices
            .iter()
            .map(|&idx| description_embeddings[idx].clone())
            .collect()
    } else if description_embeddings.len() == episodic_indices.len() {
        description_embeddings.to_vec()
    } else {
        vec![None; episodic_indices.len()]
    }
}

/// Group description indices by embedding cosine similarity with exact-match fallback.
#[must_use]
pub fn group_descriptions_by_embedding(
    descriptions: &[String],
    embeddings: &[Option<Vec<f64>>],
    similarity_threshold: f64,
) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..descriptions.len()).collect();
    for i in 0..descriptions.len() {
        for j in (i + 1)..descriptions.len() {
            if descriptions_should_group(
                &descriptions[i],
                &descriptions[j],
                embeddings.get(i).and_then(Option::as_deref),
                embeddings.get(j).and_then(Option::as_deref),
                similarity_threshold,
            ) {
                union_parent(&mut parent, i, j);
            }
        }
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..descriptions.len() {
        let root = find_parent(&parent, i);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| find_parent(&parent, group[0]) == root)
        {
            group.push(i);
        } else {
            groups.push(vec![i]);
        }
    }
    groups
}

fn descriptions_should_group(
    left: &str,
    right: &str,
    left_embedding: Option<&[f64]>,
    right_embedding: Option<&[f64]>,
    similarity_threshold: f64,
) -> bool {
    match (left_embedding, right_embedding) {
        (Some(left), Some(right)) => cosine_similarity(left, right) > similarity_threshold,
        _ => left == right,
    }
}

fn find_parent(parent: &[usize], idx: usize) -> usize {
    if parent[idx] == idx {
        idx
    } else {
        find_parent(parent, parent[idx])
    }
}

fn union_parent(parent: &mut [usize], left: usize, right: usize) {
    let left_root = find_parent(parent, left);
    let right_root = find_parent(parent, right);
    if left_root != right_root {
        parent[left_root] = right_root;
    }
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

const MAX_GROUP_SIZE: usize = 5;

/// Group memories by embedding cosine similarity using greedy clustering.
///
/// Each memory is compared to existing group centroids (average embeddings).
/// If similarity exceeds the threshold, it joins that group (up to
/// `MAX_GROUP_SIZE`). Otherwise it starts a new group.
/// Returns groups of indices into the embeddings / memories slice.
#[must_use]
pub fn group_by_similarity(embeddings: &[Vec<f64>], similarity_threshold: f64) -> Vec<Vec<usize>> {
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
            if sim > similarity_threshold && sim > best_sim {
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
    options: SmartConsolidateOptions<'_>,
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

        let emb = match options.embedding_store.get(&hash) {
            Some(cached) => cached,
            None => match options.embedding_client.embed(&text).await {
                Ok(emb) => {
                    let _ = options.embedding_store.put(&hash, "default", &emb);
                    let _ = options.embedding_store.ensure_vector_table(emb.len());
                    let _ = options.embedding_store.upsert_vector(&mem.id, &emb);
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
    let groups = group_by_similarity(&embeddings, options.similarity_threshold);

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
        let prompt = build_consolidate_prompt(options.template, &group_mems);

        let messages = vec![cortex_types::Message::user(prompt)];

        let request = crate::llm::types::LlmRequest {
            system: None,
            messages: &messages,
            tools: None,
            max_tokens: options.max_tokens,
            transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
            on_text: None,
        };

        match options.llm.complete(request).await {
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

                    if options.store.save(&merged).is_ok() {
                        let mut deleted = 0;
                        for mem in &group_mems {
                            if options.store.delete(&mem.id).is_ok() {
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
