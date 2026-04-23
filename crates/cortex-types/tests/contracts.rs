use cortex_types::{
    AssistantResponse, ContentBlock, MemoryEntry, MemoryKind, MemoryStatus, MemoryType, Message,
    NativePluginIsolation, Payload, PluginManifest, Role, TextFormat, TurnState,
    check_compatibility,
};

#[test]
fn turn_state_contract_allows_only_runtime_transitions() {
    assert!(
        TurnState::Idle
            .try_transition(TurnState::Processing)
            .is_ok()
    );
    assert!(
        TurnState::Processing
            .try_transition(TurnState::Completed)
            .is_ok()
    );
    assert!(
        TurnState::Completed
            .try_transition(TurnState::Processing)
            .is_err()
    );
    assert!(TurnState::Completed.is_terminal());
}

#[test]
fn message_and_response_contract_round_trips() {
    let message = Message::user("hello");
    assert_eq!(message.role, Role::User);
    assert!(matches!(
        message.content.first(),
        Some(ContentBlock::Text { text }) if text == "hello"
    ));

    let response = AssistantResponse {
        text: "done".to_string(),
        format: TextFormat::Markdown,
        parts: Vec::new(),
    };
    let encoded = serde_json::to_string(&response).expect("response serializes");
    let decoded: AssistantResponse = serde_json::from_str(&encoded).expect("response decodes");
    assert_eq!(decoded.plain_text(), "done");
}

#[test]
fn memory_and_payload_contracts_keep_owner_and_shape() {
    let mut entry = MemoryEntry::new(
        "prefers concise updates",
        "user preference",
        MemoryType::User,
        MemoryKind::Semantic,
    );
    entry.owner_actor = "telegram:42".to_string();
    assert_eq!(entry.status, MemoryStatus::Captured);
    assert_eq!(entry.owner_actor, "telegram:42");

    let payload = Payload::MemoryCaptured {
        memory_id: entry.id,
        memory_type: "user".to_string(),
    };
    let encoded = rmp_serde::to_vec_named(&payload).expect("payload encodes");
    let decoded: Payload = rmp_serde::from_slice(&encoded).expect("payload decodes");
    assert!(matches!(decoded, Payload::MemoryCaptured { .. }));
}

#[test]
fn plugin_manifest_requires_latest_version_field_and_process_default() {
    let manifest: PluginManifest = toml::from_str(
        r#"
name = "sample"
version = "0.1.0"
description = "sample"
cortex_version = "1.1.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"
"#,
    )
    .expect("manifest parses");
    assert_eq!(
        manifest.native.as_ref().expect("native section").isolation,
        NativePluginIsolation::Process
    );
    assert!(check_compatibility(&manifest, "1.1.0").compatible);

    let rejected: PluginManifest = toml::from_str(
        r#"
name = "sample"
version = "0.1.0"
description = "sample"
cortex_version_requirement = ">=1.1.0"
"#,
    )
    .expect("manifest parses");
    assert!(!check_compatibility(&rejected, "1.1.0").compatible);
}
