use std::sync::Arc;

use serde::{Deserialize, Serialize};

use cortex_types::{MemoryEntry, MemoryKind, MemoryType};

use crate::daemon::DaemonState;

// ── JSON-RPC 2.0 Types ────────────────────────────────────────

/// A JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC 2.0 error object with optional structured data.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    /// Structured error metadata (category, recoverability, hints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ── JSON-RPC standard error codes ────────────────────────────
const PARSE_ERROR: i32 = -32_700;
const INVALID_PARAMS: i32 = -32_602;
const METHOD_NOT_FOUND: i32 = -32_601;
const INVALID_REQUEST: i32 = -32_600;

// ── Application-level error codes (1000+) ────────────────────
// Session errors (1000-1099)
const SESSION_NOT_FOUND: i32 = 1000;
const SESSION_ALREADY_ENDED: i32 = 1001;

// Turn errors (1100-1199)
const TURN_EXECUTION_FAILED: i32 = 1100;

// Command errors (1200-1299)
const COMMAND_DISPATCH_FAILED: i32 = 1200;

// Memory errors (1300-1399)
const MEMORY_NOT_FOUND: i32 = 1300;
const MEMORY_OPERATION_FAILED: i32 = 1301;

#[must_use]
pub fn success(id: serde_json::Value, result: serde_json::Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: Some(result),
        error: None,
    }
}

#[must_use]
pub fn error(id: serde_json::Value, code: i32, message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: None,
        }),
    }
}

/// Create an application-level error with structured metadata.
#[must_use]
fn app_error(
    id: serde_json::Value,
    code: i32,
    message: &str,
    category: &'static str,
    recoverable: bool,
    hint: &'static str,
) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: Some(serde_json::json!({
                "category": category,
                "recoverable": recoverable,
                "hint": hint,
            })),
        }),
    }
}

#[must_use]
pub fn parse_error() -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: None,
        result: None,
        error: Some(RpcError {
            code: PARSE_ERROR,
            message: "Parse error".into(),
            data: None,
        }),
    }
}

/// Parse a JSON line into an `RpcRequest`, returning a parse error response on failure.
///
/// # Errors
///
/// Returns an `RpcResponse` with error code -32700 if the JSON is malformed.
pub fn parse_request(line: &str) -> Result<RpcRequest, Box<RpcResponse>> {
    serde_json::from_str::<RpcRequest>(line).map_err(|e| {
        Box::new(RpcResponse {
            jsonrpc: "2.0".into(),
            id: None,
            result: None,
            error: Some(RpcError {
                code: PARSE_ERROR,
                message: format!("Parse error: {e}"),
                data: None,
            }),
        })
    })
}

// ── RPC Handler ───────────────────────────────────────────────

/// Handles JSON-RPC requests by dispatching to the appropriate method.
#[derive(Clone)]
pub struct RpcHandler {
    state: Arc<DaemonState>,
}

impl RpcHandler {
    #[must_use]
    pub const fn new(state: Arc<DaemonState>) -> Self {
        Self { state }
    }

    /// Dispatch a parsed request to the appropriate method handler.
    #[must_use]
    pub fn handle(&self, req: &RpcRequest) -> RpcResponse {
        if req.jsonrpc != "2.0" {
            return error(
                req.id.clone(),
                INVALID_REQUEST,
                "Invalid Request: jsonrpc must be \"2.0\"",
            );
        }
        match req.method.as_str() {
            "session/prompt" => self.handle_session_prompt(req),
            "session/new" => self.handle_session_new(req),
            "session/list" => self.handle_session_list(req),
            "session/end" => self.handle_session_end(req),
            "session/initialize" => self.handle_session_initialize(req),
            "session/cancel" => Self::handle_session_cancel(req),
            "command/dispatch" => self.handle_command_dispatch(req),
            "daemon/status" => self.handle_daemon_status(req),
            "session/get" => self.handle_session_get(req),
            "skill/list" => self.handle_skill_list(req),
            "skill/invoke" => self.handle_skill_invoke(req),
            "skill/suggestions" => self.handle_skill_suggestions(req),
            "memory/list" => self.handle_memory_list(req),
            "memory/get" => self.handle_memory_get(req),
            "memory/save" => self.handle_memory_save(req),
            "memory/delete" => self.handle_memory_delete(req),
            "memory/search" => self.handle_memory_search(req),
            "health/check" => self.handle_health_check(req),
            "meta/alerts" => self.handle_meta_alerts(req),
            m if m.starts_with("mcp/") => self.handle_mcp(req),
            _ => error(req.id.clone(), METHOD_NOT_FOUND, "Method not found"),
        }
    }

