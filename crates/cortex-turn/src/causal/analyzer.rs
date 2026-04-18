use cortex_kernel::StoredEvent;
use cortex_types::{CausalChain, CausalLink, CausalRelation, Payload};
use std::fmt::Write;

use crate::llm::LlmClient;

/// Default time window (ms) for temporal proximity candidate filtering.
const DEFAULT_TIME_WINDOW_MS: i64 = 5000;

/// Known causal patterns: `(cause_event_type, effect_event_type, relation, base_confidence)`.
const KNOWN_PATTERNS: &[(&str, &str, CausalRelation, f64)] = &[
    // Context pressure triggers compression
    (
        "ContextPressureObserved",
        "ContextCompacted",
        CausalRelation::Triggers,
        0.9,
    ),
    // Tool intent triggers tool result
    (
        "ToolInvocationIntent",
        "ToolInvocationResult",
        CausalRelation::Triggers,
        0.95,
    ),
    // Permission requested triggers granted/denied
    (
        "PermissionRequested",
        "PermissionGranted",
        CausalRelation::Triggers,
        0.9,
    ),
    (
        "PermissionRequested",
        "PermissionDenied",
        CausalRelation::Triggers,
        0.9,
    ),
    // Low confidence enables meta control
    (
        "ConfidenceLow",
        "MetaControlApplied",
        CausalRelation::Enables,
        0.7,
    ),
    // Impasse triggers meta control
    (
        "ImpasseDetected",
        "MetaControlApplied",
        CausalRelation::Triggers,
        0.85,
    ),
    // Turn started triggers user message
    ("TurnStarted", "UserMessage", CausalRelation::Triggers, 0.95),
    // Working memory capacity exceeded triggers eviction
    (
        "WorkingMemoryCapacityExceeded",
        "WorkingMemoryItemEvicted",
        CausalRelation::Triggers,
        0.9,
    ),
    // Pressure response enables context compacted
    (
        "PressureResponseApplied",
        "ContextCompacted",
        CausalRelation::Enables,
        0.8,
    ),
    // Reasoning started triggers reasoning completed
    (
        "ReasoningStarted",
        "ReasoningChainCompleted",
        CausalRelation::Triggers,
        0.95,
    ),
    // Frame check enables meta control
    (
        "FrameCheckResult",
        "MetaControlApplied",
        CausalRelation::Enables,
        0.6,
    ),
    // Emergency triggered causes meta control
    (
        "EmergencyTriggered",
        "MetaControlApplied",
        CausalRelation::Triggers,
        0.8,
    ),
];

/// Causal analysis engine -- discovers cause-effect relationships from event sequences.
pub struct CausalAnalyzer {
    time_window_ms: i64,
}

