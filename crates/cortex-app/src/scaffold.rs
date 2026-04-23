use std::fs;
use std::path::Path;

use crate::plugin_manager::{PLUGIN_MANIFEST_FILE, PLUGIN_SKILLS_DIR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginBoundary {
    TrustedInProcess,
    Process,
}

/// Generate a Cortex plugin project at `<cwd>/cortex-plugin-<name>/`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_plugin(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    generate_plugin_in(name, &cwd)
}

/// Generate a process-isolated Cortex plugin project at `<cwd>/cortex-plugin-<name>/`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_process_plugin(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    generate_process_plugin_in(name, &cwd)
}

/// Generate a Cortex plugin project inside `base_dir`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_plugin_in(name: &str, base_dir: &Path) -> Result<String, String> {
    generate_plugin_in_with_boundary(name, base_dir, PluginBoundary::TrustedInProcess)
}

/// Generate a process-isolated Cortex plugin project inside `base_dir`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_process_plugin_in(name: &str, base_dir: &Path) -> Result<String, String> {
    generate_plugin_in_with_boundary(name, base_dir, PluginBoundary::Process)
}

fn generate_plugin_in_with_boundary(
    name: &str,
    base_dir: &Path,
    boundary: PluginBoundary,
) -> Result<String, String> {
    validate_name(name)?;
    let dir_name = format!("cortex-plugin-{name}");
    let dir = base_dir.join(&dir_name);
    if dir.exists() {
        return Err(format!("directory '{dir_name}' already exists"));
    }
    fs::create_dir_all(dir.join(PLUGIN_SKILLS_DIR)).map_err(|e| format!("mkdir: {e}"))?;
    fs::create_dir_all(dir.join("prompts")).map_err(|e| format!("mkdir: {e}"))?;

    match boundary {
        PluginBoundary::TrustedInProcess => {
            let src_dir = dir.join("src");
            fs::create_dir_all(&src_dir).map_err(|e| format!("mkdir: {e}"))?;
            let u = name.replace('-', "_");
            let t = to_pascal_case(name);
            write(&dir, "Cargo.toml", &gen_cargo(name))?;
            write(&dir, PLUGIN_MANIFEST_FILE, &gen_manifest(name, &u))?;
            write(&src_dir, "lib.rs", &gen_lib(name, &t))?;
            write(&dir, "README.md", &gen_readme(name, boundary))?;
        }
        PluginBoundary::Process => {
            let bin_dir = dir.join("bin");
            fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir: {e}"))?;
            let tool_file = format!("{name}-tool");
            write(&dir, PLUGIN_MANIFEST_FILE, &gen_process_manifest(name))?;
            write(&bin_dir, &tool_file, &gen_process_tool_script())?;
            make_executable(&bin_dir.join(&tool_file))?;
            write(&dir, "README.md", &gen_readme(name, boundary))?;
        }
    }
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

fn gen_cargo(name: &str) -> String {
    format!(
        "[package]\n\
         name = \"cortex-plugin-{name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         license = \"MIT\"\n\n\
         [lib]\n\
         crate-type = [\"cdylib\"]\n\n\
         [dependencies]\n\
         cortex-sdk = \"1.0\"\n\
         serde_json = \"1\"\n"
    )
}

fn gen_manifest(name: &str, u: &str) -> String {
    format!(
        "name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         description = \"A Cortex plugin\"\n\
         cortex_version = \"1.1.0\"\n\n\
         [capabilities]\n\
         provides = [\"tools\", \"skills\"]\n\n\
         [native]\n\
         library = \"lib/libcortex_plugin_{u}.so\"\n\
         entry = \"cortex_plugin_create_multi\"\n"
    )
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

fn gen_lib(name: &str, t: &str) -> String {
    format!(
        "use cortex_sdk::prelude::*;\n\n\
         #[derive(Default)]\n\
         struct {t}Plugin;\n\n\
         impl MultiToolPlugin for {t}Plugin {{\n\
         \x20   fn plugin_info(&self) -> PluginInfo {{\n\
         \x20       PluginInfo {{\n\
         \x20           name: \"{name}\".into(),\n\
         \x20           version: env!(\"CARGO_PKG_VERSION\").into(),\n\
         \x20           description: \"A Cortex plugin\".into(),\n\
         \x20       }}\n\
         \x20   }}\n\n\
         \x20   fn create_tools(&self) -> Vec<Box<dyn Tool>> {{\n\
         \x20       vec![Box::new({t}Tool)]\n\
         \x20   }}\n\
         }}\n\n\
         struct {t}Tool;\n\n\
         impl Tool for {t}Tool {{\n\
         \x20   fn name(&self) -> &'static str {{ \"{name}\" }}\n\
         \x20   fn description(&self) -> &'static str {{ \"A Cortex plugin tool\" }}\n\
         \x20   fn input_schema(&self) -> serde_json::Value {{\n\
         \x20       serde_json::json!({{\"type\":\"object\",\"properties\":{{\"input\":{{\"type\":\"string\"}}}},\"required\":[\"input\"]}})\n\
         \x20   }}\n\
         \x20   fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {{\n\
         \x20       let text = input[\"input\"].as_str().unwrap_or(\"(empty)\");\n\
         \x20       Ok(ToolResult::success(format!(\"Processed: {{text}}\")))\n\
         \x20   }}\n\
         }}\n\n\
         cortex_sdk::export_plugin!({t}Plugin);\n"
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

fn gen_readme(name: &str, boundary: PluginBoundary) -> String {
    let (summary, build, note) = match boundary {
        PluginBoundary::TrustedInProcess => (
            "A trusted in-process Cortex plugin.",
            "cargo build --release\ncortex plugin pack .",
            "This scaffold uses Cortex's trusted in-process Rust extension boundary. Use it only for plugins you trust to run inside the daemon process. For the recommended long-term compatibility boundary, generate a process plugin with `cortex --new-process-plugin <name>`.",
        ),
        PluginBoundary::Process => (
            "A process-isolated Cortex plugin.",
            "cortex plugin pack .",
            "This scaffold uses the stable process JSON protocol. Cortex starts the manifest-declared command per tool call, writes JSON to stdin, and reads a JSON result from stdout.",
        ),
    };
    format!(
        "# cortex-plugin-{name}\n\n\
         {summary}\n\n\
         ## Boundary\n\n\
         {note}\n\n\
         ## Build & Pack\n\n\
         ```bash\n\
         {build}\n\
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

fn to_pascal_case(s: &str) -> String {
    s.split(['-', '_'])
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |c| {
                let upper: String = c.to_uppercase().collect();
                upper + chars.as_str()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case() {
        assert_eq!(to_pascal_case("my-tool"), "MyTool");
        assert_eq!(to_pascal_case("simple"), "Simple");
    }

    #[test]
    fn rejects_bad_names() {
        assert!(generate_plugin("../../etc/test").is_err());
        assert!(generate_plugin("my plugin").is_err());
        assert!(generate_plugin("").is_err());
        assert!(generate_plugin("test\\bad").is_err());
    }

    #[test]
    fn creates_files() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(generate_plugin_in("test-tool", tmp.path()).is_ok());
        let dir = tmp.path().join("cortex-plugin-test-tool");
        assert!(dir.join("Cargo.toml").exists());
        assert!(dir.join(PLUGIN_MANIFEST_FILE).exists());
        assert!(dir.join("src/lib.rs").exists());
        assert!(dir.join(PLUGIN_SKILLS_DIR).is_dir());
        assert!(dir.join("prompts").is_dir());
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("cortex-sdk"));
        let lib = fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert!(lib.contains("export_plugin!"));
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
        assert!(generate_plugin_in("existing", tmp.path()).is_err());
    }
}
