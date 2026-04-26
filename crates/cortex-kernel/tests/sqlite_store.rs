use cortex_kernel::{DbWriter, SqliteStore, StoreError};
use cortex_types::{
    ActorId, AuthContext, ClientId, ConflictKind, ConflictSignal, ControlGoal, ControlLevel,
    ControlSignal, DeliveryId, DeliveryItem, DeliveryPhase, DeliveryPlan, DeliveryStatus,
    FastCapture, MemoryKind, MonitoringRecord, MonitoringReport, OutboundDeliveryRecord,
    PermissionDecision, PermissionRequest, PermissionResolution, PermissionResolutionError,
    PermissionStatus, PermissionTicket, PressureAction, SemanticMemory, SessionId,
    SideEffectIntent, SideEffectKind, SideEffectRecord, SideEffectStatus, TenantId, TokenUsage,
    TransportCapabilities, TurnId, UsageRecord, Visibility,
};

fn context(tenant: &'static str, actor: &'static str, client: &'static str) -> AuthContext {
    AuthContext::new(
        TenantId::from_static(tenant),
        ActorId::from_static(actor),
        ClientId::from_static(client),
    )
}

#[test]
fn sqlite_store_migrates_and_recovers_active_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite");
    let alice = context("tenant-a", "alice", "telegram");
    let session_id = SessionId::from_static("session-a");
    {
        let store = SqliteStore::open(&path).unwrap();
        store.upsert_tenant(&alice.tenant_id, "Tenant A").unwrap();
        store
            .upsert_client(&alice, &TransportCapabilities::plain(128))
            .unwrap();
        store
            .upsert_session(
                &session_id,
                &cortex_types::OwnedScope::new(
                    alice.tenant_id.clone(),
                    alice.actor_id.clone(),
                    None,
                    Visibility::ActorShared,
                ),
            )
            .unwrap();
        store.set_active_session(&alice, &session_id).unwrap();
        assert_eq!(
            store.applied_migrations().unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    let recovered = SqliteStore::open(&path).unwrap();
    assert_eq!(recovered.client_count(&alice.tenant_id).unwrap(), 1);
    assert_eq!(recovered.active_session(&alice).unwrap(), Some(session_id));
}

#[test]
fn sqlite_store_filters_visible_sessions_by_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");
    let bob = context("tenant-a", "bob", "qq");
    store.upsert_tenant(&alice.tenant_id, "Tenant A").unwrap();
    store
        .upsert_session(
            &SessionId::from_static("alice-private"),
            &cortex_types::OwnedScope::private_for(&alice),
        )
        .unwrap();
    store
        .upsert_session(
            &SessionId::from_static("bob-private"),
            &cortex_types::OwnedScope::private_for(&bob),
        )
        .unwrap();

    let visible = store.visible_sessions(&alice).unwrap();

    assert_eq!(visible.len(), 1);
    assert_eq!(
        visible[0].session_id,
        SessionId::from_static("alice-private")
    );
}

#[test]
fn sqlite_store_rejects_foreign_active_session() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");
    let bob = context("tenant-b", "bob", "qq");
    let session_id = SessionId::from_static("session-b");
    store.upsert_tenant(&alice.tenant_id, "Tenant A").unwrap();
    store.upsert_tenant(&bob.tenant_id, "Tenant B").unwrap();
    store
        .upsert_client(&alice, &TransportCapabilities::plain(128))
        .unwrap();
    store
        .upsert_session(&session_id, &cortex_types::OwnedScope::private_for(&bob))
        .unwrap();

    assert!(matches!(
        store.set_active_session(&alice, &session_id),
        Err(StoreError::AccessDenied)
    ));
    assert_eq!(store.active_session(&alice).unwrap(), None);
}

