use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) const PLUGIN_MANIFEST_FILE: &str = "manifest.toml";
pub(crate) const PLUGIN_LIB_DIR: &str = "lib";
pub(crate) const PLUGIN_SKILLS_DIR: &str = "skills";
pub(crate) const PLUGIN_PROMPTS_DIR: &str = "prompts";

fn should_include_plugin_entry_name(name: &str) -> bool {
    let is_backup = Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"));
    !name.starts_with('.') && !is_backup && !name.ends_with('~')
}

fn should_scan_plugin_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    should_include_plugin_entry_name(name)
}

fn normalize_plugin_rel_path(path: &Path) -> Option<std::path::PathBuf> {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(value) => normalized.push(value),
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

fn is_allowed_plugin_rel_path(path: &Path) -> bool {
    let Some(normalized) = normalize_plugin_rel_path(path) else {
        return false;
    };
    if !normalized.components().all(|component| match component {
        std::path::Component::Normal(value) => {
            value.to_str().is_some_and(should_include_plugin_entry_name)
        }
        _ => false,
    }) {
        return false;
    }
    if normalized == Path::new(PLUGIN_MANIFEST_FILE) {
        return true;
    }
    normalized
        .components()
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .is_some_and(|name| {
            matches!(
                name,
                PLUGIN_LIB_DIR | PLUGIN_SKILLS_DIR | PLUGIN_PROMPTS_DIR
            )
        })
}

fn plugins_dir(cortex_home: &Path) -> std::path::PathBuf {
    cortex_home.join("plugins")
}

fn plugin_dir(cortex_home: &Path, name: &str) -> std::path::PathBuf {
    plugins_dir(cortex_home).join(name)
}

fn plugin_backup_dir(cortex_home: &Path, name: &str) -> std::path::PathBuf {
    plugins_dir(cortex_home).join(format!("{name}.bak"))
}

/// Metadata about an installed plugin, parsed from its manifest.
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub has_native: bool,
}

// ── Helpers ────────────────────────────────────────────────────

/// Return the conventional `.cpx` archive name for a plugin directory.
///
/// The name follows release-asset convention:
/// `{directory}-v{version}-{platform}.cpx`.
/// For example, packing `cortex-plugin-dev` with manifest version `1.3.0`
/// defaults to `cortex-plugin-dev-v1.3.0-linux-amd64.cpx`.
///
/// # Errors
/// Returns an error if the directory has no manifest or no version field.
pub fn default_cpx_name(source_dir: &Path) -> Result<String, String> {
    let manifest_path = source_dir.join(PLUGIN_MANIFEST_FILE);
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let version = manifest_field(&manifest_text, "version");
    if version.is_empty() {
        return Err("manifest.toml missing 'version' field".into());
    }
    let dir_name = package_dir_name(source_dir)?;
    Ok(format!("{dir_name}-v{version}-{}.cpx", current_platform()?))
}

fn current_platform() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "macos",
        other => return Err(format!("unsupported OS for plugin archive naming: {other}")),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => {
            return Err(format!(
                "unsupported architecture for plugin archive naming: {other}"
            ));
        }
    };
    Ok(format!("{os}-{arch}"))
}

fn package_dir_name(source_dir: &Path) -> Result<String, String> {
    let path = if source_dir == Path::new(".") {
        std::env::current_dir().map_err(|e| format!("cannot read current directory: {e}"))?
    } else {
        source_dir.to_path_buf()
    };
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("cannot derive package name from {}", source_dir.display()))
}

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
    let lib_dir = plugin_dir.join(PLUGIN_LIB_DIR);
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

    let dest = plugin_dir(cortex_home, &name);

    // Back up existing installation.
    let backup = plugin_backup_dir(cortex_home, &name);
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
    let mut archive = tar::Archive::new(gz2);
    for entry in archive
        .entries()
        .map_err(|e| format!("cannot read archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("invalid archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("invalid path in archive: {e}"))?;
        let Some(relative_path) = normalize_plugin_rel_path(path.as_ref()) else {
            continue;
        };
        if !is_allowed_plugin_rel_path(&relative_path) {
            continue;
        }
        let target_path = dest.join(&relative_path);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&target_path)
                .map_err(|e| format!("cannot create {}: {e}", target_path.display()))?;
            continue;
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        entry
            .unpack(&target_path)
            .map_err(|e| format!("cannot extract {}: {e}", target_path.display()))?;
    }

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
/// Supports optional versions: `dev@1.3.0` or
/// `owner/cortex-plugin-dev@v1.3.0`.
///
/// # Errors
/// Returns an error message if the download or installation fails.
pub fn install_name(cortex_home: &Path, name: &str) -> Result<String, String> {
    let (name, version) = name
        .rsplit_once('@')
        .map_or((name, None), |(base, version)| (base, Some(version)));
    let (owner, repo) = if let Some((owner, repo)) = name.split_once('/') {
        (owner.to_string(), repo.to_string())
    } else {
        ("by-scott".to_string(), format!("cortex-plugin-{name}"))
    };
    let url = github_cpx_url(&owner, &repo, version)?;
    install_url(cortex_home, &url)
}

