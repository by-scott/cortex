use std::fs;
use std::path::Path;

use cortex_types::plugin::{PluginConformanceCheck, PluginManifest, PluginPackageMetadata};
use sha2::{Digest, Sha256};

pub(crate) const PLUGIN_MANIFEST_FILE: &str = "manifest.toml";
pub(crate) const PLUGIN_PACKAGE_FILE: &str = "package.toml";
pub(crate) const PLUGIN_SBOM_FILE: &str = "sbom.spdx.json";
pub(crate) const PLUGIN_RISK_PROFILE_FILE: &str = "risk.toml";
pub(crate) const PLUGIN_CONFORMANCE_FILE: &str = "conformance.toml";
pub(crate) const PLUGIN_LIB_DIR: &str = "lib";
pub(crate) const PLUGIN_SKILLS_DIR: &str = "skills";
pub(crate) const PLUGIN_PROMPTS_DIR: &str = "prompts";
pub(crate) const PLUGIN_TRUST_FILE: &str = "plugin-trust.toml";

mod archive;
mod conformance;
mod download;
mod pack;
mod signing;
mod types;

use archive::{extract_cpx_to_dir, read_manifest_from_cpx};
use conformance::{conformance_checks, conformance_state, recommended_risk_profile};
pub use download::{install_name, install_name_with_policy, install_url, install_url_with_policy};
pub use pack::{default_cpx_name, pack};
use signing::{PackageSignatureState, enforce_package_trust, package_signature_state};
pub use signing::{generate_signing_key, sign_directory};
pub use types::{PluginInfo, PluginReview};

/// Policy used by package installation when a signed plugin comes from a
/// publisher key not yet trusted by this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownPublisherPolicy {
    Reject,
    Prompt,
    TrustVerified,
}

/// Install-time security policy for packaged plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginInstallPolicy {
    pub require_packaged_signature: bool,
    pub unknown_publisher: UnknownPublisherPolicy,
}

impl PluginInstallPolicy {
    #[must_use]
    pub const fn release_default(unknown_publisher: UnknownPublisherPolicy) -> Self {
        Self {
            require_packaged_signature: true,
            unknown_publisher,
        }
    }

    #[must_use]
    pub const fn developer_default() -> Self {
        Self {
            require_packaged_signature: false,
            unknown_publisher: UnknownPublisherPolicy::TrustVerified,
        }
    }
}

impl Default for PluginInstallPolicy {
    fn default() -> Self {
        Self::developer_default()
    }
}

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

fn plugins_dir(cortex_home: &Path) -> std::path::PathBuf {
    cortex_home.join("plugins")
}

fn plugin_dir(cortex_home: &Path, name: &str) -> std::path::PathBuf {
    plugins_dir(cortex_home).join(name)
}

fn plugin_backup_dir(cortex_home: &Path, name: &str) -> std::path::PathBuf {
    plugins_dir(cortex_home).join(format!("{name}.bak"))
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

fn parse_manifest(text: &str) -> Result<PluginManifest, String> {
    toml::from_str(text).map_err(|err| format!("invalid manifest.toml: {err}"))
}

fn read_manifest_from_dir(dir: &Path) -> Result<(String, PluginManifest), String> {
    let manifest_path = dir.join(PLUGIN_MANIFEST_FILE);
    let text = fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let manifest = parse_manifest(&text)?;
    Ok((text, manifest))
}

fn read_package_metadata(dir: &Path, manifest: &PluginManifest) -> PluginPackageMetadata {
    let package_path = dir.join(PLUGIN_PACKAGE_FILE);
    if let Ok(text) = fs::read_to_string(package_path)
        && let Ok(package) = toml::from_str::<PluginPackageMetadata>(&text)
    {
        return package;
    }
    manifest.package.clone()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn is_native_library_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("so") || extension.eq_ignore_ascii_case("dylib")
    })
}

/// Build the operator-facing review for a plugin directory.
///
/// # Errors
/// Returns an error if the manifest cannot be read or parsed.
pub fn review_directory(dir: &Path) -> Result<PluginReview, String> {
    let (manifest_text, manifest) = read_manifest_from_dir(dir)?;
    let package = read_package_metadata(dir, &manifest);
    let manifest_hash = sha256_bytes(manifest_text.as_bytes());
    let mut checks = conformance_checks(dir, &manifest);
    let governance_check = match manifest.validate_governance() {
        Ok(()) => PluginConformanceCheck {
            name: "governance".to_string(),
            passed: true,
            message: String::new(),
        },
        Err(err) => PluginConformanceCheck {
            name: "governance".to_string(),
            passed: false,
            message: err,
        },
    };
    checks.insert(0, governance_check);

    let package_signature_state = package_signature_state(dir, &manifest, &package, &manifest_hash);
    let conformance_state = conformance_state(&package, checks.iter().all(|check| check.passed));
    let recommended_risk_profile = recommended_risk_profile(&manifest);
    let mut manifest_for_warnings = manifest.clone();
    manifest_for_warnings.package = package;
    let mut warnings = manifest_for_warnings.governance_warnings();
    if package_signature_state.is_invalid() {
        warnings.push("package signature verification failed".to_string());
    }
    if !checks.iter().all(|check| check.passed) {
        warnings.push("conformance checks are failing".to_string());
    }

    Ok(PluginReview {
        name: manifest.name,
        version: manifest.version,
        trust: format!("{:?}", manifest.trust),
        requested_capabilities: manifest.capabilities.requested_summary(),
        signature_state: package_signature_state.render(),
        conformance_state,
        recommended_risk_profile,
        warnings,
        checks,
    })
}

