use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::MemoryStore;
use cortex_sdk::{ExecutionScope, InvocationContext, Tool, ToolRuntime};
use cortex_turn::tools::memory_tools::{MemoryRecallComponents, MemorySaveTool, MemorySearchTool};
use cortex_types::{MemoryEntry, MemoryKind, MemoryType};

struct TestRuntime {
    invocation: InvocationContext,
}

impl ToolRuntime for TestRuntime {
    fn invocation(&self) -> &InvocationContext {
        &self.invocation
    }

    fn emit_progress(&self, _message: &str) {}

    fn emit_observer(&self, _source: Option<&str>, _content: &str) {}
}

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn build_tool() -> (tempfile::TempDir, MemorySearchTool, Arc<MemoryStore>) {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let base = temp.path().join("cortex-home");
    let home = base.join("default");
    let memory_dir = home.join("memory");
    let data_dir = home.join("data");

    must(
        std::fs::create_dir_all(&memory_dir),
        "memory dir should initialize",
    );
    must(
        std::fs::create_dir_all(&data_dir),
        "data dir should initialize",
    );

    let store = Arc::new(must(
        MemoryStore::open(&memory_dir),
        "memory store should initialize",
    ));
    let tool = MemorySearchTool::new(Arc::new(MemoryRecallComponents {
        store: Arc::clone(&store),
        embedding_client: None,
        embedding_store: None,
        embedding_health: None,
        data_dir: PathBuf::from(&data_dir),
        max_recall: 10,
    }));

    (temp, tool, store)
}

fn build_save_tool() -> (tempfile::TempDir, MemorySaveTool, Arc<MemoryStore>) {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let base = temp.path().join("cortex-home");
    let home = base.join("default");
    let memory_dir = home.join("memory");

    must(
        std::fs::create_dir_all(&memory_dir),
        "memory dir should initialize",
    );

    let store = Arc::new(must(
        MemoryStore::open(&memory_dir),
        "memory store should initialize",
    ));
    let tool = MemorySaveTool::new(Arc::clone(&store));

    (temp, tool, store)
}

