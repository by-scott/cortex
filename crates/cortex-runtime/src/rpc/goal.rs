use cortex_types::{
    CorrelationId, Event, Goal, GoalLevel, GoalSource, GoalStatus, Payload, TurnId,
};

use super::{
    GOAL_NOT_FOUND, GOAL_OPERATION_FAILED, INVALID_PARAMS, RpcHandler, RpcRequest, RpcResponse,
    app_error, parse_optional_deadline, success,
};

impl RpcHandler {
    pub(super) fn handle_goal_create(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let description = req
            .params
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if description.is_empty() {
            return goal_params_error(req, "missing description parameter");
        }
        let Some(level) = parse_goal_level_or_default(req, GoalLevel::Immediate) else {
            return goal_params_error(req, "invalid goal level");
        };
        let Some(source) = parse_goal_source_or_default(req, GoalSource::User) else {
            return goal_params_error(req, "invalid goal source");
        };
        let Some(status) = parse_goal_status_or_default(req, GoalStatus::Active) else {
            return goal_params_error(req, "invalid goal status");
        };

        let mut goal = Goal::new(description, level);
        goal.owner_actor = self.state.transport_actor(client);
        goal.source = source;
        goal.status = status;
        if status.is_terminal() {
            goal.completed_at = Some(goal.updated_at);
        }
        if let Some(err) = apply_goal_params(req, &mut goal) {
            return err;
        }

        match self.state.goal_store().save(&goal) {
            Ok(()) => {
                let journal_warning = self.journal_goal_event(Payload::GoalSet {
                    level: goal.level.to_string(),
                    description: goal.description.clone(),
                });
                success(
                    req.id.clone(),
                    serde_json::json!({
                        "goal": goal_to_json(&goal),
                        "journal_warning": journal_warning,
                    }),
                )
            }
            Err(err) => goal_store_error(req, &format!("failed to create goal: {err}")),
        }
    }

    pub(super) fn handle_goal_list(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let actor = self.state.transport_actor(client);
        let goals = if let Some(status_value) = req.params.get("status") {
            let Some(status) = parse_goal_status(status_value) else {
                return goal_params_error(req, "invalid goal status");
            };
            self.state
                .goal_store()
                .list_by_status_for_actor(status, &actor)
        } else if req
            .params
            .get("open_only")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            self.state.goal_store().list_open_for_actor(&actor)
        } else {
            self.state.goal_store().list_for_actor(&actor)
        };

        match goals {
            Ok(goals) => {
                let limit = parse_limit(&req.params, 100);
                let offset = parse_offset(&req.params);
                let visible: Vec<serde_json::Value> = goals
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .map(goal_to_json)
                    .collect();
                success(
                    req.id.clone(),
                    serde_json::json!({ "goals": visible, "total": goals.len() }),
                )
            }
            Err(err) => goal_store_error(req, &format!("failed to list goals: {err}")),
        }
    }

    pub(super) fn handle_goal_get(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let id = req
            .params
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if id.is_empty() {
            return goal_params_error(req, "missing id parameter");
        }

        let actor = self.state.transport_actor(client);
        self.state
            .goal_store()
            .load_for_actor(id, &actor)
            .map_or_else(
                |_| goal_not_found(req, id),
                |goal| {
                    success(
                        req.id.clone(),
                        serde_json::json!({ "goal": goal_to_json(&goal) }),
                    )
                },
            )
    }

    pub(super) fn handle_goal_delete(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let id = req
            .params
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if id.is_empty() {
            return goal_params_error(req, "missing id parameter");
        }

        let actor = self.state.transport_actor(client);
        match self.state.goal_store().delete_for_actor(id, &actor) {
            Ok(true) => success(req.id.clone(), serde_json::json!({ "status": "deleted" })),
            Ok(false) | Err(_) => goal_not_found(req, id),
        }
    }

    pub(super) fn handle_goal_update(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        let id = req
            .params
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if id.is_empty() {
            return goal_params_error(req, "missing id parameter");
        }
        let actor = self.state.transport_actor(client);
        let Ok(mut goal) = self.state.goal_store().load_for_actor(id, &actor) else {
            return goal_not_found(req, id);
        };
        let before = goal.clone();
        if let Some(err) = apply_goal_params(req, &mut goal) {
            return err;
        }
        if let Some(status_value) = req.params.get("status") {
            let Some(status) = parse_goal_status(status_value) else {
                return goal_params_error(req, "invalid goal status");
            };
            if goal.status != status
                && let Err(err) = goal.status.try_transition(status)
            {
                return goal_store_error(req, &err.to_string());
            }
            goal.status = status;
        }
        goal.updated_at = chrono::Utc::now();
        goal.completed_at = if goal.status.is_terminal() {
            Some(goal.updated_at)
        } else {
            None
        };

        match self.state.goal_store().save(&goal) {
            Ok(()) => {
                let journal_warning = self.journal_goal_update(&before, &goal);
                success(
                    req.id.clone(),
                    serde_json::json!({
                        "goal": goal_to_json(&goal),
                        "journal_warning": journal_warning,
                    }),
                )
            }
            Err(err) => goal_store_error(req, &format!("failed to update goal: {err}")),
        }
    }

    fn journal_goal_event(&self, payload: Payload) -> Option<String> {
        self.state
            .journal()
            .append(&Event::new(TurnId::new(), CorrelationId::new(), payload))
            .err()
            .map(|err| err.to_string())
    }

    fn journal_goal_update(&self, before: &Goal, after: &Goal) -> Option<String> {
        if before.status != after.status && after.status == GoalStatus::Completed {
            self.journal_goal_event(Payload::GoalCompleted {
                level: after.level.to_string(),
                description: after.description.clone(),
            })
        } else if before.status != after.status
            || before.description != after.description
            || before.level != after.level
        {
            self.journal_goal_event(Payload::GoalShifted {
                from: before.context_line(),
                to: after.context_line(),
            })
        } else {
            None
        }
    }
}

