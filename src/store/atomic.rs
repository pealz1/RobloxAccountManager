//! Crash-safe file writes (port of `atomic_io.py`).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes to a temp file in the same folder, flushes it to disk, then renames over `path`.
/// The original is never truncated if anything fails.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}

/// Moves an unreadable file aside so it is kept for inspection instead of being overwritten.
pub fn quarantine(path: &Path) {
    let target = PathBuf::from(format!("{}.corrupt", path.display()));
    if fs::rename(path, &target).is_ok() {
        crate::log_warn!(
            "{} could not be read and was moved to {}",
            path.display(),
            target.display()
        );
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let unique = format!(".{name}.{}.{}.tmp", std::process::id(), rand::random::<u32>());
    path.with_file_name(unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
