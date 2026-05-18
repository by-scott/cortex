/// Turn tracer that emits events via the `tracing` crate.
pub struct TracingTurnTracer {
    pub config: cortex_types::config::TurnTraceConfig,
}

impl cortex_turn::orchestrator::TurnTracer for TracingTurnTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat_str = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat_str) >= level {
            tracing::info!(category = cat_str.as_str(), "{message}");
        }
    }
}

/// Turn tracer that emits to both tracing and an mpsc channel for streaming delivery.
pub struct ChannelTurnTracer {
    pub config: cortex_types::config::TurnTraceConfig,
    pub tx: tokio::sync::mpsc::Sender<String>,
}

impl cortex_turn::orchestrator::TurnTracer for ChannelTurnTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat_str = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat_str) < level {
            return;
        }

        tracing::info!(category = cat_str.as_str(), "{message}");

        let payload = serde_json::json!({
            "event": "trace",
            "data": {
                "category": cat_str,
                "level": format!("{level:?}").to_lowercase(),
                "message": message,
            }
        });
        if let Ok(json) = serde_json::to_string(&payload) {
            let _ = self.tx.try_send(json);
        }
    }
}