fn goal_to_json(goal: &Goal) -> serde_json::Value {
    serde_json::json!({
        "id": &goal.id,
        "owner_actor": &goal.owner_actor,
        "parent_goal_id": &goal.parent_goal_id,
        "linked_task_id": &goal.linked_task_id,
        "level": goal.level,
        "description": &goal.description,
        "success_criteria": &goal.success_criteria,
        "source": goal.source,
        "status": goal.status,
        "priority": goal.priority,
        "evidence_refs": &goal.evidence_refs,
        "memory_refs": &goal.memory_refs,
        "created_at": goal.created_at.to_rfc3339(),
        "updated_at": goal.updated_at.to_rfc3339(),
        "deadline": goal.deadline.as_ref().map(chrono::DateTime::to_rfc3339),
        "completed_at": goal.completed_at.as_ref().map(chrono::DateTime::to_rfc3339),
    })
}

fn parse_goal_level(value: &serde_json::Value) -> Option<GoalLevel> {
    serde_json::from_value(value.clone()).ok().or_else(|| {
        value.as_str().and_then(|level| match level {
            "strategic" => Some(GoalLevel::Strategic),
            "tactical" => Some(GoalLevel::Tactical),
            "immediate" => Some(GoalLevel::Immediate),
            _ => None,
        })
    })
}

fn parse_goal_status(value: &serde_json::Value) -> Option<GoalStatus> {
    serde_json::from_value(value.clone()).ok().or_else(|| {
        value.as_str().and_then(|status| match status {
            "proposed" => Some(GoalStatus::Proposed),
            "active" => Some(GoalStatus::Active),
            "blocked" => Some(GoalStatus::Blocked),
            "completed" => Some(GoalStatus::Completed),
            "abandoned" => Some(GoalStatus::Abandoned),
            _ => None,
        })
    })
}

fn parse_goal_source(value: &serde_json::Value) -> Option<GoalSource> {
    serde_json::from_value(value.clone()).ok().or_else(|| {
        value.as_str().and_then(|source| match source {
            "user" => Some(GoalSource::User),
            "operator" => Some(GoalSource::Operator),
            "runtime" => Some(GoalSource::Runtime),
            "memory" => Some(GoalSource::Memory),
            "imported" => Some(GoalSource::Imported),
            _ => None,
        })
    })
}

fn parse_goal_level_or_default(req: &RpcRequest, default: GoalLevel) -> Option<GoalLevel> {
    req.params
        .get("level")
        .map_or(Some(default), parse_goal_level)
}

