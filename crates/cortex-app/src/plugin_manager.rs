use std::fs;
use std::io::Read;
use std::path::Path;

/// Metadata about an installed plugin, parsed from its manifest.
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub has_native: bool,
}

// ── Helpers ────────────────────────────────────────────────────

/// Read a TOML value from manifest text.
fn manifest_field(text: &str, key: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim();
            if let Some(val) = rest.strip_prefix('=') {
                return val.trim().trim_matches('"').to_string();
            }
        }
    }
    String::new()
}

/// Parse the `provides = [...]` array from manifest text.
fn manifest_provides(text: &str) -> Vec<String> {
    let mut in_capabilities = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[capabilities]" {
            in_capabilities = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[capabilities]" {
            in_capabilities = false;
            continue;
        }
        if in_capabilities && let Some(rest) = trimmed.strip_prefix("provides") {
            let rest = rest.trim();
            if let Some(val) = rest.strip_prefix('=') {
                return parse_toml_string_array(val.trim());
            }
        }
    }
    Vec::new()
}

/// Parse a simple TOML string array like `["tools", "skills"]`.
fn parse_toml_string_array(s: &str) -> Vec<String> {
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Check whether a plugin directory contains any native library files.
fn has_native_library(plugin_dir: &Path) -> bool {
    let lib_dir = plugin_dir.join("lib");
    if !lib_dir.is_dir() {
        return has_so_files(plugin_dir);
    }
    has_so_files(&lib_dir)
}

fn has_so_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let s = name.to_string_lossy();
        s.ends_with(".so") || s.ends_with(".dylib")
    })
}

// ── Install from local .cpx file ──────────────────────────────

/// Install a plugin from a local `.cpx` archive (gzip-compressed tar).
///
/// Reads `manifest.toml` from the archive to determine the plugin name,
/// then extracts all contents to `{cortex_home}/plugins/{name}/`.
///
/// # Errors
/// Returns an error message if the archive cannot be read, lacks a
/// manifest, or extraction fails.
pub fn install_cpx(cortex_home: &Path, cpx_path: &Path) -> Result<String, String> {
    // First pass: find manifest.toml to get the plugin name.
    let manifest_text = read_manifest_from_cpx(cpx_path)?;
    let name = manifest_field(&manifest_text, "name");
    if name.is_empty() {
        return Err("manifest.toml missing 'name' field".into());
    }

    let dest = cortex_home.join("plugins").join(&name);

    // Back up existing installation.
    let backup = cortex_home.join("plugins").join(format!("{name}.bak"));
    if dest.exists() {
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        fs::rename(&dest, &backup).map_err(|e| format!("failed to backup existing plugin: {e}"))?;
        eprintln!("Backed up existing plugin to {}", backup.display());
    }

    fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;

    eprintln!("Extracting to {} ...", dest.display());

    // Re-open for extraction (tar::Archive is consumed by iteration).
    let file2 = fs::File::open(cpx_path)
        .map_err(|e| format!("cannot reopen {}: {e}", cpx_path.display()))?;
    let gz2 = flate2::read::GzDecoder::new(file2);
    tar::Archive::new(gz2)
        .unpack(&dest)
        .map_err(|e| format!("extraction failed: {e}"))?;

    // Clean up backup on success.
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }

    Ok(name)
}

/// Read `manifest.toml` from a .cpx archive without fully extracting.
fn read_manifest_from_cpx(cpx_path: &Path) -> Result<String, String> {
    let file =
        fs::File::open(cpx_path).map_err(|e| format!("cannot open {}: {e}", cpx_path.display()))?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| format!("cannot read archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("invalid archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("invalid path in archive: {e}"))?;
        if path.as_ref() == Path::new("manifest.toml") {
            let mut buf = String::new();
            entry
                .read_to_string(&mut buf)
                .map_err(|e| format!("cannot read manifest.toml: {e}"))?;
            return Ok(buf);
        }
    }
    Err("cpx archive missing manifest.toml".into())
}

// ── Install from URL ──────────────────────────────────────────

/// Install a plugin by downloading a `.cpx` file from a URL.
///
/// Uses `curl` for the download (sync, no async runtime needed).
///
/// # Errors
/// Returns an error message if the download or installation fails.
pub fn install_url(cortex_home: &Path, url: &str) -> Result<String, String> {
    eprintln!("Downloading {url} ...");

    let tmp_dir = tempfile::tempdir().map_err(|e| format!("cannot create temp directory: {e}"))?;
    let tmp_path = tmp_dir.path().join("plugin.cpx");

    let output = std::process::Command::new("curl")
        .args(["-fSL", "--connect-timeout", "30", "--max-time", "300", "-o"])
        .arg(&tmp_path)
        .arg(url)
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("download failed: {stderr}"));
    }

    install_cpx(cortex_home, &tmp_path)
}

