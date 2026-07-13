//! Cooperative cancellation: a flag that blocking work polls or waits on, plus
//! run-once hooks for blocking waits that cannot poll (process kill, handle
//! close).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct CancelToken {
    canceled: AtomicBool,
    hooks: Mutex<Vec<Hook>>,
    // Condvar pairing for cancel-aware waits; the mutex guards no data
    // beyond the wait protocol (the flag is the atomic above).
    wait_lock: Mutex<()>,
    wait_cv: Condvar,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }

    /// Signal cancellation: set the flag, wake waiters, and run the
    /// registered hooks once, on the calling thread. Idempotent.
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
        self.wait_cv.notify_all();
        let hooks = {
            let mut hooks = self.hooks.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *hooks)
        };
        // Hooks run outside the lock: they must be non-blocking and
        // idempotent, but must not be able to deadlock registration.
        for hook in hooks {
            hook();
        }
    }

    /// Register a hook to run once on cancellation. If the token is already
    /// canceled, the hook runs immediately on this thread.
    pub fn on_cancel(&self, hook: Hook) {
        if self.is_canceled() {
            hook();
            return;
        }
        let mut hooks = self.hooks.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_canceled() {
            drop(hooks);
            hook();
        } else {
            hooks.push(hook);
        }
    }

    /// Block for up to `timeout`, returning early (true) on cancellation.
    /// The bounded-cancel rule for ports follows from using this instead of
    /// plain sleeps.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut guard = self.wait_lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if self.is_canceled() {
                return true;
            }
            let now = std::time::Instant::now();
            let Some(remaining) = deadline
                .checked_duration_since(now)
                .filter(|d| !d.is_zero())
            else {
                return self.is_canceled();
            };
            let (g, _timeout_result) = self
                .wait_cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|e| e.into_inner());
            guard = g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn cancel_sets_flag_and_runs_hooks_once() {
        let token = CancelToken::new();
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        token.on_cancel(Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(!token.is_canceled());
        token.cancel();
        token.cancel();
        assert!(token.is_canceled());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn hook_registered_after_cancel_runs_immediately() {
        let token = CancelToken::new();
        token.cancel();
        let ran = Arc::new(AtomicU32::new(0));
        let r = ran.clone();
        token.on_cancel(Box::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wait_returns_early_on_cancel() {
        let token = Arc::new(CancelToken::new());
        let t = token.clone();
        let waiter = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let canceled = t.wait(Duration::from_secs(30));
            (canceled, start.elapsed())
        });
        std::thread::sleep(Duration::from_millis(50));
        token.cancel();
        let (canceled, elapsed) = waiter.join().unwrap();
        assert!(canceled);
        assert!(elapsed < Duration::from_secs(5));
    }

    #[test]
    fn wait_times_out_without_cancel() {
        let token = CancelToken::new();
        assert!(!token.wait(Duration::from_millis(10)));
    }
}
