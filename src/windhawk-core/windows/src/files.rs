//! The `Files` port adapter: a thin wrapper over `std::fs`, with atomic replace
//! through `MoveFileExW` (`MOVEFILE_REPLACE_EXISTING`) so external readers
//! never observe a half-written file (the user profile). Read/list/exists/
//! mtime are plain std calls; only the atomic rename needs Win32.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MoveFileExW};

use windhawk_core_ports::{DirEntry, FileError, FileErrorKind, Files};

use crate::wide::path_to_wide;

pub struct WindowsFiles;

/// Counter mixed into temp-directory names so concurrent `create_temp_dir`
/// calls in one process never collide before the create-must-be-new loop even
/// runs. A process-local atomic is fine here - this is the sanctioned Win32
/// adapter crate, outside the no-globals gate.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn kind_of(e: &io::Error) -> FileErrorKind {
    if e.kind() == io::ErrorKind::NotFound {
        FileErrorKind::NotFound
    } else {
        FileErrorKind::Other
    }
}

fn map_err(operation: &'static str, path: &Path, e: &io::Error) -> FileError {
    FileError::new(
        operation,
        path.display().to_string(),
        kind_of(e),
        e.raw_os_error().unwrap_or(0) as u32,
        e.to_string(),
    )
}

impl Files for WindowsFiles {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FileError> {
        std::fs::read(path).map_err(|e| map_err("read", path, &e))
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), FileError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| map_err("create_dir", parent, &e))?;
        }

        // Write a temp sibling, then replace the target in one atomic rename
        // (the TS `userProfilePath + '.tmp'` then renameSync). std::fs::rename
        // does not replace an existing file on Windows, so go through
        // MoveFileExW(MOVEFILE_REPLACE_EXISTING).
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);

        std::fs::write(&temp, contents).map_err(|e| map_err("write", &temp, &e))?;

        let temp_w = path_to_wide(&temp);
        let dest_w = path_to_wide(path);
        // SAFETY: both buffers are NUL-terminated; the flag requests replacing
        // an existing target.
        let ok =
            unsafe { MoveFileExW(temp_w.as_ptr(), dest_w.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
        if ok == 0 {
            let os = io::Error::last_os_error();
            // Best effort cleanup of the temp file (mirrors the TS rmSync).
            let _ = std::fs::remove_file(&temp);
            return Err(FileError::new(
                "rename",
                path.display().to_string(),
                FileErrorKind::Other,
                os.raw_os_error().unwrap_or(0) as u32,
                format!("MoveFileEx: {os}"),
            ));
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FileError> {
        let read_dir = std::fs::read_dir(path).map_err(|e| map_err("list", path, &e))?;
        let mut out = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|e| map_err("list", path, &e))?;
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            out.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_file,
            });
        }
        Ok(out)
    }

    fn modified_ms(&self, path: &Path) -> Result<f64, FileError> {
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|e| map_err("stat", path, &e))?;
        let ms = modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Ok(ms)
    }

    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError> {
        // Bound on collision retries before giving up; with nanos + a
        // process-local counter a real collision is already vanishingly rare.
        const MAX_TEMP_DIR_ATTEMPTS: usize = 64;
        let base = std::env::temp_dir();
        // Try fresh names until create_dir succeeds (it fails if the name
        // already exists, so the winning name is exclusively ours). The name
        // mixes wall-clock nanos with a process-local counter for entropy.
        for _ in 0..MAX_TEMP_DIR_ATTEMPTS {
            let nanos = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = base.join(format!("{prefix}{nanos:x}{seq:x}"));
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(map_err("create_temp_dir", &candidate, &e)),
            }
        }
        Err(FileError::new(
            "create_temp_dir",
            base.display().to_string(),
            FileErrorKind::Other,
            0,
            "could not create a unique temp directory",
        ))
    }

    fn delete_file(&self, path: &Path) -> Result<(), FileError> {
        std::fs::remove_file(path).map_err(|e| map_err("delete", path, &e))
    }

    fn remove_dir(&self, path: &Path) -> Result<(), FileError> {
        std::fs::remove_dir(path).map_err(|e| map_err("remove_dir", path, &e))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), FileError> {
        std::fs::remove_dir_all(path).map_err(|e| map_err("remove_dir_all", path, &e))
    }

    fn create_dirs(&self, path: &Path) -> Result<(), FileError> {
        std::fs::create_dir_all(path).map_err(|e| map_err("create_dirs", path, &e))
    }
}
