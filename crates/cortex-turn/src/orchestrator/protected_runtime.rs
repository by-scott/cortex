use std::path::{Path, PathBuf};

use cortex_types::ToolEffect;

pub(super) fn protected_runtime_access(
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
    protected_roots: &[PathBuf],
    plugin_origin: Option<&str>,
) -> Option<String> {
    if protected_roots.is_empty() {
        return None;
    }
    let normalized_roots = normalized_protected_roots(protected_roots);
    if normalized_roots.is_empty() {
        return None;
    }
    if let Some(origin) = plugin_origin
        && plugin_runtime_state_mutation_requested(tool_name, input, effects)
    {
        return Some(format!(
            "runtime home is protected; plugin tool '{tool_name}' from '{origin}' cannot directly mutate prompt, config, session, journal, memory, or runtime state; return a proposal and use the checked PromptManager or runtime command path"
        ));
    }
    let mut hits = Vec::new();
    collect_protected_path_hits(input, &normalized_roots, &mut hits);
    if tool_name == "bash" {
        collect_bash_protected_hits(input, &normalized_roots, &mut hits);
    }
    hits.sort();
    hits.dedup();
    if hits.is_empty() {
        return None;
    }
    let mut effects_text = effects_summary(effects);
    if effects_text == "no declared effects" {
        effects_text = tool_name.to_string();
    }
    Some(format!(
        "runtime home is protected; ordinary tool '{tool_name}' cannot access {} via {effects_text}",
        hits.join(", ")
    ))
}

const RUNTIME_MUTATION_TEXT_LIMIT: usize = 4096;

fn plugin_runtime_state_mutation_requested(
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
) -> bool {
    let tool_text = tool_name.to_ascii_lowercase();
    let mut combined_text = tool_text.clone();
    collect_runtime_mutation_text(input, &mut combined_text);
    for effect in effects {
        append_runtime_mutation_text(&mut combined_text, &effect.label());
    }

    let mutating_effect = effects.iter().any(ToolEffect::is_mutating);
    let tool_names_runtime_state = text_mentions_runtime_state(&tool_text);
    let tool_names_mutation = text_mentions_mutation(&tool_text);
    let input_or_effect_names_runtime_state = text_mentions_runtime_state(&combined_text);
    let input_or_effect_names_mutation = text_mentions_mutation(&combined_text);

    (tool_names_runtime_state && input_or_effect_names_mutation)
        || (tool_names_mutation && input_or_effect_names_runtime_state)
        || (mutating_effect && input_or_effect_names_runtime_state)
}

fn effects_summary(effects: &[ToolEffect]) -> String {
    if effects.is_empty() {
        "no declared effects".to_string()
    } else {
        effects
            .iter()
            .map(ToolEffect::label)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn collect_runtime_mutation_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(text) => append_runtime_mutation_text(out, text),
        serde_json::Value::Array(values) => {
            for value in values {
                collect_runtime_mutation_text(value, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                append_runtime_mutation_text(out, key);
                collect_runtime_mutation_text(value, out);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn append_runtime_mutation_text(out: &mut String, value: &str) {
    if out.len() >= RUNTIME_MUTATION_TEXT_LIMIT {
        return;
    }
    out.push(' ');
    out.push_str(&value.to_ascii_lowercase());
}

fn text_mentions_runtime_state(text: &str) -> bool {
    const TERMS: [&str; 20] = [
        "prompt",
        "prompts",
        "soul",
        "identity",
        "behavioral",
        "user.md",
        "prompt template",
        "system template",
        "bootstrap template",
        "config",
        "session",
        "journal",
        "memory",
        "runtime",
        "runtime state",
        "instance state",
        "daemon state",
        "cortex_home",
        "instance home",
        "self-evolution",
    ];
    TERMS.iter().any(|term| text.contains(term))
}

fn text_mentions_mutation(text: &str) -> bool {
    const TERMS: [&str; 13] = [
        "apply",
        "commit",
        "edit",
        "evolve",
        "evolution",
        "modify",
        "patch",
        "persist",
        "replace",
        "rewrite",
        "save",
        "set",
        "update",
    ];
    TERMS.iter().any(|term| text.contains(term))
}

fn normalized_protected_roots(protected_roots: &[PathBuf]) -> Vec<String> {
    protected_roots
        .iter()
        .filter_map(|root| normalize_existing_or_lexical(root))
        .map(|root| ensure_trailing_separator(&root))
        .collect()
}

fn collect_protected_path_hits(
    value: &serde_json::Value,
    protected_roots: &[String],
    hits: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(raw) => {
            if looks_like_path(raw)
                && let Some(path) = normalize_existing_or_lexical(Path::new(raw))
                && is_under_protected_root(&path, protected_roots)
            {
                hits.push(path);
            }
        }
        serde_json::Value::Array(values) => {
            for item in values {
                collect_protected_path_hits(item, protected_roots, hits);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_protected_path_hits(item, protected_roots, hits);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn collect_bash_protected_hits(
    input: &serde_json::Value,
    protected_roots: &[String],
    hits: &mut Vec<String>,
) {
    let Some(command) = input.get("command").and_then(serde_json::Value::as_str) else {
        return;
    };
    for root in protected_roots {
        let root_without_sep = root.trim_end_matches('/');
        if command.contains(root) || command.contains(root_without_sep) {
            hits.push(root_without_sep.to_string());
        }
        if let Some(home_suffix) = protected_home_suffix(root_without_sep)
            && command.contains(&home_suffix)
        {
            hits.push(home_suffix);
        }
    }
}

fn protected_home_suffix(root: &str) -> Option<String> {
    let marker = "/.cortex/";
    root.find(marker).map(|index| {
        let relative = &root[index + 1..];
        format!("~/{relative}")
    })
}

fn looks_like_path(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('/')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.contains('/')
}

fn normalize_existing_or_lexical(path: &Path) -> Option<String> {
    let expanded = expand_home(path);
    std::fs::canonicalize(&expanded)
        .or_else(|_| canonicalize_existing_parent(&expanded))
        .or_else(|_| lexical_absolute(&expanded))
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn canonicalize_existing_parent(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut missing = Vec::new();
    let mut cursor = absolute.as_path();
    while !cursor.exists() {
        let Some(name) = cursor.file_name() else {
            return lexical_absolute(&absolute);
        };
        missing.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return lexical_absolute(&absolute);
        };
        cursor = parent;
    }
    let mut resolved = std::fs::canonicalize(cursor)?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn expand_home(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let Some(rest) = raw.strip_prefix("~/") else {
        return path.to_path_buf();
    };
    std::env::var_os("HOME")
        .map_or_else(|| path.to_path_buf(), |home| PathBuf::from(home).join(rest))
}

fn lexical_absolute(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn ensure_trailing_separator(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn is_under_protected_root(path: &str, protected_roots: &[String]) -> bool {
    let path_with_separator = ensure_trailing_separator(path);
    protected_roots
        .iter()
        .any(|root| path_with_separator.starts_with(root))
}
