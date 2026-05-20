pub mod defaults;
pub mod evolution;
pub mod loader;
pub mod skill_tool;

use cortex_types::{
    ExecutionMode, Payload, SkillActivation, SkillEvolutionProposal, SkillExecutionTrace,
    SkillHealth, SkillHealthState, SkillManifest, SkillMetadata, SkillParameter, SkillSummary,
};
use std::collections::HashMap;
use std::sync::RwLock;

/// Content returned by a skill for context injection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillContent {
    Markdown(String),
}

#[derive(Clone, Debug)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub parameters: Vec<SkillParameter>,
    pub required_tools: Vec<String>,
    pub timeout_secs: Option<u64>,
    pub execution_mode: ExecutionMode,
    pub metadata: SkillMetadata,
    pub activation: Option<SkillActivation>,
}

#[derive(Clone, Debug)]
pub struct RenderedSkill {
    pub definition: SkillDefinition,
    pub content: SkillContent,
}

impl SkillDefinition {
    #[must_use]
    pub fn manifest(&self) -> SkillManifest {
        let mut manifest = SkillManifest::basic(self.name.clone(), self.description.clone());
        manifest.version.clone_from(&self.metadata.version);
        manifest.source = self.metadata.source.clone();
        manifest.preconditions = skill_preconditions(self);
        manifest.inputs.clone_from(&self.parameters);
        manifest.outputs = skill_outputs(self);
        manifest.effects = skill_effects(self);
        manifest.required_tools.clone_from(&self.required_tools);
        manifest.risk = skill_risk(self);
        manifest.expected_duration_secs = self.timeout_secs;
        manifest.success_criteria = skill_success_criteria(self);
        manifest.fallback =
            Some("skip this skill and continue with the base turn protocol".to_string());
        manifest.observability = vec![
            "SkillInvoked".to_string(),
            "SkillCompleted".to_string(),
            "SkillExecutionTrace".to_string(),
            "utility_ewma".to_string(),
        ];
        manifest.user_invocable = self.metadata.user_invocable;
        manifest.agent_invocable = self.metadata.agent_invocable;
        manifest
    }
}

/// Core skill abstraction — externalized domain knowledge.
pub trait Skill: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn when_to_use(&self) -> &str;
    fn parameters(&self) -> Vec<SkillParameter> {
        vec![]
    }
    fn required_tools(&self) -> Vec<&str> {
        vec![]
    }
    fn timeout_secs(&self) -> Option<u64> {
        None
    }
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Inline
    }
    fn content(&self, args: &str) -> SkillContent;
    fn metadata(&self) -> SkillMetadata;
    fn activation(&self) -> Option<&SkillActivation> {
        None
    }
}

/// Registry of available skills with two-tier override (system < instance).
///
/// All fields use `RwLock` for thread-safe interior mutability, enabling
/// hot-reload and maintenance-cycle evolution via shared `Arc<SkillRegistry>`.
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, Box<dyn Skill>>>,
    utility_scores: RwLock<HashMap<String, f64>>,
    health_states: RwLock<HashMap<String, SkillHealth>>,
    proposals: RwLock<Vec<SkillEvolutionProposal>>,
    pending_events: RwLock<Vec<Payload>>,
    execution_traces: RwLock<Vec<SkillExecutionTrace>>,
    tool_call_history: RwLock<Vec<String>>,
    /// Instance-level skills directory for writing evolved skills.
    instance_skills_dir: RwLock<Option<std::path::PathBuf>>,
}

const EWMA_ALPHA: f64 = 0.3;
const INITIAL_UTILITY: f64 = 0.5;
const STRONG_THRESHOLD: f64 = 0.8;
const WEAK_THRESHOLD: f64 = 0.3;
const QUARANTINE_THRESHOLD: f64 = 0.15;
const NEEDS_REVIEW_FAILURES: u32 = 3;
const QUARANTINE_FAILURES: u32 = 5;
const TRACE_HISTORY_LIMIT: usize = 200;

