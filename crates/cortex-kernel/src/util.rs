use std::fs;
use std::io;
use std::path::Path;

/// Atomically write `data` to `path` by writing to a temp file then renaming.
///
/// # Errors
/// Returns `io::Error` if the write or rename fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = dir.join(format!(".tmp.{}", std::process::id()));
    fs::write(&temp, data)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}