#[test]
fn sqlite_store_imports_legacy_sessions_without_widening_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_dir = dir.path().join("legacy-sessions");
    std::fs::create_dir(&sessions_dir).unwrap();
    std::fs::write(
        sessions_dir.join("session-old.json"),
        r#"{
  "id": "session-old",
  "name": "old",
  "owner_actor": "telegram:test-actor",
  "created_at": "2026-04-24T00:00:00Z",
  "ended_at": null,
  "turn_count": 1,
  "start_offset": 0,
  "end_offset": null
}
"#,
    )
    .unwrap();
    std::fs::write(sessions_dir.join("bad.json"), "{not-json").unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let tenant = TenantId::from_static("tenant-a");
    let fallback = ClientId::from_static("migration");
    store.upsert_tenant(&tenant, "Tenant A").unwrap();

    let report = store
        .import_legacy_sessions(&sessions_dir, &tenant, &fallback)
        .unwrap();
    let owner = AuthContext::new(
        tenant.clone(),
        ActorId::from_static("telegram:test-actor"),
        ClientId::from_static("telegram:test-actor"),
    );
    let other = AuthContext::new(
        tenant,
        ActorId::from_static("telegram:test-actor"),
        ClientId::from_static("qq:test-client"),
    );

    assert_eq!(report.imported_sessions, 1);
    assert_eq!(report.skipped_files, 1);
    assert_eq!(store.visible_sessions(&owner).unwrap().len(), 1);
    assert!(store.visible_sessions(&other).unwrap().is_empty());
}

#[test]
fn sqlite_store_filters_memory_by_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");
    let bob = context("tenant-a", "bob", "qq");
    store.upsert_tenant(&alice.tenant_id, "Tenant A").unwrap();
    store
        .save_fast_capture(&FastCapture::new(
            "capture-alice",
            cortex_types::OwnedScope::private_for(&alice),
            "Alice likes durable local runtimes.",
        ))
        .unwrap();
    store
        .save_fast_capture(&FastCapture::new(
            "capture-bob",
            cortex_types::OwnedScope::private_for(&bob),
            "Bob has a separate private preference.",
        ))
        .unwrap();
    store
        .save_semantic_memory(&SemanticMemory::new(
            "memory-alice",
            cortex_types::OwnedScope::private_for(&alice),
            MemoryKind::Semantic,
            "Alice prefers deterministic replay.",
            vec!["capture-alice".to_string()],
        ))
        .unwrap();
    store
        .save_semantic_memory(&SemanticMemory::new(
            "memory-bob",
            cortex_types::OwnedScope::private_for(&bob),
            MemoryKind::Semantic,
            "Bob prefers another private workflow.",
            vec!["capture-bob".to_string()],
        ))
        .unwrap();

    let captures = store.visible_fast_captures(&alice).unwrap();
    let memories = store.visible_semantic_memories(&alice).unwrap();

    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id, "capture-alice");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].id, "memory-alice");
}

#[test]
fn sqlite_store_persists_permission_requests_with_owner_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    let request = PermissionRequest::new(
        cortex_types::OwnedScope::private_for(&telegram),
        "write_file",
        "/tmp/private-note.md",
    );

    store.save_permission_request(&request).unwrap();

    let visible_to_owner = store.visible_permission_requests(&telegram).unwrap();
    let visible_to_other_client = store.visible_permission_requests(&qq).unwrap();
    let wrong_client_resolution = PermissionResolution::new(
        request.id.clone(),
        cortex_types::OwnedScope::private_for(&qq),
        PermissionDecision::Allow,
    );
    let owner_resolution = PermissionResolution::new(
        request.id,
        cortex_types::OwnedScope::private_for(&telegram),
        PermissionDecision::Allow,
    );

    assert_eq!(visible_to_owner.len(), 1);
    assert!(visible_to_other_client.is_empty());
    assert!(matches!(
        store.resolve_permission(&wrong_client_resolution),
        Err(StoreError::Permission(
            PermissionResolutionError::WrongOwner
        ))
    ));
    assert_eq!(
        store.resolve_permission(&owner_resolution).unwrap(),
        PermissionDecision::Allow
    );
}

