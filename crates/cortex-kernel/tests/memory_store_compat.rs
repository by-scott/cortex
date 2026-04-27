use cortex_kernel::MemoryStore;
use cortex_types::{
    MemoryEntry, MemoryEvidence, MemoryKind, MemorySource, MemoryType, MemoryUsageOutcome,
    MemoryUsageOutcomeKind,
};
use std::fs;
use std::path::PathBuf;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn must_some<T>(value: Option<T>, context: &str) -> T {
    value.unwrap_or_else(|| panic!("{context}"))
}

#[test]
fn memory_store_loads_legacy_uuid_named_files() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = must(MemoryStore::open(temp.path()), "memory store should open");
    let entry = write_legacy_uuid_memory(temp.path(), &store, "legacy note", "legacy content");

    let loaded = must(store.load(&entry.id), "legacy memory should load");
    assert_eq!(loaded.id, entry.id);
    assert_eq!(loaded.content, "legacy content");
}

#[test]
fn memory_store_removes_legacy_uuid_file_after_resave() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = must(MemoryStore::open(temp.path()), "memory store should open");
    let entry = write_legacy_uuid_memory(temp.path(), &store, "migrated note", "legacy body");
    let legacy_file = temp.path().join(format!("{}.md", entry.id));

    let mut migrated = must(store.load(&entry.id), "legacy memory should load");
    migrated.content = "updated body".to_string();
    must(store.save(&migrated), "resave should succeed");

    assert!(
        !legacy_file.exists(),
        "legacy uuid-named file should be removed after resave"
    );

    let files = must(fs::read_dir(temp.path()), "memory dir should read")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1, "store should keep only the migrated file");

    let loaded = must(store.load(&entry.id), "migrated memory should load");
    assert_eq!(loaded.content, "updated body");
}

#[test]
fn memory_store_round_trips_evidence_backed_belief_fields() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = must(MemoryStore::open(temp.path()), "memory store should open");
    let mut entry = MemoryEntry::new(
        "Cortex should use repository Docker Compose for release gates.",
        "repository Docker Compose is release-authoritative",
        MemoryType::Project,
        MemoryKind::Semantic,
    )
    .with_claim(
        "Cortex release gate",
        "uses",
        "repository Docker Compose",
        "release validation",
    );
    entry.source = MemorySource::UserInput;
    entry.risk_if_wrong = "A release could be validated in the wrong environment.".to_string();
    entry.confirm_by_user();
    entry.add_evidence(MemoryEvidence::new(
        "turn-1",
        MemorySource::UserInput,
        0.95,
        "user explicitly required repository Docker Compose",
    ));
    entry.record_usage_outcome(MemoryUsageOutcome::new(
        "turn-2",
        MemoryUsageOutcomeKind::Helped,
        0.7,
        "the Docker gate caught release-surface drift",
    ));

    must(store.save(&entry), "evidence-backed memory should save");
    let loaded = must(store.load(&entry.id), "evidence-backed memory should load");

    assert_eq!(loaded.claim_id, entry.id);
    assert_eq!(loaded.claim.subject, "Cortex release gate");
    assert!(loaded.confirmed_by_user);
    assert_eq!(loaded.evidence_events.len(), 1);
    assert_eq!(loaded.usage_outcomes.len(), 1);
    assert_eq!(loaded.risk_if_wrong, entry.risk_if_wrong);
    assert!(loaded.can_stabilize_as_belief());
}

fn write_legacy_uuid_memory(
    dir: &std::path::Path,
    store: &MemoryStore,
    description: &str,
    content: &str,
) -> MemoryEntry {
    let entry = MemoryEntry::new(
        content,
        description,
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    must(store.save(&entry), "memory should save");

    let current_path = must_some(
        find_memory_file(dir, &entry.id),
        "saved memory file should exist",
    );
    let legacy_path = dir.join(format!("{}.md", entry.id));
    must(
        fs::rename(current_path, &legacy_path),
        "saved memory should be renamed to legacy uuid path",
    );

    entry
}

fn find_memory_file(dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") {
            let raw = fs::read_to_string(&path).ok()?;
            if raw.contains(id) {
                return Some(path);
            }
        }
    }
    None
}
