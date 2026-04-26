use cortex_sdk::{
    ABI_VERSION, PluginAuthorizationError, PluginContext, PluginManifest, ResourceLimits,
    ToolRequest, ToolResponse,
};

fn context() -> PluginContext {
    PluginContext {
        tenant_id: "tenant-a".to_string(),
        actor_id: "alice".to_string(),
        session_id: "session-a".to_string(),
        capabilities: vec!["read_project".to_string()],
        limits: ResourceLimits::strict(),
    }
}

#[test]
fn plugin_request_requires_declared_capability() {
    let context = context();
    let request =
        ToolRequest::new("deploy", serde_json::json!({})).require_capability("write_project");

    assert_eq!(
        context.authorize(&request),
        Err(PluginAuthorizationError::MissingCapability {
            capability: "write_project".to_string()
        })
    );
}

#[test]
fn plugin_request_denies_host_paths_by_default() {
    let context = context();
    let request = ToolRequest::new("read", serde_json::json!({}))
        .require_capability("read_project")
        .with_host_path("/etc/passwd");

    assert_eq!(
        context.authorize(&request),
        Err(PluginAuthorizationError::HostPathDenied {
            path: "/etc/passwd".to_string()
        })
    );
}

#[test]
fn plugin_response_must_fit_output_limit() {
    let response = ToolResponse {
        output: serde_json::json!({"text": "abcdef"}),
        audit_label: "tool".to_string(),
    };
    let limits = ResourceLimits {
        timeout_ms: 5_000,
        max_output_bytes: 4,
        max_memory_bytes: 64 * 1024 * 1024,
        allow_host_paths: false,
    };

    assert!(matches!(
        response.validate_output(limits),
        Err(PluginAuthorizationError::OutputTooLarge { .. })
    ));
}

#[test]
fn plugin_manifest_validates_abi_and_declared_capabilities() {
    let manifest =
        PluginManifest::process("project-reader", "1.0.0").with_capability("read_project");
    let request =
        ToolRequest::new("read", serde_json::json!({})).require_capability("write_project");

    assert_eq!(
        manifest.validate_request(&request),
        Err(PluginAuthorizationError::CapabilityNotDeclared {
            capability: "write_project".to_string()
        })
    );

    let mut wrong_abi = manifest;
    wrong_abi.abi_version = ABI_VERSION + 1;

    assert_eq!(
        wrong_abi.validate(),
        Err(PluginAuthorizationError::AbiMismatch {
            expected: ABI_VERSION,
            actual: ABI_VERSION + 1
        })
    );
}
