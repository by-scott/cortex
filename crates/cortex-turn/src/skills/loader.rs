use cortex_types::{ExecutionMode, SkillActivation, SkillMetadata, SkillParameter, SkillSource};
use serde::Deserialize;
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

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    when_to_use: Option<String>,
    #[serde(default)]
    parameters: Vec<SkillParameter>,
    #[serde(default)]
    required_tools: Vec<String>,
    execution_mode: Option<ExecutionMode>,
    timeout_secs: Option<u64>,
    #[serde(default)]
    tags: Vec<String>,
    user_invocable: Option<bool>,
    agent_invocable: Option<bool>,
    version: Option<String>,
    activation: Option<SkillActivation>,
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
                    "Warning: skipped skill '{name}': SKILL.md requires YAML frontmatter (---) with 'name' and 'description' fields"
                );
                return None;
            };
            Some(Box::new(skill) as Box<dyn Skill>)
        })
        .collect()
}

fn parse_skill_md(_name: &str, raw: &str, path: &Path, source: &SkillSource) -> Option<DiskSkill> {
    let (frontmatter, markdown) = split_yaml_frontmatter(raw)?;
    let fm: SkillFrontmatter = serde_norway::from_str(frontmatter).ok()?;
    let skill_name = fm.name?;
    let desc = fm.description?;

    Some(DiskSkill {
        skill_name,
        desc,
        when: fm.when_to_use.unwrap_or_default(),
        params: fm.parameters,
        tools: fm.required_tools,
        exec_mode: fm.execution_mode.unwrap_or_default(),
        timeout: fm.timeout_secs,
        tags: fm.tags,
        user_inv: fm.user_invocable.unwrap_or(true),
        agent_inv: fm.agent_invocable.unwrap_or(true),
        version: fm.version,
        activation: fm.activation,
        markdown: markdown.trim().to_string(),
        source: source.clone(),
        path: path.to_path_buf(),
    })
}

fn split_yaml_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let stripped = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = stripped.find("\n---")?;
    let frontmatter = &stripped[..end];
    let markdown = stripped[end + "\n---".len()..].trim_start_matches(['\r', '\n']);
    Some((frontmatter, markdown))
}