impl SkillRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            utility_scores: RwLock::new(HashMap::new()),
            health_states: RwLock::new(HashMap::new()),
            proposals: RwLock::new(Vec::new()),
            pending_events: RwLock::new(Vec::new()),
            execution_traces: RwLock::new(Vec::new()),
            tool_call_history: RwLock::new(Vec::new()),
            instance_skills_dir: RwLock::new(None),
        }
    }

    /// Set the instance-level skills directory (for evolution output).
    pub fn set_instance_dir(&self, dir: std::path::PathBuf) {
        *self
            .instance_skills_dir
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir);
    }

    /// Load persisted utility scores into the registry.
    pub fn load_utilities(&self, scores: HashMap<String, f64>) {
        *self
            .utility_scores
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = scores;
    }

    /// Load persisted skill health states into the registry.
    pub fn load_health(&self, states: Vec<SkillHealth>) {
        let mut health = self
            .health_states
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        health.clear();
        for state in states {
            health.insert(state.name.clone(), state);
        }
    }

    /// Load persisted skill evolution proposals into the registry.
    pub fn load_proposals(&self, proposals: Vec<SkillEvolutionProposal>) {
        *self
            .proposals
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = proposals;
    }

    /// Return a snapshot of all utility scores for persistence.
    #[must_use]
    pub fn utility_snapshot(&self) -> HashMap<String, f64> {
        self.utility_scores
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn health_snapshot(&self) -> Vec<SkillHealth> {
        let mut states: Vec<_> = self
            .health_states
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        states.sort_by(|left, right| left.name.cmp(&right.name));
        states
    }

    #[must_use]
    pub fn proposal_snapshot(&self) -> Vec<SkillEvolutionProposal> {
        self.proposals
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn accept_proposal(&self, id: &str) -> Option<SkillEvolutionProposal> {
        self.set_proposal_status(id, cortex_types::SkillEvolutionProposalStatus::Accepted)
            .inspect(|proposal| {
                if let Some(target) = &proposal.target_skill {
                    self.mark_health(
                        target,
                        SkillHealthState::Deprecated,
                        format!(
                            "superseded by accepted skill proposal '{}'",
                            proposal.candidate_skill
                        ),
                        Some(proposal.candidate_skill.clone()),
                    );
                }
                self.mark_health(
                    &proposal.candidate_skill,
                    SkillHealthState::Healthy,
                    format!("accepted skill evolution proposal '{id}'"),
                    proposal.target_skill.clone(),
                );
            })
    }

    pub fn reject_proposal(&self, id: &str) -> Option<SkillEvolutionProposal> {
        self.set_proposal_status(id, cortex_types::SkillEvolutionProposalStatus::Rejected)
    }

    pub fn drain_pending_events(&self) -> Vec<Payload> {
        std::mem::take(
            &mut *self
                .pending_events
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    fn set_proposal_status(
        &self,
        id: &str,
        status: cortex_types::SkillEvolutionProposalStatus,
    ) -> Option<SkillEvolutionProposal> {
        let proposal = {
            let mut proposals = self
                .proposals
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let proposal_index = proposal_index(&proposals, id)?;
            let proposal = proposals.get_mut(proposal_index)?;
            proposal.status = status;
            let proposal = proposal.clone();
            drop(proposals);
            proposal
        };
        Some(proposal)
    }

    /// Register a skill. Later registrations override earlier ones (instance > system).
    pub fn register(&self, skill: Box<dyn Skill>) {
        let name = skill.name().to_string();
        self.skills
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.clone(), skill);
        self.ensure_health_entry(&name);
    }

    /// Validate all registered skills' `input_patterns` regex.
    #[must_use]
    pub fn validate_all_patterns(&self) -> Vec<String> {
        self.skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flat_map(|s| validate_activation_patterns(s.activation(), s.name()))
            .collect()
    }

    /// Get a skill by name and execute a closure with it.
    ///
    /// Returns `None` if the skill is not found.
    pub fn with_skill<F, R>(&self, name: &str, f: F) -> Option<R>
    where
        F: FnOnce(&dyn Skill) -> R,
    {
        self.skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .map(|s| f(s.as_ref()))
    }

    #[must_use]
    pub fn definition(&self, name: &str) -> Option<SkillDefinition> {
        self.with_skill(name, |skill| SkillDefinition {
            name: skill.name().to_string(),
            description: skill.description().to_string(),
            when_to_use: skill.when_to_use().to_string(),
            parameters: skill.parameters(),
            required_tools: skill
                .required_tools()
                .into_iter()
                .map(str::to_string)
                .collect(),
            timeout_secs: skill.timeout_secs(),
            execution_mode: skill.execution_mode(),
            metadata: skill.metadata(),
            activation: skill.activation().cloned(),
        })
    }

    #[must_use]
    pub fn manifest(&self, name: &str) -> Option<SkillManifest> {
        self.definition(name)
            .map(|definition| definition.manifest())
    }

    #[must_use]
    pub fn manifests(&self) -> Vec<SkillManifest> {
        let mut manifests = self
            .names()
            .into_iter()
            .filter_map(|name| self.manifest(&name))
            .collect::<Vec<_>>();
        manifests.sort_by(|left, right| left.name.cmp(&right.name));
        manifests
    }

    #[must_use]
    pub fn render(&self, name: &str, args: &str) -> Option<RenderedSkill> {
        self.with_skill(name, |skill| RenderedSkill {
            definition: SkillDefinition {
                name: skill.name().to_string(),
                description: skill.description().to_string(),
                when_to_use: skill.when_to_use().to_string(),
                parameters: skill.parameters(),
                required_tools: skill
                    .required_tools()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                timeout_secs: skill.timeout_secs(),
                execution_mode: skill.execution_mode(),
                metadata: skill.metadata(),
                activation: skill.activation().cloned(),
            },
            content: skill.content(args),
        })
    }

    /// Execute a closure with read access to all registered skills.
    pub fn with_all_skills<F>(&self, f: F)
    where
        F: FnOnce(&[&dyn Skill]),
    {
        Self::invoke_with_guard(
            &self
                .skills
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            f,
        );
    }

    /// Helper: build refs from a guard and invoke the closure.
    /// Separated so the guard's drop scope is clear to clippy.
    fn invoke_with_guard<F>(guard: &HashMap<String, Box<dyn Skill>>, f: F)
    where
        F: FnOnce(&[&dyn Skill]),
    {
        let refs: Vec<&dyn Skill> = guard.values().map(AsRef::as_ref).collect();
        f(&refs);
    }

    /// Check if a skill exists by name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }

    /// Record a skill invocation outcome for utility learning (EWMA alpha=0.3).
    pub fn record_outcome(&self, name: &str, success: bool) {
        let signal = if success { 1.0 } else { 0.0 };
        let score = {
            let mut scores = self
                .utility_scores
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let current = scores.get(name).copied().unwrap_or(INITIAL_UTILITY);
            let updated = current.mul_add(1.0 - EWMA_ALPHA, signal * EWMA_ALPHA);
            scores.insert(name.to_string(), updated);
            updated
        };
        self.update_health_after_outcome(name, score, success);
    }

    fn update_health_after_outcome(&self, name: &str, score: f64, success: bool) {
        let event = {
            let mut health_states = self
                .health_states
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let health = health_states
                .entry(name.to_string())
                .or_insert_with(|| SkillHealth::new(name));
            let previous = health.state;
            health.score = score;
            if success {
                health.consecutive_successes = health.consecutive_successes.saturating_add(1);
                health.consecutive_failures = 0;
            } else {
                health.consecutive_failures = health.consecutive_failures.saturating_add(1);
                health.consecutive_successes = 0;
            }
            let (state, reason) = classify_health(health, success);
            health.state = state;
            health.reason = reason;
            health.updated_at = chrono::Utc::now();
            let event = (previous != state).then(|| Payload::SkillHealthChanged {
                name: name.to_string(),
                from: previous.as_str().to_string(),
                to: state.as_str().to_string(),
                reason: health.reason.clone(),
            });
            drop(health_states);
            event
        };
        if let Some(event) = event {
            self.pending_events
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event);
        }
    }

    fn mark_health(
        &self,
        name: &str,
        state: SkillHealthState,
        reason: String,
        related_skill: Option<String>,
    ) {
        let event = {
            let mut health_states = self
                .health_states
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let health = health_states
                .entry(name.to_string())
                .or_insert_with(|| SkillHealth::new(name));
            let previous = health.state;
            health.state = state;
            health.reason = reason;
            health.related_skill = related_skill;
            health.updated_at = chrono::Utc::now();
            let event = (previous != state).then(|| Payload::SkillHealthChanged {
                name: name.to_string(),
                from: previous.as_str().to_string(),
                to: state.as_str().to_string(),
                reason: health.reason.clone(),
            });
            drop(health_states);
            event
        };
        if let Some(event) = event {
            self.pending_events
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event);
        }
    }

    pub fn record_trace(&self, trace: SkillExecutionTrace) {
        let mut traces = self
            .execution_traces
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        traces.push(trace);
        if traces.len() > TRACE_HISTORY_LIMIT {
            let drain_count = traces.len() - TRACE_HISTORY_LIMIT;
            traces.drain(..drain_count);
        }
    }

    #[must_use]
    pub fn trace_snapshot(&self, max: usize) -> Vec<SkillExecutionTrace> {
        let traces = self
            .execution_traces
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let start = traces.len().saturating_sub(max);
        traces[start..].to_vec()
    }

    /// Lightweight summaries for system prompt injection, sorted by utility (descending).
    #[must_use]
    pub fn summaries(&self, max: usize) -> Vec<SkillSummary> {
        let mut sums: Vec<_> = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|s| self.can_auto_activate(s.name()))
            .map(|s| SkillSummary {
                name: s.name().to_string(),
                description: s.description().to_string(),
            })
            .collect();
        let scores = self.utility_snapshot();
        sums.sort_by(|a, b| {
            let sa = scores.get(&a.name).copied().unwrap_or(INITIAL_UTILITY);
            let sb = scores.get(&b.name).copied().unwrap_or(INITIAL_UTILITY);
            let ha = self.health_for(&a.name).state;
            let hb = self.health_for(&b.name).state;
            health_priority(hb)
                .cmp(&health_priority(ha))
                .then_with(|| sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.name.cmp(&b.name))
        });
        sums.truncate(max);
        sums
    }

    #[must_use]
    pub fn invocable_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .filter(|name| self.can_auto_activate(name))
            .cloned()
            .collect();
        names.sort();
        names
    }

    #[must_use]
    pub fn user_invocable(&self) -> Vec<SkillSummary> {
        let mut result: Vec<_> = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|s| s.metadata().user_invocable)
            .map(|s| SkillSummary {
                name: s.name().to_string(),
                description: s.description().to_string(),
            })
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn ensure_health_entry(&self, name: &str) {
        let score = self
            .utility_scores
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .copied()
            .unwrap_or(INITIAL_UTILITY);
        self.health_states
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name.to_string())
            .or_insert_with(|| {
                let mut health = SkillHealth::new(name);
                health.score = score;
                let (state, reason) = classify_health(&health, true);
                health.state = state;
                health.reason = reason;
                health
            });
    }

    #[must_use]
    pub fn health_for(&self, name: &str) -> SkillHealth {
        self.health_states
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .unwrap_or_else(|| {
                let mut health = SkillHealth::new(name);
                health.score = self
                    .utility_scores
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(name)
                    .copied()
                    .unwrap_or(INITIAL_UTILITY);
                let (state, reason) = classify_health(&health, true);
                health.state = state;
                health.reason = reason;
                health
            })
    }

    #[must_use]
    pub fn can_auto_activate(&self, name: &str) -> bool {
        self.health_for(name).state.allows_automatic_activation()
    }

    /// Record a tool call for pattern detection (skill evolution).
    pub fn record_tool_call(&self, tool_name: &str) {
        let mut history = self
            .tool_call_history
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history.push(tool_name.to_string());
        if history.len() > 100 {
            let drain_count = history.len() - 100;
            history.drain(..drain_count);
        }
    }

    /// Suggest new skills based on detected tool call patterns.
    #[must_use]
    pub fn suggest_skills(&self) -> Vec<evolution::SkillSuggestion> {
        let history = self
            .tool_call_history
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        evolution::detect_patterns(&history, 3)
    }

    /// Run a full skill evolution cycle: detect patterns, evaluate utility,
    /// materialize new skills, and flag weak/strong skills.
    ///
    /// Uses the configured instance skills directory. Returns `None` if
    /// no instance directory is set.
    pub fn evolve(&self) -> Option<evolution::EvolutionResult> {
        let dir = self
            .instance_skills_dir
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()?;

        let suggestions = self.suggest_skills();
        let scores = self.utility_snapshot();
        let existing = self.existing_profiles();

        let result = evolution::evolve_skills(
            &suggestions,
            &scores,
            &existing,
            &dir,
            WEAK_THRESHOLD,
            STRONG_THRESHOLD,
        );

        // Register newly created skills into the live registry
        if !result.created.is_empty() {
            let loaded = loader::load_skills(&dir, &cortex_types::SkillSource::Instance);
            for skill in loaded {
                if result.created.contains(&skill.name().to_string()) {
                    self.register(skill);
                }
            }
        }

        if !result.proposals.is_empty() {
            self.proposals
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend(result.proposals.iter().cloned());
            let events = result
                .proposals
                .iter()
                .map(|proposal| Payload::SkillEvolutionProposed {
                    proposal_id: proposal.id.clone(),
                    candidate_skill: proposal.candidate_skill.clone(),
                    target_skill: proposal.target_skill.clone(),
                    relation: proposal.relation.as_str().to_string(),
                    reason: proposal.reason.clone(),
                })
                .collect::<Vec<_>>();
            let mut pending = self
                .pending_events
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pending.extend(events);
        }

        Some(result)
    }

    fn existing_profiles(&self) -> Vec<evolution::ExistingSkillProfile> {
        let skills = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        skills
            .values()
            .map(|skill| evolution::ExistingSkillProfile {
                name: skill.name().to_string(),
                required_tools: skill
                    .required_tools()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                health: self.health_for(skill.name()),
            })
            .collect()
    }

    /// Hot-reload: re-scan a skills directory and reconcile with on-disk state.
    ///
    /// Removes stale skills from the given source that no longer exist on disk,
    /// then registers all currently-loaded skills (add/update).
    pub fn reload_from(&self, dir: &std::path::Path, source: &cortex_types::SkillSource) {
        let loaded = loader::load_skills(dir, source);
        let loaded_names: std::collections::HashSet<String> =
            loaded.iter().map(|s| s.name().to_string()).collect();

        // Remove stale skills from this source that no longer exist on disk
        {
            let to_remove: Vec<String> = self
                .skills
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .filter(|(_, s)| s.metadata().source == *source && !loaded_names.contains(s.name()))
                .map(|(name, _)| name.clone())
                .collect();
            if !to_remove.is_empty() {
                let mut skills = self
                    .skills
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for name in &to_remove {
                    skills.remove(name);
                }
            }
        }

        // Re-register (add/update) loaded skills
        for skill in loaded {
            self.register(skill);
        }
    }

    /// Return skills whose activation conditions match the given context.
    #[must_use]
    pub fn activated_skills(
        &self,
        input: &str,
        pressure_name: &str,
        alert_kinds: &[String],
    ) -> Vec<SkillSummary> {
        let mut result: Vec<_> = {
            let skills = self
                .skills
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            skills
                .values()
                .filter(|s| self.can_auto_activate(s.name()))
                .filter(|s| {
                    matches_activation(s.activation(), input, pressure_name, alert_kinds, &[])
                })
                .map(|s| SkillSummary {
                    name: s.name().to_string(),
                    description: s.description().to_string(),
                })
                .collect()
        };
        result.sort_by(|left, right| {
            health_priority(self.health_for(&right.name).state)
                .cmp(&health_priority(self.health_for(&left.name).state))
                .then_with(|| left.name.cmp(&right.name))
        });
        result
    }

    /// Return skills whose activation conditions match the given event kinds.
    #[must_use]
    pub fn activated_skills_for_events(&self, event_kinds: &[String]) -> Vec<SkillSummary> {
        let mut result: Vec<_> = {
            let skills = self
                .skills
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            skills
                .values()
                .filter(|s| self.can_auto_activate(s.name()))
                .filter(|s| matches_activation(s.activation(), "", "normal", &[], event_kinds))
                .map(|s| SkillSummary {
                    name: s.name().to_string(),
                    description: s.description().to_string(),
                })
                .collect()
        };
        result.sort_by(|left, right| {
            health_priority(self.health_for(&right.name).state)
                .cmp(&health_priority(self.health_for(&left.name).state))
                .then_with(|| left.name.cmp(&right.name))
        });
        result
    }
}

