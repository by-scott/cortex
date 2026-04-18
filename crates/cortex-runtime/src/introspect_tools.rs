//! Self-introspection tools — give the LLM read access to Cortex's own state.
//!
//! These tools use the daemon's home directory to open read-only handles to
//! the journal, prompt files, and session store. They don't share mutable
//! state with the daemon, avoiding Arc refactoring.

use cortex_turn::tools::{Tool, ToolError, ToolResult};
use std::path::PathBuf;

/// Paths needed for read-only introspection.
#[derive(Clone)]
pub struct IntrospectPaths {
    pub home: PathBuf,
    pub data_dir: PathBuf,
}

// ── Audit Tool ──────────────────────────────────────────────

pub struct AuditTool {
    paths: IntrospectPaths,
}

impl AuditTool {
    #[must_use]
    pub const fn new(paths: IntrospectPaths) -> Self {
        Self { paths }
    }
}

impl Tool for AuditTool {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn description(&self) -> &'static str {
        "Query the audit log — event counts, health score, recent events.\n\n\
         Commands: summary (default), health, recent"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "enum": ["summary", "health", "recent"] },
                "limit": { "type": "integer", "description": "Events for 'recent' (default: 20)" }
            }
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let cmd = input
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("summary");

        let journal = cortex_kernel::Journal::open(self.paths.data_dir.join("cortex.db"))
            .map_err(|e| ToolError::ExecutionFailed(format!("open journal: {e}")))?;

        let events = journal.recent_events(500).unwrap_or_default();
        let summary = cortex_turn::observability::AuditAggregator::summarize(&events);

        match cmd {
            "summary" => Ok(ToolResult::success(
                serde_json::to_string_pretty(&summary).unwrap_or_default(),
            )),
            "health" => {
                let score = if summary.turn_count == 0 {
                    1.0
                } else {
                    let r = f64::from(u32::try_from(summary.meta_alert_count).unwrap_or(u32::MAX))
                        / f64::from(u32::try_from(summary.turn_count).unwrap_or(u32::MAX));
                    (1.0 - r)
                        .clamp(0.0, 1.0)
                        .mul_add(0.5, summary.avg_confidence * 0.5)
                };
                Ok(ToolResult::success(format!(
                    "Health: {score:.2}\nTurns: {}\nAlerts: {}\nAvg confidence: {:.2}",
                    summary.turn_count, summary.meta_alert_count, summary.avg_confidence
                )))
            }
            "recent" => {
                let limit = input
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(20)
                    .try_into()
                    .unwrap_or(20);
                let recent = journal.recent_events(limit).unwrap_or_default();
                let lines: Vec<String> = recent
                    .iter()
                    .map(|e| {
                        format!(
                            "[{}] {} (turn={}, corr={})",
                            e.timestamp.format("%H:%M:%S"),
                            e.event_type,
                            &e.turn_id[..8.min(e.turn_id.len())],
                            &e.correlation_id[..8.min(e.correlation_id.len())],
                        )
                    })
                    .collect();
                Ok(ToolResult::success(format!(
                    "{} events:\n{}",
                    lines.len(),
                    lines.join("\n")
                )))
            }
            other => Err(ToolError::InvalidInput(format!("unknown: {other}"))),
        }
    }
}

// ── Prompt Inspect Tool ─────────────────────────────────────

pub struct PromptInspectTool {
    paths: IntrospectPaths,
}

impl PromptInspectTool {
    #[must_use]
    pub const fn new(paths: IntrospectPaths) -> Self {
        Self { paths }
    }
}

impl Tool for PromptInspectTool {
    fn name(&self) -> &'static str {
        "prompt_inspect"
    }

    fn description(&self) -> &'static str {
        "Read your own prompt layers — Soul, Identity, Behavioral, User.\n\n\
         Self-awareness tool: inspect what drives your behavior."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "layer": {
                    "type": "string",
                    "enum": ["soul", "identity", "behavioral", "user", "all"]
                }
            }
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let layer = input
            .get("layer")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("all");
        let prompts_dir = self.paths.home.join("prompts");

        let files: Vec<(&str, &str)> = match layer {
            "soul" => vec![("Soul", "soul.md")],
            "identity" => vec![("Identity", "identity.md")],
            "behavioral" => vec![("Behavioral", "behavioral.md")],
            "user" => vec![("User", "user.md")],
            "all" => vec![
                ("Soul", "soul.md"),
                ("Identity", "identity.md"),
                ("Behavioral", "behavioral.md"),
                ("User", "user.md"),
            ],
            other => return Err(ToolError::InvalidInput(format!("unknown layer: {other}"))),
        };

        let mut parts = Vec::new();
        for (name, file) in &files {
            let path = prompts_dir.join(file);
            match std::fs::read_to_string(&path) {
                Ok(content) => parts.push(content),
                Err(_) => parts.push(format!("({name} layer not found)")),
            }
        }

        Ok(ToolResult::success(parts.join("\n\n---\n\n")))
    }
}

