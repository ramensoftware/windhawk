//! Deterministic clock fake.

use std::sync::atomic::{AtomicI64, Ordering};

use windhawk_core_ports::Clock;

/// A clock that only moves when told to.
pub struct FakeClock(AtomicI64);

impl FakeClock {
    pub fn new(now_ms: i64) -> Self {
        Self(AtomicI64::new(now_ms))
    }

    pub fn advance_ms(&self, delta: i64) {
        self.0.fetch_add(delta, Ordering::SeqCst);
    }

    pub fn set_ms(&self, now_ms: i64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}
