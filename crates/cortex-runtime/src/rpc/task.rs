use cortex_types::{CorrelationId, Event, Payload, SharedTask, SharedTaskStatus, TurnId};

use super::{
    INVALID_PARAMS, RpcHandler, RpcRequest, RpcResponse, TASK_NOT_FOUND, TASK_OPERATION_FAILED,
    app_error, parse_optional_deadline, success,
};

impl RpcHandler {
    pub(super) fn handle_task_create(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let description = req
            .params
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if description.is_empty() {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "missing description parameter",
                "task",
                true,
                "provide a non-empty 'description' parameter",
            );
        }

        let mut task = SharedTask::new(description);
        task.owner_actor = self.state.transport_actor(client);
        if let Some(parent_task_id) = req
            .params
            .get("parent_task_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            task.parent_task_id = Some(parent_task_id.trim().to_string());
        }
        if let Some(priority) = req
            .params
            .get("priority")
            .and_then(serde_json::Value::as_u64)
        {
            let Ok(priority) = u8::try_from(priority) else {
                return app_error(
                    req.id.clone(),
                    INVALID_PARAMS,
                    "priority must fit in an unsigned byte",
                    "task",
                    true,
                    "use a priority between 0 and 255",
                );
            };
            task.priority = priority;
        }
        match parse_optional_deadline(&req.params) {
            Ok(deadline) => task.deadline = deadline,
            Err(message) => {
                return app_error(
                    req.id.clone(),
                    INVALID_PARAMS,
                    &message,
                    "task",
                    true,
                    "use an RFC3339 deadline such as 2026-04-28T10:00:00Z",
                );
            }
        }