/// Run the local plugin conformance kit.
///
/// # Errors
/// Returns an error when the directory cannot be reviewed or any check fails.
pub fn test_directory(dir: &Path) -> Result<PluginReview, String> {
    let review = review_directory(dir)?;
    if review.passed() {
        Ok(review)
    } else {
        Err(review.render())
    }
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
    entries
        .flatten()
        .any(|entry| is_native_library_path(&entry.path()))
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
    install_cpx_with_policy(cortex_home, cpx_path, PluginInstallPolicy::default())
}

/// Install a `.cpx` archive with an explicit signature and publisher policy.
///
/// # Errors
/// Returns an error if the archive cannot be read or extracted, the manifest is
/// invalid, the package signature does not satisfy `policy`, publisher trust is
/// missing, or the existing installation cannot be backed up/restored.
pub fn install_cpx_with_policy(
    cortex_home: &Path,
    cpx_path: &Path,
    policy: PluginInstallPolicy,
) -> Result<String, String> {
    // First pass: find manifest.toml to get the plugin name.
    let manifest_text = read_manifest_from_cpx(cpx_path)?;
    let name = manifest_field(&manifest_text, "name");
    if name.is_empty() {
        return Err("manifest.toml missing 'name' field".into());
    }
    parse_manifest(&manifest_text)?.validate_governance()?;

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

    let install_result = (|| {
        extract_cpx_to_dir(cpx_path, &dest)?;
        enforce_installed_package_policy(cortex_home, &dest, policy)?;
        Ok(())
    })();

    if let Err(err) = install_result {
        let _ = fs::remove_dir_all(&dest);
        if backup.exists() {
            let _ = fs::rename(&backup, &dest);
        }
        return Err(err);
    }

    // Clean up backup on success.
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }

    Ok(name)
}

fn enforce_installed_package_policy(
    cortex_home: &Path,
    plugin_dir: &Path,
    policy: PluginInstallPolicy,
) -> Result<(), String> {
    let (manifest_text, manifest) = read_manifest_from_dir(plugin_dir)?;
    manifest.validate_governance()?;
    let package = read_package_metadata(plugin_dir, &manifest);
    let state = package_signature_state(
        plugin_dir,
        &manifest,
        &package,
        &sha256_bytes(manifest_text.as_bytes()),
    );
    if !policy.require_packaged_signature && matches!(state, PackageSignatureState::Unsigned(_)) {
        return Ok(());
    }
    enforce_package_trust(cortex_home, &state, policy)
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
    install_with_policy(cortex_home, source, PluginInstallPolicy::default())
}

/// Install a plugin source with an explicit package trust policy.
///
/// # Errors
/// Returns an error if source detection succeeds but the selected installer
/// fails, including manifest errors, archive download/extraction errors,
/// package signature failures, publisher trust rejection, or file copy errors.
pub fn install_with_policy(
    cortex_home: &Path,
    source: &str,
    policy: PluginInstallPolicy,
) -> Result<String, String> {
    let source_path = Path::new(source);
    if source_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cpx"))
        && source_path.is_file()
    {
        install_cpx_with_policy(cortex_home, source_path, policy)
    } else if source.starts_with("http://") || source.starts_with("https://") {
        install_url_with_policy(cortex_home, source, policy)
    } else if source_path.is_dir() {
        install_from_directory_with_policy(cortex_home, source_path, policy)
    } else {
        install_name_with_policy(cortex_home, source, policy)
    }
}

fn install_from_directory_with_policy(
    cortex_home: &Path,
    dir: &Path,
    policy: PluginInstallPolicy,
) -> Result<String, String> {
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
    let manifest = parse_manifest(&manifest_text)?;
    manifest.validate_governance()?;
    let package = read_package_metadata(dir, &manifest);
    let state = package_signature_state(
        dir,
        &manifest,
        &package,
        &sha256_bytes(manifest_text.as_bytes()),
    );
    if policy.require_packaged_signature || !matches!(state, PackageSignatureState::Unsigned(_)) {
        enforce_package_trust(cortex_home, &state, policy)?;
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
    for file in [
        PLUGIN_PACKAGE_FILE,
        PLUGIN_SBOM_FILE,
        PLUGIN_RISK_PROFILE_FILE,
        PLUGIN_CONFORMANCE_FILE,
    ] {
        let src_file = dir.join(file);
        if src_file.is_file() {
            fs::copy(&src_file, dest.join(file))
                .map_err(|e| format!("cannot copy {}: {e}", src_file.display()))?;
        }
    }
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
        let metadata = fs::symlink_metadata(&src_path)
            .map_err(|e| format!("cannot stat {}: {e}", src_path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if metadata.is_file() {
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
        let manifest = parse_manifest(&text).ok();
        let package = manifest
            .as_ref()
            .map(|manifest| read_package_metadata(&sub, manifest));
        let signature_state = manifest.as_ref().zip(package.as_ref()).map_or_else(
            || "invalid manifest".to_string(),
            |(manifest, package)| {
                package_signature_state(&sub, manifest, package, &sha256_bytes(text.as_bytes()))
                    .render()
            },
        );
        result.push(PluginInfo {
            version: manifest_field(&text, "version"),
            description: manifest_field(&text, "description"),
            capabilities: manifest_provides(&text),
            trust: manifest.as_ref().map_or_else(
                || "Unknown".to_string(),
                |manifest| format!("{:?}", manifest.trust),
            ),
            signature_state,
            conformance_state: package.as_ref().map_or_else(
                || "invalid manifest".to_string(),
                |package| {
                    if package
                        .conformance
                        .as_ref()
                        .is_some_and(|certificate| certificate.passed)
                    {
                        "passed".to_string()
                    } else {
                        "missing".to_string()
                    }
                },
            ),
            has_native: has_native_library(&sub),
            name,
        });
    }
    result
}

// ── Pack ──────────────────────────────────────────────────────
