use std::fs;
use std::path::Path;

/// Generate a Cortex plugin project at `<cwd>/cortex-plugin-<name>/`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_plugin(name: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("cannot read cwd: {e}"))?;
    generate_plugin_in(name, &cwd)
}

/// Generate a Cortex plugin project inside `base_dir`.
///
/// # Errors
/// Returns an error string if the directory or files cannot be created.
pub fn generate_plugin_in(name: &str, base_dir: &Path) -> Result<String, String> {
    validate_name(name)?;
    let dir_name = format!("cortex-plugin-{name}");
    let dir = base_dir.join(&dir_name);
    if dir.exists() {
        return Err(format!("directory '{dir_name}' already exists"));
    }
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("mkdir: {e}"))?;
    fs::create_dir_all(dir.join("skills")).map_err(|e| format!("mkdir: {e}"))?;

    let u = name.replace('-', "_");
    let t = to_pascal_case(name);
    write(&dir, "Cargo.toml", &gen_cargo(name))?;
    write(&dir, "manifest.toml", &gen_manifest(name, &u))?;
    write(&src_dir, "lib.rs", &gen_lib(name, &t))?;
    write(&dir, "Makefile", &gen_makefile(name, &u))?;
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
         cortex_version = \"1.0.0\"\n\n\
         [capabilities]\n\
         provides = [\"tools\", \"skills\"]\n\n\
         [native]\n\
         library = \"lib/libcortex_plugin_{u}.so\"\n\
         entry = \"cortex_plugin_create_multi\"\n"
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

fn gen_makefile(name: &str, u: &str) -> String {
    format!(
        ".PHONY: build pack clean\n\n\
         build:\n\
         \tcargo build --release\n\n\
         pack: build\n\
         \tmkdir -p .pack/lib\n\
         \tcp manifest.toml .pack/\n\
         \tcp target/release/libcortex_plugin_{u}.so .pack/lib/\n\
         \t[ -d skills ] && cp -r skills .pack/ || true\n\
         \t[ -d prompts ] && cp -r prompts .pack/ || true\n\
         \tcd .pack && tar czf ../cortex-plugin-{name}.cpx .\n\
         \trm -rf .pack\n\
         \t@echo \"Created cortex-plugin-{name}.cpx\"\n\n\
         clean:\n\
         \tcargo clean\n\
         \trm -rf .pack cortex-plugin-{name}.cpx\n"
    )
}

fn gen_readme(name: &str) -> String {
    format!(
        "# cortex-plugin-{name}\n\n\
         A Cortex plugin.\n\n\
         ## Build\n\n\
         ```bash\nmake build\n```\n\n\
         ## Pack\n\n\
         ```bash\nmake pack\n```\n\n\
         ## Install\n\n\
         ```bash\n\
         cortex plugin install ./cortex-plugin-{name}.cpx\n\
         cortex restart\n\
         ```\n\n\
         ## License\n\nMIT\n"
    )
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
        assert!(dir.join("manifest.toml").exists());
        assert!(dir.join("src/lib.rs").exists());
        assert!(dir.join("Makefile").exists());
        assert!(dir.join("skills").is_dir());
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("cortex-sdk"));
        let lib = fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert!(lib.contains("export_plugin!"));
    }

    #[test]
    fn rejects_existing() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("cortex-plugin-existing")).unwrap();
        assert!(generate_plugin_in("existing", tmp.path()).is_err());
    }
}
