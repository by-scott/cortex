use crate::daemon::DaemonState;
use crate::hot_reload::ReloadTarget;

use super::{
    INVALID_PARAMS, OPERATOR_ONLY, RpcHandler, RpcRequest, RpcResponse, SESSION_NOT_FOUND,
    app_error, success,
};

impl RpcHandler {
    pub(super) fn handle_admin_reload_config(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        if self.state.transport_actor(client) != DaemonState::local_actor() {
            return app_error(
                req.id.clone(),
                OPERATOR_ONLY,
                "method requires the local operator identity",
                "operator",
                true,
                "use the local operator transport or remove custom transport bindings",
            );
        }
        self.state.reload_config();
        success(req.id.clone(), serde_json::json!({}))
    }

    pub(super) fn handle_daemon_status(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        if self.state.transport_actor(client) != DaemonState::local_actor() {
            return app_error(
                req.id.clone(),
                OPERATOR_ONLY,
                "method requires the local operator identity",
                "operator",
                true,
                "use the local operator transport or remove custom transport bindings",
            );
        }
        let status = self.state.status();
        success(req.id.clone(), status)
    }

    pub(super) fn handle_operator_dashboard(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        if self.state.transport_actor(client) != DaemonState::local_actor() {
            return app_error(
                req.id.clone(),
                OPERATOR_ONLY,
                "method requires the local operator identity",
                "operator",
                true,
                "use the local operator transport or remove custom transport bindings",
            );
        }
        let limit = req
            .params
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        success(req.id.clone(), self.state.operator_dashboard(limit))
    }

    pub(super) fn handle_health_check(&self, req: &RpcRequest, client: &str) -> RpcResponse {
        if self.state.transport_actor(client) != "local:default" {
            return app_error(
                req.id.clone(),
                OPERATOR_ONLY,
                "method requires the local operator identity",
                "operator",
                true,
                "use the local operator transport or remove custom transport bindings",
            );
        }
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

    pub(super) fn handle_meta_alerts(&self, req: &RpcRequest, client: &str) -> RpcResponse {
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
                    .check_with_confidence(0.5)
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

        let exists_on_disk = self
            .state
            .visible_sessions_for_transport(client)
            .iter()
            .any(|s| s.id.to_string() == session_id);

        if exists_on_disk {
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
}
