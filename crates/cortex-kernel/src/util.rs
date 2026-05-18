use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomically write `data` to `path` by writing to a temp file then renaming.
///
/// # Errors
/// Returns `io::Error` if the write or rename fails.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let suffix = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temp = dir.join(format!(".tmp.{}.{}.{}", std::process::id(), nanos, suffix));
    fs::write(&temp, data)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// Atomically write UTF-8 text to `path`.
///
/// # Errors
/// Returns `io::Error` if the write or rename fails.
pub fn atomic_write_text(path: &Path, text: impl AsRef<str>) -> io::Result<()> {
    atomic_write(path, text.as_ref().as_bytes())
}