fn parse_goal_status_or_default(req: &RpcRequest, default: GoalStatus) -> Option<GoalStatus> {
    req.params
        .get("status")
        .map_or(Some(default), parse_goal_status)
}

fn parse_goal_source_or_default(req: &RpcRequest, default: GoalSource) -> Option<GoalSource> {
    req.params
        .get("source")
        .map_or(Some(default), parse_goal_source)
}

fn apply_goal_params(req: &RpcRequest, goal: &mut Goal) -> Option<RpcResponse> {
    if let Some(level_value) = req.params.get("level") {
        let Some(level) = parse_goal_level(level_value) else {
            return Some(goal_params_error(req, "invalid goal level"));
        };
        goal.level = level;
    }
    if let Some(description) = req
        .params
        .get("description")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
    {
        if description.is_empty() {
            return Some(goal_params_error(req, "description cannot be empty"));
        }
        goal.description = description.to_string();
    }
    if let Some(value) = optional_string_param(&req.params, "success_criteria") {
        goal.success_criteria = value;
    }
    if let Some(value) = optional_string_param(&req.params, "parent_goal_id") {
        goal.parent_goal_id = Some(value);
    }
    if let Some(value) = optional_string_param(&req.params, "linked_task_id") {
        goal.linked_task_id = Some(value);
    }
    if let Some(source_value) = req.params.get("source") {
        let Some(source) = parse_goal_source(source_value) else {
            return Some(goal_params_error(req, "invalid goal source"));
        };
        goal.source = source;
    }
    if let Some(response) = apply_goal_priority(req, goal) {
        return Some(response);
    }
    match parse_optional_deadline(&req.params) {
        Ok(deadline) => goal.deadline = deadline,
        Err(message) => return Some(goal_deadline_error(req, &message)),
    }
    if req.params.get("evidence_refs").is_some() {
        let Some(refs) = parse_string_array_param(&req.params, "evidence_refs") else {
            return Some(goal_params_error(req, "evidence_refs must be an array"));
        };
        goal.evidence_refs = refs;
    }
    if req.params.get("memory_refs").is_some() {
        let Some(refs) = parse_string_array_param(&req.params, "memory_refs") else {
            return Some(goal_params_error(req, "memory_refs must be an array"));
        };
        goal.memory_refs = refs;
    }
    None
}

fn apply_goal_priority(req: &RpcRequest, goal: &mut Goal) -> Option<RpcResponse> {
    let priority_value = req.params.get("priority")?;
    let Some(priority) = priority_value.as_u64() else {
        return Some(goal_params_error(
            req,
            "priority must be an unsigned integer",
        ));
    };
    let Ok(priority) = u8::try_from(priority) else {
        return Some(goal_params_error(
            req,
            "priority must fit in an unsigned byte",
        ));
    };
    goal.priority = priority;
    None
}

fn optional_string_param(params: &serde_json::Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn parse_string_array_param(params: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    params
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
}

fn parse_limit(params: &serde_json::Value, default: usize) -> usize {
    params
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn parse_offset(params: &serde_json::Value) -> usize {
    params
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn goal_params_error(req: &RpcRequest, message: &str) -> RpcResponse {
    app_error(
        req.id.clone(),
        INVALID_PARAMS,
        message,
        "goal",
        true,
        "use a non-empty description, valid level, valid status, and RFC3339 deadline",
    )
}

fn goal_deadline_error(req: &RpcRequest, message: &str) -> RpcResponse {
    app_error(
        req.id.clone(),
        INVALID_PARAMS,
        message,
        "goal",
        true,
        "use an RFC3339 deadline such as 2026-04-28T10:00:00Z",
    )
}

fn goal_not_found(req: &RpcRequest, id: &str) -> RpcResponse {
    app_error(
        req.id.clone(),
        GOAL_NOT_FOUND,
        &format!("goal '{id}' not found"),
        "goal",
        true,
        "check the goal id or list visible goals",
    )
}

fn goal_store_error(req: &RpcRequest, message: &str) -> RpcResponse {
    app_error(
        req.id.clone(),
        GOAL_OPERATION_FAILED,
        message,
        "goal",
        true,
        "check goal ownership, store permissions, and the goal state transition",
    )
}