// ── Install by name (GitHub) ──────────────────────────────────

/// Install a plugin by name, resolving to a GitHub release URL.
///
/// Tries `github.com/by-scott/cortex-plugin-{name}` releases.
///
/// # Errors
/// Returns an error message if the download or installation fails.
pub fn install_name(cortex_home: &Path, name: &str) -> Result<String, String> {
    let url = if let Some((owner, repo)) = name.split_once('/') {
        format!("https://github.com/{owner}/{repo}/releases/latest/download/{repo}.cpx")
    } else {
        format!(
            "https://github.com/by-scott/cortex-plugin-{name}/releases/latest/download/cortex-plugin-{name}.cpx"
        )
    };
    install_url(cortex_home, &url)
}

// ── Install dispatcher ────────────────────────────────────────

/// Install a plugin from any source: local `.cpx` file, URL, directory,
/// or name.
///
/// Auto-detects the source type:
/// - Ends with `.cpx` and exists as a file -> local cpx
/// - Starts with `http://` or `https://` -> URL download
/// - Exists as a directory -> copy from directory
/// - Otherwise -> resolve as plugin name via GitHub
///
/// # Errors
/// Returns an error message if the installation fails.
pub fn install(cortex_home: &Path, source: &str) -> Result<String, String> {
    let source_path = Path::new(source);
    if source_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cpx"))
        && source_path.is_file()
    {
        install_cpx(cortex_home, source_path)
    } else if source.starts_with("http://") || source.starts_with("https://") {
        install_url(cortex_home, source)
    } else if source_path.is_dir() {
        install_from_directory(cortex_home, source_path)
    } else {
        install_name(cortex_home, source)
    }
}

/// Install a plugin by copying files from a local directory.
///
/// # Errors
/// Returns an error message if the directory is invalid or the copy fails.
fn install_from_directory(cortex_home: &Path, dir: &Path) -> Result<String, String> {
    let manifest_path = dir.join("manifest.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "directory {} does not contain manifest.toml",
            dir.display()
        ));
    }
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read manifest.toml: {e}"))?;
    let name = manifest_field(&manifest_text, "name");
    if name.is_empty() {
        return Err("manifest.toml missing 'name' field".into());
    }

    let dest = cortex_home.join("plugins").join(&name);

    if dest.exists() {
        let backup = cortex_home.join("plugins").join(format!("{name}.bak"));
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        fs::rename(&dest, &backup).map_err(|e| format!("failed to backup existing plugin: {e}"))?;
    }

    eprintln!("Installing from directory {} ...", dir.display());
    copy_dir_recursive(dir, &dest)?;
    copy_built_native_library_if_present(dir, &dest, &manifest_text)?;
    Ok(name)
}

fn copy_built_native_library_if_present(
    src_dir: &Path,
    dest_dir: &Path,
    manifest_text: &str,
) -> Result<(), String> {
    let mut in_native = false;
    let mut library_rel = None::<String>;
    for line in manifest_text.lines() {
        let trimmed = line.trim();
        if trimmed == "[native]" {
            in_native = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[native]" {
            in_native = false;
        }
        if in_native && let Some(rest) = trimmed.strip_prefix("library") {
            let rest = rest.trim();
            if let Some(val) = rest.strip_prefix('=') {
                library_rel = Some(val.trim().trim_matches('"').to_string());
                break;
            }
        }
    }

    let Some(library_rel) = library_rel else {
        return Ok(());
    };

    let library_name = Path::new(&library_rel)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid native.library path in manifest.toml".to_string())?;

    let built_candidates = [
        src_dir.join("target/release").join(library_name),
        src_dir.join("target/debug").join(library_name),
    ];

    let Some(built_path) = built_candidates.iter().find(|p| p.is_file()) else {
        return Ok(());
    };

    let final_path = dest_dir.join(&library_rel);
    if final_path.is_file() {
        return Ok(());
    }

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }

    fs::copy(built_path, &final_path).map_err(|e| {
        format!(
            "cannot copy built native library {} -> {}: {e}",
            built_path.display(),
            final_path.display()
        )
    })?;
    eprintln!("Copied built native library to {}", final_path.display());
    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let entries = fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("directory entry error: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "cannot copy {} -> {}: {e}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

// ── Uninstall ─────────────────────────────────────────────────

/// Remove an installed plugin.
///
/// # Errors
/// Returns an error message if the plugin is not found or removal fails.
pub fn uninstall(cortex_home: &Path, name: &str) -> Result<(), String> {
    let dest = cortex_home.join("plugins").join(name);
    if !dest.exists() {
        return Err(format!("plugin '{name}' is not installed"));
    }
    fs::remove_dir_all(&dest).map_err(|e| format!("failed to remove plugin '{name}': {e}"))
}

// ── List ──────────────────────────────────────────────────────

/// List all installed plugins by scanning
/// `{cortex_home}/plugins/*/manifest.toml`.
#[must_use]
pub fn list(cortex_home: &Path) -> Vec<PluginInfo> {
    let plugins_dir = cortex_home.join("plugins");
    let Ok(entries) = fs::read_dir(&plugins_dir) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let sub = entry.path();
        if !sub.is_dir() {
            continue;
        }
        let Ok(text) = fs::read_to_string(sub.join("manifest.toml")) else {
            continue;
        };
        let name = manifest_field(&text, "name");
        if name.is_empty() {
            continue;
        }
        result.push(PluginInfo {
            version: manifest_field(&text, "version"),
            description: manifest_field(&text, "description"),
            capabilities: manifest_provides(&text),
            has_native: has_native_library(&sub),
            name,
        });
    }
    result
}