    /// Validate the `session_id` parameter from a prompt request.
    /// Returns `Some(error)` if validation fails, `None` if valid.
    fn validate_session_id_param(req: &RpcRequest) -> Option<RpcResponse> {
        let sid = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)?;
        if sid.len() > 256 {
            return Some(app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "session_id exceeds 256 characters",
                "session",
                true,
                "provide a shorter session_id",
            ));
        }
        if !sid
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Some(app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "session_id contains invalid characters",
                "session",
                true,
                "use only alphanumeric, hyphen, underscore, or dot characters",
            ));
        }
        None
    }

    /// Resolve the session id for a prompt request: use the provided one,
    /// reuse the most recent active session, or create a new one.
    fn resolve_session_id(&self, req: &RpcRequest) -> Result<String, Box<RpcResponse>> {
        let session_id_param = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        // If an explicit session_id is given, reject if session is already ended.
        if let Some(ref sid) = session_id_param {
            let sessions = self.state.session_manager().list_sessions();
            let is_ended = sessions.iter().any(|s| {
                (s.id.to_string() == *sid || s.name.as_deref() == Some(sid.as_str()))
                    && s.ended_at.is_some()
            });
            if is_ended {
                return Err(Box::new(app_error(
                    req.id.clone(),
                    SESSION_ALREADY_ENDED,
                    &format!("session '{sid}' has already ended"),
                    "session",
                    false,
                    "start a new session or use an active session",
                )));
            }
        }

        Ok(session_id_param.unwrap_or_else(|| {
            let sessions = self.state.session_manager().list_sessions();
            sessions
                .iter()
                .filter(|s| s.ended_at.is_none())
                .max_by_key(|s| s.created_at)
                .map_or_else(
                    || {
                        let (sid, _meta) = self.state.session_manager().create_session();
                        sid.to_string()
                    },
                    |s| s.id.to_string(),
                )
        }))
    }

    fn handle_session_prompt(&self, req: &RpcRequest) -> RpcResponse {
        let prompt = req
            .params
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if prompt.trim().is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing prompt parameter",
                "session",
                true,
                "provide a non-empty 'prompt' parameter",
            );
        }

        if let Some(err) = Self::validate_session_id_param(req) {
            return err;
        }

        let session_id = match self.resolve_session_id(req) {
            Ok(id) => id,
            Err(err) => return *err,
        };

        // Serialize foreground turns (GWT: one task at a time).
        // Use tokio runtime handle to async-wait with timeout instead of
        // immediately rejecting, so ACP-mode callers can queue.
        let _permit = match tokio::runtime::Handle::current().block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.state.turn_semaphore.acquire(),
            )
            .await
        }) {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                return app_error(
                    req.id.clone(),
                    TURN_EXECUTION_FAILED,
                    "semaphore closed — service shutting down",
                    "turn",
                    false,
                    "Service is shutting down",
                );
            }
            Err(_) => {
                return app_error(
                    req.id.clone(),
                    TURN_EXECUTION_FAILED,
                    "another turn is in progress — timed out after 30s",
                    "turn",
                    true,
                    "Wait for the current turn to finish, then retry",
                );
            }
        };
        // Execute turn via DaemonState
        match self.state.execute_turn(&session_id, prompt, "rpc", &[]) {
            Ok(text) => success(
                req.id.clone(),
                serde_json::json!({
                    "session_id": session_id,
                    "response": text,
                }),
            ),
            Err(e) => app_error(
                req.id.clone(),
                TURN_EXECUTION_FAILED,
                &e,
                "turn",
                true,
                "Retry the prompt or start a new session",
            ),
        }
    }

    fn handle_session_new(&self, req: &RpcRequest) -> RpcResponse {
        let (sid, _meta) = self.state.session_manager().create_session();
        success(
            req.id.clone(),
            serde_json::json!({ "session_id": sid.to_string() }),
        )
    }

    fn handle_session_list(&self, req: &RpcRequest) -> RpcResponse {
        let sessions = self.state.session_manager().list_sessions();
        let limit = usize::try_from(
            req.params
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(100),
        )
        .unwrap_or(usize::MAX);
        let offset = usize::try_from(
            req.params
                .get("offset")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        )
        .unwrap_or(usize::MAX);
        let list: Vec<serde_json::Value> = sessions
            .iter()
            .skip(offset)
            .take(limit)
            .map(|s| {
                serde_json::json!({
                    "id": s.id.to_string(),
                    "created_at": s.created_at.to_rfc3339(),
                    "turn_count": s.turn_count,
                })
            })
            .collect();
        success(
            req.id.clone(),
            serde_json::json!({ "sessions": list, "total": sessions.len() }),
        )
    }

    fn handle_session_end(&self, req: &RpcRequest) -> RpcResponse {
        let session_id = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if session_id.is_empty() {
            return app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                "missing session_id parameter",
                "session",
                true,
                "provide a valid 'session_id' parameter",
            );
        }

        // Check if session exists in memory or on disk
        let in_memory = self
            .state
            .sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(session_id);
        let sessions_on_disk = self.state.session_manager().list_sessions();
        let disk_session = sessions_on_disk
            .iter()
            .find(|s| s.id.to_string() == session_id || s.name.as_deref() == Some(session_id));

        if !in_memory && disk_session.is_none() {
            return app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                &format!("session '{session_id}' not found"),
                "session",
                true,
                "check the session_id or list available sessions",
            );
        }

        // Reject if session is already ended
        if !in_memory
            && let Some(s) = disk_session
            && s.ended_at.is_some()
        {
            return app_error(
                req.id.clone(),
                SESSION_ALREADY_ENDED,
                "session already ended",
                "session",
                false,
                "session has already been ended",
            );
        }

        self.state.end_session(session_id);
        success(req.id.clone(), serde_json::json!({ "status": "ended" }))
    }

    fn handle_command_dispatch(&self, req: &RpcRequest) -> RpcResponse {
        let command = req
            .params
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if command.is_empty() {
            return app_error(
                req.id.clone(),
                COMMAND_DISPATCH_FAILED,
                "missing command parameter",
                "command",
                true,
                "provide a non-empty 'command' parameter",
            );
        }

        let result = self.state.dispatch_command(command);
        success(req.id.clone(), serde_json::json!({ "output": result }))
    }

    fn handle_daemon_status(&self, req: &RpcRequest) -> RpcResponse {
        let status = self.state.status();
        success(req.id.clone(), status)
    }

    fn handle_session_initialize(&self, req: &RpcRequest) -> RpcResponse {
        let tool_names = self.state.tool_names();
        success(
            req.id.clone(),
            serde_json::json!({
                "name": "cortex",
                "version": env!("CARGO_PKG_VERSION"),
                "capabilities": {
                    "content_types": ["text"],
                    "tools": tool_names,
                }
            }),
        )
    }

    fn handle_mcp(&self, req: &RpcRequest) -> RpcResponse {
        match req.method.as_str() {
            "mcp/prompts-list" => self.handle_mcp_prompts_list(req),
            "mcp/prompts-get" => self.handle_mcp_prompts_get(req),
            _ => match self.state.mcp_handle(&req.method, &req.params) {
                Ok(result) => success(req.id.clone(), result),
                Err((code, message)) => error(req.id.clone(), code, &message),
            },
        }
    }

    fn handle_mcp_prompts_list(&self, req: &RpcRequest) -> RpcResponse {
        let registry = self.state.skill_registry();
        let summaries = registry.user_invocable();
        let prompts: Vec<serde_json::Value> = summaries
            .iter()
            .filter_map(|s| {
                registry.with_skill(&s.name, |skill| {
                    let params: Vec<serde_json::Value> = skill
                        .parameters()
                        .iter()
                        .map(|p| {
                            serde_json::json!({
                                "name": p.name,
                                "description": p.description,
                                "required": p.required,
                            })
                        })
                        .collect();
                    serde_json::json!({
                        "name": skill.name(),
                        "description": skill.description(),
                        "arguments": params,
                    })
                })
            })
            .collect();
        success(req.id.clone(), serde_json::json!({ "prompts": prompts }))
    }

    fn handle_mcp_prompts_get(&self, req: &RpcRequest) -> RpcResponse {
        let name = req
            .params
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            return error(req.id.clone(), INVALID_PARAMS, "missing 'name' parameter");
        }
        let registry = self.state.skill_registry();
        let args = req
            .params
            .get("arguments")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let Some((desc, content)) = registry.with_skill(name, |s| {
            let cortex_turn::skills::SkillContent::Markdown(c) = s.content(args);
            (s.description().to_string(), c)
        }) else {
            return error(
                req.id.clone(),
                METHOD_NOT_FOUND,
                &format!("prompt '{name}' not found"),
            );
        };
        success(
            req.id.clone(),
            serde_json::json!({
                "description": desc,
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": content }
                }]
            }),
        )
    }

    fn handle_skill_list(&self, req: &RpcRequest) -> RpcResponse {
        let registry = self.state.skill_registry();
        let skills: Vec<serde_json::Value> = registry
            .names()
            .iter()
            .filter_map(|name| {
                registry.with_skill(name, |s| {
                    serde_json::json!({
                        "name": s.name(),
                        "description": s.description(),
                        "user_invocable": s.metadata().user_invocable,
                        "agent_invocable": s.metadata().agent_invocable,
                        "execution_mode": format!("{:?}", s.execution_mode()),
                    })
                })
            })
            .collect();
        success(req.id.clone(), serde_json::json!({ "skills": skills }))
    }

    fn handle_skill_invoke(&self, req: &RpcRequest) -> RpcResponse {
        let name = req
            .params
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            return error(req.id.clone(), INVALID_PARAMS, "missing 'name' parameter");
        }
        let registry = self.state.skill_registry();
        let args = req
            .params
            .get("args")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let Some(content) = registry.with_skill(name, |s| {
            let cortex_turn::skills::SkillContent::Markdown(c) = s.content(args);
            c
        }) else {
            return error(
                req.id.clone(),
                METHOD_NOT_FOUND,
                &format!("skill '{name}' not found"),
            );
        };
        success(
            req.id.clone(),
            serde_json::json!({
                "name": name,
                "content": content,
            }),
        )
    }

    fn handle_skill_suggestions(&self, req: &RpcRequest) -> RpcResponse {
        let input = req
            .params
            .get("input")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let registry = self.state.skill_registry();

        let mut suggestions: Vec<serde_json::Value> = registry
            .suggest_skills()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "tool_sequence": s.tool_sequence,
                    "frequency": s.frequency,
                })
            })
            .collect();

        if !input.is_empty() {
            let expanded = expand_keywords_with_synonyms(input);
            append_keyword_matches(registry, &expanded, &mut suggestions);
        }

        success(
            req.id.clone(),
            serde_json::json!({ "suggestions": suggestions }),
        )
    }

    fn handle_session_get(&self, req: &RpcRequest) -> RpcResponse {
        let session_id = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if session_id.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing session_id parameter",
                "session",
                true,
                "provide a valid 'session_id' parameter",
            );
        }

        // Try in-memory first (active sessions have full state).
        let in_memory = {
            let sessions = self
                .state
                .sessions()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            sessions.get(session_id).map(|session| {
                serde_json::json!({
                    "session_id": session.meta.id.to_string(),
                    "created_at": session.meta.created_at.to_rfc3339(),
                    "turn_count": session.turn_count,
                    "history_len": session.history.len(),
                })
            })
        };

        if let Some(data) = in_memory {
            return success(req.id.clone(), data);
        }

        // Fall back to persisted store (inactive/historical sessions).
        let persisted = self
            .state
            .session_manager()
            .list_sessions()
            .into_iter()
            .find(|s| s.id.to_string() == session_id)
            .map(|s| {
                serde_json::json!({
                    "session_id": s.id.to_string(),
                    "created_at": s.created_at.to_rfc3339(),
                    "turn_count": s.turn_count,
                })
            });

        persisted.map_or_else(
            || {
                app_error(
                    req.id.clone(),
                    SESSION_NOT_FOUND,
                    &format!("session '{session_id}' not found"),
                    "session",
                    true,
                    "check session_id or create a new session",
                )
            },
            |data| success(req.id.clone(), data),
        )
    }

    fn handle_memory_list(&self, req: &RpcRequest) -> RpcResponse {
        match self.state.memory_store().list_all() {
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

    fn handle_memory_get(&self, req: &RpcRequest) -> RpcResponse {
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

        self.state.memory_store().load(id).map_or_else(
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

    fn handle_memory_save(&self, req: &RpcRequest) -> RpcResponse {
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

        let entry = MemoryEntry::new(content, description, memory_type, kind);
        let id = entry.id.clone();

        match self.state.memory_store().save(&entry) {
            Ok(()) => {
                // Signal heartbeat to embed this new memory.
                self.state
                    .heartbeat_state()
                    .pending_embeddings
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

    fn handle_memory_delete(&self, req: &RpcRequest) -> RpcResponse {
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

        match self.state.memory_store().delete(id) {
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

    fn handle_memory_search(&self, req: &RpcRequest) -> RpcResponse {
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

        let mut memories = match self.state.memory_store().list_all() {
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
                let shared_mem_dir = self
                    .state
                    .home()
                    .parent()
                    .map(|base| base.join(&share.instance_id).join("memory"));
                if let Some(dir) = shared_mem_dir
                    && let Ok(shared_store) = cortex_kernel::MemoryStore::open(&dir)
                    && let Ok(shared) = shared_store.list_all()
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

    fn handle_health_check(&self, req: &RpcRequest) -> RpcResponse {
        let uptime_secs = chrono::Utc::now()
            .signed_duration_since(self.state.start_time())
            .num_seconds();
        let session_count = self
            .state
            .sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let journal_event_count = self.state.journal().event_count().unwrap_or(0);

        success(
            req.id.clone(),
            serde_json::json!({
                "status": "ok",
                "uptime_secs": uptime_secs,
                "session_count": session_count,
                "journal_event_count": journal_event_count,
            }),
        )
    }

    fn handle_meta_alerts(&self, req: &RpcRequest) -> RpcResponse {
        let session_id = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if session_id.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing session_id parameter",
                "session",
                true,
                "provide a valid 'session_id' parameter",
            );
        }

        // Alerts live in memory only (MetaMonitor is not persisted).
        // For active sessions, return live alerts; for inactive/historical
        // sessions that exist in the persisted store, return empty alerts.
        let alert_list = self
            .state
            .sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .map(|session| {
                session
                    .monitor
                    .check()
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "kind": format!("{:?}", a.kind),
                            "message": a.message,
                        })
                    })
                    .collect::<Vec<serde_json::Value>>()
            });

        if let Some(list) = alert_list {
            return success(req.id.clone(), serde_json::json!({ "alerts": list }));
        }

        // Session not in memory — check if it exists in persisted store.
        let exists_on_disk = self
            .state
            .session_manager()
            .list_sessions()
            .iter()
            .any(|s| s.id.to_string() == session_id);

        if exists_on_disk {
            // Historical session — no live alerts available.
            success(req.id.clone(), serde_json::json!({ "alerts": [] }))
        } else {
            app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                &format!("session '{session_id}' not found"),
                "session",
                true,
                "check session_id or create a new session",
            )
        }
    }

    fn handle_session_cancel(req: &RpcRequest) -> RpcResponse {
        success(
            req.id.clone(),
            serde_json::json!({
                "status": "acknowledged",
                "message": "No active Turn to cancel",
            }),
        )
    }
}

/// Expand input keywords with synonym groups for skill matching.
fn expand_keywords_with_synonyms(input: &str) -> Vec<String> {
    let input_lower = input.to_lowercase();
    let keywords: Vec<&str> = input_lower.split_whitespace().collect();

    let synonym_groups: &[&[&str]] = &[
        &[
            "debug",
            "debugging",
            "crash",
            "bug",
            "broken",
            "failing",
            "error",
            "fix",
        ],
        &[
            "plan",
            "planning",
            "decompose",
            "breakdown",
            "organize",
            "structure",
        ],
        &["review", "examine", "inspect", "audit", "check", "scrutiny"],
        &[
            "orient",
            "understand",
            "explore",
            "unfamiliar",
            "new",
            "codebase",
        ],
        &[
            "decide",
            "deliberate",
            "evaluate",
            "compare",
            "choose",
            "tradeoff",
        ],
        &["diagnose", "root cause", "trace", "symptom", "investigate"],
    ];

    let mut expanded: Vec<String> = keywords.iter().map(|s| (*s).to_string()).collect();
    for kw in &keywords {
        for group in synonym_groups {
            if group.contains(kw) {
                for syn in *group {
                    let s = (*syn).to_string();
                    if !expanded.contains(&s) {
                        expanded.push(s);
                    }
                }
            }
        }
    }
    expanded
}

/// Append keyword-matched skills to the suggestions list.
fn append_keyword_matches(
    registry: &cortex_turn::skills::SkillRegistry,
    expanded: &[String],
    suggestions: &mut Vec<serde_json::Value>,
) {
    registry.with_all_skills(|skills| {
        for skill in skills {
            let desc_lower = skill.description().to_lowercase();
            let when_lower = skill.when_to_use().to_lowercase();
            let name = skill.name();
            if suggestions
                .iter()
                .any(|s| s.get("name").and_then(|v| v.as_str()) == Some(name))
            {
                continue;
            }
            let haystack = format!("{desc_lower} {when_lower} {name}");
            let hits = expanded
                .iter()
                .filter(|kw| kw.len() >= 3 && haystack.contains(kw.as_str()))
                .count();
            if hits >= 1 {
                suggestions.push(serde_json::json!({
                    "name": name,
                    "description": skill.description(),
                    "match_type": "keyword",
                }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_request() {
        let json = r#"{"jsonrpc":"2.0","method":"daemon/status","id":1}"#;
        let req = parse_request(json).unwrap();
        assert_eq!(req.method, "daemon/status");
        assert_eq!(req.id, serde_json::json!(1));
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        let result = parse_request("{broken");
        assert!(result.is_err());
        let resp = *result.unwrap_err();
        assert_eq!(resp.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn success_response_serializes() {
        let resp = success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
    }

    #[test]
    fn error_response_serializes() {
        let resp = error(serde_json::json!(2), METHOD_NOT_FOUND, "not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32601"));
    }

    #[test]
    fn parse_error_has_null_id() {
        let resp = parse_error();
        assert!(resp.id.is_none());
        assert_eq!(resp.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn app_error_includes_structured_data() {
        let resp = app_error(
            serde_json::json!(3),
            TURN_EXECUTION_FAILED,
            "turn failed",
            "turn",
            true,
            "retry",
        );
        let err = resp.error.unwrap();
        assert_eq!(err.code, TURN_EXECUTION_FAILED);
        let data = err.data.unwrap();
        assert_eq!(data["category"], "turn");
        assert_eq!(data["recoverable"], true);
        assert_eq!(data["hint"], "retry");
    }

    #[test]
    fn app_error_codes_in_expected_ranges() {
        assert!((1000..1100).contains(&SESSION_NOT_FOUND));
        assert!((1100..1200).contains(&TURN_EXECUTION_FAILED));
        assert!((1200..1300).contains(&COMMAND_DISPATCH_FAILED));
        assert!((1300..1400).contains(&MEMORY_NOT_FOUND));
        assert!((1300..1400).contains(&MEMORY_OPERATION_FAILED));
    }

    #[test]
    fn standard_error_has_no_data() {
        let resp = error(serde_json::json!(4), METHOD_NOT_FOUND, "not found");
        let err = resp.error.unwrap();
        assert!(err.data.is_none());
    }

    #[test]
    fn app_error_serializes_with_data() {
        let resp = app_error(
            serde_json::json!(5),
            SESSION_NOT_FOUND,
            "session not found",
            "session",
            true,
            "create a new session",
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"recoverable\":true"));
        assert!(json.contains("\"category\":\"session\""));
    }

    #[test]
    fn all_known_methods_parse() {
        let methods = [
            "session/prompt",
            "session/new",
            "session/list",
            "session/end",
            "session/initialize",
            "session/cancel",
            "session/get",
            "command/dispatch",
            "daemon/status",
            "skill/list",
            "skill/invoke",
            "skill/suggestions",
            "memory/list",
            "memory/get",
            "memory/save",
            "memory/delete",
            "memory/search",
            "health/check",
            "meta/alerts",
            "mcp/prompts-list",
            "mcp/prompts-get",
        ];
        for method in methods {
            let json = format!(r#"{{"jsonrpc":"2.0","method":"{method}","id":1,"params":{{}}}}"#);
            let req = parse_request(&json).unwrap_or_else(|_| panic!("failed to parse {method}"));
            assert_eq!(req.method, method);
        }
    }

    #[test]
    fn request_with_params_parses() {
        let json = r#"{"jsonrpc":"2.0","method":"session/prompt","id":1,"params":{"session_id":"abc","input":"hello"}}"#;
        let req = parse_request(json).unwrap();
        assert_eq!(req.params["session_id"], "abc");
        assert_eq!(req.params["input"], "hello");
    }

    #[test]
    fn request_without_params_defaults_to_null() {
        let json = r#"{"jsonrpc":"2.0","method":"daemon/status","id":1}"#;
        let req = parse_request(json).unwrap();
        assert!(req.params.is_null());
    }
}
