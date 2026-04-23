use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Payload, TurnId};

use crate::attention::ChannelScheduler;
use crate::confidence::ConfidenceTracker;
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::working_memory::WorkingMemoryManager;

use super::TurnConfig;
use super::journal_append;

/// Extract meaningful keywords from user input to seed working memory.
///
/// Returns up to five unique lowercase tokens (length >= 4).
pub fn extract_input_keywords(input: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    input
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|w| w.len() >= 4)
        .filter_map(|w| {
            let lower = w.to_lowercase();
            if seen.insert(lower.clone()) {
                Some(lower)
            } else {
                None
            }
        })
        .take(5)
        .collect()
}

pub fn init_sn_phase(
    input: &str,
    config: &TurnConfig,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) -> (WorkingMemoryManager, ChannelScheduler) {
    // Working Memory: initialize and activate input keywords
    let mut working_mem = WorkingMemoryManager::new(config.working_memory_capacity);
    {
        let keywords = extract_input_keywords(input);
        for kw in keywords {
            let result = working_mem.activate(kw, 0.8);
            for ev in result.events {
                journal_append(journal, turn_id, corr_id, &ev);
                events_log.push(ev);
            }
        }
    }

    // Attention Channels: initialize scheduler with maintenance/emergency tasks
    let mut scheduler = ChannelScheduler::new();
    let input_for_guard = input.to_string();
    scheduler.register(
        cortex_types::AttentionChannel::Emergency,
        "input_guard",
        move || {
            if let crate::guardrails::GuardResult::Suspicious(finding) =
                crate::guardrails::input_guard(&input_for_guard)
            {
                vec![Payload::EmergencyTriggered {
                    task_name: "input_guard".into(),
                    details: finding.to_string(),
                }]
            } else {
                vec![]
            }
        },
    );

    // Initial emergency check (SN phase)
    {
        let sched_events = scheduler.tick();
        for ev in sched_events {
            journal_append(journal, turn_id, corr_id, &ev);
            events_log.push(ev);
        }
    }

    (working_mem, scheduler)
}

pub fn init_reasoning(
    input: &str,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) -> ReasoningEngine {
    let mut engine = ReasoningEngine::new();
    if ReasoningEngine::should_activate(input) {
        let mode = ReasoningEngine::select_mode(input);
        let ev = engine.activate(mode, input);
        journal_append(journal, turn_id, corr_id, &ev);
        events_log.push(ev);
    }
    engine
}

pub fn init_turn_state(
    input: &str,
    config: &TurnConfig,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) -> (
    ConfidenceTracker,
    MetaMonitor,
    WorkingMemoryManager,
    ChannelScheduler,
    ReasoningEngine,
) {
    let mc = &config.metacognition;
    let mut meta_monitor = MetaMonitor::new(
        mc.doom_loop_threshold,
        mc.fatigue_threshold,
        mc.duration_limit_secs,
        mc.frame_anchoring_threshold,
        mc.frame_audit.clone(),
    );
    meta_monitor.start_turn();
    let (working_mem, scheduler) =
        init_sn_phase(input, config, journal, turn_id, corr_id, events_log);
    let reasoning_engine = init_reasoning(input, journal, turn_id, corr_id, events_log);
    (
        ConfidenceTracker::new(),
        meta_monitor,
        working_mem,
        scheduler,
        reasoning_engine,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_input_keywords_basic() {
        let kws = extract_input_keywords("read the data file please");
        assert!(kws.contains(&"read".to_string()));
        assert!(kws.contains(&"data".to_string()));
        assert!(kws.contains(&"file".to_string()));
        assert!(kws.contains(&"please".to_string()));
        // "the" is < 4 chars, excluded
        assert!(!kws.contains(&"the".to_string()));
    }

    #[test]
    fn extract_input_keywords_dedup() {
        let kws = extract_input_keywords("read read read");
        assert_eq!(kws.len(), 1);
        assert_eq!(kws[0], "read");
    }

    #[test]
    fn extract_input_keywords_max_five() {
        let kws = extract_input_keywords("one two three four five six seven eight");
        assert!(kws.len() <= 5);
    }
}