/// Validate regex patterns in a skill's activation conditions.
fn validate_activation_patterns(
    activation: Option<&SkillActivation>,
    skill_name: &str,
) -> Vec<String> {
    let Some(act) = activation else {
        return Vec::new();
    };
    act.input_patterns
        .iter()
        .filter_map(|p| {
            regex::Regex::new(p)
                .err()
                .map(|e| format!("skill '{skill_name}': invalid regex '{p}': {e}"))
        })
        .collect()
}

/// Check if a skill's activation conditions match the current context.
fn matches_activation(
    activation: Option<&SkillActivation>,
    input: &str,
    pressure_name: &str,
    alert_kinds: &[String],
    event_kinds: &[String],
) -> bool {
    let Some(act) = activation else {
        return false;
    };
    if act
        .input_patterns
        .iter()
        .any(|p| regex::Regex::new(p).is_ok_and(|r| r.is_match(input)))
    {
        return true;
    }
    if let Some(ref threshold) = act.pressure_above {
        let levels = ["normal", "alert", "compress", "urgent", "degrade"];
        let threshold_idx = levels
            .iter()
            .position(|l| l.eq_ignore_ascii_case(threshold));
        let current_idx = levels
            .iter()
            .position(|l| l.eq_ignore_ascii_case(pressure_name));
        if threshold_idx.zip(current_idx).is_some_and(|(t, c)| c >= t) {
            return true;
        }
    }
    if !act.alert_kinds.is_empty()
        && act
            .alert_kinds
            .iter()
            .any(|ak| alert_kinds.iter().any(|a| a.eq_ignore_ascii_case(ak)))
    {
        return true;
    }
    if !act.event_kinds.is_empty()
        && act
            .event_kinds
            .iter()
            .any(|ek| event_kinds.iter().any(|e| e.eq_ignore_ascii_case(ek)))
    {
        return true;
    }
    false
}

