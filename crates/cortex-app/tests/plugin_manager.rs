use cortex_app::plugin_manager::{
    PluginInstallPolicy, UnknownPublisherPolicy, generate_signing_key, install,
    install_with_policy, list, pack, review_directory, sign_directory,
};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const PLUGIN_MANIFEST_FILE: &str = "manifest.toml";
const PLUGIN_PACKAGE_FILE: &str = "package.toml";
const PLUGIN_LIB_DIR: &str = "lib";
const PLUGIN_SKILLS_DIR: &str = "skills";
const PLUGIN_PROMPTS_DIR: &str = "prompts";

fn write_text(path: &Path, text: &str) {
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        panic!("failed to create {}: {err}", parent.display());
    }
    if let Err(err) = fs::write(path, text) {
        panic!("failed to write {}: {err}", path.display());
    }
}

fn build_native_manifest(name: &str) -> String {
    format!(
        "name = \"{name}\"\nversion = \"1.6.3\"\ndescription = \"test plugin\"\ncortex_version = \"1.6.3\"\ntrust = \"trusted_native\"\n\n[capabilities]\nprovides = [\"tools\"]\nsecrets = false\n\n[sandbox]\nlevel = \"trusted_in_process\"\n\n[native]\nlibrary = \"lib/lib{name}.so\"\nisolation = \"trusted_in_process\"\nabi_version = 1\n"
    )
}

fn plugin_install_home() -> (tempfile::TempDir, PathBuf) {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("failed to create tempdir: {err}"),
    };
    let cortex_home = temp.path().join("cortex-home");
    if let Err(err) = fs::create_dir_all(&cortex_home) {
        panic!("failed to create {}: {err}", cortex_home.display());
    }
    (temp, cortex_home)
}

#[test]
fn directory_install_filters_files_and_extracts_built_library() {
    let (_temp, cortex_home) = plugin_install_home();
    let source_dir = cortex_home.join("source-plugin");
    let manifest = build_native_manifest("sample");
    write_text(&source_dir.join(PLUGIN_MANIFEST_FILE), &manifest);
    write_text(
        &source_dir.join(PLUGIN_SKILLS_DIR).join("tool.md"),
        "skill body",
    );
    write_text(
        &source_dir.join(PLUGIN_PROMPTS_DIR).join("system.md"),
        "prompt body",
    );
    write_text(
        &source_dir.join(PLUGIN_SKILLS_DIR).join(".hidden.md"),
        "ignore",
    );
    write_text(
        &source_dir.join(PLUGIN_PROMPTS_DIR).join("draft.bak"),
        "ignore",
    );
    write_text(&source_dir.join("README.md"), "ignore");
    write_text(&source_dir.join(".git").join("config"), "ignore");
    write_text(
        &source_dir.join("target/release").join("libsample.so"),
        "native release bytes",
    );
    write_text(
        &source_dir.join("target/debug").join("libsample.so"),
        "native debug bytes",
    );

    let installed = match install(&cortex_home, &source_dir.to_string_lossy()) {
        Ok(value) => value,
        Err(err) => panic!("directory install should succeed: {err}"),
    };
    assert_eq!(installed, "sample");

    let plugin_root = cortex_home.join("plugins").join("sample");
    assert!(plugin_root.join(PLUGIN_MANIFEST_FILE).is_file());
    assert!(
        plugin_root
            .join(PLUGIN_SKILLS_DIR)
            .join("tool.md")
            .is_file()
    );
    assert!(
        plugin_root
            .join(PLUGIN_PROMPTS_DIR)
            .join("system.md")
            .is_file()
    );
    assert!(
        plugin_root
            .join(PLUGIN_LIB_DIR)
            .join("libsample.so")
            .is_file()
    );
    assert!(!plugin_root.join("README.md").exists());
    assert!(!plugin_root.join(".git").exists());
    assert!(!plugin_root.join("target").exists());
    assert!(
        !plugin_root
            .join(PLUGIN_SKILLS_DIR)
            .join(".hidden.md")
            .exists()
    );
    assert!(
        !plugin_root
            .join(PLUGIN_PROMPTS_DIR)
            .join("draft.bak")
            .exists()
    );

    let native_bytes =
        match fs::read_to_string(plugin_root.join(PLUGIN_LIB_DIR).join("libsample.so")) {
            Ok(value) => value,
            Err(err) => panic!("failed to read installed library: {err}"),
        };
    assert_eq!(native_bytes, "native release bytes");
}

