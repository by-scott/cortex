use std::fmt::Write as _;
use std::fs;
use std::io::Read;
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

mod conformance;
mod pack;
mod signing;

use conformance::{conformance_checks, conformance_state, recommended_risk_profile};
pub use pack::{default_cpx_name, pack};
use signing::{PackageSignatureState, enforce_package_trust, package_signature_state};
pub use signing::{generate_signing_key, sign_directory};

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
    if [
        PLUGIN_MANIFEST_FILE,
        PLUGIN_PACKAGE_FILE,
        PLUGIN_SBOM_FILE,
        PLUGIN_RISK_PROFILE_FILE,
        PLUGIN_CONFORMANCE_FILE,
    ]
    .iter()
    .any(|allowed| normalized == Path::new(allowed))
    {
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
    pub trust: String,
    pub signature_state: String,
    pub conformance_state: String,
    pub has_native: bool,
}

/// Install/conformance review for one plugin package.
#[derive(Debug, Clone)]
pub struct PluginReview {
    pub name: String,
    pub version: String,
    pub trust: String,
    pub requested_capabilities: Vec<String>,
    pub signature_state: String,
    pub conformance_state: String,
    pub recommended_risk_profile: Vec<String>,
    pub warnings: Vec<String>,
    pub checks: Vec<PluginConformanceCheck>,
}

impl PluginReview {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Plugin review: {} v{}", self.name, self.version);
        let _ = writeln!(out, "  Trust: {}", self.trust);
        let _ = writeln!(out, "  Signature: {}", self.signature_state);
        let _ = writeln!(out, "  Conformance: {}", self.conformance_state);
        out.push_str("  Requested capabilities:\n");
        if self.requested_capabilities.is_empty() {
            out.push_str("    - no host capabilities declared\n");
        } else {
            for line in &self.requested_capabilities {
                let _ = writeln!(out, "    - {line}");
            }
        }
        if !self.recommended_risk_profile.is_empty() {
            out.push_str("  Recommended risk profile:\n");
            for line in &self.recommended_risk_profile {
                let _ = writeln!(out, "    {line}");
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("  Warnings:\n");
            for warning in &self.warnings {
                let _ = writeln!(out, "    - {warning}");
            }
        }
        if !self.checks.is_empty() {
            out.push_str("  Checks:\n");
            for check in &self.checks {
                let status = if check.passed { "ok" } else { "fail" };
                if check.message.is_empty() {
                    let _ = writeln!(out, "    - {status}: {}", check.name);
                } else {
                    let _ = writeln!(out, "    - {status}: {} ({})", check.name, check.message);
                }
            }
        }
        out
    }
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

fn extract_cpx_to_dir(cpx_path: &Path, dest: &Path) -> Result<(), String> {
    // Re-open for extraction (tar::Archive is consumed by iteration).
    let file = fs::File::open(cpx_path)
        .map_err(|e| format!("cannot reopen {}: {e}", cpx_path.display()))?;
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
        if !entry.header().entry_type().is_file() {
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
    Ok(())
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
    install_url_with_policy(cortex_home, url, PluginInstallPolicy::default())
}

/// Download and install a `.cpx` archive with an explicit package policy.
///
/// # Errors
/// Returns an error if the temporary directory cannot be created, `curl` fails,
/// the downloaded archive is invalid, or package verification/trust policy
/// rejects the package.
pub fn install_url_with_policy(
    cortex_home: &Path,
    url: &str,
    policy: PluginInstallPolicy,
) -> Result<String, String> {
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

    install_cpx_with_policy(cortex_home, &tmp_path, policy)
}

// ── Install by name (GitHub) ──────────────────────────────────

/// Install a plugin by name, resolving to a GitHub release URL.
///
/// Tries `github.com/by-scott/cortex-plugin-{name}` releases.
/// Supports optional versions: `dev@1.6.4` or
/// `owner/cortex-plugin-dev@v1.6.4`.
///
/// # Errors
/// Returns an error message if the download or installation fails.
pub fn install_name(cortex_home: &Path, name: &str) -> Result<String, String> {
    install_name_with_policy(cortex_home, name, PluginInstallPolicy::default())
}

/// Resolve a GitHub release asset by plugin name and install it with policy.
///
/// # Errors
/// Returns an error if release metadata cannot be fetched, the release has no
/// platform-matching `.cpx` asset, the download fails, or package verification
/// rejects the archive.
pub fn install_name_with_policy(
    cortex_home: &Path,
    name: &str,
    policy: PluginInstallPolicy,
) -> Result<String, String> {
    let (name, version) = name
        .rsplit_once('@')
        .map_or((name, None), |(base, version)| (base, Some(version)));
    let (owner, repo) = if let Some((owner, repo)) = name.split_once('/') {
        (owner.to_string(), repo.to_string())
    } else {
        ("by-scott".to_string(), format!("cortex-plugin-{name}"))
    };
    let url = github_cpx_url(&owner, &repo, version)?;
    install_url_with_policy(cortex_home, &url, policy)
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

    let platform = pack::current_platform()?;
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
