//! Filesystem operations the services need: read, atomic-replace (temp file
//! then `MoveFileExW`), existence, directory listing, and mtime. A port (rather
//! than direct `std::fs`) buys uniform error mapping and in-memory fakes for
//! tests; the production adapter is a thin wrapper over std/Win32.
//!
//! The port provides the read/list/exists/mtime/atomic-write subset the mod
//! source and profile commands need (`getModSource`, `doesModExist`,
//! `listInstalledMods`, and the profile read-modify-write), the temp-directory
//! and delete/remove-dir subset the update download needs (a private folder for
//! the installer, cleaned up on the way out), and create-dirs (the
//! per-architecture compiled-DLL folders).

use std::path::{Path, PathBuf};

use crate::os_error::{OsError, render};

/// Whether a failure was "the path does not exist" (which several callers
/// treat as benign: a missing source file is `MOD_NOT_INSTALLED`, a missing
/// `ModsSource`/profile is an empty result) or any other I/O failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileErrorKind {
    NotFound,
    Other,
}

/// A filesystem failure carrying the OS-call triple (the embedded `OsError`)
/// and the typed `path` the adapter touched. Services map this onto the wire
/// codes; the adapter never chooses a user-facing code.
#[derive(Debug, Clone)]
pub struct FileError {
    /// The shared OS-call triple (operation, raw code, message).
    pub os: OsError,
    pub path: String,
    pub kind: FileErrorKind,
}

impl FileError {
    pub fn new(
        operation: &'static str,
        path: impl Into<String>,
        kind: FileErrorKind,
        os_error: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            os: OsError::new(operation, os_error, message),
            path: path.into(),
            kind,
        }
    }

    pub fn is_not_found(&self) -> bool {
        self.kind == FileErrorKind::NotFound
    }

    /// The bare OS message, WITHOUT the decorated `{operation} failed for ...`
    /// prefix or the `(os error N)` suffix. Use this where a caller logs or
    /// forwards just the cause (preserving the wording from before the OsError
    /// refactor); use `to_string()`/`Display` where the decorated form is
    /// wanted. The two are NOT interchangeable.
    pub fn message(&self) -> &str {
        &self.os.message
    }
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&render(&self.path, &self.os))
    }
}

impl std::error::Error for FileError {}

/// One entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_file: bool,
}

pub trait Files: Send + Sync {
    /// Read a file's bytes. A missing file is a `NotFound` error.
    fn read(&self, path: &Path) -> Result<Vec<u8>, FileError>;

    /// Write `contents` to `path` durably: write a temp sibling, then replace
    /// the target via `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`, so external
    /// readers never see a half-written file. Creates the parent directory if
    /// needed.
    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), FileError>;

    /// Whether `path` exists (the `fs.existsSync` of `doesSourceExist`).
    fn exists(&self, path: &Path) -> bool;

    /// List a directory's immediate entries. A missing directory is a
    /// `NotFound` error (callers treat it as "no entries").
    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FileError>;

    /// Last-modified time in milliseconds since the Unix epoch (the JS
    /// `fs.statSync().mtimeMs` the profile watcher compares against).
    fn modified_ms(&self, path: &Path) -> Result<f64, FileError>;

    /// Create a fresh, uniquely-named directory under the OS temp area, named
    /// `<prefix><random>`, and return its path. The update download uses this
    /// for an installer-private folder so the launched installer cannot load
    /// DLLs an attacker planted in the shared temp directory (the TS
    /// `crypto.randomBytes` subfolder).
    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError>;

    /// Delete a single file. A missing file is a `NotFound` error; callers
    /// that clean up best-effort ignore it.
    fn delete_file(&self, path: &Path) -> Result<(), FileError>;

    /// Remove an (empty) directory, like the JS `fs.rmdirSync` the update
    /// cleanup uses. Best-effort callers ignore failures (a non-empty or
    /// in-use folder).
    fn remove_dir(&self, path: &Path) -> Result<(), FileError>;

    /// Recursively remove a directory and its contents (the JS `fs.rmSync(p,
    /// {recursive: true, force: true})` the mod removal uses for the per-mod
    /// `ModsWritable\mod-storage\<modId>` folder). A missing directory is a
    /// `NotFound` error; the only caller cleans up best-effort and ignores it.
    fn remove_dir_all(&self, path: &Path) -> Result<(), FileError>;

    /// Create a directory and all missing parents (the JS `fs.mkdirSync(p,
    /// {recursive: true})` the compiler does before writing a DLL). Succeeds if
    /// the directory already exists.
    fn create_dirs(&self, path: &Path) -> Result<(), FileError>;
}
