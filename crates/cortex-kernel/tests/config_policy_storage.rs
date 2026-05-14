use std::fs;

use cortex_kernel::{
    CortexPaths, Journal, MemoryStore, PolicySimulationRequest, ensure_home_dirs,
    load_config_for_paths, simulate_policy, validate,
};
use cortex_types::config::ProviderRegistry;
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryStatus, MemoryType, Payload, RiskLevel,
    ToolEffect, ToolEffectKind, TurnId,
};

#[test]
fn config_loader_creates_minimal_instance_config() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let paths = CortexPaths::from_instance_home(temp.path());
    ensure_home_dirs(temp.path())?;

    let config = load_config_for_paths(&paths, None, &ProviderRegistry::default());

    assert!(paths.config_path().is_file());
    assert_eq!(config.daemon.addr, "127.0.0.1:0");
    assert!(validate(&config).len() <= 1);
    Ok(())
}

#[test]
fn policy_requires_confirmation_for_file_writes() {
    let mut config = cortex_types::config::CortexConfig::default();
    config.risk.auto_approve_up_to = RiskLevel::Review;

    let report = simulate_policy(
        &config,
        &[],
        &PolicySimulationRequest {
            actor: "local:default".to_string(),
            tool: "write_file".to_string(),
            effects: vec![ToolEffect::new(ToolEffectKind::WriteFile)],
            background: false,
        },
    );

    assert_eq!(report.risk_level, RiskLevel::RequireConfirmation);
    assert!(!report.allowed);
    assert!(report.confirmation_required);
}

#[test]
fn memory_store_persists_toml_frontmatter() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = MemoryStore::open(temp.path())?;
    let mut entry = MemoryEntry::new(
        "Cortex keeps memory metadata in TOML frontmatter.",
        "TOML memory metadata",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    entry.status = MemoryStatus::Materialized;

    store.save(&entry)?;
    let loaded = store.load(&entry.id)?;
    let raw_files = fs::read_dir(temp.path())?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(loaded.id, entry.id);
    assert_eq!(loaded.status, MemoryStatus::Materialized);
    assert!(raw_files.iter().any(|path| {
        fs::read_to_string(path)
            .is_ok_and(|raw| raw.starts_with("+++\n") && raw.contains("type = \"Project\""))
    }));
    Ok(())
}

#[test]
fn journal_round_trip_preserves_correlation_order() -> Result<(), Box<dyn std::error::Error>> {
    let journal = Journal::open_in_memory()?;
    let turn_id = TurnId::new();
    let correlation_id = CorrelationId::new();
    let first = Event::new(
        turn_id,
        correlation_id,
        Payload::UserMessage {
            content: "hello".to_string(),
        },
    );
    let second = Event::new(
        turn_id,
        correlation_id,
        Payload::AssistantMessage {
            content: "world".to_string(),
        },
    );

    journal.append_batch(&[first, second])?;
    let events = journal.query_by_correlation(&correlation_id)?;
    let payloads = events
        .into_iter()
        .map(|event| event.payload)
        .collect::<Vec<_>>();

    assert_eq!(journal.event_count()?, 2);
    assert!(
        payloads.iter().any(
            |payload| matches!(payload, Payload::UserMessage { content } if content == "hello")
        )
    );
    assert!(payloads.iter().any(
        |payload| matches!(payload, Payload::AssistantMessage { content } if content == "world")
    ));
    Ok(())
}
