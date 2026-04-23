use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResumePacket {
    pub summary: String,
    pub last_actions: Vec<String>,
    pub pending_context: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub meta_alerts: Vec<String>,
    #[serde(default)]
    pub active_skills: Vec<String>,
}

impl ResumePacket {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.summary.is_empty()
            && self.last_actions.is_empty()
            && self.pending_context.is_none()
            && self.session_id.is_none()
            && self.goals.is_empty()
            && self.meta_alerts.is_empty()
            && self.active_skills.is_empty()
    }

    #[must_use]
    pub fn format_prompt(&self) -> String {
        use std::fmt::Write;
        if self.is_empty() {
            return String::from("No previous session context.");
        }
        let mut out = String::new();
        if let Some(sid) = &self.session_id {
            let _ = writeln!(out, "Session: {sid}");
        }
        if !self.summary.is_empty() {
            let _ = writeln!(out, "Previous session summary: {}", self.summary);
        }
        if !self.last_actions.is_empty() {
            let _ = writeln!(out, "Recent actions: {}", self.last_actions.join("; "));
        }
        if !self.goals.is_empty() {
            let _ = writeln!(out, "Active goals: {}", self.goals.join("; "));
        }
        if !self.meta_alerts.is_empty() {
            let _ = writeln!(out, "Metacognition alerts: {}", self.meta_alerts.join("; "));
        }
        if !self.active_skills.is_empty() {
            let _ = writeln!(out, "Active skills: {}", self.active_skills.join("; "));
        }
        if let Some(ctx) = &self.pending_context {
            let _ = writeln!(out, "Pending user request: {ctx}");
        }
        out
    }
}
