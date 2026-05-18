use std::fmt::Write as _;
use std::fs;
use std::io::{IsTerminal, Read, Write as _};
use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use cortex_types::plugin::{PluginConformanceCheck, PluginManifest, PluginPackageMetadata};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
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
const SIGNATURE_ALGORITHM_ED25519: &str = "ed25519";
const SIGNATURE_PAYLOAD_VERSION: &str = "cortex-plugin-signature-v1";

mod pack;

pub use pack::{default_cpx_name, pack};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PluginTrustStore {
    #[serde(default)]
    publishers: Vec<TrustedPluginPublisher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustedPluginPublisher {
    publisher_id: String,
    fingerprint_sha256: String,
    public_key: String,
    trusted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageSignatureState {
    Unsigned(String),
    Verified {
        publisher_id: String,
        fingerprint_sha256: String,
        public_key: String,
    },
    Invalid(String),
}

impl PackageSignatureState {
    fn render(&self) -> String {
        match self {
            Self::Unsigned(reason) => format!("unsigned ({reason})"),
            Self::Verified {
                publisher_id,
                fingerprint_sha256,
                ..
            } => {
                format!(
                    "verified ed25519; publisher={publisher_id}; fingerprint={fingerprint_sha256}"
                )
            }
            Self::Invalid(reason) => format!("invalid ({reason})"),
        }
    }

    const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
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

fn trust_store_path(cortex_home: &Path) -> std::path::PathBuf {
    cortex_home.join(PLUGIN_TRUST_FILE)
}

fn load_trust_store(cortex_home: &Path) -> PluginTrustStore {
    fs::read_to_string(trust_store_path(cortex_home))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_trust_store(cortex_home: &Path, store: &PluginTrustStore) -> Result<(), String> {
    let path = trust_store_path(cortex_home);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    let text =
        toml::to_string_pretty(store).map_err(|err| format!("cannot encode trust store: {err}"))?;
    fs::write(&path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn is_publisher_trusted(
    store: &PluginTrustStore,
    publisher_id: &str,
    fingerprint_sha256: &str,
) -> bool {
    store.publishers.iter().any(|publisher| {
        publisher.publisher_id == publisher_id
            && publisher
                .fingerprint_sha256
                .eq_ignore_ascii_case(fingerprint_sha256)
    })
}

fn trust_publisher(
    cortex_home: &Path,
    publisher_id: &str,
    fingerprint_sha256: &str,
    public_key: &str,
) -> Result<(), String> {
    let mut store = load_trust_store(cortex_home);
    if !is_publisher_trusted(&store, publisher_id, fingerprint_sha256) {
        store.publishers.push(TrustedPluginPublisher {
            publisher_id: publisher_id.to_string(),
            fingerprint_sha256: fingerprint_sha256.to_string(),
            public_key: public_key.to_string(),
            trusted_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    save_trust_store(cortex_home, &store)
}

fn prompt_trust_publisher(publisher_id: &str, fingerprint_sha256: &str) -> Result<bool, String> {
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    eprintln!("Plugin publisher is not trusted on this machine.");
    eprintln!("  publisher: {publisher_id}");
    eprintln!("  key sha256: {fingerprint_sha256}");
    eprint!("Trust this publisher key and install this verified package? [y/N] ");
    std::io::stderr()
        .flush()
        .map_err(|err| format!("cannot flush prompt: {err}"))?;
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|err| format!("cannot read confirmation: {err}"))?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn enforce_package_trust(
    cortex_home: &Path,
    signature_state: &PackageSignatureState,
    policy: PluginInstallPolicy,
) -> Result<(), String> {
    match signature_state {
        PackageSignatureState::Verified {
            publisher_id,
            fingerprint_sha256,
            public_key,
        } => {
            let store = load_trust_store(cortex_home);
            if is_publisher_trusted(&store, publisher_id, fingerprint_sha256) {
                return Ok(());
            }
            match policy.unknown_publisher {
                UnknownPublisherPolicy::Reject => Err(format!(
                    "plugin publisher '{publisher_id}' is verified but not trusted; run in an interactive terminal or pass --yes after reviewing fingerprint {fingerprint_sha256}"
                )),
                UnknownPublisherPolicy::Prompt => {
                    if prompt_trust_publisher(publisher_id, fingerprint_sha256)? {
                        trust_publisher(cortex_home, publisher_id, fingerprint_sha256, public_key)
                    } else {
                        Err(format!(
                            "plugin publisher '{publisher_id}' was not trusted; installation cancelled"
                        ))
                    }
                }
                UnknownPublisherPolicy::TrustVerified => {
                    trust_publisher(cortex_home, publisher_id, fingerprint_sha256, public_key)
                }
            }
        }
        PackageSignatureState::Unsigned(reason) => {
            if policy.require_packaged_signature {
                Err(format!("plugin package is unsigned: {reason}"))
            } else {
                Ok(())
            }
        }
        PackageSignatureState::Invalid(reason) => {
            Err(format!("plugin package signature invalid: {reason}"))
        }
    }
}

fn parse_base64_32(value: &str, label: &str) -> Result<[u8; 32], String> {
    let bytes = BASE64_STANDARD
        .decode(value.trim())
        .map_err(|err| format!("{label} is not valid base64: {err}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("{label} must be 32 bytes, got {}", bytes.len()))
}

fn parse_base64_64(value: &str, label: &str) -> Result<[u8; 64], String> {
    let bytes = BASE64_STANDARD
        .decode(value.trim())
        .map_err(|err| format!("{label} is not valid base64: {err}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("{label} must be 64 bytes, got {}", bytes.len()))
}

fn public_key_fingerprint(public_key: &str) -> Result<String, String> {
    let key = parse_base64_32(public_key, "public key")?;
    Ok(sha256_bytes(&key))
}

fn signature_payload(dir: &Path, package: &PluginPackageMetadata) -> Result<String, String> {
    let mut lines = vec![
        SIGNATURE_PAYLOAD_VERSION.to_string(),
        format!("publisher_id={}", package.publisher_id),
        format!("algorithm={}", package.signature_algorithm),
        format!("public_key={}", package.public_key),
        format!("manifest_sha256={}", package.manifest_sha256),
        format!("binary_sha256={}", package.binary_sha256),
        format!("sbom={}", package.sbom),
        format!("risk_profile={}", package.risk_profile),
    ];
    if let Some(certificate) = &package.conformance {
        lines.push(format!("conformance.suite={}", certificate.suite));
        lines.push(format!("conformance.passed={}", certificate.passed));
        lines.push(format!("conformance.checked_at={}", certificate.checked_at));
        for check in &certificate.checks {
            lines.push(format!(
                "conformance.check\t{}\t{}\t{}",
                check.name, check.passed, check.message
            ));
        }
    }
    let mut file_hashes = Vec::new();
    for rel in signed_package_files(dir)? {
        let hash = sha256_file(&dir.join(&rel))?;
        file_hashes.push((rel, hash));
    }
    if let Some((archive_path, disk_path)) = resolved_package_native_artifact(dir)
        && !file_hashes
            .iter()
            .any(|(existing, _)| existing == &archive_path)
    {
        file_hashes.push((archive_path, sha256_file(&disk_path)?));
    }
    file_hashes.sort_by(|(left, _), (right, _)| left.cmp(right));
    for (rel, hash) in file_hashes {
        lines.push(format!("file\t{}\t{}", rel.to_string_lossy(), hash));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn resolved_package_native_artifact(
    dir: &Path,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    if dir.join(PLUGIN_LIB_DIR).is_dir() {
        return None;
    }
    pack::resolve_native_library(dir)
}

fn signed_package_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    for file in [
        PLUGIN_MANIFEST_FILE,
        PLUGIN_SBOM_FILE,
        PLUGIN_RISK_PROFILE_FILE,
        PLUGIN_CONFORMANCE_FILE,
    ] {
        let path = dir.join(file);
        if path.is_file() {
            files.push(std::path::PathBuf::from(file));
        }
    }
    for subdir in [PLUGIN_LIB_DIR, PLUGIN_SKILLS_DIR, PLUGIN_PROMPTS_DIR] {
        collect_signed_files(dir, Path::new(subdir), &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_signed_files(
    root: &Path,
    rel_dir: &Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    let dir = root.join(rel_dir);
    if !dir.is_dir() {
        return Ok(());
    }
    let entries =
        fs::read_dir(&dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("directory entry error: {err}"))?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        if !should_include_plugin_entry_name(name_text) {
            continue;
        }
        let rel_path = rel_dir.join(name);
        if entry.path().is_dir() {
            collect_signed_files(root, &rel_path, files)?;
        } else {
            files.push(rel_path);
        }
    }
    Ok(())
}

fn verify_package_signature(dir: &Path, package: &PluginPackageMetadata) -> PackageSignatureState {
    match verify_package_signature_inner(dir, package) {
        Ok(state) => state,
        Err(reason) => PackageSignatureState::Invalid(reason),
    }
}

fn verify_package_signature_inner(
    dir: &Path,
    package: &PluginPackageMetadata,
) -> Result<PackageSignatureState, String> {
    if package.signature.trim().is_empty() {
        return Ok(PackageSignatureState::Unsigned(
            "signature missing".to_string(),
        ));
    }
    if package.publisher_id.trim().is_empty() {
        return Err("publisher_id missing".to_string());
    }
    if package.signature_algorithm != SIGNATURE_ALGORITHM_ED25519 {
        return Err(format!(
            "unsupported signature_algorithm '{}'",
            package.signature_algorithm
        ));
    }
    if package.public_key.trim().is_empty() {
        return Err("public_key missing".to_string());
    }
    let public_key_bytes = parse_base64_32(&package.public_key, "public key")?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|err| format!("invalid ed25519 public key: {err}"))?;
    let signature_bytes = parse_base64_64(&package.signature, "signature")?;
    let signature = Signature::from_bytes(&signature_bytes);
    let payload = signature_payload(dir, package)?;
    verifying_key
        .verify(payload.as_bytes(), &signature)
        .map_err(|err| format!("signature verification failed: {err}"))?;
    Ok(PackageSignatureState::Verified {
        publisher_id: package.publisher_id.clone(),
        fingerprint_sha256: public_key_fingerprint(&package.public_key)?,
        public_key: package.public_key.clone(),
    })
}

/// Create a raw Ed25519 plugin signing key.
///
/// # Errors
/// Returns an error if the parent directory cannot be created, the key already
/// exists, key material cannot be written, permissions cannot be tightened, or
/// the generated public key cannot be fingerprinted.
pub fn generate_signing_key(private_key_path: &Path) -> Result<String, String> {
    if let Some(parent) = private_key_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    if private_key_path.exists() {
        return Err(format!(
            "signing key already exists: {}",
            private_key_path.display()
        ));
    }
    let mut rng = rand_core::OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    fs::write(private_key_path, signing_key.to_bytes())
        .map_err(|err| format!("cannot write {}: {err}", private_key_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(private_key_path, fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("cannot chmod {}: {err}", private_key_path.display()))?;
    }
    let public_key = BASE64_STANDARD.encode(signing_key.verifying_key().to_bytes());
    let fingerprint = public_key_fingerprint(&public_key)?;
    Ok(format!(
        "Created signing key: {}\nPublic key: {public_key}\nPublic key SHA-256: {fingerprint}",
        private_key_path.display()
    ))
}

/// Sign a plugin directory and write its `package.toml` metadata.
///
/// # Errors
/// Returns an error if the manifest or signing key cannot be read, the
/// publisher id is missing, package metadata cannot be encoded, or any signed
/// artifact cannot be hashed.
pub fn sign_directory(
    dir: &Path,
    key_path: &Path,
    publisher_id: Option<&str>,
) -> Result<PluginPackageMetadata, String> {
    let package = sign_directory_with_key(dir, key_path, publisher_id)?;
    let text = toml::to_string_pretty(&package)
        .map_err(|err| format!("cannot encode package.toml: {err}"))?;
    fs::write(dir.join(PLUGIN_PACKAGE_FILE), text)
        .map_err(|err| format!("cannot write package.toml: {err}"))?;
    Ok(package)
}

fn sign_directory_with_key(
    dir: &Path,
    key_path: &Path,
    publisher_id: Option<&str>,
) -> Result<PluginPackageMetadata, String> {
    let (manifest_text, manifest) = read_manifest_from_dir(dir)?;
    let mut package = read_package_metadata(dir, &manifest);
    package.publisher_id =
        publisher_id.map_or_else(|| package.publisher_id.clone(), str::to_string);
    package.publisher_id = if package.publisher_id.trim().is_empty() {
        manifest.author.clone()
    } else {
        package.publisher_id
    };
    if package.publisher_id.trim().is_empty() {
        return Err("publisher_id is required before signing".into());
    }
    package.signature_algorithm = SIGNATURE_ALGORITHM_ED25519.to_string();
    let key_bytes = fs::read(key_path)
        .map_err(|err| format!("cannot read signing key {}: {err}", key_path.display()))?;
    let signing_key = SigningKey::from_bytes(&key_bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!(
            "ed25519 signing key must be 32 raw bytes, got {}",
            bytes.len()
        )
    })?);
    let verifying_key = signing_key.verifying_key();
    package.public_key = BASE64_STANDARD.encode(verifying_key.to_bytes());
    package.manifest_sha256 = sha256_bytes(manifest_text.as_bytes());
    package.binary_sha256 = first_native_artifact(dir, &manifest)
        .map(|path| sha256_file(&path))
        .transpose()?
        .unwrap_or_default();
    package.signature.clear();
    let payload = signature_payload(dir, &package)?;
    package.signature = BASE64_STANDARD.encode(signing_key.sign(payload.as_bytes()).to_bytes());
    Ok(package)
}

fn first_native_artifact(dir: &Path, manifest: &PluginManifest) -> Option<std::path::PathBuf> {
    manifest
        .native
        .as_ref()
        .and_then(|native| (!native.library.is_empty()).then(|| dir.join(&native.library)))
        .filter(|path| path.is_file())
        .or_else(|| {
            let lib_dir = dir.join(PLUGIN_LIB_DIR);
            if lib_dir.is_dir() {
                first_library_file(&lib_dir)
            } else {
                first_library_file(dir)
            }
        })
        .or_else(|| pack::resolve_native_library(dir).map(|(_, disk_path)| disk_path))
}

fn first_library_file(dir: &Path) -> Option<std::path::PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .find(|path| is_native_library_path(path))
}

fn is_native_library_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("so") || extension.eq_ignore_ascii_case("dylib")
    })
}

fn verify_hash(expected: &str, actual: &str, label: &str) -> String {
    if expected.is_empty() {
        format!("{label} hash missing")
    } else if expected == actual {
        format!("{label} hash verified")
    } else {
        format!("{label} hash mismatch")
    }
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

fn package_signature_state(
    dir: &Path,
    manifest: &PluginManifest,
    package: &PluginPackageMetadata,
    manifest_hash: &str,
) -> PackageSignatureState {
    let manifest_state = verify_hash(&package.manifest_sha256, manifest_hash, "manifest");
    let binary_state = first_native_artifact(dir, manifest).map_or_else(
        || "binary hash not applicable".to_string(),
        |path| match sha256_file(&path) {
            Ok(hash) => verify_hash(&package.binary_sha256, &hash, "binary"),
            Err(err) => err,
        },
    );
    let hash_summary = format!("{manifest_state}; {binary_state}");
    let has_signature = !package.signature.trim().is_empty();
    if manifest_state.contains("mismatch")
        || binary_state.contains("mismatch")
        || (has_signature
            && (manifest_state.contains("missing") || binary_state.contains("missing")))
    {
        return PackageSignatureState::Invalid(hash_summary);
    }
    match verify_package_signature(dir, package) {
        PackageSignatureState::Unsigned(reason) => {
            PackageSignatureState::Unsigned(format!("{hash_summary}; {reason}"))
        }
        PackageSignatureState::Invalid(reason) => {
            PackageSignatureState::Invalid(format!("{hash_summary}; {reason}"))
        }
        verified @ PackageSignatureState::Verified { .. } => verified,
    }
}

fn conformance_state(package: &PluginPackageMetadata, local_passed: bool) -> String {
    let package_state =
        package
            .conformance
            .as_ref()
            .map_or("no package certificate", |certificate| {
                if certificate.passed {
                    "package certificate passed"
                } else {
                    "package certificate failed"
                }
            });
    if local_passed {
        format!("local checks passed; {package_state}")
    } else {
        format!("local checks failed; {package_state}")
    }
}

fn recommended_risk_profile(manifest: &PluginManifest) -> Vec<String> {
    manifest
        .capabilities
        .declared_effects()
        .into_iter()
        .map(|effect| {
            format!(
                "[risk.tools.{}] effect = {:?} target = \"{}\" floor = {:?}",
                manifest.name,
                effect.kind,
                effect.target,
                effect.risk_floor()
            )
        })
        .collect()
}

fn conformance_checks(dir: &Path, manifest: &PluginManifest) -> Vec<PluginConformanceCheck> {
    let mut checks = vec![
        check("manifest identity", !manifest.name.trim().is_empty(), ""),
        check(
            "manifest version",
            !manifest.version.trim().is_empty(),
            "version is required",
        ),
        check(
            "cortex version target",
            cortex_types::plugin::check_plugin_version(manifest, env!("CARGO_PKG_VERSION"))
                .accepted,
            "cortex_version must be less than or equal to this Cortex release",
        ),
        check(
            "capability declaration",
            !manifest.capabilities.provides.is_empty(),
            "capabilities.provides must not be empty",
        ),
    ];

    if let Some(native) = &manifest.native {
        checks.push(check(
            "native isolation boundary",
            native.isolation == cortex_types::plugin::NativePluginIsolation::Process
                || native.abi_version == Some(cortex_sdk::NATIVE_ABI_VERSION),
            "trusted native plugins must declare the current ABI",
        ));
        for tool in &native.tools {
            checks.extend(process_tool_checks(dir, manifest, tool));
        }
    }
    checks
}

fn check(name: &str, passed: bool, message: &str) -> PluginConformanceCheck {
    PluginConformanceCheck {
        name: name.to_string(),
        passed,
        message: if passed {
            String::new()
        } else {
            message.to_string()
        },
    }
}

fn process_tool_checks(
    dir: &Path,
    manifest: &PluginManifest,
    tool: &cortex_types::plugin::ProcessToolConfig,
) -> Vec<PluginConformanceCheck> {
    let command = resolve_plugin_tool_path(dir, &tool.command);
    let command_bound = tool.allow_host_paths || path_stays_under(dir, &command);
    let mut checks = vec![
        check(
            &format!("tool {} command path", tool.name),
            command_bound,
            "command escapes plugin directory",
        ),
        check(
            &format!("tool {} command exists", tool.name),
            command.is_file(),
            "command file is missing",
        ),
        check(
            &format!("tool {} output limit", tool.name),
            tool.max_output_bytes.unwrap_or(1) > 0,
            "max_output_bytes must be positive",
        ),
        check(
            &format!("tool {} timeout", tool.name),
            tool.timeout_secs.unwrap_or(1) > 0,
            "timeout_secs must be positive when set",
        ),
    ];
    if let Some(working_dir) = &tool.working_dir {
        let working_dir = resolve_plugin_tool_path(dir, working_dir);
        checks.push(check(
            &format!("tool {} working_dir path", tool.name),
            tool.allow_host_paths || path_stays_under(dir, &working_dir),
            "working_dir escapes plugin directory",
        ));
        checks.push(check(
            &format!("tool {} working_dir exists", tool.name),
            working_dir.is_dir(),
            "working_dir is missing",
        ));
    }
    if !manifest.capabilities.secrets {
        checks.push(check(
            &format!("tool {} env allowlist", tool.name),
            !tool
                .inherit_env
                .iter()
                .any(|name| looks_like_secret_env_name(name)),
            "secret-like inherited env requires capabilities.secrets = true",
        ));
    }
    checks
}

fn resolve_plugin_tool_path(dir: &Path, value: &str) -> std::path::PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

fn path_stays_under(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(candidate) = candidate.canonicalize() else {
        return false;
    };
    candidate.starts_with(root)
}

fn looks_like_secret_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|needle| upper.contains(needle))
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