impl CausalAnalyzer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            time_window_ms: DEFAULT_TIME_WINDOW_MS,
        }
    }

    #[must_use]
    pub const fn with_time_window(mut self, ms: i64) -> Self {
        self.time_window_ms = ms;
        self
    }

    /// Full analysis pipeline: heuristic first, then optional LLM enhancement.
    ///
    /// Returns discovered causal chains and a `CausalAnalysisCompleted` event.
    pub async fn analyze(
        &self,
        events: &[StoredEvent],
        llm: Option<&dyn LlmClient>,
    ) -> (Vec<CausalChain>, Payload) {
        let mut links = self.analyze_heuristic(events);

        // LLM enhancement for unlinked events
        if let Some(llm_client) = llm {
            let linked_events: std::collections::HashSet<&str> = links
                .iter()
                .flat_map(|l| [l.cause_event.as_str(), l.effect_event.as_str()])
                .collect();

            let unlinked: Vec<&StoredEvent> = events
                .iter()
                .filter(|e| !linked_events.contains(e.event_type.as_str()))
                .collect();

            if unlinked.len() >= 2
                && let Some(llm_links) = self.analyze_with_llm(&unlinked, llm_client).await
            {
                links.extend(llm_links);
            }
        }

        let chains = self.build_chains(&links);
        let chain_count = chains.len();
        let root_causes: Vec<String> = chains.iter().map(|c| c.root_cause.clone()).collect();
        let total_links: usize = chains.iter().map(CausalChain::link_count).sum();

        let event = Payload::CausalAnalysisCompleted {
            chain_count,
            root_causes,
            total_links,
        };

        (chains, event)
    }

    /// Heuristic-based causal inference using known patterns and temporal proximity.
    ///
    /// Returns a list of discovered `CausalLink` instances.
    #[must_use]
    pub fn analyze_heuristic(&self, events: &[StoredEvent]) -> Vec<CausalLink> {
        let mut links = Vec::new();

        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                let cause = &events[i];
                let effect = &events[j];

                // Must share correlation_id
                if cause.correlation_id != effect.correlation_id {
                    continue;
                }

                // Temporal proximity check
                let delta_ms = (effect.timestamp - cause.timestamp).num_milliseconds();
                if delta_ms < 0 || delta_ms > self.time_window_ms {
                    continue;
                }

                // Check known patterns
                if let Some(pattern) = find_pattern(&cause.event_type, &effect.event_type) {
                    links.push(
                        CausalLink::new(
                            cause.event_type.clone(),
                            effect.event_type.clone(),
                            pattern.relation,
                            pattern.confidence,
                        )
                        .with_temporal_delta(delta_ms),
                    );
                }
            }
        }

        links
    }

    /// LLM-assisted causal reasoning for events not covered by heuristics.
    async fn analyze_with_llm(
        &self,
        events: &[&StoredEvent],
        llm: &dyn LlmClient,
    ) -> Option<Vec<CausalLink>> {
        if events.len() < 2 {
            return None;
        }

        let mut prompt = format!(
            "{}\nEvents:\n",
            cortex_kernel::prompt_manager::DEFAULT_CAUSAL_ANALYZE
        );

        for e in events {
            let _ = writeln!(
                prompt,
                "- [{}] {} (correlation: {})",
                e.timestamp.format("%H:%M:%S%.3f"),
                e.event_type,
                &e.correlation_id[..8.min(e.correlation_id.len())]
            );
        }

        let request = crate::llm::types::LlmRequest {
            system: Some(&prompt),
            messages: &[],
            tools: None,
            max_tokens: 2048,
            on_text: None,
        };

        let response = llm.complete(request).await.ok()?;
        let text = response.text?;

        parse_llm_causal_links(&text)
    }

    /// Build chains from individual links by connecting cause-to-effect sequences.
    ///
    /// # Panics
    /// Panics if the internal chain link list is unexpectedly empty (should not happen in practice).
    #[must_use]
    pub fn build_chains(&self, links: &[CausalLink]) -> Vec<CausalChain> {
        if links.is_empty() {
            return vec![];
        }

        // Group links into chains by following cause->effect sequences
        let mut used = vec![false; links.len()];
        let mut chains = Vec::new();

        for start_idx in 0..links.len() {
            if used[start_idx] {
                continue;
            }

            // Check if this link is the start of a chain (no other link has its cause as effect)
            let is_root = !links.iter().enumerate().any(|(j, other)| {
                j != start_idx && !used[j] && other.effect_event == links[start_idx].cause_event
            });

            if !is_root {
                continue;
            }

            let mut chain_links = vec![links[start_idx].clone()];
            used[start_idx] = true;

            // Follow the chain
            while let Some(last_link) = chain_links.last() {
                let current_effect = &last_link.effect_event;
                let next = links
                    .iter()
                    .enumerate()
                    .find(|(j, l)| !used[*j] && l.cause_event == *current_effect);

                if let Some((j, l)) = next {
                    chain_links.push(l.clone());
                    used[j] = true;
                } else {
                    break;
                }
            }

            chains.push(CausalChain::from_links(chain_links));
        }

        // Any remaining unlinked links become single-link chains
        for (i, link) in links.iter().enumerate() {
            if !used[i] {
                chains.push(CausalChain::from_links(vec![link.clone()]));
            }
        }

        // Sort by confidence descending
        chains.sort_by(|a, b| {
            b.overall_confidence
                .partial_cmp(&a.overall_confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        chains
    }

    /// Analyze causal links across multiple event streams.
    ///
    /// Merges the streams by timestamp, then runs the standard heuristic
    /// analysis on the unified timeline.  Cross-stream links are naturally
    /// detected by the existing time-window matching.
    #[must_use]
    pub fn analyze_cross_stream(&self, streams: &[&[StoredEvent]]) -> Vec<CausalLink> {
        let merged = merge_streams(streams);
        self.analyze_heuristic(&merged)
    }
}

/// Merge multiple event streams into a single timeline sorted by timestamp.
#[must_use]
pub fn merge_streams(streams: &[&[StoredEvent]]) -> Vec<StoredEvent> {
    let total: usize = streams.iter().map(|s| s.len()).sum();
    let mut merged = Vec::with_capacity(total);
    for stream in streams {
        merged.extend_from_slice(stream);
    }
    merged.sort_by_key(|a| a.timestamp);
    merged
}

impl Default for CausalAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

struct PatternMatch {
    relation: CausalRelation,
    confidence: f64,
}

fn find_pattern(cause_type: &str, effect_type: &str) -> Option<PatternMatch> {
    KNOWN_PATTERNS
        .iter()
        .find(|(c, e, _, _)| *c == cause_type && *e == effect_type)
        .map(|(_, _, rel, conf)| PatternMatch {
            relation: *rel,
            confidence: *conf,
        })
}

fn parse_llm_causal_links(text: &str) -> Option<Vec<CausalLink>> {
    // Try direct JSON parse, then try extracting from markdown code block
    let trimmed = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let items: Vec<serde_json::Value> = serde_json::from_str(trimmed).ok()?;
    let mut links = Vec::new();

    for item in &items {
        let cause = item.get("cause")?.as_str()?;
        let effect = item.get("effect")?.as_str()?;
        let relation_str = item
            .get("relation")
            .and_then(|r| r.as_str())
            .unwrap_or("contributes");
        let confidence = item
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.5);

        let relation = match relation_str {
            "triggers" => CausalRelation::Triggers,
            "enables" => CausalRelation::Enables,
            _ => CausalRelation::Contributes,
        };

        links.push(CausalLink::new(
            cause.to_string(),
            effect.to_string(),
            relation,
            confidence,
        ));
    }

    if links.is_empty() { None } else { Some(links) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn make_event(event_type: &str, correlation_id: &str, offset_ms: i64) -> StoredEvent {
        let base = Utc.with_ymd_and_hms(2026, 4, 10, 12, 0, 0).unwrap();
        StoredEvent {
            offset: u64::try_from(offset_ms).unwrap_or(0),
            event_id: format!("evt-{offset_ms}"),
            turn_id: String::from("turn-1"),
            correlation_id: String::from(correlation_id),
            timestamp: base + chrono::Duration::milliseconds(offset_ms),
            event_type: String::from(event_type),
            payload: Payload::TurnStarted, // payload not used by analyzer
            execution_version: String::new(),
        }
    }

    #[test]
    fn known_pattern_detected() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("ContextPressureObserved", "corr-1", 0),
            make_event("ContextCompacted", "corr-1", 1000),
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].cause_event, "ContextPressureObserved");
        assert_eq!(links[0].effect_event, "ContextCompacted");
        assert_eq!(links[0].relation, CausalRelation::Triggers);
        assert!(links[0].confidence >= 0.8);
    }

    #[test]
    fn different_correlation_not_linked() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("ContextPressureObserved", "corr-1", 0),
            make_event("ContextCompacted", "corr-2", 1000),
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert!(links.is_empty());
    }

    #[test]
    fn outside_time_window_not_linked() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("ContextPressureObserved", "corr-1", 0),
            make_event("ContextCompacted", "corr-1", 10000), // 10s > 5s window
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert!(links.is_empty());
    }

    #[test]
    fn unknown_pattern_not_heuristically_linked() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("GoalSet", "corr-1", 0),
            make_event("MemoryCaptured", "corr-1", 1000),
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert!(links.is_empty());
    }

    #[test]
    fn multiple_patterns_in_sequence() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("TurnStarted", "corr-1", 0),
            make_event("UserMessage", "corr-1", 100),
            make_event("ToolInvocationIntent", "corr-1", 500),
            make_event("ToolInvocationResult", "corr-1", 2000),
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert!(links.len() >= 2); // TurnStarted->UserMessage and Intent->Result
    }

    #[test]
    fn build_chains_links_sequence() {
        let analyzer = CausalAnalyzer::new();
        let links = vec![
            CausalLink::new("A", "B", CausalRelation::Triggers, 0.9),
            CausalLink::new("B", "C", CausalRelation::Enables, 0.8),
        ];
        let chains = analyzer.build_chains(&links);
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].link_count(), 2);
        assert_eq!(chains[0].root_cause, "A");
        assert_eq!(chains[0].final_effect, "C");
    }

    #[test]
    fn build_chains_separate_chains() {
        let analyzer = CausalAnalyzer::new();
        let links = vec![
            CausalLink::new("A", "B", CausalRelation::Triggers, 0.9),
            CausalLink::new("X", "Y", CausalRelation::Triggers, 0.7),
        ];
        let chains = analyzer.build_chains(&links);
        assert_eq!(chains.len(), 2);
    }

    #[test]
    fn parse_llm_causal_links_valid_json() {
        let json = r#"[
            {"cause":"A","effect":"B","relation":"triggers","confidence":0.8},
            {"cause":"B","effect":"C","relation":"enables","confidence":0.6}
        ]"#;
        let links = parse_llm_causal_links(json).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].relation, CausalRelation::Triggers);
        assert_eq!(links[1].relation, CausalRelation::Enables);
    }

    #[test]
    fn parse_llm_causal_links_markdown_fenced() {
        let text = "```json\n[{\"cause\":\"X\",\"effect\":\"Y\",\"relation\":\"contributes\",\"confidence\":0.5}]\n```";
        let links = parse_llm_causal_links(text).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].relation, CausalRelation::Contributes);
    }

    #[test]
    fn parse_llm_causal_links_invalid() {
        assert!(parse_llm_causal_links("not json at all").is_none());
        assert!(parse_llm_causal_links("[]").is_none());
    }

    #[test]
    fn custom_time_window() {
        let analyzer = CausalAnalyzer::new().with_time_window(1000);
        let events = vec![
            make_event("ContextPressureObserved", "corr-1", 0),
            make_event("ContextCompacted", "corr-1", 2000), // 2s > 1s window
        ];
        let links = analyzer.analyze_heuristic(&events);
        assert!(links.is_empty());
    }

    #[tokio::test]
    async fn analyze_full_pipeline_heuristic_only() {
        let analyzer = CausalAnalyzer::new();
        let events = vec![
            make_event("ContextPressureObserved", "corr-1", 0),
            make_event("ContextCompacted", "corr-1", 1000),
        ];
        let (chains, event) = analyzer.analyze(&events, None).await;
        assert_eq!(chains.len(), 1);
        assert!(matches!(
            event,
            Payload::CausalAnalysisCompleted {
                chain_count: 1,
                total_links: 1,
                ..
            }
        ));
    }
}
