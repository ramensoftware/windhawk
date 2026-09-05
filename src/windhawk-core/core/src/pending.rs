//! The pending-artifact set: the compiled-DLL paths in-flight operations have
//! reserved and not yet committed. Old-DLL cleanup skips anything in this set,
//! so a concurrent operation's not-yet-committed output is never deleted; the
//! protection lasts until the committing operation's config write points at the
//! DLL.
//!
//! Reserving is also what keeps two operations on one mod off a single DLL
//! name: a path enters the set only through the all-or-nothing `claim_all`, so
//! it belongs to at most one handle and no handle's drop can unprotect
//! another's artifact.
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

    /// Take every path for one caller, all or nothing: `false` when any of them
    /// is already reserved, leaving the set untouched.
    fn claim_all(&self, paths: &[PathBuf]) -> bool {
        let mut set = self.paths.lock().unwrap_or_else(|e| e.into_inner());
        if paths.iter().any(|p| set.contains(p)) {
            return false;
        }
        set.extend(paths.iter().cloned());
        true
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

    /// Reserve the paths one candidate DLL name occupies, before the slow phase
    /// writes anything. All or nothing: `false` when another in-flight
    /// operation already holds one of them, which is the caller's cue to try
    /// the next name rather than share a path with it.
    pub fn claim_all(&mut self, paths: Vec<PathBuf>) -> bool {
        if !self.set.claim_all(&paths) {
            return false;
        }
        self.paths.extend(paths);
        true
    }

    /// Best-effort unlink of every reserved path - the cancel path, matching
    /// the TS `cancelCompilation` unlink of `pendingDllPaths`. A reservation
    /// whose DLL was never written has nothing there to delete.
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
        let a = PathBuf::from("C:\\mods\\32\\m_1.0_1.dll");
        let b = PathBuf::from("C:\\mods\\64\\m_1.0_1.dll");
        {
            let mut h = PendingHandle::new(set.clone());
            assert!(h.claim_all(vec![a.clone(), b.clone()]));
            assert!(set.contains(&a));
            assert!(set.contains(&b));
        }
        // Dropping the handle clears the session registry.
        assert!(!set.contains(&a));
        assert!(!set.contains(&b));
    }

    #[test]
    fn a_claim_overlapping_another_handle_takes_nothing() {
        // Two handles must never hold one path: `Drop` deregisters by path, so
        // a shared entry would let the first handle to finish unprotect the
        // other operation's in-flight DLL.
        let set = Arc::new(PendingArtifacts::new());
        let shared = PathBuf::from("C:\\mods\\64\\m_1.0_1.dll");
        let fresh = PathBuf::from("C:\\mods\\32\\m_1.0_1.dll");

        let mut first = PendingHandle::new(set.clone());
        assert!(first.claim_all(vec![shared.clone()]));

        let mut second = PendingHandle::new(set.clone());
        assert!(!second.claim_all(vec![fresh.clone(), shared.clone()]));
        // All or nothing: the free path of the refused claim stays free.
        assert!(!set.contains(&fresh));

        // The path is claimable again once its holder is done with it.
        drop(first);
        assert!(second.claim_all(vec![shared.clone()]));
        assert!(set.contains(&shared));
    }
}