#[test]
fn sqlite_store_persists_permission_ticket_lifecycle_by_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    let request = PermissionRequest::new(
        cortex_types::OwnedScope::private_for(&telegram),
        "write",
        "/tmp/file",
    );
    let mut ticket = PermissionTicket::new(request);
    let resolution = PermissionResolution::new(
        ticket.request.id.clone(),
        cortex_types::OwnedScope::private_for(&telegram),
        PermissionDecision::Allow,
    );

    store.save_permission_ticket(&ticket).unwrap();
    assert_eq!(
        store.visible_permission_tickets(&telegram).unwrap()[0].status,
        PermissionStatus::Pending
    );
    assert!(store.visible_permission_tickets(&qq).unwrap().is_empty());

    let resolved_at = ticket.created_at;
    ticket.resolve(&resolution, resolved_at).unwrap();
    store.save_permission_ticket(&ticket).unwrap();
    assert_eq!(
        store.visible_permission_tickets(&telegram).unwrap()[0].status,
        PermissionStatus::Approved
    );
}

#[test]
fn sqlite_store_persists_delivery_outbox_per_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    let session_id = SessionId::from_static("session-delivery");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    store
        .upsert_session(
            &session_id,
            &cortex_types::OwnedScope::new(
                telegram.tenant_id.clone(),
                telegram.actor_id.clone(),
                None,
                Visibility::ActorShared,
            ),
        )
        .unwrap();
    let plan = DeliveryPlan {
        items: vec![DeliveryItem::Text {
            text: "private delivery".to_string(),
            markdown: false,
            phase: DeliveryPhase::Final,
        }],
    };
    let mut record = OutboundDeliveryRecord::planned(
        DeliveryId::from_static("delivery-a"),
        session_id,
        &telegram,
        plan,
    );

    store.save_delivery_record(&record).unwrap();

    assert_eq!(store.visible_delivery_records(&telegram).unwrap().len(), 1);
    assert!(store.visible_delivery_records(&qq).unwrap().is_empty());

    record.mark_sent();
    store.save_delivery_record(&record).unwrap();
    let visible = store.visible_delivery_records(&telegram).unwrap();
    assert_eq!(visible[0].status, DeliveryStatus::Sent);
    assert_eq!(visible[0].attempts, 1);

    record.mark_failed("rate limited");
    store.save_delivery_record(&record).unwrap();
    let visible = store.visible_delivery_records(&telegram).unwrap();
    assert_eq!(visible[0].status, DeliveryStatus::Failed);
    assert_eq!(visible[0].attempts, 2);
    assert_eq!(visible[0].last_error.as_deref(), Some("rate limited"));
}

#[test]
fn sqlite_store_persists_token_usage_by_owner_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    let session_id = SessionId::from_static("session-usage");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    store
        .upsert_session(
            &session_id,
            &cortex_types::OwnedScope::new(
                telegram.tenant_id.clone(),
                telegram.actor_id.clone(),
                None,
                Visibility::ActorShared,
            ),
        )
        .unwrap();
    let first = UsageRecord::new(
        cortex_types::OwnedScope::private_for(&telegram),
        TurnId::from_static("turn-a"),
        session_id.clone(),
        "glm-5.1",
        TokenUsage::new(120, 30),
    );
    let second = UsageRecord::new(
        cortex_types::OwnedScope::private_for(&telegram),
        TurnId::from_static("turn-b"),
        session_id,
        "glm-5.1",
        TokenUsage::new(80, 20),
    );

    store.save_usage_record(&first).unwrap();
    store.save_usage_record(&second).unwrap();

    let visible = store.visible_usage_records(&telegram).unwrap();
    let total = store.usage_total(&telegram).unwrap();

    assert_eq!(visible.len(), 2);
    assert!(store.visible_usage_records(&qq).unwrap().is_empty());
    assert_eq!(total.input_tokens, 200);
    assert_eq!(total.output_tokens, 50);
    assert_eq!(total.total(), 250);
}