// ── Pack ──────────────────────────────────────────────────────

/// Create a `.cpx` archive (gzip-compressed tar) from a plugin directory.
///
/// The directory must contain a `manifest.toml`. The archive will include
/// `manifest.toml` plus any `lib/`, `skills/`, and `prompts/`
/// subdirectories.
///
/// # Errors
/// Returns an error message if the source directory is invalid or archive
/// creation fails.
pub fn pack(source_dir: &Path, output_path: &Path) -> Result<(), String> {
    let manifest_path = source_dir.join("manifest.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "directory {} does not contain manifest.toml",
            source_dir.display()
        ));
    }

    let file = fs::File::create(output_path)
        .map_err(|e| format!("cannot create {}: {e}", output_path.display()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Add manifest.toml at the root of the archive.
    tar.append_path_with_name(&manifest_path, "manifest.toml")
        .map_err(|e| format!("cannot add manifest.toml: {e}"))?;

    // Add known subdirectories.
    for subdir in ["lib", "skills", "prompts"] {
        let full = source_dir.join(subdir);
        if full.is_dir() {
            tar.append_dir_all(subdir, &full)
                .map_err(|e| format!("cannot add {subdir}/: {e}"))?;
        }
    }

    tar.into_inner()
        .map_err(|e| format!("finalize tar: {e}"))?
        .finish()
        .map_err(|e| format!("finalize gzip: {e}"))?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_field_parses_name() {
        let text = "name = \"my-plugin\"\nversion = \"1.0.0\"\n";
        assert_eq!(manifest_field(text, "name"), "my-plugin");
        assert_eq!(manifest_field(text, "version"), "1.0.0");
        assert_eq!(manifest_field(text, "missing"), "");
    }

    #[test]
    fn manifest_provides_parses_capabilities() {
        let text = "[capabilities]\nprovides = [\"tools\", \"skills\"]\n";
        assert_eq!(manifest_provides(text), vec!["tools", "skills"]);
    }

    #[test]
    fn pack_and_install_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        fs::create_dir_all(home.join("plugins")).unwrap();

        // Build source directory.
        let src = tmp.path().join("src-plugin");
        fs::create_dir_all(src.join("skills/demo")).unwrap();
        fs::create_dir_all(src.join("prompts")).unwrap();
        fs::write(
            src.join("manifest.toml"),
            "name = \"demo\"\nversion = \"0.1.0\"\ndescription = \"A demo\"\n\n[capabilities]\nprovides = [\"skills\", \"prompts\"]\n",
        ).unwrap();
        fs::write(src.join("skills/demo/SKILL.md"), "# Demo\n").unwrap();
        fs::write(src.join("prompts/hint.md"), "Be helpful.\n").unwrap();

        // Pack.
        let cpx = tmp.path().join("demo.cpx");
        pack(&src, &cpx).unwrap();
        assert!(cpx.exists());

        // Install.
        let name = install_cpx(home, &cpx).unwrap();
        assert_eq!(name, "demo");
        assert!(home.join("plugins/demo/manifest.toml").exists());
        assert!(home.join("plugins/demo/skills/demo/SKILL.md").exists());
        assert!(home.join("plugins/demo/prompts/hint.md").exists());

        // List.
        let plugins = list(home);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "demo");

        // Uninstall.
        uninstall(home, "demo").unwrap();
        assert!(!home.join("plugins/demo").exists());
    }

    #[test]
    fn install_from_dir_works() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir_all(home.join("plugins")).unwrap();

        let src = tmp.path().join("my-plugin");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("manifest.toml"),
            "name = \"my-plugin\"\nversion = \"2.0.0\"\ndescription = \"test\"\n",
        )
        .unwrap();

        let name = install(&home, src.to_str().unwrap()).unwrap();
        assert_eq!(name, "my-plugin");
        assert!(home.join("plugins/my-plugin/manifest.toml").exists());
    }

    #[test]
    fn uninstall_nonexistent_fails() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("plugins")).unwrap();
        assert!(
            uninstall(tmp.path(), "nonexistent")
                .unwrap_err()
                .contains("not installed")
        );
    }
}