fn save_owned_memory(
    store: &MemoryStore,
    owner_actor: &str,
    description: &str,
    content: &str,
) -> String {
    let mut entry = MemoryEntry::new(
        content,
        description,
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    entry.owner_actor = owner_actor.to_string();
    let id = entry.id.clone();
    must(store.save(&entry), "memory should save");
    id
}

#[test]
fn memory_search_without_actor_sees_all_visible_memories() {
    let (_temp, tool, store) = build_tool();
    let scott_id = save_owned_memory(
        &store,
        "user:scott",
        "shared architecture note",
        "The architecture note mentions cortex runtime boundaries.",
    );
    let bob_id = save_owned_memory(
        &store,
        "user:bob",
        "shared architecture note",
        "The architecture note mentions cortex runtime boundaries too.",
    );

    let result = must(
        tool.execute(serde_json::json!({
            "query": "architecture note",
            "limit": 10
        })),
        "memory search should succeed",
    );

    assert!(result.output.contains(&scott_id));
    assert!(result.output.contains(&bob_id));
}

#[test]
fn memory_search_with_actor_only_returns_actor_visible_memories() {
    let (_temp, tool, store) = build_tool();
    let scott_id = save_owned_memory(
        &store,
        "user:scott",
        "release checklist",
        "Scott-specific release checklist and runtime notes.",
    );
    let bob_id = save_owned_memory(
        &store,
        "user:bob",
        "release checklist",
        "Bob-specific release checklist and runtime notes.",
    );

    let runtime = TestRuntime {
        invocation: InvocationContext {
            tool_name: "memory_search".to_string(),
            session_id: Some("session-1".to_string()),
            actor: Some("user:scott".to_string()),
            source: Some("telegram".to_string()),
            execution_scope: ExecutionScope::Foreground,
        },
    };

    let result = must(
        tool.execute_with_runtime(
            serde_json::json!({
                "query": "release checklist",
                "limit": 10
            }),
            &runtime,
        ),
        "actor-scoped memory search should succeed",
    );

    assert!(result.output.contains(&scott_id));
    assert!(!result.output.contains(&bob_id));
}

#[test]
fn memory_save_with_actor_assigns_owner_actor() {
    let (_temp, tool, store) = build_save_tool();
    let runtime = TestRuntime {
        invocation: InvocationContext {
            tool_name: "memory_save".to_string(),
            session_id: Some("session-2".to_string()),
            actor: Some("user:scott".to_string()),
            source: Some("qq".to_string()),
            execution_scope: ExecutionScope::Foreground,
        },
    };

    must(
        tool.execute_with_runtime(
            serde_json::json!({
                "content": "Scott-specific preference about release reporting.",
                "description": "release reporting preference",
                "type": "Feedback"
            }),
            &runtime,
        ),
        "actor-scoped memory save should succeed",
    );

    let scott_memories = must(
        store.list_for_actor("user:scott"),
        "scott memories should load",
    );
    let bob_memories = must(store.list_for_actor("user:bob"), "bob memories should load");

    assert_eq!(scott_memories.len(), 1);
    assert!(bob_memories.is_empty());
    assert_eq!(scott_memories[0].owner_actor, "user:scott");
    assert_eq!(
        scott_memories[0].description,
        "release reporting preference"
    );
}

#[test]
fn memory_save_without_actor_uses_default_local_owner() {
    let (_temp, tool, store) = build_save_tool();

    must(
        tool.execute(serde_json::json!({
            "content": "Global release checklist that is not tied to a specific actor.",
            "description": "global release checklist",
            "type": "Project"
        })),
        "memory save without runtime actor should succeed",
    );

    let all_memories = must(store.list_all(), "all memories should load");
    assert_eq!(all_memories.len(), 1);
    assert_eq!(all_memories[0].owner_actor, "local:default");
    assert_eq!(all_memories[0].description, "global release checklist");
}

#[test]
fn actor_scoped_memory_save_and_search_form_an_isolated_tool_surface() {
    let (_temp, search_tool, store) = build_tool();
    let save_tool = MemorySaveTool::new(Arc::clone(&store));
    let scott_runtime = TestRuntime {
        invocation: InvocationContext {
            tool_name: "memory_save".to_string(),
            session_id: Some("session-3".to_string()),
            actor: Some("user:scott".to_string()),
            source: Some("telegram".to_string()),
            execution_scope: ExecutionScope::Foreground,
        },
    };
    let bob_runtime = TestRuntime {
        invocation: InvocationContext {
            tool_name: "memory_save".to_string(),
            session_id: Some("session-4".to_string()),
            actor: Some("user:bob".to_string()),
            source: Some("qq".to_string()),
            execution_scope: ExecutionScope::Foreground,
        },
    };

    must(
        save_tool.execute_with_runtime(
            serde_json::json!({
                "content": "Scott-only migration note for the local runtime.",
                "description": "migration note",
                "type": "Project"
            }),
            &scott_runtime,
        ),
        "scott memory save should succeed",
    );
    must(
        save_tool.execute_with_runtime(
            serde_json::json!({
                "content": "Bob-only migration note for the local runtime.",
                "description": "migration note",
                "type": "Project"
            }),
            &bob_runtime,
        ),
        "bob memory save should succeed",
    );

    let search_runtime = TestRuntime {
        invocation: InvocationContext {
            tool_name: "memory_search".to_string(),
            session_id: Some("session-5".to_string()),
            actor: Some("user:scott".to_string()),
            source: Some("telegram".to_string()),
            execution_scope: ExecutionScope::Foreground,
        },
    };
    let result = must(
        search_tool.execute_with_runtime(
            serde_json::json!({
                "query": "migration note",
                "limit": 10
            }),
            &search_runtime,
        ),
        "scott memory search should succeed",
    );

    assert!(result.output.contains("Scott-only migration note"));
    assert!(!result.output.contains("Bob-only migration note"));
}
