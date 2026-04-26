use cortex_kernel::SqliteStore;
use cortex_runtime::{CortexRuntime, RuntimeError, SideEffectExecutor, ToolExecutor, ToolOutcome};
use cortex_sdk::{
    PluginAuthorizationError, PluginContext, PluginManifest, ToolRequest, ToolResponse,
};
use cortex_types::{
    ActorId, AuthContext, ClientId, DeliveryItem, DeliveryTextMode, OutboundBlock, OutboundMessage,
    OwnedScope, SideEffectIntent, SideEffectKind, SideEffectRecord, TenantId,
    TransportCapabilities, Visibility,
};

fn context(tenant: &'static str, actor: &'static str, client: &'static str) -> AuthContext {
    AuthContext::new(
        TenantId::from_static(tenant),
        ActorId::from_static(actor),
        ClientId::from_static(client),
    )
}

struct DigestExecutor;

impl SideEffectExecutor for DigestExecutor {
    fn execute(&self, intent: &SideEffectIntent) -> Result<SideEffectRecord, RuntimeError> {
        Ok(SideEffectRecord::succeeded(
            intent.id.clone(),
            intent.scope.clone(),
            "sha256:test",
            intent.created_at,
        ))
    }
}

struct EchoToolExecutor;

impl ToolExecutor for EchoToolExecutor {
    fn execute_tool(&self, context: &PluginContext, request: &ToolRequest) -> ToolOutcome {
        ToolOutcome::succeeded(ToolResponse {
            output: serde_json::json!({
                "tenant": context.tenant_id,
                "session": context.session_id,
                "tool": request.name,
                "input": request.input,
            }),
            audit_label: format!("{}:{}", context.actor_id, request.name),
        })
    }
}

struct FailingToolExecutor;

impl ToolExecutor for FailingToolExecutor {
    fn execute_tool(&self, _context: &PluginContext, _request: &ToolRequest) -> ToolOutcome {
        ToolOutcome::failed("process exited with code 2")
    }
}

#[test]
fn runtime_sessions_are_visible_only_to_matching_tenant_actor() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = CortexRuntime::open(dir.path().join("journal.jsonl")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");
    let bob = context("tenant-b", "bob", "qq");
    runtime.add_tenant(alice.tenant_id.clone(), "Tenant A");
    runtime.add_tenant(bob.tenant_id.clone(), "Tenant B");

    runtime
        .bind_client(&alice, TransportCapabilities::plain(128))
        .unwrap();
    runtime
        .bind_client(&bob, TransportCapabilities::plain(128))
        .unwrap();
    runtime.create_session(&alice).unwrap();
    runtime.create_session(&bob).unwrap();

    assert_eq!(runtime.visible_events(&alice).unwrap().len(), 2);
    assert_eq!(runtime.visible_events(&bob).unwrap().len(), 2);
}

#[test]
fn runtime_rejects_unknown_tenant_before_state_creation() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = CortexRuntime::open(dir.path().join("journal.jsonl")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");

    assert!(runtime.create_session(&alice).is_err());
    assert!(runtime.visible_events(&alice).unwrap().is_empty());
}

#[test]
fn first_turn_reuses_actor_session_across_clients() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = CortexRuntime::open(dir.path().join("journal.jsonl")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    runtime.add_tenant(telegram.tenant_id.clone(), "Tenant A");
    runtime
        .bind_client(&telegram, TransportCapabilities::plain(128))
        .unwrap();
    runtime
        .bind_client(&qq, TransportCapabilities::plain(128))
        .unwrap();

    let first = runtime.ensure_session_for_turn(&telegram).unwrap();
    let second = runtime.ensure_session_for_turn(&qq).unwrap();

    assert_eq!(first, second);
    assert_eq!(runtime.known_clients(&telegram.tenant_id), 2);
}

