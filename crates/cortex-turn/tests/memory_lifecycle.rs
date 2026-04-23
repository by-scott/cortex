//! Memory lifecycle integration tests.
//!
//! Tests the complete CLS pipeline: create → consolidate → decay → deprecate.

use cortex_kernel::MemoryStore;
use cortex_turn::memory::consolidate::{consolidate_memories, upgrade_episodic_to_semantic};
use cortex_turn::memory::decay::{freshness, should_deprecate};
use cortex_turn::memory::lifecycle::{should_consolidate, should_extract};
use cortex_turn::memory::recall::rank_memories;
use cortex_types::{MemoryEntry, MemoryKind, MemoryStatus, MemoryType};

fn make_entry(content: &str, kind: MemoryKind, access: u32, strength: f64) -> MemoryEntry {
    let mut e = MemoryEntry::new(content, "desc", MemoryType::Project, kind);
    e.access_count = access;
    e.strength = strength;
    e
}

#[test]
fn cls_captured_to_materialized() {
    let mut entries = vec![make_entry("test", MemoryKind::Episodic, 3, 1.0)];
    assert_eq!(entries[0].status, MemoryStatus::Captured);
    let result = consolidate_memories(&mut entries);
    assert_eq!(entries[0].status, MemoryStatus::Materialized);
    assert!(result.upgraded > 0);
}

#[test]
fn cls_materialized_to_stabilized() {
    let mut entries = vec![make_entry("test", MemoryKind::Episodic, 6, 0.8)];
    entries[0].status = MemoryStatus::Materialized;
    let result = consolidate_memories(&mut entries);
    assert_eq!(entries[0].status, MemoryStatus::Stabilized);
    assert!(result.upgraded > 0);
}

#[test]
fn cls_insufficient_access_no_upgrade() {
    let mut entries = vec![make_entry("test", MemoryKind::Episodic, 1, 1.0)];
    consolidate_memories(&mut entries);
    assert_eq!(entries[0].status, MemoryStatus::Captured);
}

#[test]
fn cls_low_strength_no_stabilize() {
    let mut entries = vec![make_entry("test", MemoryKind::Episodic, 10, 0.3)];
    entries[0].status = MemoryStatus::Materialized;
    consolidate_memories(&mut entries);
    assert_eq!(entries[0].status, MemoryStatus::Materialized);
}

#[test]
fn episodic_to_semantic_upgrade() {
    let mut entries: Vec<MemoryEntry> = (0..3)
        .map(|_| {
            let mut e = MemoryEntry::new(
                "recurring pattern",
                "same desc",
                MemoryType::Project,
                MemoryKind::Episodic,
            );
            e.access_count = 5;
            e.strength = 0.8;
            e.status = MemoryStatus::Stabilized;
            e
        })
        .collect();
    // Need at least one Stabilized for upgrade
    let upgrades = upgrade_episodic_to_semantic(&mut entries, &[], 0.90);
    // Upgraded entries should now be Semantic
    assert!(
        !upgrades.is_empty() || entries.iter().any(|e| e.kind == MemoryKind::Semantic),
        "Expected at least one episodic→semantic upgrade"
    );
}

#[test]
fn freshness_decay_over_time() {
    let fresh = freshness(0.0, 5, 0.05);
    let old = freshness(500.0, 5, 0.05);
    assert!(
        fresh > old,
        "Fresh should be higher than old: {fresh} vs {old}"
    );
}

#[test]
fn deprecation_threshold() {
    assert!(
        !should_deprecate(1.0, 5, 0.05),
        "Recent should not deprecate"
    );
    assert!(
        should_deprecate(5000.0, 0, 0.05),
        "Very old unused should deprecate"
    );
}

#[test]
fn lifecycle_extract_timing() {
    assert!(!should_extract(2, 5));
    assert!(should_extract(5, 5));
    assert!(should_extract(10, 5));
}

#[test]
fn lifecycle_consolidate_timing() {
    assert!(should_consolidate(25.0, 24, 0), "Time-based trigger");
    assert!(should_consolidate(1.0, 24, 5), "Count-based trigger");
    assert!(!should_consolidate(1.0, 24, 2), "Neither trigger");
}

#[test]
fn recall_ranks_by_relevance() {
    let entries = vec![
        make_entry(
            "rust programming language guide",
            MemoryKind::Semantic,
            10,
            0.9,
        ),
        make_entry(
            "python machine learning tutorial",
            MemoryKind::Semantic,
            10,
            0.9,
        ),
        make_entry("rust compiler error handling", MemoryKind::Episodic, 5, 0.7),
    ];

    let ranked = rank_memories("rust programming", &entries, 2);
    assert_eq!(ranked.len(), 2);
    assert!(
        ranked[0].content.contains("rust"),
        "Top result should be about rust"
    );
}

#[test]
fn store_save_and_recall() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();

    let e1 = MemoryEntry::new(
        "rust async patterns",
        "desc",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    let e2 = MemoryEntry::new(
        "cooking recipes",
        "desc",
        MemoryType::User,
        MemoryKind::Episodic,
    );
    store.save(&e1).unwrap();
    store.save(&e2).unwrap();

    let all = store.list_all().unwrap();
    assert_eq!(all.len(), 2);

    let ranked = rank_memories("rust async", &all, 1);
    assert_eq!(ranked.len(), 1);
    assert!(ranked[0].content.contains("rust"));
}

#[test]
fn full_lifecycle_create_consolidate_recall() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();

    // Create
    let mut entry = MemoryEntry::new(
        "Cortex uses CLS memory pipeline with three stages",
        "CLS architecture",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    entry.access_count = 6;
    entry.strength = 0.9;
    store.save(&entry).unwrap();

    // Load and consolidate
    let mut entries = store.list_all().unwrap();
    let result = consolidate_memories(&mut entries);
    assert!(result.upgraded > 0 || entries[0].status != MemoryStatus::Captured);

    // Save consolidated
    for e in &entries {
        store.save(e).unwrap();
    }

    // Recall
    let reloaded = store.list_all().unwrap();
    let ranked = rank_memories("CLS memory pipeline", &reloaded, 1);
    assert_eq!(ranked.len(), 1);
    assert!(ranked[0].content.contains("CLS"));
}
