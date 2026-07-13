//! Command-level locks: the keyed `Mod` RW locks and the single `AppSettings`
//! RW lock, acquired by dispatch around a command's handler per the table
//! entry's declaration. Reads take the shared side, writes the exclusive side;
//! commands on different mods run concurrently, writes to the same mod
//! serialize.
//!
//! Synchronous commands follow the acquire-around-the-handler discipline. The
//! rank-1 `Update` lock is a try-acquire busy flag, not a queueing lock - a
//! second `startUpdate` fails immediately with `UPDATE_IN_PROGRESS` (matching
//! the TS `_isUpdating` guard), and the flag is held across the whole download
//! by an `UpdateGuard` owned by the operation body. The staged keyed-`Mod`
//! capture/commit acquisitions serve `compileInstalledMod` and `installMod`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// The session's command locks (the rank-1 locks of the inventory). Rank-2/3
/// locks (the profile artifact lock, the registries) live with their owners.
pub struct ResourceLocks {
    app_settings: RwLock<()>,
    /// One RW lock per mod id, created on first use. The map only grows; a
    /// session touches a bounded set of mods, so this is not a leak in
    /// practice (a prune story can be added if it ever matters).
    mods: Mutex<HashMap<String, Arc<RwLock<()>>>>,
    /// At-most-one-in-flight installer flag (rank 1), shared by `startUpdate` and
    /// `startInstallDevTools` (both run the same installer). An exclusion flag,
    /// not a queueing lock: nothing ever waits on it.
    update_in_progress: Arc<AtomicBool>,
}

/// Holds the `Update` busy flag for the lifetime of one installer operation
/// (`startUpdate` or `startInstallDevTools`) and clears it on drop (whether the
/// operation completed, failed, or was canceled, or the operation body was
/// dropped without running). `Send` so the operation body can own it across its
/// thread.
pub struct UpdateGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for UpdateGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl ResourceLocks {
    pub fn new() -> Self {
        Self {
            app_settings: RwLock::new(()),
            mods: Mutex::new(HashMap::new()),
            update_in_progress: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn app_settings(&self) -> &RwLock<()> {
        &self.app_settings
    }

    pub fn mod_lock(&self, mod_id: &str) -> Arc<RwLock<()>> {
        let mut map = self.mods.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(mod_id.to_owned())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// Try to claim the single-update flag. `Some(guard)` when this caller won
    /// it (the flag is now set and clears when the guard drops); `None` when an
    /// update is already in flight (the caller maps that to
    /// `UPDATE_IN_PROGRESS`).
    pub fn try_acquire_update(&self) -> Option<UpdateGuard> {
        self.update_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then(|| UpdateGuard {
                flag: self.update_in_progress.clone(),
            })
    }
}

impl Default for ResourceLocks {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_locks_are_keyed_and_stable() {
        let locks = ResourceLocks::new();
        let a1 = locks.mod_lock("a");
        let a2 = locks.mod_lock("a");
        let b = locks.mod_lock("b");
        // Same id -> the same underlying lock (so commands on one mod
        // serialize); different ids -> distinct locks (so they don't).
        assert!(Arc::ptr_eq(&a1, &a2));
        assert!(!Arc::ptr_eq(&a1, &b));
    }

    #[test]
    fn rw_semantics_hold() {
        let locks = ResourceLocks::new();
        let m = locks.mod_lock("m");

        // A held write excludes both readers and writers.
        {
            let _w = m.write().unwrap();
            assert!(m.try_read().is_err());
            assert!(m.try_write().is_err());
        }
        // A held read shares with readers but excludes writers.
        {
            let _r1 = m.read().unwrap();
            assert!(m.try_read().is_ok());
            assert!(m.try_write().is_err());
        }
        // Released: both succeed again.
        assert!(m.try_write().is_ok());
    }

    #[test]
    fn app_settings_lock_is_independent_of_mod_locks() {
        let locks = ResourceLocks::new();
        let _app = locks.app_settings().write().unwrap();
        // A held app-settings write does not block a mod lock.
        assert!(locks.mod_lock("m").try_write().is_ok());
    }

    #[test]
    fn update_flag_excludes_a_second_acquire_until_the_guard_drops() {
        let locks = ResourceLocks::new();
        let guard = locks.try_acquire_update().expect("first acquire wins");
        // A second startUpdate finds the flag set -> UPDATE_IN_PROGRESS.
        assert!(locks.try_acquire_update().is_none());
        drop(guard);
        // Once the in-flight update finishes, a new one may start.
        assert!(locks.try_acquire_update().is_some());
    }
}
