/// R6 Situational context — compile-time guarantee that Bootstrap and Active are mutually exclusive.
///
/// Bootstrap guides the user through initial profile creation on first launch, injecting R6 without replacing R1+R2.
/// Active carries phase declarations, goal hierarchy, and session resumption information during normal operation.
pub enum SituationalContext {
    /// First launch: guide the user through initial profile creation (name, role, language preferences)
    Bootstrap(String),
    /// Normal operation: phase + goals + session resumption
    Active {
        phase: String,
        goals: String,
        resume: String,
    },
}

impl SituationalContext {
    fn render(&self) -> String {
        match self {
            Self::Bootstrap(content) => content.clone(),
            Self::Active {
                phase,
                goals,
                resume,
            } => {
                let mut parts = Vec::new();
                if !phase.is_empty() {
                    parts.push(format!("[Phase: {phase}]"));
                }
                if !goals.is_empty() {
                    parts.push(format!("[Goals]\n{goals}"));
                }
                if !resume.is_empty() {
                    parts.push(resume.clone());
                }
                parts.join("\n\n")
            }
        }
    }
}

/// Assembles the system prompt from 7 cognitive regions.
///
/// Region ordering (position = attention weight):
/// 1. Soul — cognitive principles and invariants
/// 2. Identity — self-awareness and capabilities
/// 3. Behavioral — operational rules and protocols
/// 4. User — who the user is
/// 5. Skills — available domain strategies
/// 6. Situational — phase/goals/resume OR bootstrap initialization
/// 7. Memory — recalled long-term knowledge
pub struct ContextBuilder {
    pub soul: Option<String>,
    pub identity: Option<String>,
    pub behavioral: Option<String>,
    pub user: Option<String>,
    pub skills: Option<String>,
    pub situational: Option<SituationalContext>,
    pub memory: Option<String>,
}

impl ContextBuilder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            soul: None,
            identity: None,
            behavioral: None,
            user: None,
            skills: None,
            situational: None,
            memory: None,
        }
    }

    pub fn set_soul(&mut self, content: String) {
        self.soul = Some(content);
    }
    pub fn set_identity(&mut self, content: String) {
        self.identity = Some(content);
    }
    pub fn set_behavioral(&mut self, content: String) {
        self.behavioral = Some(content);
    }
    pub fn set_user(&mut self, content: String) {
        self.user = Some(content);
    }
    pub fn set_skills(&mut self, content: String) {
        self.skills = Some(content);
    }
    pub fn set_situational(&mut self, ctx: SituationalContext) {
        self.situational = Some(ctx);
    }
    pub fn set_memory(&mut self, content: String) {
        self.memory = Some(content);
    }

    /// Build the system prompt. Returns `None` if all regions are empty.
    ///
    /// Soul and Identity are always present when set — bootstrap never replaces them.
    /// R6 (Situational) renders according to its variant: Bootstrap content for
    /// first interaction, or Phase+Goals+Resume for normal operation.
    #[must_use]
    pub fn build(&self) -> Option<String> {
        let mut parts = Vec::new();

        // R1 Soul — always first when present
        if let Some(s) = &self.soul {
            parts.push(s.as_str());
        }

        // R2 Identity — always second when present
        if let Some(s) = &self.identity {
            parts.push(s.as_str());
        }

        // R3-R7: Behavioral, User, Skills, Situational, Memory
        for s in [&self.behavioral, &self.user, &self.skills]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
        {
            parts.push(s.as_str());
        }

        // R6: Situational — rendered inline, owned by this scope
        let situational_rendered = self.situational.as_ref().map(SituationalContext::render);
        if let Some(ref s) = situational_rendered
            && !s.is_empty()
        {
            parts.push(s.as_str());
        }

        if let Some(s) = &self.memory
            && !s.is_empty()
        {
            parts.push(s.as_str());
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_full() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_identity("identity".into());
        b.set_behavioral("behavioral".into());
        b.set_user("user".into());
        b.set_skills("skills".into());
        b.set_memory("memory".into());
        let ctx = b.build().unwrap();
        assert!(ctx.contains("soul"));
        assert!(ctx.contains("memory"));
    }

    #[test]
    fn empty_returns_none() {
        assert!(ContextBuilder::new().build().is_none());
    }

    #[test]
    fn bootstrap_preserves_soul_identity() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_identity("identity".into());
        b.set_situational(SituationalContext::Bootstrap("welcome".into()));
        let ctx = b.build().unwrap();
        // Soul and Identity are always present — bootstrap does NOT replace them
        assert!(ctx.contains("soul"));
        assert!(ctx.contains("identity"));
        assert!(ctx.contains("welcome"));
    }

    #[test]
    fn order_preserved() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_behavioral("behavioral".into());
        b.set_memory("memory".into());
        let ctx = b.build().unwrap();
        let soul_pos = ctx.find("soul").unwrap();
        let beh_pos = ctx.find("behavioral").unwrap();
        let mem_pos = ctx.find("memory").unwrap();
        assert!(soul_pos < beh_pos);
        assert!(beh_pos < mem_pos);
    }

    #[test]
    fn situational_active_renders_phase_goals_resume() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_situational(SituationalContext::Active {
            phase: "implementation".into(),
            goals: "finish auth module".into(),
            resume: "last turn: read auth.rs".into(),
        });
        let ctx = b.build().unwrap();
        assert!(ctx.contains("[Phase: implementation]"));
        assert!(ctx.contains("[Goals]"));
        assert!(ctx.contains("finish auth module"));
        assert!(ctx.contains("last turn: read auth.rs"));
    }

    #[test]
    fn situational_active_skips_empty_fields() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_situational(SituationalContext::Active {
            phase: String::new(),
            goals: String::new(),
            resume: "resuming".into(),
        });
        let ctx = b.build().unwrap();
        assert!(!ctx.contains("[Phase:"));
        assert!(!ctx.contains("[Goals]"));
        assert!(ctx.contains("resuming"));
    }

    #[test]
    fn region_order_with_situational() {
        let mut b = ContextBuilder::new();
        b.set_soul("soul".into());
        b.set_skills("skills".into());
        b.set_situational(SituationalContext::Bootstrap("bootstrap".into()));
        b.set_memory("memory".into());
        let ctx = b.build().unwrap();
        let soul_pos = ctx.find("soul").unwrap();
        let skills_pos = ctx.find("skills").unwrap();
        let boot_pos = ctx.find("bootstrap").unwrap();
        let mem_pos = ctx.find("memory").unwrap();
        // Order: Soul < Skills < Situational < Memory
        assert!(soul_pos < skills_pos);
        assert!(skills_pos < boot_pos);
        assert!(boot_pos < mem_pos);
    }
}
