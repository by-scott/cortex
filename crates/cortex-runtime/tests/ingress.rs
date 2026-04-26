use cortex_runtime::{CortexRuntime, IngressError, IngressRegistry, RuntimeError};
use cortex_types::{ActorId, AuthContext, ClientId, TenantId, TransportCapabilities};

fn context(tenant: &'static str, actor: &'static str, client: &'static str) -> AuthContext {
    AuthContext::new(
        TenantId::from_static(tenant),
        ActorId::from_static(actor),
        ClientId::from_static(client),
    )
}

#[test]
fn ingress_registry_binds_only_authenticated_clients() {
    let dir = tempfile::tempdir().unwrap();
    let mut runtime = CortexRuntime::open(dir.path().join("journal.jsonl")).unwrap();
    let mut registry = IngressRegistry::default();
    let telegram = context("tenant-a", "alice", "telegram");
    runtime
        .register_tenant(&telegram.tenant_id, "Tenant A")
        .unwrap();
    registry
        .register(
            telegram.clone(),
            "high-entropy-telegram-token",
            TransportCapabilities::plain(256),
        )
        .unwrap();

    let rejected = runtime.bind_authenticated_client(
        &registry,
        &telegram.tenant_id,
        &telegram.actor_id,
        &telegram.client_id,
        "wrong-token",
    );

    assert!(matches!(
        rejected,
        Err(RuntimeError::Ingress(IngressError::InvalidToken))
    ));
    assert_eq!(runtime.known_clients(&telegram.tenant_id), 0);
    assert_eq!(runtime.visible_events(&telegram).unwrap().len(), 1);

    runtime
        .bind_authenticated_client(
            &registry,
            &telegram.tenant_id,
            &telegram.actor_id,
            &telegram.client_id,
            "high-entropy-telegram-token",
        )
        .unwrap();

    assert_eq!(registry.registered_clients(), 1);
    assert_eq!(runtime.known_clients(&telegram.tenant_id), 1);
    assert_eq!(runtime.visible_events(&telegram).unwrap().len(), 2);
}

#[test]
fn ingress_registry_rejects_empty_and_unknown_credentials() {
    let mut registry = IngressRegistry::default();
    let telegram = context("tenant-a", "alice", "telegram");
    let unknown = registry.authenticate(
        &telegram.tenant_id,
        &telegram.actor_id,
        &telegram.client_id,
        "token",
    );

    assert!(matches!(unknown, Err(IngressError::UnknownClient)));
    assert!(matches!(
        registry.register(telegram, "", TransportCapabilities::plain(256)),
        Err(IngressError::EmptyToken)
    ));
}
