use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{EmbeddingClient, EmbeddingStore, MemoryGraph, MemoryStore};

use super::{Tool, ToolError, ToolResult};
use crate::memory::{
    EmbeddingHealthStatus, EmbeddingRecaller, graph_reasoning_scores, mark_reconsolidation,
    rank_memories,
};

// ── Shared recall context ──────────────────────────────────

/// Components needed for full 6-dimensional hybrid memory recall.
///
/// Shared between `MemorySearchTool` and `TurnExecutor::build_system_prompt`.
/// The runtime layer constructs this once and passes it to both the tool
/// registration and the turn executor.
pub struct MemoryRecallComponents {
    pub store: Arc<MemoryStore>,
    pub embedding_client: Option<Arc<EmbeddingClient>>,
    pub embedding_store: Option<Arc<EmbeddingStore>>,
    pub embedding_health: Option<Arc<EmbeddingHealthStatus>>,
    pub data_dir: PathBuf,
    pub max_recall: usize,
}

impl MemoryRecallComponents {
    fn memory_graph_path(&self) -> PathBuf {
        let instance_home = self.data_dir.parent().unwrap_or(self.data_dir.as_path());
        cortex_kernel::CortexPaths::from_instance_home(instance_home).memory_graph_path()
    }

    /// Perform full 6-dimensional hybrid recall (or BM25 fallback if embeddings unavailable).
    fn recall(
        &self,
        query: &str,
        limit: usize,
        actor: Option<&str>,
    ) -> Result<Vec<cortex_types::MemoryEntry>, String> {
        let all = actor
            .map_or_else(
                || self.store.list_all(),
                |actor| self.store.list_for_actor(actor),
            )
            .map_err(|e| format!("failed to list memories: {e}"))?;

        let top_n = if limit > 0 { limit } else { self.max_recall };

        let results: Vec<&cortex_types::MemoryEntry> =
            match (&self.embedding_client, &self.embedding_store) {
                (Some(ec), Some(cache)) => {
                    let recaller = self.embedding_health.as_ref().map_or_else(
                        || EmbeddingRecaller::new(ec, cache),
                        |health| EmbeddingRecaller::with_health(ec, cache, health),
                    );
                    let graph_scores = MemoryGraph::open(&self.memory_graph_path()).ok().map(|g| {
                        let seeds: Vec<String> = rank_memories(query, &all, 10)
                            .iter()
                            .map(|m| m.id.clone())
                            .collect();
                        graph_reasoning_scores(&seeds, &g, 2)
                    });
                    // Embedding recall needs tokio runtime; fall back to BM25 if
                    // running in a scoped OS thread without runtime context.
                    tokio::runtime::Handle::try_current().map_or_else(
                        |_| rank_memories(query, &all, top_n),
                        |handle| {
                            tokio::task::block_in_place(|| {
                                handle.block_on(recaller.recall(query, &all, top_n, graph_scores))
                            })
                        },
                    )
                }
                _ => rank_memories(query, &all, top_n),
            };

        // Mark reconsolidation for recalled stabilized memories
        mark_reconsolidation(&results, &self.store, 30);

        Ok(results.into_iter().cloned().collect())
    }
}

// ── Search Tool ────────────────────────────────────────────

/// Search memories by query using full 6-dimensional hybrid recall.
pub struct MemorySearchTool {
    ctx: Arc<MemoryRecallComponents>,
}

impl MemorySearchTool {
    #[must_use]
    pub const fn new(ctx: Arc<MemoryRecallComponents>) -> Self {
        Self { ctx }
    }
}

impl Tool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }
    fn description(&self) -> &'static str {
        "Recall persistent actor-scoped memory for prior preferences, corrections, decisions, \
         project conventions, and references. Hybrid ranking uses text, semantic similarity, \
         recency, trust, access history, and graph distance. Search early when continuity matters; \
         stale recall must yield to current observation."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Specific natural-language recall query."
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum results."
                }
            },
            "required": ["query"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("query required".into()))?;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(10, |v| usize::try_from(v).unwrap_or(10));

        let ranked = self
            .ctx
            .recall(query, limit, None)
            .map_err(ToolError::ExecutionFailed)?;
        Ok(format_memory_results(&ranked))
    }

    fn execute_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &dyn cortex_sdk::ToolRuntime,
    ) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("query required".into()))?;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(10, |v| usize::try_from(v).unwrap_or(10));

        let ranked = self
            .ctx
            .recall(query, limit, runtime.invocation().actor.as_deref())
            .map_err(ToolError::ExecutionFailed)?;

        Ok(format_memory_results(&ranked))
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::IntrospectRuntime)
                .with_target("memory"),
        )
    }
}

