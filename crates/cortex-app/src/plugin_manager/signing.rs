use std::fs;
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use cortex_types::plugin::{PluginManifest, PluginPackageMetadata};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::{
    PLUGIN_CONFORMANCE_FILE, PLUGIN_LIB_DIR, PLUGIN_MANIFEST_FILE, PLUGIN_PACKAGE_FILE,
    PLUGIN_PROMPTS_DIR, PLUGIN_RISK_PROFILE_FILE, PLUGIN_SBOM_FILE, PLUGIN_SKILLS_DIR,
    PLUGIN_TRUST_FILE, PluginInstallPolicy, UnknownPublisherPolicy, pack, read_manifest_from_dir,
    read_package_metadata, sha256_bytes, sha256_file, should_include_plugin_entry_name,
};

const SIGNATURE_ALGORITHM_ED25519: &str = "ed25519";
const SIGNATURE_PAYLOAD_VERSION: &str = "cortex-plugin-signature-v1";

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
pub(super) enum PackageSignatureState {
    Unsigned(String),
    Verified {
        publisher_id: String,
        fingerprint_sha256: String,
        public_key: String,
    },
    Invalid(String),
}

impl PackageSignatureState {
    pub(super) fn render(&self) -> String {
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

    pub(super) const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }
}

fn trust_store_path(cortex_home: &Path) -> PathBuf {
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
    cortex_kernel::atomic_write_text(&path, text)
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
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

pub(super) fn enforce_package_trust(
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

fn resolved_package_native_artifact(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    if dir.join(PLUGIN_LIB_DIR).is_dir() {
        return None;
    }
    pack::resolve_native_library(dir)
}

fn signed_package_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for file in [
        PLUGIN_MANIFEST_FILE,
        PLUGIN_SBOM_FILE,
        PLUGIN_RISK_PROFILE_FILE,
        PLUGIN_CONFORMANCE_FILE,
    ] {
        let path = dir.join(file);
        if path.is_file() {
            files.push(PathBuf::from(file));
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
    files: &mut Vec<PathBuf>,
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
    cortex_kernel::atomic_write(private_key_path, &signing_key.to_bytes())
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
    cortex_kernel::atomic_write_text(&dir.join(PLUGIN_PACKAGE_FILE), text)
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

fn first_native_artifact(dir: &Path, manifest: &PluginManifest) -> Option<PathBuf> {
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

fn first_library_file(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .find(|path| super::is_native_library_path(path))
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

pub(super) fn package_signature_state(
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
