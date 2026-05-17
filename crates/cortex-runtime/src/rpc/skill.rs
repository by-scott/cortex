use super::{
    INVALID_PARAMS, METHOD_NOT_FOUND, RpcHandler, RpcRequest, RpcResponse, error, success,
};

impl RpcHandler {
    pub(super) fn handle_skill_list(&self, req: &RpcRequest) -> RpcResponse {
        let registry = self.state.skill_registry();
        let skills: Vec<serde_json::Value> = registry
            .user_invocable()
            .iter()
            .filter_map(|summary| {
                registry.with_skill(&summary.name, |s| {
                    serde_json::json!({
                        "name": s.name(),
                        "description": s.description(),
                        "user_invocable": s.metadata().user_invocable,
                        "agent_invocable": s.metadata().agent_invocable,
                        "execution_mode": format!("{:?}", s.execution_mode()),
                    })
                })
            })
            .collect();
        success(req.id.clone(), serde_json::json!({ "skills": skills }))
    }

    pub(super) fn handle_skill_invoke(&self, req: &RpcRequest) -> RpcResponse {
        let name = req
            .params
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            return error(req.id.clone(), INVALID_PARAMS, "missing 'name' parameter");
        }
        let registry = self.state.skill_registry();
        let args = req
            .params
            .get("args")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let Some(content) = registry
            .with_skill(name, |s| {
                if !s.metadata().user_invocable {
                    return None;
                }
                let cortex_turn::skills::SkillContent::Markdown(c) = s.content(args);
                Some(c)
            })
            .flatten()
        else {
            return error(
                req.id.clone(),
                METHOD_NOT_FOUND,
                &format!("skill '{name}' not found"),
            );
        };
        success(
            req.id.clone(),
            serde_json::json!({
                "name": name,
                "content": content,
            }),
        )
    }

    pub(super) fn handle_skill_suggestions(&self, req: &RpcRequest) -> RpcResponse {
        let input = req
            .params
            .get("input")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let registry = self.state.skill_registry();

        let mut suggestions: Vec<serde_json::Value> = registry
            .suggest_skills()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "description": s.description,
                    "tool_sequence": s.tool_sequence,
                    "frequency": s.frequency,
                })
            })
            .collect();

        if !input.is_empty() {
            let expanded = expand_keywords_with_synonyms(input);
            append_keyword_matches(registry, &expanded, &mut suggestions);
        }

        success(
            req.id.clone(),
            serde_json::json!({ "suggestions": suggestions }),
        )
    }
}

/// Expand input keywords with synonym groups for skill matching.
fn expand_keywords_with_synonyms(input: &str) -> Vec<String> {
    let input_lower = input.to_lowercase();
    let keywords: Vec<&str> = input_lower.split_whitespace().collect();

    let synonym_groups: &[&[&str]] = &[
        &[
            "debug",
            "debugging",
            "crash",
            "bug",
            "broken",
            "failing",
            "error",
            "fix",
        ],
        &[
            "plan",
            "planning",
            "decompose",
            "breakdown",
            "organize",
            "structure",
        ],
        &["review", "examine", "inspect", "audit", "check", "scrutiny"],
        &[
            "orient",
            "understand",
            "explore",
            "unfamiliar",
            "new",
            "codebase",
        ],
        &[
            "decide",
            "deliberate",
            "evaluate",
            "compare",
            "choose",
            "tradeoff",
        ],
        &["diagnose", "root cause", "trace", "symptom", "investigate"],
    ];

    let mut expanded: Vec<String> = keywords.iter().map(|s| (*s).to_string()).collect();
    for kw in &keywords {
        for group in synonym_groups {
            if group.contains(kw) {
                for syn in *group {
                    let s = (*syn).to_string();
                    if !expanded.contains(&s) {
                        expanded.push(s);
                    }
                }
            }
        }
    }
    expanded
}

/// Append keyword-matched skills to the suggestions list.
fn append_keyword_matches(
    registry: &cortex_turn::skills::SkillRegistry,
    expanded: &[String],
    suggestions: &mut Vec<serde_json::Value>,
) {
    registry.with_all_skills(|skills| {
        for skill in skills {
            if !skill.metadata().user_invocable {
                continue;
            }
            let desc_lower = skill.description().to_lowercase();
            let when_lower = skill.when_to_use().to_lowercase();
            let name = skill.name();
            if suggestions
                .iter()
                .any(|s| s.get("name").and_then(|v| v.as_str()) == Some(name))
            {
                continue;
            }
            let haystack = format!("{desc_lower} {when_lower} {name}");
            let hits = expanded
                .iter()
                .filter(|kw| kw.len() >= 3 && haystack.contains(kw.as_str()))
                .count();
            if hits >= 1 {
                suggestions.push(serde_json::json!({
                    "name": name,
                    "description": skill.description(),
                    "match_type": "keyword",
                }));
            }
        }
    });
}
