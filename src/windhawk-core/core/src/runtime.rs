//! The asynchronous-operation runtime: per-operation threads, the operation
//! registry, and the `OpHandle` state machine whose terminal transition emits
//! the terminal event - "exactly one completed or failed event" is enforced
//! here, not by discipline in operation bodies.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::Value;
use windhawk_core_ports::CancelToken;
use windhawk_core_protocol::OperationEvent;

use crate::callbacks::{CallbackDispatcher, LogLevel};
use crate::error::CoreError;

#[derive(PartialEq, Eq)]
enum OpState {
    Running,
    Terminal,
}

/// State shared between an operation thread, the registry, and the
/// `OpContext` handed to the operation body.
pub struct OpShared {
    op_id: u64,
    cancel: CancelToken,
    state: Mutex<OpState>,
    dispatcher: Arc<CallbackDispatcher>,
}

impl OpShared {
    /// Transition to terminal and emit the terminal event; returns false
    /// (emitting nothing) if already terminal.
    fn transition_terminal(&self, event: &OperationEvent) -> bool {
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if *state == OpState::Terminal {
                return false;
            }
            *state = OpState::Terminal;
        }
        self.dispatcher.event(self.op_id, event.to_json());
        true
    }

    fn is_terminal(&self) -> bool {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) == OpState::Terminal
    }
}

/// The capability surface of an operation body: progress events and
/// cooperative cancellation. Terminal events are not emittable from here;
/// they belong to the state machine.
pub struct OpContext {
    shared: Arc<OpShared>,
}

impl OpContext {
    pub fn op_id(&self) -> u64 {
        self.shared.op_id
    }

    pub fn emit_progress(&self, payload: Value) {
        self.shared.dispatcher.event(
            self.shared.op_id,
            OperationEvent::Progress { payload }.to_json(),
        );
    }

    /// `startUpdate`'s one-shot download-to-install transition (the TS
    /// `onInstalling`); a non-terminal event, like progress.
    pub fn emit_installing(&self) {
        self.shared
            .dispatcher
            .event(self.shared.op_id, OperationEvent::Installing.to_json());
    }

    pub fn cancel_token(&self) -> &CancelToken {
        &self.shared.cancel
    }

    /// Convenience for poll-style cancellation points.
    pub fn check_canceled(&self) -> Result<(), CoreError> {
        if self.shared.cancel.is_canceled() {
            Err(CoreError::canceled())
        } else {
            Ok(())
        }
    }
}

/// The operation body: runs on the operation thread with its context.
pub type OpBody = Box<dyn FnOnce(&OpContext) -> Result<Value, CoreError> + Send>;

/// The synchronously validated part of an async command: a closure that
/// runs the operation body on its operation thread.
pub struct PreparedOp(pub OpBody);

struct OpEntry {
    shared: Arc<OpShared>,
    join: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct RegistryInner {
    ops: HashMap<u64, OpEntry>,
    /// Join handles of terminal operations, joined opportunistically on the
    /// next spawn and finally on destroy, so every thread is joined without the
    /// registry growing unboundedly.
    finished: Vec<JoinHandle<()>>,
}

pub struct OperationRegistry {
    next_id: AtomicU64,
    inner: Mutex<RegistryInner>,
}

impl OperationRegistry {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            inner: Mutex::new(RegistryInner::default()),
        }
    }

    /// Spawn an operation thread for a validated async command and return its
    /// nonzero operation id.
    pub fn spawn(&self, dispatcher: Arc<CallbackDispatcher>, prepared: PreparedOp) -> u64 {
        self.reap_finished();

        let op_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(OpShared {
            op_id,
            cancel: CancelToken::new(),
            state: Mutex::new(OpState::Running),
            dispatcher: dispatcher.clone(),
        });

        // Register before spawning so the thread's terminal bookkeeping
        // always finds its entry.
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.ops.insert(
                op_id,
                OpEntry {
                    shared: shared.clone(),
                    join: None,
                },
            );
        }

        let thread_shared = shared.clone();
        let body = prepared.0;
        let spawned = std::thread::Builder::new()
            .name(format!("windhawk-core op {op_id}"))
            .spawn(move || {
                let ctx = OpContext {
                    shared: thread_shared.clone(),
                };
                // The operation-thread top-frame panic firewall.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&ctx)));
                let event = match result {
                    Ok(Ok(value)) => OperationEvent::Completed { result: value },
                    Ok(Err(error)) => OperationEvent::Failed {
                        error: error.to_wire(),
                    },
                    Err(panic) => {
                        let message = panic_message(&panic);
                        thread_shared.dispatcher.log(
                            LogLevel::Error,
                            format!("operation {op_id} panicked: {message}"),
                        );
                        OperationEvent::Failed {
                            error: CoreError::internal(format!("operation panicked: {message}"))
                                .to_wire(),
                        }
                    }
                };
                thread_shared.transition_terminal(&event);
            });

        match spawned {
            Ok(handle) => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = inner.ops.get_mut(&op_id) {
                    entry.join = Some(handle);
                }
            }
            Err(e) => {
                // Spawn failure: terminate the operation ourselves; the
                // caller already received the operation id.
                shared.transition_terminal(&OperationEvent::Failed {
                    error: CoreError::internal(format!("failed to spawn operation thread: {e}"))
                        .to_wire(),
                });
            }
        }

        op_id
    }

    /// Signal cancellation (`WhCoreCancel`): true if the operation was found
    /// and signaled, false for unknown or terminal ids (a harmless no-op).
    pub fn cancel(&self, op_id: u64) -> bool {
        let shared = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            match inner.ops.get(&op_id) {
                Some(entry) if !entry.shared.is_terminal() => Some(entry.shared.clone()),
                _ => None,
            }
        };
        match shared {
            // Cancel outside the registry lock: hooks run on the
            // canceling thread and must not run under a rank-3 lock.
            Some(shared) => {
                shared.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Cancel everything and join every operation thread (destroy steps 2
    /// and 3). Each operation posts its terminal event - `CANCELED` or, if
    /// it won the race, its real result.
    pub fn cancel_all_and_join(&self) {
        let (live, finished) = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                std::mem::take(&mut inner.ops),
                std::mem::take(&mut inner.finished),
            )
        };
        for entry in live.values() {
            entry.shared.cancel.cancel();
        }
        for (_, entry) in live {
            if let Some(join) = entry.join {
                let _ = join.join();
            }
        }
        for join in finished {
            let _ = join.join();
        }
    }

    /// Move terminal entries' join handles aside and join the (already
    /// exited) threads, keeping the registry bounded on long-lived
    /// sessions.
    fn reap_finished(&self) {
        let to_join = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let terminal_ids: Vec<u64> = inner
                .ops
                .iter()
                .filter(|(_, e)| e.shared.is_terminal())
                .map(|(id, _)| *id)
                .collect();
            for id in terminal_ids {
                if let Some(entry) = inner.ops.remove(&id)
                    && let Some(join) = entry.join
                {
                    inner.finished.push(join);
                }
            }
            std::mem::take(&mut inner.finished)
        };
        for join in to_join {
            let _ = join.join();
        }
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
