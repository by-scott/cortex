use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use cortex_types::{
    FeedbackAttribution, FeedbackReplayCheck, MemoryClaim, MemoryEntry, MemoryEvidence, MemoryKind,
    MemorySource, MemoryStatus, MemoryType, MemoryUsageOutcome,
};
use serde::{Deserialize, Serialize};

use crate::util::atomic_write;

pub struct MemoryStore {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Frontmatter {
    id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    claim_id: String,
    #[serde(rename = "type")]
    memory_type: MemoryType,
    kind: MemoryKind,
    status: MemoryStatus,
    strength: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    created_at: String,
    updated_at: String,
    access_count: u32,
    #[serde(default = "default_memory_owner_actor")]
    owner_actor: String,
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    source: MemorySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reconsolidation_until: Option<String>,
    #[serde(default, skip_serializing_if = "MemoryClaim::is_empty")]
    claim: MemoryClaim,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    evidence_events: Vec<MemoryEvidence>,
    #[serde(default)]
    confirmed_by_user: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    contradicted_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    supersedes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    risk_if_wrong: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    usage_outcomes: Vec<MemoryUsageOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feedback_attributions: Vec<FeedbackAttribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    feedback_replay_checks: Vec<FeedbackReplayCheck>,
}

fn default_memory_owner_actor() -> String {
    "local:default".to_string()
}

/// Generate a human-readable slug from content, max 50 chars.
fn slugify(content: &str) -> String {
    let mut slug: String = content
        .chars()
        .take(50)
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    // Collapse multiple dashes
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    // Trim leading/trailing dashes
    slug.trim_matches('-').to_string()
}

/// Generate the filename for a memory entry: `{type}_{slug}.md`
///
/// Prefers `description` (LLM-generated one-line summary) for the slug when
/// available; falls back to `content` truncation for manually saved entries.
fn memory_filename(entry: &MemoryEntry) -> String {
    let source = if entry.description.is_empty() {
        &entry.content
    } else {
        &entry.description
    };
    let slug = slugify(source);
    if slug.is_empty() {
        // Fallback to UUID if neither description nor content produces a slug
        return format!("{}.md", entry.id);
    }
    format!("{}_{slug}.md", entry.memory_type)
}

/// Return a path that avoids collisions with entries that have a different ID.
fn unique_memory_path(dir: &Path, entry: &MemoryEntry) -> PathBuf {
    let base_filename = memory_filename(entry);
    let path = dir.join(&base_filename);
    if !path.exists() {
        return path;
    }
    // Check if existing file has same ID (overwrite is OK)
    if let Ok(raw) = fs::read_to_string(&path)
        && let Ok(existing) = parse_memory_file(&raw)
        && existing.id == entry.id
    {
        return path;
    }
    // Collision: append full ID to guarantee uniqueness
    let stem = base_filename.trim_end_matches(".md");
    dir.join(format!("{stem}-{}.md", entry.id))
}

impl MemoryStore {
    /// # Errors
    /// Returns `io::Error` if the directory cannot be created.
    pub fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// # Errors
    /// Returns `io::Error` if the file cannot be written.
    pub fn save(&self, entry: &MemoryEntry) -> io::Result<()> {
        let fm = Frontmatter {
            id: entry.id.clone(),
            claim_id: entry.claim_id.clone(),
            memory_type: entry.memory_type,
            kind: entry.kind,
            status: entry.status,
            strength: entry.strength,
            description: entry.description.clone(),
            created_at: entry.created_at.to_rfc3339(),
            updated_at: entry.updated_at.to_rfc3339(),
            access_count: entry.access_count,
            owner_actor: entry.owner_actor.clone(),
            instance_id: if entry.instance_id.is_empty() {
                None
            } else {
                Some(entry.instance_id.clone())
            },
            source: entry.source,
            reconsolidation_until: entry.reconsolidation_until.map(|dt| dt.to_rfc3339()),
            claim: entry.claim.clone(),
            evidence_events: entry.evidence_events.clone(),
            confirmed_by_user: entry.confirmed_by_user,
            contradicted_by: entry.contradicted_by.clone(),
            supersedes: entry.supersedes.clone(),
            valid_from: entry.valid_from.map(|dt| dt.to_rfc3339()),
            valid_until: entry.valid_until.map(|dt| dt.to_rfc3339()),
            risk_if_wrong: entry.risk_if_wrong.clone(),
            usage_outcomes: entry.usage_outcomes.clone(),
            feedback_attributions: entry.feedback_attributions.clone(),
            feedback_replay_checks: entry.feedback_replay_checks.clone(),
        };
        let yaml = serde_yaml::to_string(&fm)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let content = format!("---\n{yaml}---\n{}", entry.content);
        let path = unique_memory_path(&self.dir, entry);
        atomic_write(&path, content.as_bytes())
    }

