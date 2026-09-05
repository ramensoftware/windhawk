//! Filesystem operations the services need: read, atomic-replace (temp file
//! then `MoveFileExW`), existence, directory listing, and mtime. A port (rather
//! than direct `std::fs`) buys uniform error mapping and in-memory fakes for
//! tests; the production adapter is a thin wrapper over std/Win32.
//!
//! The port provides the read/list/exists/mtime/atomic-write subset the mod
//! source and profile commands need (`getModSource`, `doesModExist`,
//! `listInstalledMods`, and the profile read-modify-write), the temp-directory
//! and delete subset the update download needs (a private folder for the
//! installer, given up on the way out), create-dirs (the per-architecture
//! compiled-DLL folders), and a write probe (whether a directory would take a
//! write at all).

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
    /// the target via `MoveFileExW(MOVEFILE_REPLACE_EXISTING |
    /// MOVEFILE_WRITE_THROUGH)`, so external readers never see a half-written
    /// file and the rename is on disk before the call returns. Creates the
    /// parent directory if needed.
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
    /// `<prefix><random>`, and return its path.
    ///
    /// The directory is the caller's alone, in access and not only in name: an
    /// implementation running with more privilege than the temp area's owner
    /// must give it an access list that admits nobody less privileged, rather
    /// than whatever the enclosing area admits. The update download writes the
    /// installer it is about to launch here, so a folder someone else can write
    /// is one they can plant a DLL in or swap the installer in.
    ///
    /// Directories that earlier calls with the same prefix left behind are swept
    /// first, best effort, once they are old enough that no live caller could
    /// still hold one. Nothing else ever comes back for them: `release_temp_dir`
    /// gives up whatever it could not remove, and the next call under the same
    /// prefix is the only moment anything looks at the temp area again.
    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError>;

    /// Give up a directory from `create_temp_dir`, removing it and whatever is
    /// left inside.
    ///
    /// What cannot be removed must be left removable by the temp area's owner:
    /// an implementation that gave the directory an access list of its own hands
    /// back the right to DELETE the remains, and only that right, since the
    /// folder may still hold an executable this process launched out of it. The
    /// update's installer is exactly that - it holds its own image open, so the
    /// removal here routinely cannot take it, and the folder is the ordinary
    /// user's temp area, where every tool that would clean up after us runs
    /// unelevated.
    ///
    /// Best effort: the only caller cleans up on the way out and ignores the
    /// result.
    fn release_temp_dir(&self, dir: &Path) -> Result<(), FileError>;

    /// Delete a single file. A missing file is a `NotFound` error; callers
    /// that clean up best-effort ignore it.
    fn delete_file(&self, path: &Path) -> Result<(), FileError>;

    /// Recursively remove a directory and its contents (the JS `fs.rmSync(p,
    /// {recursive: true, force: true})` the mod removal uses for the per-mod
    /// `ModsWritable\mod-storage\<modId>` folder). A missing directory is a
    /// `NotFound` error; the only caller cleans up best-effort and ignores it.
    fn remove_dir_all(&self, path: &Path) -> Result<(), FileError>;

    /// Create a directory and all missing parents (the JS `fs.mkdirSync(p,
    /// {recursive: true})` the compiler does before writing a DLL). Succeeds if
    /// the directory already exists.
    fn create_dirs(&self, path: &Path) -> Result<(), FileError>;

    /// Whether `dir` would take a write, decided by making one: the probe
    /// creates a uniquely named file in it and removes it again. `Ok(())` means
    /// a write would have been allowed; the error carries the OS refusal, so a
    /// caller can tell a lack of rights (`ERROR_ACCESS_DENIED`) from a folder
    /// that is missing, full, or otherwise unusable.
    fn probe_writable(&self, dir: &Path) -> Result<(), FileError>;
}
