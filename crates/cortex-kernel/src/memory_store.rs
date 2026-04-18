use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use cortex_types::{MemoryEntry, MemoryKind, MemorySource, MemoryStatus, MemoryType};
use serde::{Deserialize, Serialize};

use crate::util::atomic_write;

pub struct MemoryStore {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Frontmatter {
    id: String,
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
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    source: MemorySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reconsolidation_until: Option<String>,
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
            memory_type: entry.memory_type,
            kind: entry.kind,
            status: entry.status,
            strength: entry.strength,
            description: entry.description.clone(),
            created_at: entry.created_at.to_rfc3339(),
            updated_at: entry.updated_at.to_rfc3339(),
            access_count: entry.access_count,
            instance_id: if entry.instance_id.is_empty() {
                None
            } else {
                Some(entry.instance_id.clone())
            },
            source: entry.source,
            reconsolidation_until: entry.reconsolidation_until.map(|dt| dt.to_rfc3339()),
        };
        let yaml = serde_yaml::to_string(&fm)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let content = format!("---\n{yaml}---\n{}", entry.content);
        let path = unique_memory_path(&self.dir, entry);
        // If a different file exists for this ID (e.g., old UUID-named file), remove it
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.cleanup_old_file(&entry.id, &filename);
        atomic_write(&path, content.as_bytes())
    }

    /// # Errors
    /// Returns `io::Error` if the file cannot be read or parsed.
    pub fn load(&self, id: &str) -> io::Result<MemoryEntry> {
        // Fast path: try UUID-based filename (backward compat)
        let uuid_path = self.dir.join(format!("{id}.md"));
        if uuid_path.exists() {
            let raw = fs::read_to_string(&uuid_path)?;
            return parse_memory_file(&raw);
        }
        // Slow path: scan directory for file containing this ID
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

    /// # Errors
    /// Returns `io::Error` if the file cannot be removed.
    pub fn delete(&self, id: &str) -> io::Result<()> {
        // Fast path: UUID-based filename
        let uuid_path = self.dir.join(format!("{id}.md"));
        if uuid_path.exists() {
            return fs::remove_file(uuid_path);
        }
        // Slow path: scan for file with this ID in frontmatter
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

    /// Remove any old file for this ID if it has a different filename (migration).
    fn cleanup_old_file(&self, id: &str, current_filename: &str) {
        let old_path = self.dir.join(format!("{id}.md"));
        if old_path.exists()
            && old_path
                .file_name()
                .is_some_and(|f| f.to_string_lossy().as_ref() != current_filename)
        {
            let _ = fs::remove_file(old_path);
        }
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

    Ok(MemoryEntry {
        id: fm.id,
        content: content.to_string(),
        description: fm.description,
        memory_type: fm.memory_type,
        kind: fm.kind,
        status: fm.status,
        strength: fm.strength,
        created_at,
        updated_at,
        access_count: fm.access_count,
        instance_id: fm.instance_id.unwrap_or_default(),
        reconsolidation_until,
        source: fm.source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let entry = MemoryEntry::new(
            "test content",
            "desc",
            MemoryType::User,
            MemoryKind::Episodic,
        );
        store.save(&entry).unwrap();
        let loaded = store.load(&entry.id).unwrap();
        assert_eq!(loaded.content, "test content");
        assert_eq!(loaded.status, MemoryStatus::Captured);
        assert_eq!(loaded.source, MemorySource::LlmGenerated);
    }

    #[test]
    fn list_all() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let e1 = MemoryEntry::new("a", "d", MemoryType::User, MemoryKind::Episodic);
        let e2 = MemoryEntry::new("b", "d", MemoryType::Feedback, MemoryKind::Semantic);
        store.save(&e1).unwrap();
        store.save(&e2).unwrap();
        let all = store.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let entry = MemoryEntry::new("x", "d", MemoryType::Project, MemoryKind::Semantic);
        store.save(&entry).unwrap();
        store.delete(&entry.id).unwrap();
        assert!(store.load(&entry.id).is_err());
    }

    #[test]
    fn load_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        assert!(store.load("nonexistent").is_err());
    }

    #[test]
    fn semantic_filename() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(dir.path()).unwrap();
        let entry = MemoryEntry::new(
            "Rust developer likes async",
            "desc",
            MemoryType::User,
            MemoryKind::Episodic,
        );
        store.save(&entry).unwrap();

        // File should be named with type prefix and slug, not UUID
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(files.len(), 1);
        assert!(
            files[0].starts_with("user_"),
            "filename should start with type prefix: {}",
            files[0]
        );
        assert!(
            !files[0].contains("019d"),
            "filename should not contain UUID: {}",
            files[0]
        );
    }
}
