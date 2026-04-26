use std::process::Command;

#[test]
fn status_reports_runtime_and_gate_surface() {
    let output = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .arg("status")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("1.5 full rewrite"));
    assert!(stdout.contains("multi-user"));
    assert!(stdout.contains("journal recovery"));
    assert!(stdout.contains("BM25"));
}

#[test]
fn release_plan_lists_required_order() {
    let output = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .arg("release-plan")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("- backup"));
    assert!(stdout.contains("- migrate"));
    assert!(stdout.contains("- install"));
    assert!(stdout.contains("- smoke-test"));
    assert!(stdout.contains("- package"));
    assert!(stdout.contains("- publish"));
}
