use cortex_kernel::{SqliteStore, StoreError};
use cortex_types::{
    ActorId, AuthContext, ClientId, DeliveryId, DeliveryItem, DeliveryPhase, DeliveryPlan,
    DeliveryStatus, FastCapture, MemoryKind, OutboundDeliveryRecord, PermissionDecision,
    PermissionRequest, PermissionResolution, PermissionResolutionError, SemanticMemory, SessionId,
    TenantId, TokenUsage, TransportCapabilities, TurnId, UsageRecord, Visibility,
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
        assert_eq!(store.applied_migrations().unwrap(), vec![1, 2, 3, 4, 5]);
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
    let sessions_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/legacy-1.4/sessions");
    let store = SqliteStore::open(dir.path().join("state.sqlite")).unwrap();
    let tenant = TenantId::from_static("tenant-a");
    let fallback = ClientId::from_static("migration");
    store.upsert_tenant(&tenant, "Tenant A").unwrap();

    let report = store
        .import_legacy_sessions(&sessions_dir, &tenant, &fallback)
        .unwrap();
    let owner = AuthContext::new(
        tenant.clone(),
        ActorId::from_static("telegram:5188621876"),
        ClientId::from_static("telegram:5188621876"),
    );
    let other = AuthContext::new(
        tenant,
        ActorId::from_static("telegram:5188621876"),
        ClientId::from_static("qq:E94C84AC"),
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
