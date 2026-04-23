use cortex_types::{ExecutionMode, SkillActivation, SkillMetadata, SkillParameter, SkillSource};
use std::fs;
use std::path::{Path, PathBuf};

use super::{Skill, SkillContent};

/// A skill loaded from a `SKILL.md` file.
pub struct DiskSkill {
    skill_name: String,
    desc: String,
    when: String,
    params: Vec<SkillParameter>,
    tools: Vec<String>,
    exec_mode: ExecutionMode,
    timeout: Option<u64>,
    tags: Vec<String>,
    user_inv: bool,
    agent_inv: bool,
    version: Option<String>,
    activation: Option<SkillActivation>,
    markdown: String,
    source: SkillSource,
    path: PathBuf,
}

impl Skill for DiskSkill {
    fn name(&self) -> &str {
        &self.skill_name
    }
    fn description(&self) -> &str {
        &self.desc
    }
    fn when_to_use(&self) -> &str {
        &self.when
    }
    fn parameters(&self) -> Vec<SkillParameter> {
        self.params.clone()
    }
    fn required_tools(&self) -> Vec<&str> {
        self.tools.iter().map(String::as_str).collect()
    }
    fn timeout_secs(&self) -> Option<u64> {
        self.timeout
    }
    fn execution_mode(&self) -> ExecutionMode {
        self.exec_mode
    }
    fn content(&self, args: &str) -> SkillContent {
        SkillContent::Markdown(self.markdown.replace("${ARGS}", args))
    }
    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            source: self.source.clone(),
            version: self.version.clone(),
            tags: self.tags.clone(),
            user_invocable: self.user_inv,
            agent_invocable: self.agent_inv,
            path: Some(self.path.clone()),
        }
    }
    fn activation(&self) -> Option<&SkillActivation> {
        self.activation.as_ref()
    }
}

/// Load all `SKILL.md` files from `base_dir/<name>/SKILL.md`.
#[must_use]
pub fn load_skills(base_dir: &Path, source: &SkillSource) -> Vec<Box<dyn Skill>> {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .filter_map(|entry| {
            let file = entry.path().join("SKILL.md");
            let name = entry.file_name().to_str()?.to_string();
            let raw = fs::read_to_string(&file).ok()?;
            let Some(skill) = parse_skill_md(&name, &raw, &file, source) else {
                eprintln!(
                    "Warning: skipped skill '{name}': SKILL.md requires YAML frontmatter (---) with a 'description' field"
                );
                return None;
            };
            Some(Box::new(skill) as Box<dyn Skill>)
        })
        .collect()
}

fn parse_skill_md(name: &str, raw: &str, path: &Path, source: &SkillSource) -> Option<DiskSkill> {
    let stripped = raw.strip_prefix("---")?;
    let end = stripped.find("---")?;
    let fm: serde_json::Value = serde_yaml::from_str(&stripped[..end]).ok()?;
    let markdown = stripped[end + 3..].trim().to_string();
    let desc = fm.get("description")?.as_str()?.to_string();

    Some(DiskSkill {
        skill_name: fm
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string(),
        desc,
        when: fm
            .get("when_to_use")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        params: parse_params(&fm),
        tools: str_array(&fm, "required_tools"),
        exec_mode: match fm.get("execution_mode").and_then(|v| v.as_str()) {
            Some("fork") => ExecutionMode::Fork,
            _ => ExecutionMode::Inline,
        },
        timeout: fm.get("timeout_secs").and_then(serde_json::Value::as_u64),
        tags: str_array(&fm, "tags"),
        user_inv: fm
            .get("user_invocable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        agent_inv: fm
            .get("agent_invocable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        version: fm.get("version").and_then(|v| v.as_str()).map(String::from),
        activation: fm
            .get("activation")
            .and_then(|v| serde_json::from_value::<SkillActivation>(v.clone()).ok()),
        markdown,
        source: source.clone(),
        path: path.to_path_buf(),
    })
}

fn str_array(fm: &serde_json::Value, key: &str) -> Vec<String> {
    fm.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_params(fm: &serde_json::Value) -> Vec<SkillParameter> {
    fm.get("parameters")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(SkillParameter {
                        name: p.get("name")?.as_str()?.to_string(),
                        description: p
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        required: p
                            .get("required")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
