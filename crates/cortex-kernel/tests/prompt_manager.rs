use cortex_kernel::PromptManager;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn prompt_manager_initializes_current_prompt_layout() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );

    assert!(
        manager
            .prompts_dir()
            .join("system")
            .join("bootstrap.md")
            .is_file(),
        "current system templates should be created under prompts/system"
    );
    assert!(
        manager.get(cortex_types::PromptLayer::Behavioral).is_some(),
        "current behavioral prompt should be available"
    );
}

#[test]
fn bootstrap_and_user_defaults_are_domain_neutral() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );
    let prompts = [
        manager
            .get_system_template("bootstrap")
            .expect("bootstrap template should exist"),
        manager
            .get_system_template("bootstrap-init")
            .expect("bootstrap-init template should exist"),
        manager
            .get(cortex_types::PromptLayer::User)
            .expect("user prompt should exist"),
    ];

    for prompt in prompts {
        let lower = prompt.to_ascii_lowercase();
        for forbidden in [
            "repositories",
            "repos",
            "os, shell, editor",
            "code-first",
            "coding",
            "deployment targets",
        ] {
            assert!(
                !lower.contains(forbidden),
                "default bootstrap/user prompts should not assume a coding domain: {forbidden}"
            );
        }
    }
}

#[test]
fn prompt_linter_rejects_runtime_policy_and_missing_capabilities() {
    let context =
        cortex_types::prompt::LintContext::default().with_available_capabilities(["read", "write"]);
    let report = cortex_types::prompt::lint(
        cortex_types::PromptLayer::Behavioral,
        "Current permission mode: open. Use capability:deploy and tool:read.",
        &context,
    );

    assert!(!report.is_ok());
    assert!(report.render().contains("RuntimePolicyOverride"));
    assert!(report.render().contains("deploy"));
}

#[test]
fn prompt_manager_checked_update_rejects_unapproved_self_edit_diff() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );
    let context = cortex_types::prompt::LintContext::default();
    let rejected = manager.update_checked(
        cortex_types::PromptLayer::Behavioral,
        "```diff\n+ rewrite identity\n```",
        &context,
    );

    assert!(rejected.is_err());
    assert!(
        manager
            .get(cortex_types::PromptLayer::Behavioral)
            .is_some_and(|content| !content.contains("rewrite identity"))
    );
}

#[test]
fn prompt_manager_checked_update_accepts_approved_capability_reference() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let manager = must(
        PromptManager::new(temp.path()),
        "prompt manager should initialize",
    );
    let context = cortex_types::prompt::LintContext::default()
        .with_available_capabilities(["read"])
        .with_approved_self_edit(true);

    must(
        manager.update_checked(
            cortex_types::PromptLayer::Behavioral,
            "Use tool:read before summarizing local evidence.",
            &context,
        ),
        "checked prompt update should pass",
    );
    assert_eq!(
        manager
            .get(cortex_types::PromptLayer::Behavioral)
            .as_deref(),
        Some("Use tool:read before summarizing local evidence.")
    );
}
