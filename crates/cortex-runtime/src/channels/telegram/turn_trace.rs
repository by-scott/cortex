use super::StreamChunk;

/// Turn tracer that sends trace events to the Telegram streaming channel.
pub(super) struct TelegramTracer {
    pub(super) tx: tokio::sync::mpsc::Sender<StreamChunk>,
    pub(super) config: cortex_types::config::TurnTraceConfig,
}

impl cortex_turn::orchestrator::TurnTracer for TelegramTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat) >= level {
            tracing::info!(category = cat.as_str(), "{message}");
            let _ = self
                .tx
                .try_send(StreamChunk::Event(crate::daemon::BroadcastEvent::Trace {
                    category: cat,
                    message: message.to_string(),
                }));
        }
    }
}
