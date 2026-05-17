use std::sync::atomic::Ordering;

use cortex_types::{MemoryEntry, MemoryKind, MemoryType};

use super::{
    INVALID_PARAMS, MEMORY_NOT_FOUND, MEMORY_OPERATION_FAILED, RpcHandler, RpcRequest, RpcResponse,
    app_error, success,
};

impl RpcHandler {
    pub(super) fn handle_memory_list(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        match self.state.memory_store().list_for_actor(&actor) {
            Ok(entries) => {
                let list: Vec<serde_json::Value> = entries
                    .iter()
                    .map(|e| {
                        serde_json::json!({
                            "id": e.id,
                            "content": e.content,
                            "description": e.description,
                            "memory_type": e.memory_type,
                            "kind": e.kind,
                            "status": e.status,
                            "strength": e.strength,
                            "created_at": e.created_at.to_rfc3339(),
                            "access_count": e.access_count,
                        })
                    })
                    .collect();
                success(req.id.clone(), serde_json::json!({ "memories": list }))
            }
            Err(e) => app_error(
                req.id.clone(),
                MEMORY_OPERATION_FAILED,
                &format!("failed to list memories: {e}"),
                "memory",
                true,
                "check memory store directory permissions",
            ),
        }
    }

    pub(super) fn handle_memory_get(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let id = req
            .params
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if id.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing id parameter",
                "memory",
                true,
                "provide a non-empty 'id' parameter",
            );
        }

        let actor = self.state.transport_actor(client);
        self.state
            .memory_store()
            .load_for_actor(id, &actor)
            .map_or_else(
                |_| {
                    app_error(
                        req.id.clone(),
                        MEMORY_NOT_FOUND,
                        &format!("memory '{id}' not found"),
                        "memory",
                        true,
                        "check the memory id or list available memories",
                    )
                },
                |entry| {
                    success(
                        req.id.clone(),
                        serde_json::to_value(&entry).unwrap_or_default(),
                    )
                },
            )
    }

    pub(super) fn handle_memory_save(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let content = req
            .params
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if content.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing content parameter",
                "memory",
                true,
                "provide a non-empty 'content' parameter",
            );
        }

        let description = req
            .params
            .get("description")
            .and_then(serde_json::Value::as_str)
            .or_else(|| req.params.get("title").and_then(serde_json::Value::as_str))
            .unwrap_or("");

        let memory_type: MemoryType = req
            .params
            .get("memory_type")
            .or_else(|| req.params.get("type"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(MemoryType::User);

        let kind: MemoryKind = req
            .params
            .get("kind")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(MemoryKind::Episodic);

        let mut entry = MemoryEntry::new(content, description, memory_type, kind);
        entry.owner_actor = self.state.transport_actor(client);
        let id = entry.id.clone();

        match self.state.memory_store().save(&entry) {
            Ok(()) => {
                // Signal heartbeat to embed this new memory.
                self.state
                    .heartbeat_state()
                    .pending_embeddings
                    .fetch_add(1, Ordering::Relaxed);
                success(
                    req.id.clone(),
                    serde_json::json!({ "id": id, "status": "saved" }),
                )
            }
            Err(e) => app_error(
                req.id.clone(),
                MEMORY_OPERATION_FAILED,
                &format!("failed to save memory: {e}"),
                "memory",
                true,
                "check memory store directory permissions",
            ),
        }
    }

    pub(super) fn handle_memory_delete(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let id = req
            .params
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if id.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing id parameter",
                "memory",
                true,
                "provide a non-empty 'id' parameter",
            );
        }

        let actor = self.state.transport_actor(client);
        match self.state.memory_store().delete_for_actor(id, &actor) {
            Ok(()) => success(req.id.clone(), serde_json::json!({ "status": "deleted" })),
            Err(_) => app_error(
                req.id.clone(),
                MEMORY_NOT_FOUND,
                &format!("memory '{id}' not found"),
                "memory",
                true,
                "check the memory id or list available memories",
            ),
        }
    }

    pub(super) fn handle_memory_search(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let query = req
            .params
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if query.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing query parameter",
                "memory",
                true,
                "provide a non-empty 'query' parameter",
            );
        }

        let limit = req
            .params
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(10, |v| usize::try_from(v).unwrap_or(10));

        let actor = self.state.transport_actor(client);
        let mut memories = match self.state.memory_store().list_for_actor(&actor) {
            Ok(m) => m,
            Err(e) => {
                return app_error(
                    req.id.clone(),
                    MEMORY_OPERATION_FAILED,
                    &format!("failed to list memories: {e}"),
                    "memory",
                    true,
                    "check memory store directory permissions",
                );
            }
        };

        // Merge memories from shared instance if memory_share is enabled.
        {
            let share = self.state.config().memory_share.clone();
            if matches!(
                share.mode,
                cortex_types::config::MemoryShareMode::Readonly
                    | cortex_types::config::MemoryShareMode::Readwrite
            ) && !share.instance_id.is_empty()
            {
                let shared_mem_dir = self.state.home().parent().map(|base| {
                    cortex_kernel::CortexPaths::new(base, &share.instance_id).memory_dir()
                });
                if let Some(dir) = shared_mem_dir
                    && let Ok(shared_store) = cortex_kernel::MemoryStore::open(&dir)
                    && let Ok(shared) = shared_store.list_for_actor(&actor)
                {
                    memories.extend(shared);
                }
            }
        }

        let ranked = cortex_turn::memory::recall::rank_memories(query, &memories, limit);
        let results: Vec<serde_json::Value> = ranked
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "description": e.description,
                    "content": e.content,
                    "memory_type": e.memory_type,
                    "kind": e.kind,
                    "strength": e.strength,
                })
            })
            .collect();

        success(req.id.clone(), serde_json::json!({ "results": results }))
    }
}