#[test]
fn cpx_install_filters_files_and_listing_ignores_backup_dirs() {
    let (_temp, cortex_home) = plugin_install_home();
    let archive_path = cortex_home.join("sample-plugin.cpx");
    let archive_file = match fs::File::create(&archive_path) {
        Ok(value) => value,
        Err(err) => panic!("failed to create {}: {err}", archive_path.display()),
    };
    let encoder = flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);

    append_cpx_file(
        &mut tar,
        PLUGIN_MANIFEST_FILE,
        &build_native_manifest("sample"),
    );
    append_cpx_file(&mut tar, "skills/tool.md", "skill body");
    append_cpx_file(&mut tar, "prompts/system.md", "prompt body");
    append_cpx_file(&mut tar, "lib/libsample.so", "native bytes");
    append_cpx_file(&mut tar, "README.md", "ignore");
    append_cpx_file(&mut tar, "skills/.hidden.md", "ignore");
    append_cpx_file(&mut tar, "prompts/draft.bak", "ignore");
    append_cpx_file(&mut tar, "target/release/libsample.so", "ignore");

    let encoder = match tar.into_inner() {
        Ok(value) => value,
        Err(err) => panic!("failed to finalize tar: {err}"),
    };
    if let Err(err) = encoder.finish() {
        panic!("failed to finalize gzip: {err}");
    }

    let installed = match install(&cortex_home, &archive_path.to_string_lossy()) {
        Ok(value) => value,
        Err(err) => panic!("archive install should succeed: {err}"),
    };
    assert_eq!(installed, "sample");

    let plugin_root = cortex_home.join("plugins").join("sample");
    assert!(plugin_root.join(PLUGIN_MANIFEST_FILE).is_file());
    assert!(
        plugin_root
            .join(PLUGIN_SKILLS_DIR)
            .join("tool.md")
            .is_file()
    );
    assert!(
        plugin_root
            .join(PLUGIN_PROMPTS_DIR)
            .join("system.md")
            .is_file()
    );
    assert!(
        plugin_root
            .join(PLUGIN_LIB_DIR)
            .join("libsample.so")
            .is_file()
    );
    assert!(!plugin_root.join("README.md").exists());
    assert!(!plugin_root.join("target").exists());
    assert!(
        !plugin_root
            .join(PLUGIN_SKILLS_DIR)
            .join(".hidden.md")
            .exists()
    );
    assert!(
        !plugin_root
            .join(PLUGIN_PROMPTS_DIR)
            .join("draft.bak")
            .exists()
    );
    let backup_dir = cortex_home.join("plugins").join("sample.bak");
    if let Err(err) = fs::create_dir_all(&backup_dir) {
        panic!("failed to create {}: {err}", backup_dir.display());
    }
    write_text(
        &backup_dir.join(PLUGIN_MANIFEST_FILE),
        &build_native_manifest("sample"),
    );

    let plugins = list(&cortex_home);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "sample");
}

