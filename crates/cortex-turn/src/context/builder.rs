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

/// Assembles the system prompt from 8 cognitive regions.
///
/// Region ordering (position = attention weight):
/// 1. Soul — cognitive principles and invariants
/// 2. Identity — self-awareness and capabilities
/// 3. Behavioral — operational rules and protocols
/// 4. User — who the user is
/// 5. Runtime — live runtime facts and policies
/// 6. Skills — available domain strategies
/// 7. Situational — phase/goals/resume OR bootstrap initialization
/// 8. Memory — recalled long-term knowledge
pub struct ContextBuilder {
    pub soul: Option<String>,
    pub identity: Option<String>,
    pub behavioral: Option<String>,
    pub user: Option<String>,
    pub runtime: Option<String>,
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
            runtime: None,
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
    pub fn set_runtime(&mut self, content: String) {
        self.runtime = Some(content);
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

        // R3-R8: Behavioral, User, Runtime, Skills, Situational, Memory
        for s in [&self.behavioral, &self.user, &self.runtime, &self.skills]
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
