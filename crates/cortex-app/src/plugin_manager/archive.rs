use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use super::{
    PLUGIN_CONFORMANCE_FILE, PLUGIN_LIB_DIR, PLUGIN_MANIFEST_FILE, PLUGIN_PACKAGE_FILE,
    PLUGIN_PROMPTS_DIR, PLUGIN_RISK_PROFILE_FILE, PLUGIN_SBOM_FILE, PLUGIN_SKILLS_DIR,
    should_include_plugin_entry_name,
};

fn normalize_plugin_rel_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

fn is_allowed_plugin_rel_path(path: &Path) -> bool {
    let Some(normalized) = normalize_plugin_rel_path(path) else {
        return false;
    };
    if !normalized.components().all(|component| match component {
        Component::Normal(value) => value.to_str().is_some_and(should_include_plugin_entry_name),
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
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .is_some_and(|name| {
            matches!(
                name,
                PLUGIN_LIB_DIR | PLUGIN_SKILLS_DIR | PLUGIN_PROMPTS_DIR
            )
        })
}

pub(super) fn extract_cpx_to_dir(cpx_path: &Path, dest: &Path) -> Result<(), String> {
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

/// Read `manifest.toml` from a .cpx archive without fully extracting.
pub(super) fn read_manifest_from_cpx(cpx_path: &Path) -> Result<String, String> {
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
        if path.as_ref() == Path::new(PLUGIN_MANIFEST_FILE) {
            let mut buf = String::new();
            entry
                .read_to_string(&mut buf)
                .map_err(|e| format!("cannot read manifest.toml: {e}"))?;
            return Ok(buf);
        }
    }
    Err("cpx archive missing manifest.toml".into())
}