fn classify_health(health: &SkillHealth, last_success: bool) -> (SkillHealthState, String) {
    if health.consecutive_failures >= QUARANTINE_FAILURES || health.score <= QUARANTINE_THRESHOLD {
        return (
            SkillHealthState::Quarantined,
            format!(
                "quarantined after {} consecutive failures or low utility {:.2}",
                health.consecutive_failures, health.score
            ),
        );
    }
    if health.consecutive_failures >= NEEDS_REVIEW_FAILURES || health.score < WEAK_THRESHOLD {
        return (
            SkillHealthState::NeedsReview,
            format!(
                "needs review after {} consecutive failures or weak utility {:.2}",
                health.consecutive_failures, health.score
            ),
        );
    }
    if last_success && health.score >= STRONG_THRESHOLD && health.consecutive_successes >= 2 {
        return (
            SkillHealthState::Strong,
            format!(
                "strong utility {:.2} with repeated successful use",
                health.score
            ),
        );
    }
    (
        SkillHealthState::Healthy,
        format!("healthy utility {:.2}", health.score),
    )
}

const fn health_priority(state: SkillHealthState) -> u8 {
    match state {
        SkillHealthState::Strong => 4,
        SkillHealthState::Healthy => 3,
        SkillHealthState::NeedsReview => 2,
        SkillHealthState::Quarantined => 1,
        SkillHealthState::Deprecated => 0,
    }
}