#[test]
fn signed_cpx_installs_under_release_policy_and_records_publisher_trust() {
    let (_temp, cortex_home) = plugin_install_home();
    let source_dir = cortex_home.join("signed-plugin");
    let key_path = cortex_home.join("keys").join("publisher.key");
    let archive_path = cortex_home.join("signed-plugin.cpx");

    write_text(
        &source_dir.join(PLUGIN_MANIFEST_FILE),
        &build_native_manifest("signed"),
    );
    write_text(
        &source_dir.join(PLUGIN_LIB_DIR).join("libsigned.so"),
        "native bytes",
    );
    if let Err(err) = generate_signing_key(&key_path) {
        panic!("key generation should succeed: {err}");
    }
    if let Err(err) = sign_directory(&source_dir, &key_path, Some("example.publisher")) {
        panic!("signing should succeed: {err}");
    }

    let review = match review_directory(&source_dir) {
        Ok(value) => value,
        Err(err) => panic!("review should succeed: {err}"),
    };
    assert!(review.signature_state.contains("verified ed25519"));
    assert!(source_dir.join(PLUGIN_PACKAGE_FILE).is_file());

    if let Err(err) = pack(&source_dir, &archive_path) {
        panic!("pack should succeed: {err}");
    }
    let policy = PluginInstallPolicy::release_default(UnknownPublisherPolicy::TrustVerified);
    let installed = match install_with_policy(&cortex_home, &archive_path.to_string_lossy(), policy)
    {
        Ok(value) => value,
        Err(err) => panic!("signed archive install should succeed: {err}"),
    };

    assert_eq!(installed, "signed");
    assert!(cortex_home.join("plugin-trust.toml").is_file());
    let plugins = list(&cortex_home);
    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].signature_state.contains("verified ed25519"));
}

#[test]
fn signed_cpx_can_use_auto_resolved_release_library() {
    let (_temp, cortex_home) = plugin_install_home();
    let source_dir = cortex_home.join("auto-lib-plugin");
    let key_path = cortex_home.join("keys").join("publisher.key");
    let archive_path = cortex_home.join("auto-lib-plugin.cpx");

    write_text(
        &source_dir.join(PLUGIN_MANIFEST_FILE),
        &build_native_manifest("autolib"),
    );
    write_text(
        &source_dir.join("target/release").join("libautolib.so"),
        "native release bytes",
    );
    if let Err(err) = generate_signing_key(&key_path) {
        panic!("key generation should succeed: {err}");
    }
    if let Err(err) = sign_directory(&source_dir, &key_path, Some("example.publisher")) {
        panic!("signing should succeed: {err}");
    }

    let review = match review_directory(&source_dir) {
        Ok(value) => value,
        Err(err) => panic!("review should succeed: {err}"),
    };
    assert!(review.signature_state.contains("verified ed25519"));

    if let Err(err) = pack(&source_dir, &archive_path) {
        panic!("pack should succeed: {err}");
    }
    let policy = PluginInstallPolicy::release_default(UnknownPublisherPolicy::TrustVerified);
    let installed = match install_with_policy(&cortex_home, &archive_path.to_string_lossy(), policy)
    {
        Ok(value) => value,
        Err(err) => panic!("auto-resolved signed archive install should succeed: {err}"),
    };

    assert_eq!(installed, "autolib");
    let plugins = list(&cortex_home);
    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].signature_state.contains("verified ed25519"));
}

#[test]
fn unsigned_cpx_is_rejected_under_release_policy() {
    let (_temp, cortex_home) = plugin_install_home();
    let archive_path = cortex_home.join("unsigned-plugin.cpx");
    let archive_file = match fs::File::create(&archive_path) {
        Ok(value) => value,
        Err(err) => panic!("failed to create {}: {err}", archive_path.display()),
    };
    let encoder = flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    append_cpx_file(
        &mut tar,
        PLUGIN_MANIFEST_FILE,
        &build_native_manifest("unsigned"),
    );
    append_cpx_file(&mut tar, "lib/libunsigned.so", "native bytes");
    let encoder = match tar.into_inner() {
        Ok(value) => value,
        Err(err) => panic!("failed to finalize tar: {err}"),
    };
    if let Err(err) = encoder.finish() {
        panic!("failed to finalize gzip: {err}");
    }

    let policy = PluginInstallPolicy::release_default(UnknownPublisherPolicy::TrustVerified);
    let err = match install_with_policy(&cortex_home, &archive_path.to_string_lossy(), policy) {
        Ok(value) => panic!("unsigned archive should fail, installed {value}"),
        Err(err) => err,
    };

    assert!(err.contains("unsigned"));
    assert!(!cortex_home.join("plugins").join("unsigned").exists());
}