    /// # Errors
    /// Returns `io::Error` if the file cannot be read or parsed.
    pub fn load(&self, id: &str) -> io::Result<MemoryEntry> {
        for dir_entry in fs::read_dir(&self.dir)? {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if path.extension().is_some_and(|ext| ext == "md")
                && let Ok(raw) = fs::read_to_string(&path)
                && let Ok(entry) = parse_memory_file(&raw)
                && entry.id == id
            {
                return Ok(entry);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("memory '{id}' not found"),
        ))
    }

    /// # Errors
    /// Returns `io::Error` if the directory cannot be read.
    pub fn list_all(&self) -> io::Result<Vec<MemoryEntry>> {
        let mut entries = Vec::new();
        for dir_entry in fs::read_dir(&self.dir)? {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                let raw = fs::read_to_string(&path)?;
                if let Ok(entry) = parse_memory_file(&raw) {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }

    /// List memories visible to an actor. `local:default` is the local
    /// administrator actor and can see all memories.
    ///
    /// # Errors
    /// Returns `io::Error` if the directory cannot be read.
    pub fn list_for_actor(&self, actor: &str) -> io::Result<Vec<MemoryEntry>> {
        let all = self.list_all()?;
        if actor == "local:default" {
            return Ok(all);
        }
        Ok(all
            .into_iter()
            .filter(|entry| entry.owner_actor == actor)
            .collect())
    }

    /// Load a memory only if it is visible to the actor.
    ///
    /// # Errors
    /// Returns `NotFound` if the memory does not exist or is not visible.
    pub fn load_for_actor(&self, id: &str, actor: &str) -> io::Result<MemoryEntry> {
        let entry = self.load(id)?;
        if actor == "local:default" || entry.owner_actor == actor {
            Ok(entry)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("memory '{id}' not found"),
            ))
        }
    }

    /// Delete a memory only if it is visible to the actor.
    ///
    /// # Errors
    /// Returns `NotFound` if the memory does not exist or is not visible.
    pub fn delete_for_actor(&self, id: &str, actor: &str) -> io::Result<()> {
        let _ = self.load_for_actor(id, actor)?;
        self.delete(id)
    }

    /// # Errors
    /// Returns `io::Error` if the file cannot be removed.
    pub fn delete(&self, id: &str) -> io::Result<()> {
        for dir_entry in fs::read_dir(&self.dir)? {
            let dir_entry = dir_entry?;
            let path = dir_entry.path();
            if path.extension().is_some_and(|ext| ext == "md")
                && let Ok(raw) = fs::read_to_string(&path)
                && let Ok(entry) = parse_memory_file(&raw)
                && entry.id == id
            {
                return fs::remove_file(path);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("memory '{id}' not found"),
        ))
    }
}

fn parse_memory_file(raw: &str) -> io::Result<MemoryEntry> {
    let Some(rest) = raw.strip_prefix("---\n") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing YAML frontmatter",
        ));
    };
    let Some(end) = rest.find("---\n") else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unterminated YAML frontmatter",
        ));
    };
    let yaml_str = &rest[..end];
    let content = &rest[end + 4..];

    let fm: Frontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let created_at: DateTime<Utc> = fm
        .created_at
        .parse()
        .map_err(|e: chrono::ParseError| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let updated_at: DateTime<Utc> = fm
        .updated_at
        .parse()
        .map_err(|e: chrono::ParseError| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let reconsolidation_until = fm
        .reconsolidation_until
        .as_deref()
        .map(str::parse::<DateTime<Utc>>)
        .transpose()
        .map_err(|e: chrono::ParseError| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let valid_from = parse_optional_datetime(fm.valid_from.as_deref())?;
    let valid_until = parse_optional_datetime(fm.valid_until.as_deref())?;

    let claim_id = if fm.claim_id.is_empty() {
        fm.id.clone()
    } else {
        fm.claim_id
    };
    Ok(MemoryEntry {
        id: fm.id,
        claim_id,
        content: content.to_string(),
        description: fm.description,
        memory_type: fm.memory_type,
        kind: fm.kind,
        status: fm.status,
        strength: fm.strength,
        created_at,
        updated_at,
        access_count: fm.access_count,
        owner_actor: fm.owner_actor,
        instance_id: fm.instance_id.unwrap_or_default(),
        reconsolidation_until,
        source: fm.source,
        claim: fm.claim,
        evidence_events: fm.evidence_events,
        confirmed_by_user: fm.confirmed_by_user,
        contradicted_by: fm.contradicted_by,
        supersedes: fm.supersedes,
        valid_from,
        valid_until,
        risk_if_wrong: fm.risk_if_wrong,
        usage_outcomes: fm.usage_outcomes,
        feedback_attributions: fm.feedback_attributions,
        feedback_replay_checks: fm.feedback_replay_checks,
    })
}

fn parse_optional_datetime(value: Option<&str>) -> io::Result<Option<DateTime<Utc>>> {
    value
        .map(str::parse::<DateTime<Utc>>)
        .transpose()
        .map_err(|e: chrono::ParseError| io::Error::new(io::ErrorKind::InvalidData, e))
}
