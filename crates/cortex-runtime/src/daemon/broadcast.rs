use cortex_types::RiskLevel;

/// A message broadcast to subscribers of a session's event channel.
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    /// Session ID that produced this message.
    pub session_id: String,
    /// Transport that originated this event (`"telegram"`, `"whatsapp"`, `"ws"`, `"sse"`,
    /// `"socket"`, `"rpc"`, `"http"`, `"heartbeat"`, or the channel's session
    /// prefix). Subscribers use this to skip their own events.
    pub source: String,
    /// Event payload.
    pub event: BroadcastEvent,
}

/// Events broadcast across channels. Mirrors the streaming event types.
#[derive(Clone, Debug)]
pub enum BroadcastEvent {
    /// Incremental text chunk during generation.
    Text(String),
    /// Observer text from sub-turns or internal execution lanes.
    Observer { source: String, content: String },
    /// Boundary between two narration segments within one transport stream.
    Boundary,
    /// Trace event (phase, llm, meta, etc.).
    Trace { category: String, message: String },
    /// Turn completed with final structured response.
    Done {
        response: String,
        response_parts: Vec<cortex_types::ResponsePart>,
    },
    /// Error during turn execution.
    Error(String),
    /// Tool execution is waiting for user confirmation.
    PermissionRequested(PendingPermissionInfo),
}

impl BroadcastEvent {
    #[must_use]
    pub const fn done(response: String, response_parts: Vec<cortex_types::ResponsePart>) -> Self {
        Self::Done {
            response,
            response_parts,
        }
    }

    #[must_use]
    pub fn from_turn_stream_event(
        event: &cortex_turn::orchestrator::TurnStreamEvent,
    ) -> Option<Self> {
        match event {
            cortex_turn::orchestrator::TurnStreamEvent::Text {
                lane: cortex_turn::orchestrator::StreamLane::UserVisible,
                content,
                ..
            } => Some(Self::Text(content.clone())),
            cortex_turn::orchestrator::TurnStreamEvent::Text {
                lane: cortex_turn::orchestrator::StreamLane::Observer,
                source,
                content,
            } => Some(Self::Observer {
                source: source.clone().unwrap_or_else(|| "observer".to_string()),
                content: content.clone(),
            }),
            cortex_turn::orchestrator::TurnStreamEvent::Boundary(_) => Some(Self::Boundary),
            cortex_turn::orchestrator::TurnStreamEvent::ToolProgress(progress)
                if matches!(
                    progress.status,
                    cortex_turn::orchestrator::ToolProgressStatus::Started
                        | cortex_turn::orchestrator::ToolProgressStatus::Completed
                ) =>
            {
                None
            }
            cortex_turn::orchestrator::TurnStreamEvent::ToolProgress(progress) => {
                Some(Self::Trace {
                    category: "tool".to_string(),
                    message: format!(
                        "Tool: {} ({})",
                        progress.tool_name,
                        tool_progress_status_label(progress),
                    ),
                })
            }
        }
    }

    #[must_use]
    pub fn plain_text(&self) -> String {
        match self {
            Self::Text(content) => content.clone(),
            Self::Observer { source, content } => format!("[observer:{source}] {content}"),
            Self::Boundary => String::new(),
            Self::Trace { category, message } => format!("[{category}] {message}"),
            Self::Done { response, .. } => response.clone(),
            Self::Error(error) => format!("[error] {error}"),
            Self::PermissionRequested(info) => info.prompt_text(),
        }
    }

    #[must_use]
    pub fn plain_chunks(&self) -> Vec<String> {
        match self {
            Self::Done { response_parts, .. } if !response_parts.is_empty() => response_parts
                .iter()
                .map(|part| match part {
                    cortex_types::ResponsePart::Text { text, .. } => text.clone(),
                    cortex_types::ResponsePart::Media { attachment } => {
                        match attachment.media_type.as_str() {
                            "image" => "[image]".to_string(),
                            "audio" => "[audio]".to_string(),
                            "video" => "[video]".to_string(),
                            "file" => "[file]".to_string(),
                            _ => "[media]".to_string(),
                        }
                    }
                })
                .filter(|chunk| !chunk.trim().is_empty())
                .collect(),
            Self::Done { response, .. } => vec![response.clone()],
            Self::PermissionRequested(info) => vec![info.prompt_text()],
            _ => vec![self.plain_text()],
        }
    }
}

#[derive(Clone, Debug)]
pub struct PendingPermissionInfo {
    pub id: String,
    pub session_id: String,
    pub actor: String,
    pub source: String,
    pub tool_name: String,
    pub risk_level: RiskLevel,
    pub explanation: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl PendingPermissionInfo {
    #[must_use]
    pub fn prompt_text(&self) -> String {
        let mut text = format!(
            "Tool confirmation required\nTool: {}\nRisk: {:?}\nApprove: /approve {}\nDeny: /deny {}",
            self.tool_name, self.risk_level, self.id, self.id
        );
        if !self.explanation.trim().is_empty() {
            text.push_str("\n\nDecision trace:\n");
            text.push_str(self.explanation.trim());
        }
        text
    }
}

const fn tool_progress_status_label(
    progress: &cortex_turn::orchestrator::ToolProgress,
) -> &'static str {
    match progress.status {
        cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
        cortex_turn::orchestrator::ToolProgressStatus::Running => "running",
        cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
        cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
    }
}