        match self.state.task_store().save(&task) {
            Ok(()) => success(
                req.id.clone(),
                serde_json::json!({ "task": task_to_json(&task) }),
            ),
            Err(err) => app_error(
                req.id.clone(),
                TASK_OPERATION_FAILED,
                &format!("failed to create task: {err}"),
                "task",
                true,
                "check task store permissions",
            ),
        }
    }

    pub(super) fn handle_task_list(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        let tasks = if let Some(status_value) = req.params.get("status") {
            let Some(status) = parse_task_status(status_value) else {
                return app_error(
                    req.id.clone(),
                    INVALID_PARAMS,
                    "invalid task status",
                    "task",
                    true,
                    "use Pending, Assigned, InProgress, Completed, Failed, or Cancelled",
                );
            };
            self.state
                .task_store()
                .list_by_status_for_actor(status, &actor)
        } else {
            self.state.task_store().list_for_actor(&actor)
        };

        match tasks {
            Ok(tasks) => {
                let limit = req
                    .params
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(100);
                let offset = req
                    .params
                    .get("offset")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(0);
                let visible: Vec<serde_json::Value> = tasks
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(task_to_json)
                    .collect();
                success(
                    req.id.clone(),
                    serde_json::json!({ "tasks": visible, "total": tasks.len() }),
                )
            }
            Err(err) => app_error(
                req.id.clone(),
                TASK_OPERATION_FAILED,
                &format!("failed to list tasks: {err}"),
                "task",
                true,
                "check task store permissions",
            ),
        }
    }

    pub(super) fn handle_task_get(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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
                "task",
                true,
                "provide a non-empty 'id' parameter",
            );
        }

        let actor = self.state.transport_actor(client);
        self.state
            .task_store()
            .load_for_actor(id, &actor)
            .map_or_else(
                |_| {
                    app_error(
                        req.id.clone(),
                        TASK_NOT_FOUND,
                        &format!("task '{id}' not found"),
                        "task",
                        true,
                        "check the task id or list visible tasks",
                    )
                },
                |task| {
                    success(
                        req.id.clone(),
                        serde_json::json!({ "task": task_to_json(&task) }),
                    )
                },
            )
    }

    pub(super) fn handle_task_delete(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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
                "task",
                true,
                "provide a non-empty 'id' parameter",
            );
        }

        let actor = self.state.transport_actor(client);
        match self.state.task_store().delete_for_actor(id, &actor) {
            Ok(true) => success(req.id.clone(), serde_json::json!({ "status": "deleted" })),
            Ok(false) | Err(_) => app_error(
                req.id.clone(),
                TASK_NOT_FOUND,
                &format!("task '{id}' not found"),
                "task",
                true,
                "check the task id or list visible tasks",
            ),
        }
    }

    pub(super) fn handle_task_claim(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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
                "task",
                true,
                "provide a non-empty 'id' parameter",
            );
        }
        let instance_id = req
            .params
            .get("instance_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(client);
        let actor = self.state.transport_actor(client);

        match self
            .state
            .task_store()
            .claim_for_actor(id, instance_id, &actor)
        {
            Ok((assignment, payload)) => {
                let journal_warning = self.journal_task_event(payload);
                success(
                    req.id.clone(),
                    serde_json::json!({
                        "assignment": {
                            "task_id": assignment.task_id,
                            "target_instance": assignment.target_instance,
                            "assigned_at": assignment.assigned_at.to_rfc3339(),
                            "deadline": assignment.deadline.map(|deadline| deadline.to_rfc3339()),
                        },
                        "journal_warning": journal_warning,
                    }),
                )
            }
            Err(err) => app_error(
                req.id.clone(),
                TASK_OPERATION_FAILED,
                &format!("failed to claim task: {err}"),
                "task",
                true,
                "check task ownership, status, or instance id",
            ),
        }
    }

    pub(super) fn handle_task_update(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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
                "task",
                true,
                "provide a non-empty 'id' parameter",
            );
        }
        let Some(status) = req.params.get("status").and_then(parse_task_status) else {
            return app_error(
                req.id.clone(),
                INVALID_PARAMS,
                "invalid or missing task status",
                "task",
                true,
                "use Pending, Assigned, InProgress, Completed, Failed, or Cancelled",
            );
        };
        let result = req.params.get("result").and_then(serde_json::Value::as_str);
        let actor = self.state.transport_actor(client);
        match self
            .state
            .task_store()
            .update_status_for_actor(id, &actor, status, result)
        {
            Ok(task) => success(
                req.id.clone(),
                serde_json::json!({ "task": task_to_json(&task) }),
            ),
            Err(err) => app_error(
                req.id.clone(),
                TASK_OPERATION_FAILED,
                &format!("failed to update task: {err}"),
                "task",
                true,
                "check task ownership and the task state transition",
            ),
        }
    }

    fn journal_task_event(&self, payload: Payload) -> Option<String> {
        self.state
            .journal()
            .append(&Event::new(TurnId::new(), CorrelationId::new(), payload))
            .err()
            .map(|err| err.to_string())
    }
}

fn task_to_json(task: &SharedTask) -> serde_json::Value {
    serde_json::json!({
        "id": &task.id,
        "owner_actor": &task.owner_actor,
        "parent_task_id": &task.parent_task_id,
        "description": &task.description,
        "status": task.status,
        "assigned_instance": &task.assigned_instance,
        "priority": task.priority,
        "result": &task.result,
        "created_at": task.created_at.to_rfc3339(),
        "updated_at": task.updated_at.to_rfc3339(),
        "deadline": task.deadline.as_ref().map(chrono::DateTime::to_rfc3339),
    })
}

fn parse_task_status(value: &serde_json::Value) -> Option<SharedTaskStatus> {
    serde_json::from_value(value.clone()).ok().or_else(|| {
        value.as_str().and_then(|status| match status {
            "pending" => Some(SharedTaskStatus::Pending),
            "assigned" => Some(SharedTaskStatus::Assigned),
            "in_progress" | "in-progress" => Some(SharedTaskStatus::InProgress),
            "completed" => Some(SharedTaskStatus::Completed),
            "failed" => Some(SharedTaskStatus::Failed),
            "cancelled" | "canceled" => Some(SharedTaskStatus::Cancelled),
            _ => None,
        })
    })
}
