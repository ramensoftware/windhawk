//! In-memory `Files` port (core-internals.md section 3.2, testkit). A
//! behavioral fake: it stores file bytes keyed by path, models atomic writes
//! as a plain insert, and hands out a monotonically increasing mtime so the
//! profile's last-own-write bookkeeping is observable without touching disk.
//! Byte-format and real Win32 sharing semantics are the WindowsFiles adapter's
//! job (verified by the fixture-replay suite).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windhawk_core_ports::{DirEntry, FileError, FileErrorKind, Files};

/// A stored file: its bytes and its last-modified mtime (epoch milliseconds).
type StoredFile = (Vec<u8>, f64);

#[derive(Clone, Default)]
pub struct FakeFiles {
    store: Arc<Mutex<BTreeMap<PathBuf, StoredFile>>>,
    clock: Arc<Mutex<f64>>,
    /// Counter for unique `create_temp_dir` names.
    temp_seq: Arc<AtomicU64>,
    /// When set, the read-side ops (`read`, `list_dir`, `modified_ms`) fail
    /// with this error, so command tests can exercise the file-error -> wire
    /// mapping (`IO_FAILED`) the way `FakeSettings::set_fault` does for the
    /// registry/INI backends.
    read_fault: Arc<Mutex<Option<FileError>>>,
    /// When set, `write_atomic` fails with this error (the profile write is
    /// best effort, so this drives the logged-and-swallowed path).
    write_fault: Arc<Mutex<Option<FileError>>>,
    /// When set, `probe_writable` fails with this error - the folder a caller
    /// asked about will not take a write.
    probe_fault: Arc<Mutex<Option<FileError>>>,
    /// When set, `create_dirs` fails with this error.
    create_dirs_fault: Arc<Mutex<Option<FileError>>>,
}

impl FakeFiles {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_mtime(&self) -> f64 {
        let mut clock = self.clock.lock().unwrap_or_else(|e| e.into_inner());
        *clock += 1.0;
        *clock
    }

    /// Seed a file (test setup).
    pub fn seed(&self, path: impl AsRef<Path>, contents: impl Into<Vec<u8>>) {
        let mtime = self.next_mtime();
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.as_ref().to_path_buf(), (contents.into(), mtime));
    }

    /// Make the read-side ops (`read`/`list_dir`/`modified_ms`) fail with
    /// `error` (fault injection, core-internals.md section 3).
    pub fn set_fault(&self, error: FileError) {
        *self.read_fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    /// Make `write_atomic` fail with `error`.
    pub fn set_write_fault(&self, error: FileError) {
        *self.write_fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    /// Make `probe_writable` fail with `error`, so a command test can drive the
    /// folder-is-not-writable path.
    pub fn set_probe_fault(&self, error: FileError) {
        *self.probe_fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    /// Make `create_dirs` fail with `error`.
    pub fn set_create_dirs_fault(&self, error: FileError) {
        *self
            .create_dirs_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    fn read_fault(&self) -> Option<FileError> {
        self.read_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn write_fault(&self) -> Option<FileError> {
        self.write_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn probe_fault(&self) -> Option<FileError> {
        self.probe_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn create_dirs_fault(&self) -> Option<FileError> {
        self.create_dirs_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The current bytes of a file as UTF-8 text, for assertions.
    pub fn read_text(&self, path: impl AsRef<Path>) -> Option<String> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path.as_ref())
            .map(|(bytes, _)| String::from_utf8_lossy(bytes).into_owned())
    }

    /// The raw bytes of a file, for assertions (e.g. the downloaded installer).
    pub fn read_bytes(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path.as_ref())
            .map(|(bytes, _)| bytes.clone())
    }
}

fn not_found(op: &'static str, path: &Path) -> FileError {
    FileError::new(
        op,
        path.display().to_string(),
        FileErrorKind::NotFound,
        2, // ERROR_FILE_NOT_FOUND
        "no such file or directory",
    )
}

impl Files for FakeFiles {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FileError> {
        if let Some(fault) = self.read_fault() {
            return Err(fault);
        }
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .map(|(bytes, _)| bytes.clone())
            .ok_or_else(|| not_found("read", path))
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), FileError> {
        if let Some(fault) = self.write_fault() {
            return Err(fault);
        }
        let mtime = self.next_mtime();
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_path_buf(), (contents.to_vec(), mtime));
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(path)
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FileError> {
        if let Some(fault) = self.read_fault() {
            return Err(fault);
        }
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for file in store.keys() {
            if file.parent() == Some(path)
                && let Some(name) = file.file_name()
            {
                out.push(DirEntry {
                    name: name.to_string_lossy().into_owned(),
                    is_file: true,
                });
            }
        }
        Ok(out)
    }

    fn modified_ms(&self, path: &Path) -> Result<f64, FileError> {
        if let Some(fault) = self.read_fault() {
            return Err(fault);
        }
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .map(|(_, mtime)| *mtime)
            .ok_or_else(|| not_found("stat", path))
    }

    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError> {
        // A synthetic, unique path under a fixed fake temp root; the fake does
        // not model directories, so this just hands back a fresh path the
        // installer write/cleanup can use.
        let seq = self.temp_seq.fetch_add(1, Ordering::Relaxed);
        Ok(PathBuf::from(format!("C:\\fake-temp\\{prefix}{seq}")))
    }

    fn release_temp_dir(&self, dir: &Path) -> Result<(), FileError> {
        // No directory modeling and no access lists: dropping what is stored
        // under `dir` is the whole of it. A folder with nothing left in it is
        // released fine, unlike `remove_dir_all` on a path that never existed.
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|file, _| !file.starts_with(dir));
        Ok(())
    }

    fn delete_file(&self, path: &Path) -> Result<(), FileError> {
        if self
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path)
            .is_some()
        {
            Ok(())
        } else {
            Err(not_found("delete", path))
        }
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), FileError> {
        // No directory modeling: drop every stored file under `path`. A miss
        // reports NotFound, like the real recursive remove on an absent folder.
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let before = store.len();
        store.retain(|file, _| !file.starts_with(path));
        if store.len() == before {
            Err(not_found("remove_dir_all", path))
        } else {
            Ok(())
        }
    }

    fn create_dirs(&self, _path: &Path) -> Result<(), FileError> {
        // No directory modeling: files exist independent of their parents.
        match self.create_dirs_fault() {
            Some(fault) => Err(fault),
            None => Ok(()),
        }
    }

    fn probe_writable(&self, _dir: &Path) -> Result<(), FileError> {
        // No permission modeling: a folder takes writes unless a test says
        // otherwise.
        match self.probe_fault() {
            Some(fault) => Err(fault),
            None => Ok(()),
        }
    }
}