fn proposal_index(proposals: &[SkillEvolutionProposal], id: &str) -> Option<usize> {
    if let Some(index) = proposals.iter().position(|proposal| proposal.id == id) {
        return Some(index);
    }
    let mut matches = proposals
        .iter()
        .enumerate()
        .filter(|(_, proposal)| proposal.id.starts_with(id))
        .map(|(index, _)| index);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn skill_preconditions(definition: &SkillDefinition) -> Vec<String> {
    let mut preconditions =
        vec!["skill is registered and visible to the current actor".to_string()];
    if definition.activation.is_some() {
        preconditions.push("activation rule matched current turn context".to_string());
    }
    if !definition.required_tools.is_empty() {
        preconditions.push("required tools are available under runtime policy".to_string());
    }
    preconditions
}

fn skill_outputs(definition: &SkillDefinition) -> Vec<String> {
    match definition.execution_mode {
        ExecutionMode::Inline => vec!["markdown_context".to_string()],
        ExecutionMode::Fork => vec!["subturn_result".to_string()],
    }
}

fn skill_effects(definition: &SkillDefinition) -> Vec<String> {
    let mut effects = match definition.execution_mode {
        ExecutionMode::Inline => vec!["context_injection".to_string()],
        ExecutionMode::Fork => vec!["forked_skill_turn".to_string()],
    };
    effects.extend(
        definition
            .required_tools
            .iter()
            .map(|tool| format!("requires_tool:{tool}")),
    );
    effects
}

fn skill_success_criteria(definition: &SkillDefinition) -> Vec<String> {
    let mut criteria = vec!["skill execution completed without error".to_string()];
    match definition.execution_mode {
        ExecutionMode::Inline => {
            criteria.push("content was rendered into the active turn".to_string());
        }
        ExecutionMode::Fork => {
            criteria.push("forked skill turn returned a result".to_string());
        }
    }
    criteria
}

fn skill_risk(definition: &SkillDefinition) -> f32 {
    let tool_risk = if definition.required_tools.is_empty() {
        0.0_f32
    } else {
        0.20_f32
    };
    let mode_risk = match definition.execution_mode {
        ExecutionMode::Inline => 0.05_f32,
        ExecutionMode::Fork => 0.15_f32,
    };
    (tool_risk + mode_risk).clamp(0.0, 1.0)
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