// ── Save Tool ─────────────────────────────────────────────

/// Tool for the LLM to actively save a memory entry.
pub struct MemorySaveTool {
    store: Arc<MemoryStore>,
}

impl MemorySaveTool {
    #[must_use]
    pub const fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }
}

impl Tool for MemorySaveTool {
    fn name(&self) -> &'static str {
        "memory_save"
    }
    fn description(&self) -> &'static str {
        "Persist durable memory that should affect future sessions: corrections, preferences, \
         project decisions, conventions, collaborator profile, and stable references. Do not save \
         transient chatter, raw logs, secrets, or facts recoverable from files/git. Each memory must \
         be self-contained; description drives future recall."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Self-contained durable memory content."
                },
                "description": {
                    "type": "string",
                    "description": "Precise one-line search summary."
                },
                "type": {
                    "type": "string",
                    "enum": ["User", "Feedback", "Project", "Reference"],
                    "description": "Memory class."
                }
            },
            "required": ["content"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let content = input
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing 'content'".into()))?;

        if content.trim().is_empty() {
            return Err(ToolError::InvalidInput("content must not be empty".into()));
        }

        let description = input
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let memory_type: cortex_types::MemoryType = input
            .get("type")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(cortex_types::MemoryType::User);

        let mut entry = cortex_types::MemoryEntry::new(
            content,
            description,
            memory_type,
            cortex_types::MemoryKind::Episodic,
        );
        entry.add_evidence(cortex_types::MemoryEvidence::new(
            "memory_save_tool",
            entry.source,
            entry.strength,
            description,
        ));
        self.save_entry(&entry)
    }

    fn execute_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &dyn cortex_sdk::ToolRuntime,
    ) -> Result<ToolResult, ToolError> {
        let content = input
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing 'content'".into()))?;

        if content.trim().is_empty() {
            return Err(ToolError::InvalidInput("content must not be empty".into()));
        }

        let description = input
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let memory_type: cortex_types::MemoryType = input
            .get("type")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(cortex_types::MemoryType::User);

        let mut entry = cortex_types::MemoryEntry::new(
            content,
            description,
            memory_type,
            cortex_types::MemoryKind::Episodic,
        );
        if let Some(actor) = runtime.invocation().actor.as_deref()
            && !actor.is_empty()
        {
            entry.owner_actor = actor.to_string();
        }
        entry.add_evidence(cortex_types::MemoryEvidence::new(
            "memory_save_tool",
            entry.source,
            entry.strength,
            description,
        ));
        self.save_entry(&entry)
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::PersistMemory)
                .with_target("content"),
        )
    }
}

impl MemorySaveTool {
    fn save_entry(&self, entry: &cortex_types::MemoryEntry) -> Result<ToolResult, ToolError> {
        let id = entry.id.clone();
        let memory_type = entry.memory_type;

        self.store
            .save(entry)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to save memory: {e}")))?;

        Ok(ToolResult::success(format!(
            "Memory saved (id: {id}, type: {memory_type})"
        )))
    }
}

// ── Helpers ────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let end = s
            .char_indices()
            .take_while(|(i, _)| *i <= max)
            .last()
            .map_or(0, |(i, _)| i);
        &s[..end]
    }
}

fn format_memory_results(ranked: &[cortex_types::MemoryEntry]) -> ToolResult {
    if ranked.is_empty() {
        return ToolResult::success("No memories found matching the query.");
    }

    let mut out = String::new();
    for (i, mem) in ranked.iter().enumerate() {
        let _ = writeln!(
            out,
            "{}. [{}] ({:?}/{:?}) {}\n   {}",
            i + 1,
            mem.id,
            mem.memory_type,
            mem.status,
            mem.description,
            truncate(&mem.content, 200),
        );
    }
    ToolResult::success(out)
}
