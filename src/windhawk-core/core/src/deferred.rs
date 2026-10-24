//! Deferred removals: the background retry of removals a process blocked. A
//! process the engine has not yet unloaded a mod from can hold the mod's DLL
//! loaded and the files in its storage folder open, which Windows refuses to
//! delete. A removal that fails that way is queued here, so the command that
//! made it does not wait for the engine.
//!
//! Attempts run on one worker thread, spawned when a removal is queued and
//! exiting once none is left. Each removal is retried, more and more sparsely,
//! until a minute after the config change that stopped naming what it removes.
//! Shutdown retries what is left until a second after its config change, so a
//! session that ends right after a removal, as a CLI command's does, still
//! gives the engine time to unload the mod from the running processes.
//!
//! The queue is a rank-3 leaf lock: attempts run after it is released.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long after its config change a removal is retried while the session
/// runs. A process the engine cannot reach meanwhile, such as a suspended
/// packaged app, holds the files past it.
const RETRY_WINDOW: Duration = Duration::from_secs(60);

/// How long after its config change a removal is retried once the session
/// shuts down: enough for the engine to unload the mod from the processes that
/// are running.
const SHUTDOWN_RETRY_WINDOW: Duration = Duration::from_secs(1);

const MIN_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(2);

/// One attempt at a removal, run against the queue's context: `true` once
/// nothing it removes is left in use, whether removed or given up on.
pub type Attempt<C> = Box<dyn FnMut(&C) -> bool + Send>;

struct Removal<C> {
    attempt: Attempt<C>,
    /// The config write that stopped naming what the removal removes.
    config_changed: Instant,
    next_attempt: Instant,
}

struct State<C> {
    removals: Vec<Removal<C>>,
    /// The worker, which may have exited when not `running`.
    worker: Option<JoinHandle<()>>,
    running: bool,
    shutting_down: bool,
}

struct Shared<C> {
    state: Mutex<State<C>>,
    wake: Condvar,
}

impl<C> Shared<C> {
    fn lock(&self) -> MutexGuard<'_, State<C>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct DeferredRemovals<C> {
    context: Weak<C>,
    shared: Arc<Shared<C>>,
}