fn github_cpx_url(owner: &str, repo: &str, version: Option<&str>) -> Result<String, String> {
    let api = version.map_or_else(
        || format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
        |version| {
            let tag = if version.starts_with('v') {
                version.to_string()
            } else {
                format!("v{version}")
            };
            format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}")
        },
    );
    let output = std::process::Command::new("curl")
        .args([
            "-fSL",
            "--connect-timeout",
            "30",
            "--max-time",
            "300",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: cortex-plugin-installer",
        ])
        .arg(&api)
        .output()
        .map_err(|e| format!("failed to run curl: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cannot read GitHub release metadata: {stderr}"));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("invalid GitHub release metadata: {e}"))?;
    let assets = json
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "GitHub release metadata missing assets".to_string())?;

    let platform = current_platform()?;
    let mut candidates = assets
        .iter()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let url = asset.get("browser_download_url")?.as_str()?;
            Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cpx"))
                .then(|| (name.to_string(), url.to_string()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(asset_name, _)| {
        let versioned = asset_name.starts_with(&format!("{repo}-v"));
        let platform_match = asset_name
            .strip_suffix(".cpx")
            .is_some_and(|name| name.ends_with(&format!("-{platform}")));
        (u8::from(!platform_match), u8::from(!versioned))
    });

    candidates
        .into_iter()
        .find_map(|(asset_name, url)| {
            asset_name
                .strip_suffix(".cpx")
                .is_some_and(|name| name.ends_with(&format!("-{platform}")))
                .then_some(url)
        })
        .ok_or_else(|| {
            format!("selected release for {owner}/{repo} has no .cpx asset for {platform}")
        })
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
    let manifest_path = dir.join(PLUGIN_MANIFEST_FILE);
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

    let dest = plugin_dir(cortex_home, &name);
    let backup = plugin_backup_dir(cortex_home, &name);

    if dest.exists() {
        if backup.exists() {
            let _ = fs::remove_dir_all(&backup);
        }
        fs::rename(&dest, &backup).map_err(|e| format!("failed to backup existing plugin: {e}"))?;
    }

    eprintln!("Installing from directory {} ...", dir.display());
    fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    fs::copy(&manifest_path, dest.join(PLUGIN_MANIFEST_FILE))
        .map_err(|e| format!("cannot copy {}: {e}", manifest_path.display()))?;
    for subdir in [PLUGIN_LIB_DIR, PLUGIN_SKILLS_DIR, PLUGIN_PROMPTS_DIR] {
        let src_subdir = dir.join(subdir);
        if src_subdir.is_dir() {
            copy_dir_recursive(&src_subdir, &dest.join(subdir))?;
        }
    }
    copy_built_native_library_if_present(dir, &dest, &manifest_text)?;
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
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
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        if !should_include_plugin_entry_name(name_text) {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(name);
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
    let dest = plugin_dir(cortex_home, name);
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
    let plugins_dir = plugins_dir(cortex_home);
    let Ok(entries) = fs::read_dir(&plugins_dir) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let sub = entry.path();
        if !sub.is_dir() || !should_scan_plugin_dir(&sub) {
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
/// **Auto-resolve native library:** If no `lib/` directory exists but the
/// manifest declares a `[native].library` path, the packer looks for the
/// corresponding `.so`/`.dylib` in `target/release/`. This lets developers
/// run `cortex plugin pack .` directly from the project root after
/// `cargo build --release` — no staging directory needed.
///
/// # Errors
/// Returns an error message if the source directory is invalid or archive
/// creation fails.
pub fn pack(source_dir: &Path, output_path: &Path) -> Result<(), String> {
    let manifest_path = source_dir.join(PLUGIN_MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Err(format!(
            "directory {} does not contain {PLUGIN_MANIFEST_FILE}",
            source_dir.display()
        ));
    }

    let file = fs::File::create(output_path)
        .map_err(|e| format!("cannot create {}: {e}", output_path.display()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);

    // Add manifest.toml at the root of the archive.
    tar.append_path_with_name(&manifest_path, PLUGIN_MANIFEST_FILE)
        .map_err(|e| format!("cannot add {PLUGIN_MANIFEST_FILE}: {e}"))?;

    // Resolve native library: prefer lib/ directory, fall back to target/release/.
    let lib_dir = source_dir.join(PLUGIN_LIB_DIR);
    if lib_dir.is_dir() {
        tar.append_dir_all(PLUGIN_LIB_DIR, &lib_dir)
            .map_err(|e| format!("cannot add {PLUGIN_LIB_DIR}/: {e}"))?;
    } else if let Some(lib_archive_path) = resolve_native_library(source_dir) {
        let (archive_path, disk_path) = lib_archive_path;
        // Create lib/ entry in the archive with the resolved file.
        tar.append_path_with_name(&disk_path, &archive_path)
            .map_err(|e| format!("cannot add {}: {e}", archive_path.display()))?;
    }

    // Add skills/ and prompts/ if present.
    for subdir in [PLUGIN_SKILLS_DIR, PLUGIN_PROMPTS_DIR] {
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

/// Resolve the native library from `target/release/` when no `lib/` directory exists.
///
/// Reads `[native].library` from the manifest (e.g. `lib/libfoo.so`) and looks
/// for the filename in `target/release/`. Returns `(archive_path, disk_path)`.
fn resolve_native_library(source_dir: &Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let manifest_text = fs::read_to_string(source_dir.join(PLUGIN_MANIFEST_FILE)).ok()?;
    let lib_field = manifest_field(&manifest_text, "library");
    if lib_field.is_empty() {
        return None;
    }
    // lib_field is typically "lib/libfoo.so" — extract the filename.
    let lib_filename = Path::new(&lib_field).file_name()?.to_str()?;
    let candidate = source_dir.join("target/release").join(lib_filename);
    if candidate.is_file() {
        // Archive path preserves the manifest's declared path (e.g. "lib/libfoo.so").
        Some((Path::new(&lib_field).to_path_buf(), candidate))
    } else {
        None
    }
}

// ── Tests ─────────────────────────────────────────────────────