#[test]
fn signed_cpx_with_unknown_publisher_is_rejected_when_policy_rejects() {
    let (_temp, cortex_home) = plugin_install_home();
    let source_dir = cortex_home.join("reject-plugin");
    let key_path = cortex_home.join("keys").join("publisher.key");
    let archive_path = cortex_home.join("reject-plugin.cpx");

    write_text(
        &source_dir.join(PLUGIN_MANIFEST_FILE),
        &build_native_manifest("rejectme"),
    );
    write_text(
        &source_dir.join(PLUGIN_LIB_DIR).join("librejectme.so"),
        "native bytes",
    );
    if let Err(err) = generate_signing_key(&key_path) {
        panic!("key generation should succeed: {err}");
    }
    if let Err(err) = sign_directory(&source_dir, &key_path, Some("new.publisher")) {
        panic!("signing should succeed: {err}");
    }
    if let Err(err) = pack(&source_dir, &archive_path) {
        panic!("pack should succeed: {err}");
    }

    let policy = PluginInstallPolicy::release_default(UnknownPublisherPolicy::Reject);
    let err = match install_with_policy(&cortex_home, &archive_path.to_string_lossy(), policy) {
        Ok(value) => panic!("untrusted publisher should fail, installed {value}"),
        Err(err) => err,
    };

    assert!(err.contains("not trusted"));
    assert!(!cortex_home.join("plugins").join("rejectme").exists());
}

#[test]
fn signed_cpx_rejects_tampered_payload() {
    let (_temp, cortex_home) = plugin_install_home();
    let source_dir = cortex_home.join("tampered-plugin");
    let key_path = cortex_home.join("keys").join("publisher.key");
    let archive_path = cortex_home.join("tampered-plugin.cpx");

    write_text(
        &source_dir.join(PLUGIN_MANIFEST_FILE),
        &build_native_manifest("tampered"),
    );
    write_text(
        &source_dir.join(PLUGIN_LIB_DIR).join("libtampered.so"),
        "native bytes",
    );
    if let Err(err) = generate_signing_key(&key_path) {
        panic!("key generation should succeed: {err}");
    }
    if let Err(err) = sign_directory(&source_dir, &key_path, Some("example.publisher")) {
        panic!("signing should succeed: {err}");
    }
    write_text(
        &source_dir.join(PLUGIN_LIB_DIR).join("libtampered.so"),
        "tampered native bytes",
    );
    if let Err(err) = pack(&source_dir, &archive_path) {
        panic!("pack should succeed: {err}");
    }

    let policy = PluginInstallPolicy::release_default(UnknownPublisherPolicy::TrustVerified);
    let err = match install_with_policy(&cortex_home, &archive_path.to_string_lossy(), policy) {
        Ok(value) => panic!("tampered archive should fail, installed {value}"),
        Err(err) => err,
    };

    assert!(err.contains("signature invalid") || err.contains("mismatch"));
    assert!(!cortex_home.join("plugins").join("tampered").exists());
}

fn append_cpx_file(
    tar: &mut tar::Builder<flate2::write::GzEncoder<fs::File>>,
    path: &str,
    contents: &str,
) {
    let mut header = tar::Header::new_gnu();
    let bytes = contents.as_bytes();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    if let Err(err) = tar.append_data(&mut header, path, Cursor::new(bytes)) {
        panic!("failed to add {path}: {err}");
    }
}
