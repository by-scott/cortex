use cortex_kernel::PromptManager;
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn prompt_manager_migrates_legacy_root_templates_into_system_directory() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let prompts_dir = temp.path().join("prompts");
    let system_dir = prompts_dir.join("system");
    must(
        fs::create_dir_all(&prompts_dir),
        "prompts dir should initialize",
    );
    must(
        fs::write(
            prompts_dir.join("memory-extract.md"),
            "legacy memory extract template",
        ),
        "legacy memory-extract should write",
    );
    must(
        fs::write(
            prompts_dir.join("context-compress.md"),
            "legacy context compress template",
        ),
        "legacy context-compress should write",
    );

    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );

    assert!(
        !prompts_dir.join("memory-extract.md").exists(),
        "legacy memory-extract should move out of prompt root"
    );
    assert!(
        !prompts_dir.join("context-compress.md").exists(),
        "legacy context-compress should move out of prompt root"
    );
    assert_eq!(
        must(
            fs::read_to_string(system_dir.join("memory-extract.md")),
            "migrated memory-extract should load",
        ),
        "legacy memory extract template"
    );
    assert_eq!(
        must(
            fs::read_to_string(system_dir.join("context-compress.md")),
            "migrated context-compress should load",
        ),
        "legacy context compress template"
    );
    assert_eq!(
        manager.get_system_template("memory-extract").as_deref(),
        Some("legacy memory extract template")
    );
    assert_eq!(
        manager.get_system_template("context-compress").as_deref(),
        Some("legacy context compress template")
    );
}

#[test]
fn prompt_manager_migrates_agent_md_only_when_behavioral_is_missing() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let prompts_dir = temp.path().join("prompts");
    must(
        fs::create_dir_all(&prompts_dir),
        "prompts dir should initialize",
    );
    must(
        fs::write(prompts_dir.join("agent.md"), "legacy behavioral prompt"),
        "legacy agent prompt should write",
    );

    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );

    assert!(
        !prompts_dir.join("agent.md").exists(),
        "legacy agent.md should be renamed away"
    );
    assert_eq!(
        manager
            .get(cortex_types::PromptLayer::Behavioral)
            .as_deref(),
        Some("legacy behavioral prompt")
    );
}

#[test]
fn prompt_manager_does_not_overwrite_existing_behavioral_prompt_during_migration() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let prompts_dir = temp.path().join("prompts");
    must(
        fs::create_dir_all(&prompts_dir),
        "prompts dir should initialize",
    );
    must(
        fs::write(prompts_dir.join("agent.md"), "legacy behavioral prompt"),
        "legacy agent prompt should write",
    );
    must(
        fs::write(
            prompts_dir.join("behavioral.md"),
            "current behavioral prompt",
        ),
        "current behavioral prompt should write",
    );

    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );

    assert!(
        prompts_dir.join("agent.md").exists(),
        "legacy agent.md should remain when behavioral.md already exists"
    );
    assert_eq!(
        manager
            .get(cortex_types::PromptLayer::Behavioral)
            .as_deref(),
        Some("current behavioral prompt")
    );
    assert_eq!(
        must(
            fs::read_to_string(prompts_dir.join("agent.md")),
            "legacy agent prompt should still load",
        ),
        "legacy behavioral prompt"
    );
}