#[test]
fn sqlite_store_persists_side_effect_intent_result_ledger_by_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    let intent = SideEffectIntent::new(
        cortex_types::OwnedScope::private_for(&telegram),
        SideEffectKind::ToolCall,
        "tool:read:Cargo.toml",
        "read Cargo manifest",
    );
    let record = SideEffectRecord::succeeded(
        intent.id.clone(),
        cortex_types::OwnedScope::private_for(&telegram),
        "sha256:abc",
        intent.created_at,
    );

    store.save_side_effect_intent(&intent).unwrap();
    store.save_side_effect_record(&record).unwrap();

    let owner_intents = store.visible_side_effect_intents(&telegram).unwrap();
    let owner_records = store.visible_side_effect_records(&telegram).unwrap();

    assert_eq!(owner_intents.len(), 1);
    assert_eq!(owner_records.len(), 1);
    assert!(store.visible_side_effect_intents(&qq).unwrap().is_empty());
    assert!(store.visible_side_effect_records(&qq).unwrap().is_empty());
    assert_eq!(owner_records[0].status, SideEffectStatus::Succeeded);
}

#[test]
fn sqlite_store_persists_cognitive_control_records_by_owner() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let telegram = context("tenant-a", "alice", "telegram");
    let qq = context("tenant-a", "alice", "qq");
    store
        .upsert_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    let scope = cortex_types::OwnedScope::private_for(&telegram);
    let goal = ControlGoal::new(
        "release-goal",
        scope.clone(),
        ControlLevel::Strategic,
        "ship production-ready Cortex 1.5",
    )
    .with_tag("release");
    let report = MonitoringReport {
        pressure: 0.8,
        pressure_action: PressureAction::AskHuman,
        signals: vec![ConflictSignal {
            kind: ConflictKind::GoalConflict,
            intensity: 1.0,
            evidence: "release-goal inhibits rewrite-loop".to_string(),
            control: ControlSignal::AskHuman,
        }],
        recommended_control: ControlSignal::AskHuman,
    };
    let record = MonitoringRecord::new("monitoring-a", scope, report);

    store.save_control_goal(&goal).unwrap();
    store.save_monitoring_record(&record).unwrap();

    let owner_goals = store.visible_control_goals(&telegram).unwrap();
    let owner_records = store.visible_monitoring_records(&telegram).unwrap();

    assert_eq!(owner_goals.len(), 1);
    assert_eq!(owner_goals[0].id, "release-goal");
    assert_eq!(owner_records.len(), 1);
    assert_eq!(
        owner_records[0].report.recommended_control,
        ControlSignal::AskHuman
    );
    assert!(store.visible_control_goals(&qq).unwrap().is_empty());
    assert!(store.visible_monitoring_records(&qq).unwrap().is_empty());
}

#[test]
fn sqlite_store_uses_wal_operational_pragmas() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let health = store.health().unwrap();

    assert_eq!(health.journal_mode, "wal");
    assert_eq!(health.synchronous, "normal");
    assert!(health.foreign_keys);
    assert_eq!(health.busy_timeout_ms, 5_000);
    assert_eq!(health.wal_autocheckpoint_pages, 1_000);
    store.checkpoint_passive().unwrap();
}

#[test]
fn db_writer_serializes_sqlite_writes_on_one_thread() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DbWriter::open(dir.path().join("state.sqlite")).unwrap();
    let alice = context("tenant-a", "alice", "telegram");
    let session_id = SessionId::from_static("writer-session");

    let tenant = alice.tenant_id.clone();
    writer
        .write(move |store| store.upsert_tenant(&tenant, "Tenant A"))
        .unwrap();

    let alice_for_client = alice.clone();
    writer
        .write(move |store| {
            store.upsert_client(&alice_for_client, &TransportCapabilities::plain(64))
        })
        .unwrap();

    let alice_for_session = alice.clone();
    let session_for_insert = session_id.clone();
    writer
        .write(move |store| {
            store.upsert_session(
                &session_for_insert,
                &cortex_types::OwnedScope::private_for(&alice_for_session),
            )
        })
        .unwrap();

    let alice_for_active = alice.clone();
    let session_for_active = session_id.clone();
    writer
        .write(move |store| store.set_active_session(&alice_for_active, &session_for_active))
        .unwrap();

    let active = writer
        .write(move |store| store.active_session(&alice))
        .unwrap();
    assert_eq!(active, Some(session_id));
}
