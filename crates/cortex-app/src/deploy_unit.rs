use std::path::PathBuf;

const PH_CORTEX_BIN: &str = "{cortex_bin}";
const PH_CORTEX_HOME: &str = "{cortex_home}";
const PH_CORTEX_ID: &str = "{cortex_id}";
const PH_PATH: &str = "{path}";

const USER_UNIT_TEMPLATE: &str = r"[Unit]
Description=Cortex Cognitive Harness
After=network.target

[Service]
Type=simple
ExecStart={cortex_bin} --daemon --id {cortex_id}
Environment=CORTEX_HOME={cortex_home}
Environment=PATH={path}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
";

const SYSTEM_UNIT_TEMPLATE: &str = r"[Unit]
Description=Cortex Cognitive Harness
After=network.target

[Service]
Type=simple
User=cortex
ExecStart={cortex_bin} --daemon --id {cortex_id}
Environment=CORTEX_HOME={cortex_home}
Environment=PATH={path}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
";

/// Generate systemd user service unit file content with resolved paths.
#[must_use]
pub fn generate_unit_file(cortex_bin: &str, cortex_home: &str, instance_id: &str) -> String {
    let path_env = service_path_env(cortex_home, instance_id);
    USER_UNIT_TEMPLATE
        .replace(PH_CORTEX_BIN, cortex_bin)
        .replace(PH_CORTEX_HOME, cortex_home)
        .replace(PH_CORTEX_ID, instance_id)
        .replace(PH_PATH, &path_env)
}

/// Generate systemd system-level service unit file content with resolved paths.
#[must_use]
pub fn generate_system_unit_file(cortex_bin: &str, cortex_home: &str, instance_id: &str) -> String {
    let path_env = service_path_env(cortex_home, instance_id);
    SYSTEM_UNIT_TEMPLATE
        .replace(PH_CORTEX_BIN, cortex_bin)
        .replace(PH_CORTEX_HOME, cortex_home)
        .replace(PH_CORTEX_ID, instance_id)
        .replace(PH_PATH, &path_env)
}

fn service_path_env(cortex_home: &str, instance_id: &str) -> String {
    let data_dir = PathBuf::from(cortex_home).join(instance_id).join("data");
    crate::node_manager::path_with_managed_node_bin(&data_dir)
}
