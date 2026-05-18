use std::fs;
use std::path::Path;

use super::{
    PLUGIN_CONFORMANCE_FILE, PLUGIN_LIB_DIR, PLUGIN_MANIFEST_FILE, PLUGIN_PACKAGE_FILE,
    PLUGIN_PROMPTS_DIR, PLUGIN_RISK_PROFILE_FILE, PLUGIN_SBOM_FILE, PLUGIN_SKILLS_DIR,
    manifest_field,
};

/// Return the conventional `.cpx` archive name for a plugin directory.
///
/// The name follows release-asset convention:
/// `{directory}-v{version}-{platform}.cpx`.
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

pub(super) fn current_platform() -> Result<String, String> {
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

/// Create a `.cpx` archive from a plugin directory.
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

    for file in [
        PLUGIN_MANIFEST_FILE,
        PLUGIN_PACKAGE_FILE,
        PLUGIN_SBOM_FILE,
        PLUGIN_RISK_PROFILE_FILE,
        PLUGIN_CONFORMANCE_FILE,
    ] {
        let path = source_dir.join(file);
        if path.is_file() {
            tar.append_path_with_name(&path, file)
                .map_err(|e| format!("cannot add {file}: {e}"))?;
        }
    }

    let lib_dir = source_dir.join(PLUGIN_LIB_DIR);
    if lib_dir.is_dir() {
        tar.append_dir_all(PLUGIN_LIB_DIR, &lib_dir)
            .map_err(|e| format!("cannot add {PLUGIN_LIB_DIR}/: {e}"))?;
    } else if let Some(lib_archive_path) = resolve_native_library(source_dir) {
        let (archive_path, disk_path) = lib_archive_path;
        tar.append_path_with_name(&disk_path, &archive_path)
            .map_err(|e| format!("cannot add {}: {e}", archive_path.display()))?;
    }

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

pub(super) fn resolve_native_library(
    source_dir: &Path,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let manifest_text = fs::read_to_string(source_dir.join(PLUGIN_MANIFEST_FILE)).ok()?;
    let lib_field = manifest_field(&manifest_text, "library");
    if lib_field.is_empty() {
        return None;
    }
    let lib_filename = Path::new(&lib_field).file_name()?.to_str()?;
    let candidate = source_dir.join("target/release").join(lib_filename);
    if candidate.is_file() {
        Some((Path::new(&lib_field).to_path_buf(), candidate))
    } else {
        None
    }
}
