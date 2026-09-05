//! In-memory `NamedLock` port (core-internals.md section 3.5, testkit). The
//! lock only coordinates separate OS processes, so within a single test
//! process a no-op guard is the right fake; acquisitions are recorded so a
//! test can assert the profile read-modify-write took the lock.

use std::sync::{Arc, Mutex};

use windhawk_core_ports::{NamedLock, NamedLockGuard};

#[derive(Clone, Default)]
pub struct FakeNamedLock {
    acquisitions: Arc<Mutex<Vec<String>>>,
}

impl FakeNamedLock {
    pub fn new() -> Self {
        Self::default()
    }

    /// The names acquired so far, for assertions.
    pub fn acquisitions(&self) -> Vec<String> {
        self.acquisitions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

struct NoopGuard;
impl NamedLockGuard for NoopGuard {}

impl NamedLock for FakeNamedLock {
    fn acquire(&self, name: &str, _timeout_ms: u32) -> Box<dyn NamedLockGuard> {
        self.acquisitions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(name.to_owned());
        Box::new(NoopGuard)
    }
}