#[test]
fn delivery_only_targets_active_subscribers_for_that_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = CortexRuntime::open(dir.path().join("journal.jsonl")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    runtime.add_tenant(telegram.tenant_id.clone(), "Tenant A");
    runtime
        .bind_client(
            &telegram,
            TransportCapabilities {
                text_mode: DeliveryTextMode::Markdown,
                max_chars: 128,
                media: Vec::new(),
            },
        )
        .unwrap();
    runtime
        .bind_client(&qq, TransportCapabilities::plain(128))
        .unwrap();
    let telegram_session = runtime.create_session(&telegram).unwrap();
    let qq_session = runtime.create_session(&qq).unwrap();
    runtime
        .activate_session(&telegram, &telegram_session)
        .unwrap();
    runtime.activate_session(&qq, &qq_session).unwrap();

    let mut message = OutboundMessage::new(
        OwnedScope::private_for(&telegram),
        cortex_types::DeliveryPhase::Final,
    );
    message.push(OutboundBlock::Text {
        text: "**complete** answer".to_string(),
        markdown: true,
    });
    let delivered = runtime
        .deliver_to_active_subscribers(&telegram_session, &message)
        .unwrap();

    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].client_id, telegram.client_id);
    assert!(matches!(
        delivered[0].plan.items.as_slice(),
        [DeliveryItem::Text { markdown: true, .. }]
    ));

    runtime.activate_session(&qq, &telegram_session).unwrap();
    let delivered = runtime
        .deliver_to_active_subscribers(&telegram_session, &message)
        .unwrap();

    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].client_id, telegram.client_id);

    let mut actor_shared = OutboundMessage::new(
        OwnedScope::new(
            telegram.tenant_id,
            telegram.actor_id,
            None,
            Visibility::ActorShared,
        ),
        cortex_types::DeliveryPhase::Final,
    );
    actor_shared.push(OutboundBlock::Text {
        text: "**shared** answer".to_string(),
        markdown: true,
    });
    let delivered = runtime
        .deliver_to_active_subscribers(&telegram_session, &actor_shared)
        .unwrap();
    let clients = delivered
        .iter()
        .map(|delivery| delivery.client_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(clients, vec!["qq", "telegram"]);
    let qq_delivery = delivered
        .iter()
        .find(|delivery| delivery.client_id == qq.client_id)
        .unwrap();
    assert!(matches!(
        qq_delivery.plan.items.as_slice(),
        [DeliveryItem::Text {
            markdown: false,
            ..
        }]
    ));
}

#[test]
fn runtime_recovers_client_sessions_and_active_delivery_from_journal() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");

    let session = {
        let mut runtime = CortexRuntime::open(&journal_path).unwrap();
        runtime
            .register_tenant(&telegram.tenant_id, "Tenant A")
            .unwrap();
        runtime
            .bind_client(&telegram, TransportCapabilities::plain(128))
            .unwrap();
        runtime
            .bind_client(&qq, TransportCapabilities::plain(128))
            .unwrap();
        let session = runtime.ensure_session_for_turn(&telegram).unwrap();
        runtime.activate_session(&qq, &session).unwrap();
        session
    };

    let recovered = CortexRuntime::open(&journal_path).unwrap();
    assert_eq!(recovered.known_clients(&telegram.tenant_id), 2);
    assert_eq!(
        recovered.active_session(&telegram).unwrap(),
        Some(session.clone())
    );
    assert_eq!(
        recovered.active_session(&qq).unwrap(),
        Some(session.clone())
    );

    let mut message = OutboundMessage::new(
        OwnedScope::new(
            telegram.tenant_id,
            telegram.actor_id,
            None,
            Visibility::ActorShared,
        ),
        cortex_types::DeliveryPhase::Final,
    );
    message.push(OutboundBlock::Text {
        text: "Recovered delivery state".to_string(),
        markdown: false,
    });
    let delivered = recovered
        .deliver_to_active_subscribers(&session, &message)
        .unwrap();

    assert_eq!(delivered.len(), 2);
}

#[test]
fn persistent_runtime_writes_sqlite_state_and_recovers_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let state_path = dir.path().join("state.sqlite");
    let telegram = context("tenant-a", "alice", "telegram");

    let session = {
        let mut runtime = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
        runtime
            .register_tenant(&telegram.tenant_id, "Tenant A")
            .unwrap();
        runtime
            .bind_client(&telegram, TransportCapabilities::plain(128))
            .unwrap();
        let session = runtime.ensure_session_for_turn(&telegram).unwrap();
        let mut message = OutboundMessage::new(
            OwnedScope::private_for(&telegram),
            cortex_types::DeliveryPhase::Final,
        );
        message.push(OutboundBlock::Text {
            text: "persisted delivery".to_string(),
            markdown: false,
        });
        let delivered = runtime
            .deliver_to_active_subscribers(&session, &message)
            .unwrap();
        assert_eq!(
            runtime.persisted_client_count(&telegram.tenant_id).unwrap(),
            1
        );
        assert_eq!(delivered.len(), 1);
        session
    };

    let recovered = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
    let store = SqliteStore::open(&state_path).unwrap();
    let delivery_records = store.visible_delivery_records(&telegram).unwrap();

    assert_eq!(recovered.active_session(&telegram).unwrap(), Some(session));
    assert_eq!(
        recovered
            .persisted_client_count(&telegram.tenant_id)
            .unwrap(),
        1
    );
    assert_eq!(delivery_records.len(), 1);
    assert_eq!(
        delivery_records[0].plan.combined_text(),
        "persisted delivery"
    );
}

