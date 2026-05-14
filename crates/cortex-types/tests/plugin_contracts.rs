use cortex_types::plugin::{
    PluginSandboxLevel, PluginSandboxProfile, SandboxFilesystemMode, SandboxNetworkMode,
};

#[test]
fn sandbox_profile_rejects_unenforced_isolation_claims() {
    let profile = PluginSandboxProfile {
        level: PluginSandboxLevel::ContainerVm,
        network: SandboxNetworkMode::None,
        filesystem: SandboxFilesystemMode::PluginOnly,
        writable_paths: Vec::new(),
        seccomp: String::new(),
        uid_drop: false,
        memory_mb: None,
        cpu_seconds: None,
    };

    assert_eq!(
        profile.unsupported_runtime_claim(),
        Some("container_vm is not enforced by this runtime")
    );
}
