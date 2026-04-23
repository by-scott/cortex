use std::fs;
use std::path::Path;

use crate::plugin_manager::{PLUGIN_MANIFEST_FILE, PLUGIN_SKILLS_DIR};

/// Generate a process-isolated Cortex plugin project at `<cwd>/cortex-plugin-<name>/`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_process_plugin(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    generate_process_plugin_in(name, &cwd)
}

/// Generate a process-isolated Cortex plugin project inside `base_dir`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_process_plugin_in(name: &str, base_dir: &Path) -> Result<String, String> {
    validate_name(name)?;
    let dir_name = format!("cortex-plugin-{name}");
    let dir = base_dir.join(&dir_name);
    if dir.exists() {
        return Err(format!("directory '{dir_name}' already exists"));
    }
    fs::create_dir_all(dir.join(PLUGIN_SKILLS_DIR)).map_err(|e| format!("mkdir: {e}"))?;
    fs::create_dir_all(dir.join("prompts")).map_err(|e| format!("mkdir: {e}"))?;

    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir: {e}"))?;
    let tool_file = format!("{name}-tool");
    write(&dir, PLUGIN_MANIFEST_FILE, &gen_process_manifest(name))?;
    write(&bin_dir, &tool_file, &gen_process_tool_script())?;
    make_executable(&bin_dir.join(&tool_file))?;
    write(&dir, "README.md", &gen_readme(name))?;
    Ok(dir_name)
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("plugin name cannot be empty".into());
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("plugin name must not contain path separators or '..'".into());
    }
    if name.contains(' ') {
        return Err("plugin name must not contain spaces".into());
    }
    Ok(())
}

fn write(dir: &Path, file: &str, content: &str) -> Result<(), String> {
    fs::write(dir.join(file), content).map_err(|e| format!("write {file}: {e}"))
}

fn gen_process_manifest(name: &str) -> String {
    format!(
        "name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         description = \"A process-isolated Cortex plugin\"\n\
         cortex_version = \"1.1.0\"\n\n\
         [capabilities]\n\
         provides = [\"tools\", \"skills\"]\n\n\
         [native]\n\
         isolation = \"process\"\n\n\
         [[native.tools]]\n\
         name = \"{name}\"\n\
         description = \"A process-isolated Cortex tool\"\n\
         command = \"bin/{name}-tool\"\n\
         inherit_env = [\"PATH\"]\n\
         timeout_secs = 5\n\
         max_output_bytes = 1048576\n\
         max_memory_bytes = 67108864\n\
         max_cpu_secs = 2\n\
         input_schema = {{ type = \"object\", properties = {{ input = {{ type = \"string\" }} }}, required = [\"input\"] }}\n"
    )
}

fn gen_process_tool_script() -> String {
    "#!/bin/sh\n\
     set -eu\n\
     request=$(cat)\n\
     input=$(printf '%s' \"$request\" | sed -n 's/.*\"input\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p')\n\
     printf '{\"output\":\"Processed: %s\",\"is_error\":false}\\n' \"$input\"\n"
        .into()
}

fn gen_readme(name: &str) -> String {
    format!(
        "# cortex-plugin-{name}\n\n\
         A process-isolated Cortex plugin.\n\n\
         ## Boundary\n\n\
         This scaffold uses the stable process JSON protocol. Cortex starts the manifest-declared command per tool call, writes JSON to stdin, and reads a JSON result from stdout.\n\n\
         ## Build & Pack\n\n\
         ```bash\n\
         cortex plugin pack .\n\
         ```\n\n\
         ## Install\n\n\
         ```bash\n\
         cortex plugin install ./cortex-plugin-{name}-v0.1.0-linux-amd64.cpx\n\
         cortex restart\n\
         ```\n\n\
         ## License\n\nMIT\n"
    )
}

fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| format!("read permissions for {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("set executable bit for {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_names() {
        assert!(generate_process_plugin("../../etc/test").is_err());
        assert!(generate_process_plugin("my plugin").is_err());
        assert!(generate_process_plugin("").is_err());
        assert!(generate_process_plugin("test\\bad").is_err());
    }

    #[test]
    fn creates_process_plugin_files() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(generate_process_plugin_in("test-tool", tmp.path()).is_ok());
        let dir = tmp.path().join("cortex-plugin-test-tool");
        assert!(!dir.join("Cargo.toml").exists());
        assert!(dir.join(PLUGIN_MANIFEST_FILE).exists());
        assert!(dir.join("bin/test-tool-tool").exists());
        assert!(dir.join(PLUGIN_SKILLS_DIR).is_dir());
        assert!(dir.join("prompts").is_dir());

        let manifest = fs::read_to_string(dir.join(PLUGIN_MANIFEST_FILE)).unwrap();
        assert!(manifest.contains("isolation = \"process\""));
        assert!(manifest.contains("max_memory_bytes"));
        assert!(manifest.contains("command = \"bin/test-tool-tool\""));

        let readme = fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(readme.contains("stable process JSON protocol"));
    }

    #[test]
    fn rejects_existing() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("cortex-plugin-existing")).unwrap();
        assert!(generate_process_plugin_in("existing", tmp.path()).is_err());
    }
}