#[test]
fn runtime_dispatches_side_effects_with_intent_then_result_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let state_path = dir.path().join("state.sqlite");
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    let mut runtime = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
    runtime
        .register_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    let intent = SideEffectIntent::new(
        OwnedScope::private_for(&telegram),
        SideEffectKind::ToolCall,
        "tool:read:Cargo.toml",
        "read Cargo manifest",
    );

    let record = runtime
        .dispatch_side_effect(&intent, &DigestExecutor)
        .unwrap();
    let store = SqliteStore::open(&state_path).unwrap();

    assert_eq!(record.output_digest.as_deref(), Some("sha256:test"));
    assert_eq!(
        store.visible_side_effect_intents(&telegram).unwrap().len(),
        1
    );
    assert_eq!(
        store.visible_side_effect_records(&telegram).unwrap().len(),
        1
    );
    assert!(store.visible_side_effect_intents(&qq).unwrap().is_empty());
    assert!(store.visible_side_effect_records(&qq).unwrap().is_empty());
    assert_eq!(runtime.visible_events(&telegram).unwrap().len(), 3);
}

#[test]
fn tool_execution_validates_sdk_contract_and_persists_private_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let state_path = dir.path().join("state.sqlite");
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    let mut runtime = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
    runtime
        .register_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    runtime
        .bind_client(&telegram, TransportCapabilities::plain(4_096))
        .unwrap();
    runtime
        .bind_client(&qq, TransportCapabilities::plain(4_096))
        .unwrap();
    let session = runtime.ensure_session_for_turn(&telegram).unwrap();
    let manifest =
        PluginManifest::process("project-reader", "1.0.0").with_capability("project.read");
    let request = ToolRequest::new("read", serde_json::json!({"path": "Cargo.toml"}))
        .require_capability("project.read");
    let grants = vec!["project.read".to_string()];

    let executed = runtime
        .execute_tool(
            &telegram,
            &session,
            &manifest,
            &request,
            &grants,
            &EchoToolExecutor,
        )
        .unwrap();
    let store = SqliteStore::open(&state_path).unwrap();

    assert_eq!(executed.response.audit_label, "alice:read");
    assert!(executed.side_effect.output_digest.is_some());
    assert_eq!(
        store.visible_side_effect_intents(&telegram).unwrap().len(),
        1
    );
    assert_eq!(
        store.visible_side_effect_records(&telegram).unwrap().len(),
        1
    );
    assert!(store.visible_side_effect_intents(&qq).unwrap().is_empty());
    assert!(store.visible_side_effect_records(&qq).unwrap().is_empty());
}

#[test]
fn tool_execution_rejects_host_paths_before_side_effect_intent() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let state_path = dir.path().join("state.sqlite");
    let telegram = context("tenant-a", "alice", "telegram");
    let mut runtime = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
    runtime
        .register_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    runtime
        .bind_client(&telegram, TransportCapabilities::plain(4_096))
        .unwrap();
    let session = runtime.ensure_session_for_turn(&telegram).unwrap();
    let manifest =
        PluginManifest::process("project-reader", "1.0.0").with_capability("project.read");
    let request = ToolRequest::new("read", serde_json::json!({}))
        .require_capability("project.read")
        .with_host_path("/etc/passwd");
    let grants = vec!["project.read".to_string()];

    let result = runtime.execute_tool(
        &telegram,
        &session,
        &manifest,
        &request,
        &grants,
        &EchoToolExecutor,
    );
    let store = SqliteStore::open(&state_path).unwrap();

    assert!(matches!(
        result,
        Err(RuntimeError::PluginAuthorization(
            PluginAuthorizationError::HostPathDenied { .. }
        ))
    ));
    assert!(
        store
            .visible_side_effect_intents(&telegram)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .visible_side_effect_records(&telegram)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tool_execution_failures_are_durable_side_effect_records() {
    let dir = tempfile::tempdir().unwrap();
    let journal_path = dir.path().join("journal.jsonl");
    let state_path = dir.path().join("state.sqlite");
    let telegram = context("tenant-a", "alice", "telegram");
    let mut runtime = CortexRuntime::open_persistent(&journal_path, &state_path).unwrap();
    runtime
        .register_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    runtime
        .bind_client(&telegram, TransportCapabilities::plain(4_096))
        .unwrap();
    let session = runtime.ensure_session_for_turn(&telegram).unwrap();
    let manifest =
        PluginManifest::process("project-reader", "1.0.0").with_capability("project.read");
    let request = ToolRequest::new("read", serde_json::json!({"path": "Cargo.toml"}))
        .require_capability("project.read");
    let grants = vec!["project.read".to_string()];

    let result = runtime.execute_tool(
        &telegram,
        &session,
        &manifest,
        &request,
        &grants,
        &FailingToolExecutor,
    );
    let store = SqliteStore::open(&state_path).unwrap();
    let records = store.visible_side_effect_records(&telegram).unwrap();

    assert!(matches!(result, Err(RuntimeError::ToolExecutionFailed(_))));
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].error.as_deref(),
        Some("process exited with code 2")
    );
}
