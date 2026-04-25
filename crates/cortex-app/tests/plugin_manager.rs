use cortex_app::plugin_manager::{install, list};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const PLUGIN_MANIFEST_FILE: &str = "manifest.toml";
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
        "name = \"{name}\"\nversion = \"1.4.0\"\ndescription = \"test plugin\"\ncortex_version = \"1.4.0\"\n\n[capabilities]\nprovides = [\"tools\"]\n\n[native]\nlibrary = \"lib/lib{name}.so\"\n"
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
