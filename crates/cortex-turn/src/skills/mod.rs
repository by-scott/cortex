pub mod defaults;
pub mod evolution;
pub mod loader;
pub mod skill_tool;

use cortex_types::{ExecutionMode, SkillActivation, SkillMetadata, SkillParameter, SkillSummary};
use std::collections::HashMap;
use std::sync::RwLock;

/// Content returned by a skill for context injection.
pub enum SkillContent {
    Markdown(String),
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
    tool_call_history: RwLock<Vec<String>>,
    /// Instance-level skills directory for writing evolved skills.
    instance_skills_dir: RwLock<Option<std::path::PathBuf>>,
}

const EWMA_ALPHA: f64 = 0.3;
const INITIAL_UTILITY: f64 = 0.5;

impl SkillRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            utility_scores: RwLock::new(HashMap::new()),
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

    /// Return a snapshot of all utility scores for persistence.
    #[must_use]
    pub fn utility_snapshot(&self) -> HashMap<String, f64> {
        self.utility_scores
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Register a skill. Later registrations override earlier ones (instance > system).
    pub fn register(&self, skill: Box<dyn Skill>) {
        self.skills
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(skill.name().to_string(), skill);
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
        let mut scores = self
            .utility_scores
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = scores.get(name).copied().unwrap_or(INITIAL_UTILITY);
        scores.insert(
            name.to_string(),
            current.mul_add(1.0 - EWMA_ALPHA, signal * EWMA_ALPHA),
        );
    }

    /// Lightweight summaries for system prompt injection, sorted by utility (descending).
    #[must_use]
    pub fn summaries(&self, max: usize) -> Vec<SkillSummary> {
        let mut sums: Vec<_> = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|s| SkillSummary {
                name: s.name().to_string(),
                description: s.description().to_string(),
            })
            .collect();
        let scores = self.utility_snapshot();
        sums.sort_by(|a, b| {
            let sa = scores.get(&a.name).copied().unwrap_or(INITIAL_UTILITY);
            let sb = scores.get(&b.name).copied().unwrap_or(INITIAL_UTILITY);
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        sums.truncate(max);
        sums
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
        let existing = self.names();

        let result = evolution::evolve_skills(&suggestions, &scores, &existing, &dir, 0.3, 0.8);

        // Register newly created skills into the live registry
        if !result.created.is_empty() {
            let loaded = loader::load_skills(&dir, &cortex_types::SkillSource::Instance);
            for skill in loaded {
                if result.created.contains(&skill.name().to_string()) {
                    self.register(skill);
                }
            }
        }

        Some(result)
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
        let skills = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        skills
            .values()
            .filter(|s| matches_activation(s.activation(), input, pressure_name, alert_kinds, &[]))
            .map(|s| SkillSummary {
                name: s.name().to_string(),
                description: s.description().to_string(),
            })
            .collect()
    }

    /// Return skills whose activation conditions match the given event kinds.
    #[must_use]
    pub fn activated_skills_for_events(&self, event_kinds: &[String]) -> Vec<SkillSummary> {
        let skills = self
            .skills
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        skills
            .values()
            .filter(|s| matches_activation(s.activation(), "", "normal", &[], event_kinds))
            .map(|s| SkillSummary {
                name: s.name().to_string(),
                description: s.description().to_string(),
            })
            .collect()
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

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::SkillSource;

    struct TestSkill {
        name: &'static str,
        desc: &'static str,
    }

    impl TestSkill {
        const fn new() -> Self {
            Self {
                name: "test",
                desc: "a test skill",
            }
        }
    }

    impl Skill for TestSkill {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            self.desc
        }
        fn when_to_use(&self) -> &str {
            self.desc
        }
        fn content(&self, args: &str) -> SkillContent {
            SkillContent::Markdown(format!("test: {args}"))
        }
        fn metadata(&self) -> SkillMetadata {
            SkillMetadata {
                source: SkillSource::System,
                ..SkillMetadata::default()
            }
        }
    }

    #[test]
    fn register_and_get() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill::new()));
        assert!(reg.contains("test"));
        assert!(!reg.contains("nope"));
    }

    #[test]
    fn later_registration_overrides() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill::new()));
        reg.register(Box::new(TestSkill::new()));
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn summaries_sorted_and_capped() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill::new()));
        let sums = reg.summaries(10);
        assert_eq!(sums.len(), 1);
        assert_eq!(sums[0].name, "test");
    }

    #[test]
    fn utility_learning_sorts_by_success() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill {
            name: "a",
            desc: "a",
        }));
        reg.register(Box::new(TestSkill {
            name: "b",
            desc: "b",
        }));
        let sums = reg.summaries(10);
        assert_eq!(sums[0].name, "a");

        reg.record_outcome("b", true);
        let sums = reg.summaries(10);
        assert_eq!(sums[0].name, "b");

        reg.record_outcome("a", true);
        reg.record_outcome("a", true);
        let sums = reg.summaries(10);
        assert_eq!(sums[0].name, "a");
    }

    #[test]
    fn utility_failure_lowers_score() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill {
            name: "x",
            desc: "x",
        }));
        reg.record_outcome("x", false);
        let score = *reg.utility_scores.read().unwrap().get("x").unwrap();
        assert!(score < 0.5);
    }

    #[test]
    fn activation_input_pattern_matches() {
        let act = SkillActivation {
            input_patterns: vec!["(?i)bug".to_string()],
            ..SkillActivation::default()
        };
        assert!(matches_activation(
            Some(&act),
            "I found a Bug",
            "normal",
            &[],
            &[]
        ));
        assert!(!matches_activation(
            Some(&act),
            "hello world",
            "normal",
            &[],
            &[]
        ));
    }

    #[test]
    fn activation_pressure_threshold() {
        let act = SkillActivation {
            pressure_above: Some("compress".to_string()),
            ..SkillActivation::default()
        };
        assert!(matches_activation(Some(&act), "", "compress", &[], &[]));
        assert!(matches_activation(Some(&act), "", "urgent", &[], &[]));
        assert!(!matches_activation(Some(&act), "", "alert", &[], &[]));
        assert!(!matches_activation(Some(&act), "", "normal", &[], &[]));
    }

    #[test]
    fn activation_alert_kind_matches() {
        let act = SkillActivation {
            alert_kinds: vec!["DoomLoop".to_string()],
            ..SkillActivation::default()
        };
        assert!(matches_activation(
            Some(&act),
            "",
            "normal",
            &["DoomLoop".to_string()],
            &[]
        ));
        assert!(!matches_activation(
            Some(&act),
            "",
            "normal",
            &["Fatigue".to_string()],
            &[]
        ));
    }

    #[test]
    fn activation_event_kind_matches() {
        let act = SkillActivation {
            event_kinds: vec!["QualityCheckSuggested".to_string()],
            ..SkillActivation::default()
        };
        assert!(matches_activation(
            Some(&act),
            "",
            "normal",
            &[],
            &["QualityCheckSuggested".to_string()]
        ));
        assert!(!matches_activation(
            Some(&act),
            "",
            "normal",
            &[],
            &["ExplorationTriggered".to_string()]
        ));
    }

    #[test]
    fn activation_none_never_activates() {
        assert!(!matches_activation(
            None,
            "anything",
            "degrade",
            &["DoomLoop".to_string()],
            &[]
        ));
    }

    #[test]
    fn invalid_regex_detected_by_validation() {
        let act = SkillActivation {
            input_patterns: vec!["(?P<broken".to_string(), "(?i)valid".to_string()],
            ..SkillActivation::default()
        };
        let warnings = validate_activation_patterns(Some(&act), "test-skill");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("broken"));
        assert!(warnings[0].contains("test-skill"));
    }

    #[test]
    fn valid_regex_produces_no_warnings() {
        let act = SkillActivation {
            input_patterns: vec!["(?i)(review|audit)".to_string()],
            ..SkillActivation::default()
        };
        let warnings = validate_activation_patterns(Some(&act), "review");
        assert!(warnings.is_empty());
    }

    #[test]
    fn invalid_regex_does_not_match_but_other_dims_work() {
        let act = SkillActivation {
            input_patterns: vec!["(?P<broken".to_string()],
            alert_kinds: vec!["DoomLoop".to_string()],
            ..SkillActivation::default()
        };
        assert!(!matches_activation(
            Some(&act),
            "anything",
            "normal",
            &[],
            &[]
        ));
        assert!(matches_activation(
            Some(&act),
            "",
            "normal",
            &["DoomLoop".to_string()],
            &[]
        ));
    }

    #[test]
    fn three_tier_override() {
        let reg = SkillRegistry::new();
        reg.register(Box::new(TestSkill {
            name: "x",
            desc: "system",
        }));
        reg.register(Box::new(TestSkill {
            name: "x",
            desc: "instance",
        }));
        assert_eq!(
            reg.with_skill("x", |s| s.description().to_string()),
            Some("instance".into())
        );
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn tool_call_history_tracking() {
        let reg = SkillRegistry::new();
        reg.record_tool_call("read");
        reg.record_tool_call("grep");
        reg.record_tool_call("read");
        reg.record_tool_call("grep");
        reg.record_tool_call("read");
        reg.record_tool_call("grep");
        let suggestions = reg.suggest_skills();
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn tool_call_history_capped() {
        let reg = SkillRegistry::new();
        for i in 0..150 {
            reg.record_tool_call(&format!("tool_{i}"));
        }
        let len = reg.tool_call_history.read().unwrap().len();
        assert_eq!(len, 100);
    }

    #[test]
    fn hot_reload_adds_new_skills() {
        let dir = tempfile::tempdir().unwrap();
        let sd = dir.path().join("hot-skill");
        std::fs::create_dir_all(&sd).unwrap();
        std::fs::write(sd.join("SKILL.md"), "---\ndescription: Hot\n---\nContent").unwrap();

        let reg = SkillRegistry::new();
        assert!(!reg.contains("hot-skill"));

        reg.reload_from(dir.path(), &SkillSource::Instance);
        assert!(reg.contains("hot-skill"));
    }
}
