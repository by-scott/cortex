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
        "Recall from persistent cross-session memory.\n\n\
         Use before starting work to check for prior context: collaborator \
         preferences, past decisions, project conventions, corrections given \
         in earlier sessions. Memories survive across sessions — they are the \
         primary continuity mechanism.\n\n\
         Ranking uses hybrid scoring: text relevance, semantic similarity, \
         recency, reliability (source trust), access frequency, and knowledge \
         graph distance. Natural language queries work best.\n\n\
         Search early and search from multiple angles for important context. \
         A memory that contradicts current observation may be stale — verify \
         before acting on recalled information."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language search query. Be specific for better recall."
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "description": "Maximum number of results to return."
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
        "Persist information to cross-session long-term memory.\n\n\
         Save when information will matter in future sessions: collaborator \
         corrections (highest priority — always save), project decisions, \
         architectural patterns, learned preferences, external references.\n\n\
         Do NOT save: transient task details, information already in files, \
         content derivable from code or git history.\n\n\
         Types:\n\
         - Feedback: Collaborator corrections and preferences. Highest signal.\n\
         - Project: Technical decisions, architecture, goals, conventions.\n\
         - User: Collaborator identity, expertise, communication style.\n\
         - Reference: URLs, documentation pointers, external resources.\n\n\
         Each memory must be self-contained — readable without the original \
         conversation. Write precise descriptions; they drive search recall."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Detailed memory content. Must be self-contained and meaningful without context."
                },
                "description": {
                    "type": "string",
                    "description": "One-line summary for search ranking. Be precise — this drives recall."
                },
                "type": {
                    "type": "string",
                    "enum": ["User", "Feedback", "Project", "Reference"],
                    "description": "Feedback (corrections) > Project (decisions) > User (identity) > Reference (links)."
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

        let entry = cortex_types::MemoryEntry::new(
            content,
            description,
            memory_type,
            cortex_types::MemoryKind::Episodic,
        );
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
        self.save_entry(&entry)
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

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_sdk::{ExecutionScope, InvocationContext, ToolRuntime};
    use cortex_types::{MemoryKind, MemoryType};

    struct DummyRuntime {
        invocation: InvocationContext,
    }

    impl DummyRuntime {
        fn actor(actor: &str) -> Self {
            Self {
                invocation: InvocationContext {
                    tool_name: "memory".into(),
                    session_id: Some("session".into()),
                    actor: Some(actor.into()),
                    source: Some("test".into()),
                    execution_scope: ExecutionScope::Foreground,
                },
            }
        }
    }

    impl ToolRuntime for DummyRuntime {
        fn invocation(&self) -> &InvocationContext {
            &self.invocation
        }

        fn emit_progress(&self, _message: &str) {}

        fn emit_observer(&self, _source: Option<&str>, _content: &str) {}
    }

    fn make_components() -> (tempfile::TempDir, Arc<MemoryRecallComponents>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(MemoryStore::open(dir.path()).unwrap());
        let mut entry = cortex_types::MemoryEntry::new(
            "The user explicitly asked for shorter responses in all contexts.",
            "user prefers concise output style",
            MemoryType::User,
            MemoryKind::Semantic,
        );
        entry.id = "mem-001".into();
        store.save(&entry).unwrap();
        let ctx = Arc::new(MemoryRecallComponents {
            store,
            embedding_client: None,
            embedding_store: None,
            embedding_health: None,
            data_dir: dir.path().to_path_buf(),
            max_recall: 10,
        });
        (dir, ctx)
    }

    #[test]
    fn search_finds_relevant() {
        let (_dir, ctx) = make_components();
        let store = ctx.store.clone();
        // Verify the store has data
        let all = store.list_all().unwrap();
        assert!(!all.is_empty(), "store is empty after save");

        let tool = MemorySearchTool::new(ctx);
        let result = tool
            .execute(serde_json::json!({"query": "user prefers shorter responses"}))
            .unwrap();
        assert!(
            result.output.contains("mem-001"),
            "output was: {}",
            result.output
        );
    }

    #[test]
    fn search_no_results() {
        let (_dir, ctx) = make_components();
        let tool = MemorySearchTool::new(ctx);
        let result = tool
            .execute(serde_json::json!({"query": "zzzznonexistent"}))
            .unwrap();
        assert!(result.output.contains("No memories found"));
    }

    #[test]
    fn save_creates_entry() {
        let (_dir, ctx) = make_components();
        let store = ctx.store.clone();
        let tool = MemorySaveTool::new(store.clone());
        let result = tool
            .execute(serde_json::json!({
                "content": "The deploy uses systemd user services",
                "description": "deploy strategy",
                "type": "Project"
            }))
            .unwrap();
        assert!(!result.is_error, "error: {}", result.output);
        assert!(result.output.contains("Memory saved"));
        // Verify it persisted
        let all = store.list_all().unwrap();
        assert!(
            all.iter().any(|m| m.description == "deploy strategy"),
            "saved memory not found in store"
        );
    }

    #[test]
    fn save_rejects_empty_content() {
        let (_dir, ctx) = make_components();
        let tool = MemorySaveTool::new(ctx.store.clone());
        let result = tool.execute(serde_json::json!({"content": "   "}));
        assert!(result.is_err());
    }

    #[test]
    fn save_defaults_to_user_type() {
        let (_dir, ctx) = make_components();
        let store = ctx.store.clone();
        let tool = MemorySaveTool::new(store);
        let result = tool
            .execute(serde_json::json!({"content": "default type test"}))
            .unwrap();
        assert!(result.output.contains("type: user"));
    }

    #[test]
    fn save_with_runtime_records_owner_actor() {
        let (_dir, ctx) = make_components();
        let tool = MemorySaveTool::new(ctx.store.clone());
        tool.execute_with_runtime(
            serde_json::json!({
                "content": "tenant-owned memory",
                "description": "tenant marker"
            }),
            &DummyRuntime::actor("telegram:42"),
        )
        .unwrap();

        let all = ctx.store.list_all().unwrap();
        let saved = all
            .iter()
            .find(|memory| memory.description == "tenant marker")
            .unwrap();
        assert_eq!(saved.owner_actor, "telegram:42");
    }

    #[test]
    fn search_with_runtime_filters_other_actors_memories() {
        let (_dir, ctx) = make_components();
        let mut own = cortex_types::MemoryEntry::new(
            "secret project codename alpha",
            "own secret",
            MemoryType::Project,
            MemoryKind::Semantic,
        );
        own.id = "own-memory".into();
        own.owner_actor = "telegram:42".into();
        ctx.store.save(&own).unwrap();

        let mut other = cortex_types::MemoryEntry::new(
            "secret project codename beta",
            "other secret",
            MemoryType::Project,
            MemoryKind::Semantic,
        );
        other.id = "other-memory".into();
        other.owner_actor = "telegram:99".into();
        ctx.store.save(&other).unwrap();

        let tool = MemorySearchTool::new(ctx);
        let result = tool
            .execute_with_runtime(
                serde_json::json!({"query": "secret project codename", "limit": 10}),
                &DummyRuntime::actor("telegram:42"),
            )
            .unwrap();

        assert!(result.output.contains("own-memory"), "{}", result.output);
        assert!(!result.output.contains("other-memory"), "{}", result.output);
    }
}