// ── Memory Graph Tool ───────────────────────────────────────

pub struct MemoryGraphTool {
    paths: IntrospectPaths,
}

impl MemoryGraphTool {
    #[must_use]
    pub const fn new(paths: IntrospectPaths) -> Self {
        Self { paths }
    }
}

impl Tool for MemoryGraphTool {
    fn name(&self) -> &'static str {
        "memory_graph"
    }

    fn description(&self) -> &'static str {
        "Query the entity relationship graph built from memory.\n\n\
         Commands:\n\
         - `neighbors <entity>`: direct connections of an entity\n\
         - `stats`: graph size (nodes, edges)\n\
         - `hubs`: most connected entities"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "enum": ["neighbors", "stats", "hubs"] },
                "entity": { "type": "string", "description": "Entity name (for neighbors)" },
                "limit": { "type": "integer", "description": "Max results (default: 10)" }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let cmd = input
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing 'command'".into()))?;

        let db_path = self.paths.data_dir.join("memory_graph.db");
        let graph = cortex_kernel::MemoryGraph::open(&db_path)
            .map_err(|e| ToolError::ExecutionFailed(format!("open graph: {e}")))?;

        match cmd {
            "stats" => {
                let nodes = graph.all_node_ids().map_or(0, |s| s.len());
                let edges = graph.all_relations().map_or(0, |r| r.len());
                Ok(ToolResult::success(format!(
                    "Graph: {nodes} entities, {edges} relations"
                )))
            }
            "hubs" => {
                let limit: usize = input
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(10)
                    .try_into()
                    .unwrap_or(10);
                let degrees = graph.degree_map().unwrap_or_default();
                let mut sorted: Vec<_> = degrees.into_iter().collect();
                sorted.sort_by_key(|item| std::cmp::Reverse(item.1));
                sorted.truncate(limit);
                if sorted.is_empty() {
                    return Ok(ToolResult::success("Graph is empty".to_string()));
                }
                let lines: Vec<String> = sorted
                    .iter()
                    .map(|(name, deg)| format!("  {name}: {deg} connections"))
                    .collect();
                Ok(ToolResult::success(format!(
                    "Top entities:\n{}",
                    lines.join("\n")
                )))
            }
            "neighbors" => {
                let entity = input
                    .get("entity")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| ToolError::InvalidInput("'entity' required".into()))?;
                let rels = graph.relations_of(entity).unwrap_or_default();
                if rels.is_empty() {
                    return Ok(ToolResult::success(format!(
                        "No connections for '{entity}'"
                    )));
                }
                let lines: Vec<String> = rels
                    .iter()
                    .map(|r| {
                        format!(
                            "  {} --[{}]--> {}",
                            r.source_id, r.relation_type, r.target_id
                        )
                    })
                    .collect();
                Ok(ToolResult::success(format!(
                    "{entity} ({} edges):\n{}",
                    lines.len(),
                    lines.join("\n")
                )))
            }
            other => Err(ToolError::InvalidInput(format!("unknown: {other}"))),
        }
    }
}

/// Register introspection tools into a tool registry.
pub fn register_introspect_tools(
    registry: &mut cortex_turn::tools::ToolRegistry,
    home: &std::path::Path,
    data_dir: &std::path::Path,
) {
    let paths = IntrospectPaths {
        home: home.to_path_buf(),
        data_dir: data_dir.to_path_buf(),
    };
    registry.register(Box::new(AuditTool::new(paths.clone())));
    registry.register(Box::new(PromptInspectTool::new(paths.clone())));
    registry.register(Box::new(MemoryGraphTool::new(paths)));
}