impl<C: Send + Sync + 'static> DeferredRemovals<C> {
    /// A queue whose attempts run against `context` while it lives.
    pub fn new(context: Weak<C>) -> Self {
        Self {
            context,
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    removals: Vec::new(),
                    worker: None,
                    running: false,
                    shutting_down: false,
                }),
                wake: Condvar::new(),
            }),
        }
    }

    /// Queue a removal whose first attempt left something in use, retried from
    /// `config_changed` on. Best effort, like the removal itself: one queued
    /// during shutdown, or one no worker can be spawned for, is dropped.
    pub fn push(&self, config_changed: Instant, attempt: Attempt<C>) {
        let mut state = self.shared.lock();
        if state.shutting_down {
            return;
        }
        state.removals.push(Removal {
            attempt,
            config_changed,
            next_attempt: next_attempt(config_changed, Instant::now()),
        });
        if state.running {
            self.shared.wake.notify_all();
            return;
        }
        let shared = self.shared.clone();
        let context = self.context.clone();
        let spawned = std::thread::Builder::new()
            .name("windhawk-core removals".to_owned())
            .spawn(move || run(&shared, &context));
        let exited = match spawned {
            Ok(worker) => {
                state.running = true;
                state.worker.replace(worker)
            }
            Err(_) => {
                state.removals.clear();
                None
            }
        };
        drop(state);
        // A worker that stopped running has returned or is returning, holding
        // nothing.
        if let Some(exited) = exited {
            let _ = exited.join();
        }
    }

    /// Attempt every queued removal at once, retry each until
    /// [`SHUTDOWN_RETRY_WINDOW`] past its config change, and return once the
    /// worker has given up on the rest.
    pub fn shutdown(&self) {
        let worker = {
            let mut state = self.shared.lock();
            state.shutting_down = true;
            let now = Instant::now();
            for removal in &mut state.removals {
                removal.next_attempt = now;
            }
            self.shared.wake.notify_all();
            state.worker.take()
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

/// When to attempt a removal next: after a tenth of the time since its config
/// change, kept between the interval bounds, so it is retried often while the
/// engine reaches the processes and sparsely once it has stayed blocked.
fn next_attempt(config_changed: Instant, now: Instant) -> Instant {
    let elapsed = now.saturating_duration_since(config_changed);
    now + (elapsed / 10).clamp(MIN_RETRY_INTERVAL, MAX_RETRY_INTERVAL)
}

/// The worker: attempt each removal when it is due, until none is left.
fn run<C>(shared: &Shared<C>, context: &Weak<C>) {
    let mut state = shared.lock();
    loop {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.removals)
            .into_iter()
            .partition(|removal| removal.next_attempt <= now);
        state.removals = later;
        if due.is_empty() {
            let Some(next) = state.removals.iter().map(|r| r.next_attempt).min() else {
                state.running = false;
                return;
            };
            state = shared
                .wake
                .wait_timeout(state, next.saturating_duration_since(now))
                .unwrap_or_else(|e| e.into_inner())
                .0;
            continue;
        }
        drop(state);
        let left = attempt_all(context, due);
        state = shared.lock();
        let now = Instant::now();
        let window = if state.shutting_down {
            SHUTDOWN_RETRY_WINDOW
        } else {
            RETRY_WINDOW
        };
        for mut removal in left {
            if now.saturating_duration_since(removal.config_changed) < window {
                removal.next_attempt = next_attempt(removal.config_changed, now);
                state.removals.push(removal);
            }
        }
    }
}

/// Attempt each removal, keeping the ones that left something in use. Once the
/// context is gone, nothing is left to attempt.
fn attempt_all<C>(context: &Weak<C>, mut removals: Vec<Removal<C>>) -> Vec<Removal<C>> {
    let Some(context) = context.upgrade() else {
        return Vec::new();
    };
    // A panicking attempt is given up on rather than taking the worker down.
    removals.retain_mut(|removal| {
        !catch_unwind(AssertUnwindSafe(|| (removal.attempt)(&context))).unwrap_or(true)
    });
    removals
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// An attempt that counts its calls and reports done on call `done_at`.
    fn counted(calls: &Arc<AtomicU32>, done_at: u32) -> Attempt<()> {
        let calls = calls.clone();
        Box::new(move |_| calls.fetch_add(1, Ordering::SeqCst) + 1 >= done_at)
    }

    fn wait_until(condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "condition not reached");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn running(removals: &DeferredRemovals<()>) -> bool {
        removals.shared.lock().running
    }

    #[test]
    fn the_interval_grows_with_the_time_since_the_config_change() {
        let changed = Instant::now();
        let after = |elapsed: Duration| next_attempt(changed, changed + elapsed) - changed;
        assert_eq!(after(Duration::ZERO), MIN_RETRY_INTERVAL);
        assert_eq!(after(Duration::from_secs(5)), Duration::from_millis(5500));
        assert_eq!(
            after(Duration::from_secs(50)),
            Duration::from_secs(50) + MAX_RETRY_INTERVAL
        );
    }

    #[test]
    fn a_removal_is_retried_until_it_is_done_and_the_worker_then_exits() {
        let context = Arc::new(());
        let removals = DeferredRemovals::new(Arc::downgrade(&context));
        let calls = Arc::new(AtomicU32::new(0));

        removals.push(Instant::now(), counted(&calls, 3));

        wait_until(|| !running(&removals));
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // A removal queued after the worker exited gets a worker of its own.
        let later = Arc::new(AtomicU32::new(0));
        removals.push(Instant::now(), counted(&later, 1));
        wait_until(|| later.load(Ordering::SeqCst) == 1);
        removals.shutdown();
    }

    #[test]
    fn a_panicking_attempt_is_dropped_and_the_others_go_on() {
        let context = Arc::new(());
        let removals = DeferredRemovals::new(Arc::downgrade(&context));
        let calls = Arc::new(AtomicU32::new(0));

        removals.push(Instant::now(), Box::new(|_| panic!("attempt failed")));
        removals.push(Instant::now(), counted(&calls, 2));

        wait_until(|| !running(&removals));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        removals.shutdown();
    }

    #[test]
    fn nothing_is_attempted_once_the_context_is_gone() {
        let context = Arc::new(());
        let removals = DeferredRemovals::new(Arc::downgrade(&context));
        let calls = Arc::new(AtomicU32::new(0));

        drop(context);
        removals.push(Instant::now(), counted(&calls, u32::MAX));

        wait_until(|| !running(&removals));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn shutdown_retries_until_a_second_after_the_config_change() {
        let context = Arc::new(());
        let removals = DeferredRemovals::new(Arc::downgrade(&context));
        let calls = Arc::new(AtomicU32::new(0));
        let changed = Instant::now();

        removals.push(changed, counted(&calls, u32::MAX));
        removals.shutdown();

        assert!(changed.elapsed() >= SHUTDOWN_RETRY_WINDOW);
        assert!(calls.load(Ordering::SeqCst) > 1);
        removals.push(Instant::now(), counted(&calls, 1));
        assert!(!running(&removals), "nothing is queued after shutdown");
    }

    #[test]
    fn shutdown_attempts_a_removal_past_the_shutdown_window_once_more() {
        let context = Arc::new(());
        let removals = DeferredRemovals::new(Arc::downgrade(&context));
        let calls = Arc::new(AtomicU32::new(0));
        let changed = Instant::now() - Duration::from_secs(5);

        removals.push(changed, counted(&calls, u32::MAX));
        removals.shutdown();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
