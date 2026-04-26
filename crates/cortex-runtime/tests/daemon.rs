use std::thread;
use std::time::Duration;

use cortex_runtime::{
    DaemonBootstrap, DaemonClientConfig, DaemonConfig, DaemonRequest, DaemonResponse, DaemonServer,
    DaemonTenantConfig, send_request,
};
use cortex_types::{ActorId, AuthContext, ClientId, TenantId, TransportCapabilities};

fn context() -> AuthContext {
    AuthContext::new(
        TenantId::from_static("tenant-a"),
        ActorId::from_static("alice"),
        ClientId::from_static("cli"),
    )
}

#[test]
fn daemon_serves_status_and_persistent_turn_requests() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("cortex.sock");
    let config = DaemonConfig::new(dir.path().join("data"), &socket_path);
    let server = DaemonServer::open(config).unwrap();
    let handle = thread::spawn(move || server.serve());

    wait_for_socket(&socket_path);

    assert_eq!(
        send_request(
            &socket_path,
            &DaemonRequest::RegisterTenant {
                tenant_id: context().tenant_id,
                name: "Tenant A".to_string(),
            },
        )
        .unwrap(),
        DaemonResponse::Ack
    );
    assert_eq!(
        send_request(
            &socket_path,
            &DaemonRequest::BindClient {
                context: context(),
                capabilities: TransportCapabilities::plain(512),
            },
        )
        .unwrap(),
        DaemonResponse::Ack
    );

    let submitted = send_request(
        &socket_path,
        &DaemonRequest::SubmitUserMessage {
            context: context(),
            input: "hello daemon".to_string(),
        },
    )
    .unwrap();
    let DaemonResponse::SubmittedTurn { turn, usage } = submitted else {
        panic!("unexpected submit response: {submitted:?}");
    };
    assert_eq!(usage.total(), 0);

    let status = send_request(&socket_path, &DaemonRequest::Status).unwrap();
    let DaemonResponse::Status { status } = status else {
        panic!("unexpected status response: {status:?}");
    };
    assert_eq!(status.tenants, 1);
    assert_eq!(status.clients, 1);
    assert_eq!(status.sessions, 1);
    assert!(status.persistent);
    assert_eq!(status.journal_mode.as_deref(), Some("wal"));
    assert_eq!(status.wal_autocheckpoint_pages, Some(1_000));
    assert!(!turn.session_id.as_str().is_empty());
    assert!(!turn.turn_id.as_str().is_empty());

    assert_eq!(
        send_request(&socket_path, &DaemonRequest::Shutdown).unwrap(),
        DaemonResponse::Ack
    );
    handle.join().unwrap().unwrap();
    assert!(!socket_path.exists());
}

#[test]
fn daemon_bootstrap_registers_initial_tenants_and_clients() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("bootstrap.sock");
    let config = DaemonConfig::new(dir.path().join("data"), &socket_path);
    let mut server = DaemonServer::open(config).unwrap();
    server
        .bootstrap(&DaemonBootstrap {
            tenants: vec![DaemonTenantConfig {
                id: "tenant-a".to_string(),
                name: "Tenant A".to_string(),
            }],
            clients: vec![DaemonClientConfig {
                tenant_id: "tenant-a".to_string(),
                actor_id: "alice".to_string(),
                client_id: "cli".to_string(),
                max_chars: 512,
            }],
        })
        .unwrap();
    let handle = thread::spawn(move || server.serve());

    wait_for_socket(&socket_path);

    let status = send_request(&socket_path, &DaemonRequest::Status).unwrap();
    let DaemonResponse::Status { status } = status else {
        panic!("unexpected status response: {status:?}");
    };

    assert_eq!(status.tenants, 1);
    assert_eq!(status.clients, 1);
    assert_eq!(
        send_request(&socket_path, &DaemonRequest::Shutdown).unwrap(),
        DaemonResponse::Ack
    );
    handle.join().unwrap().unwrap();
}

fn wait_for_socket(socket_path: &std::path::Path) {
    for _attempt in 0..50 {
        if socket_path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("daemon socket was not created: {}", socket_path.display());
}
