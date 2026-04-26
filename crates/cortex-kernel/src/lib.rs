#![forbid(unsafe_code)]

pub mod store;

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use cortex_types::{AuthContext, Event, OwnedScope};

pub use store::{DbWriter, DbWriterError, SessionRecord, SqliteStore, StoreError, StoreHealth};

#[derive(Debug)]
pub enum JournalError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for JournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone)]
pub struct FileJournal {
    path: PathBuf,
}

impl FileJournal {
    /// # Errors
    /// Returns an error when the parent directory cannot be created or the
    /// journal file cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { path })
    }

    /// # Errors
    /// Returns an error when serialization or append fails.
    pub fn append(&self, event: &Event) -> Result<(), JournalError> {
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        serde_json::to_writer(&mut file, event)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the journal cannot be read or decoded.
    pub fn replay_visible(&self, context: &AuthContext) -> Result<Vec<Event>, JournalError> {
        let file = File::open(&self.path)?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            let event: Event = serde_json::from_str(&line?)?;
            if event.scope.is_visible_to(context) {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// # Errors
    /// Returns an error when the journal cannot be read or decoded.
    pub fn replay_all(&self) -> Result<Vec<Event>, JournalError> {
        let file = File::open(&self.path)?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            events.push(serde_json::from_str(&line?)?);
        }
        Ok(events)
    }
}

#[must_use]
pub fn cross_owner(scope: &OwnedScope, context: &AuthContext) -> bool {
    !scope.is_visible_to(context)
}
