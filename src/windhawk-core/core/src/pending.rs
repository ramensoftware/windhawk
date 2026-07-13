//! The pending-artifact set: the compiled DLLs that in-flight operations have
//! written but not yet committed. Old-DLL cleanup skips anything in this set,
//! so a concurrent operation's not-yet-committed output is never deleted; the
//! protection lasts until the committing operation's config write points at the
//! DLL.
//!
//! The set is a rank-3 leaf lock: every insert/remove/contains is a quick map
//! op that takes no other lock and makes no port call while held.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use windhawk_core_ports::Files;

/// Session-scoped registry of DLL paths in-flight operations are writing.
#[derive(Default)]
pub struct PendingArtifacts {
    paths: Mutex<HashSet<PathBuf>>,
}

impl PendingArtifacts {
    pub fn new() -> Self {
        Self::default()
    }

    fn add(&self, path: PathBuf) {
        self.paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path);
    }

    fn remove(&self, path: &Path) {
        self.paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path);
    }

    /// Whether `path` is a not-yet-committed artifact of some in-flight
    /// operation (old-DLL cleanup consults this before deleting).
    pub fn contains(&self, path: &Path) -> bool {
        self.paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(path)
    }
}

/// The pending DLLs one in-flight compile registered. Dropping it deregisters
/// them from the session set (so the set never leaks), which the commit
/// section does only after writing the config that points at them - until then
/// the DLLs stay protected from a concurrent cleanup.
pub struct PendingHandle {
    set: Arc<PendingArtifacts>,
    paths: Vec<PathBuf>,
}

impl PendingHandle {
    pub fn new(set: Arc<PendingArtifacts>) -> Self {
        Self {
            set,
            paths: Vec::new(),
        }
    }

    /// Register a DLL the slow phase is about to write.
    pub fn add(&mut self, path: PathBuf) {
        self.set.add(path.clone());
        self.paths.push(path);
    }

    /// Best-effort unlink of every registered DLL - the cancel path, matching
    /// the TS `cancelCompilation` unlink of `pendingDllPaths`.
    pub fn unlink_all(&self, files: &dyn Files) {
        for path in &self.paths {
            let _ = files.delete_file(path);
        }
    }
}

impl Drop for PendingHandle {
    fn drop(&mut self) {
        for path in &self.paths {
            self.set.remove(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_registers_and_deregisters() {
        let set = Arc::new(PendingArtifacts::new());
        let a = PathBuf::from("C:\\mods\\64\\m_1.0_1.dll");
        let b = PathBuf::from("C:\\mods\\64\\m_1.0_2.dll");
        {
            let mut h = PendingHandle::new(set.clone());
            h.add(a.clone());
            h.add(b.clone());
            assert!(set.contains(&a));
            assert!(set.contains(&b));
        }
        // Dropping the handle clears the session registry.
        assert!(!set.contains(&a));
        assert!(!set.contains(&b));
    }
}
