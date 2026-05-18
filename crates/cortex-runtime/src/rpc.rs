use std::sync::Arc;

use crate::daemon::{CancelTurnError, DaemonState};

mod goal;
mod memory;
mod operator;
mod protocol;
mod skill;
mod task;

use protocol::{
    COMMAND_DISPATCH_FAILED, GOAL_NOT_FOUND, GOAL_OPERATION_FAILED, INVALID_PARAMS,
    INVALID_REQUEST, MEMORY_NOT_FOUND, MEMORY_OPERATION_FAILED, METHOD_NOT_FOUND, OPERATOR_ONLY,
    SESSION_ALREADY_ENDED, SESSION_NOT_FOUND, TASK_NOT_FOUND, TASK_OPERATION_FAILED,
    TURN_EXECUTION_FAILED, app_error,
};
pub use protocol::{
    RpcError, RpcRequest, RpcResponse, error, invalid_request, parse_error, parse_request, success,
};

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
        self.handle_for_client(req, "rpc")
    }

    /// Dispatch a parsed request using the identity bound to the given client transport.
    #[must_use]
    pub fn handle_for_client(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        if req.jsonrpc != "2.0" {
            return error(
                req.id.clone(),
                INVALID_REQUEST,
                "Invalid Request: jsonrpc must be \"2.0\"",
            );
        }
        match req.method.as_str() {
            "session/prompt" => self.handle_session_prompt(req, client),
            "session/new" => self.handle_session_new(req, client),
            "session/list" => self.handle_session_list(req, client),
            "session/end" => self.handle_session_end(req, client),
            "session/initialize" => self.handle_session_initialize(req, client),
            "session/cancel" => self.handle_session_cancel(req, client),
            "command/dispatch" => self.handle_command_dispatch(req, client),
            "admin/reload-config" => self.handle_admin_reload_config(req, client),
            "daemon/status" => self.handle_daemon_status(req, client),
            "operator/dashboard" => self.handle_operator_dashboard(req, client),
            "session/get" => self.handle_session_get(req, client),
            "skill/list" => self.handle_skill_list(req),
            "skill/invoke" => self.handle_skill_invoke(req),
            "skill/suggestions" => self.handle_skill_suggestions(req),
            "memory/list" => self.handle_memory_list(req, client),
            "memory/get" => self.handle_memory_get(req, client),
            "memory/save" => self.handle_memory_save(req, client),
            "memory/delete" => self.handle_memory_delete(req, client),
            "memory/search" => self.handle_memory_search(req, client),
            "task/create" => self.handle_task_create(req, client),
            "task/list" => self.handle_task_list(req, client),
            "task/get" => self.handle_task_get(req, client),
            "task/delete" => self.handle_task_delete(req, client),
            "task/claim" => self.handle_task_claim(req, client),
            "task/update" => self.handle_task_update(req, client),
            "goal/create" => self.handle_goal_create(req, client),
            "goal/list" => self.handle_goal_list(req, client),
            "goal/get" => self.handle_goal_get(req, client),
            "goal/delete" => self.handle_goal_delete(req, client),
            "goal/update" => self.handle_goal_update(req, client),
            "health/check" => self.handle_health_check(req, client),
            "meta/alerts" => self.handle_meta_alerts(req, client),
            m if m.starts_with("mcp/") => self.handle_mcp(req, client),
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

    /// Resolve the session id for a prompt request: use the provided one, or
    /// fall back to the client's remembered session, creating a new one if the
    /// remembered session no longer exists.
    fn resolve_session_id(
        &self,
        req: &RpcRequest,
        client: &str,
    ) -> Result<String, Box<RpcResponse>> {
        let session_id_param = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        // If an explicit session_id is given, reject if session is already ended.
        if let Some(ref sid) = session_id_param {
            if !self.state.transport_can_access_session(client, sid) {
                return Err(Box::new(app_error(
                    req.id.clone(),
                    SESSION_NOT_FOUND,
                    &format!("session '{sid}' not found"),
                    "session",
                    true,
                    "you can only access sessions owned by your configured identity",
                )));
            }
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

        Ok(session_id_param.unwrap_or_else(|| self.state.resolve_client_session(client)))
    }

    fn handle_session_prompt(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let prompt = req
            .params
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let inline_images: Vec<(String, String)> = req
            .params
            .get("images")
            .cloned()
            .and_then(|value| {
                serde_json::from_value::<Vec<cortex_types::web::ImageData>>(value).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|image| (image.media_type, image.data))
            .collect();
        let attachments: Vec<cortex_types::Attachment> = req
            .params
            .get("attachments")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();

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

        let session_id = match self.resolve_session_id(req, client) {
            Ok(id) => id,
            Err(err) => return *err,
        };

        self.execute_rpc_prompt(req, &session_id, prompt, &attachments, &inline_images)
    }

    fn execute_rpc_prompt(
        &self,
        req: &RpcRequest,
        session_id: &str,
        prompt: &str,
        attachments: &[cortex_types::Attachment],
        inline_images: &[(String, String)],
    ) -> RpcResponse {
        let foreground = match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                self.state
                    .acquire_foreground_execution(std::time::Duration::from_secs(30)),
            )
        }) {
            Ok(foreground) => foreground,
            Err(err @ crate::daemon::ForegroundSlotError::ShuttingDown) => {
                return app_error(
                    req.id.clone(),
                    TURN_EXECUTION_FAILED,
                    err.operator_detail(),
                    "turn",
                    false,
                    "Service is shutting down",
                );
            }
            Err(err @ crate::daemon::ForegroundSlotError::Timeout) => {
                return app_error(
                    req.id.clone(),
                    TURN_EXECUTION_FAILED,
                    err.operator_detail(),
                    "turn",
                    true,
                    "Wait for the current turn to finish, then retry",
                );
            }
        };
        let turn_input = crate::turn_executor::TurnInput {
            text: prompt,
            attachments,
            inline_images,
        };
        let tracer = crate::daemon::TracingTurnTracer {
            config: self.state.config().turn.trace.clone(),
        };
        match self.state.execute_foreground_turn_streaming(
            &foreground,
            session_id,
            &turn_input,
            "rpc",
            |_| {},
            &tracer,
        ) {
            Ok(output) => success(
                req.id.clone(),
                serde_json::json!({
                    "session_id": session_id,
                    "response": output.response_text.unwrap_or_default(),
                    "response_format": cortex_types::TextFormat::Markdown,
                    "response_parts": output.response_parts,
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

    fn handle_session_new(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        let (sid, _meta) = self
            .state
            .session_manager()
            .create_session_for_actor(&actor);
        success(
            req.id.clone(),
            serde_json::json!({ "session_id": sid.to_string() }),
        )
    }

    fn handle_session_list(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let sessions = self.state.visible_sessions_for_transport(client);
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

    fn handle_session_end(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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

        if !self.state.transport_can_access_session(client, session_id) {
            return app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                &format!("session '{session_id}' not found"),
                "session",
                true,
                "you can only access sessions owned by your configured identity",
            );
        }

        // Check if session exists in memory or on disk
        let in_memory = self
            .state
            .sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(session_id);
        let sessions_on_disk = self.state.visible_sessions_for_transport(client);
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

    fn handle_command_dispatch(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let command = req
            .params
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let session_id = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str);

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

        if session_id.as_ref().is_some_and(|sid| !sid.is_empty())
            && let Some(err) = Self::validate_session_id_param(req)
        {
            return err;
        }

        if let Some(session_id) = session_id.filter(|sid| !sid.is_empty())
            && !self.state.transport_can_access_session(client, session_id)
        {
            return app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                &format!("session '{session_id}' not found"),
                "session",
                true,
                "you can only access sessions owned by your configured identity",
            );
        }

        let result = self.state.dispatch_command_for_session(session_id, command);
        success(req.id.clone(), serde_json::json!({ "output": result }))
    }

    fn handle_session_initialize(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        let tool_names = self.state.tool_names_for_actor(Some(&actor));
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

    fn handle_mcp(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        match req.method.as_str() {
            "mcp/prompts-list" => self.handle_mcp_prompts_list(req),
            "mcp/prompts-get" => self.handle_mcp_prompts_get(req),
            _ => match self.state.mcp_handle(
                &req.method,
                &req.params,
                &self.state.transport_actor(client),
            ) {
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
        let Some((desc, content)) = registry
            .with_skill(name, |s| {
                if !s.metadata().user_invocable {
                    return None;
                }
                let cortex_turn::skills::SkillContent::Markdown(c) = s.content(args);
                Some((s.description().to_string(), c))
            })
            .flatten()
        else {
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

    fn handle_session_get(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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

        if !self.state.transport_can_access_session(client, session_id) {
            return app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                &format!("session '{session_id}' not found"),
                "session",
                true,
                "you can only access sessions owned by your configured identity",
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
            .visible_sessions_for_transport(client)
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

    fn handle_session_cancel(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        let session_id = req
            .params
            .get("session_id")
            .and_then(serde_json::Value::as_str);
        match self.state.cancel_turn_for_actor(&actor, session_id) {
            Ok(target_session) => success(
                req.id.clone(),
                serde_json::json!({
                    "status": "acknowledged",
                    "message": "Turn cancellation requested",
                    "session_id": target_session,
                }),
            ),
            Err(CancelTurnError::NoActiveTurn) => success(
                req.id.clone(),
                serde_json::json!({
                    "status": "acknowledged",
                    "message": "No active Turn to cancel",
                }),
            ),
            Err(CancelTurnError::SessionNotFound) => app_error(
                req.id.clone(),
                SESSION_NOT_FOUND,
                "session not found",
                "session",
                true,
                "check session_id or use a visible active session",
            ),
        }
    }
}

fn parse_optional_deadline(
    params: &serde_json::Value,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    let Some(deadline) = params
        .get("deadline")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(deadline)
        .map(|value| Some(value.with_timezone(&chrono::Utc)))
        .map_err(|err| format!("invalid deadline: {err}"))
}
