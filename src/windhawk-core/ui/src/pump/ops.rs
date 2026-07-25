//! The op-id registry: the ONE owner of async-op correlation. Against each core
//! op-id it holds the originating `(command, messageId)`, the per-command
//! [`AsyncKind`], the captured `context`, and the [`CancelToken`] - so cancel
//! bookkeeping is not a second map. Both the bridge (which starts ops) and the
//! pump (which routes their events) reach it through one `Arc<OpRegistry>`
//! handle.
//!
//! Register/event ordering: the core never fires an op's event on the thread
//! inside `invoke_async`, but it MAY fire it on another thread before the
//! bridge - on its own worker - records the op. So an event for an unregistered
//! op is BUFFERED here; [`OpRegistry::register`] returns the buffered events
//! for the registrant to dispatch, making the early-terminal race impossible to
//! lose rather than merely improbable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use windhawk_core_host::CancelToken;

use crate::ipc::outcome::AsyncKind;

/// One registered async op: its originating correlation, per-command knowledge,
/// captured context, and cancel handle.
pub struct OpEntry {
    pub command: String,
    /// The originating `messageWithReply` id the terminal reply echoes. `0` for an
    /// internal background op (the startup refresh), whose `Terminal::Internal`
    /// emits no reply, so the id is unused.
    pub message_id: i64,
    pub kind: AsyncKind,
    pub context: Value,
    /// The op's cancel handle. Always `Some` in production (the bridge binds it to
    /// the op-id); `None` only in the headless dispatcher tests, which store but
    /// never invoke it (a [`CancelToken`] needs a real session to construct).
    pub cancel: Option<CancelToken>,
}

#[derive(Default)]
struct Inner {
    ops: HashMap<u64, OpEntry>,
    /// Events that arrived before their op was registered (the register/event
    /// race); drained by [`OpRegistry::register`].
    pending: HashMap<u64, Vec<String>>,
}

/// The single owner of op correlation (and cancel bookkeeping). Internally
/// `Mutex`-guarded so the bridge's worker threads and the pump thread share it.
#[derive(Default)]
pub struct OpRegistry {
    inner: Mutex<Inner>,
}

impl OpRegistry {
    pub fn new() -> Arc<OpRegistry> {
        Arc::new(OpRegistry::default())
    }

    /// Record a started op. Returns any events that arrived before this call (the
    /// race window); the caller dispatches them, by which point the op is present.
    pub fn register(&self, op_id: u64, entry: OpEntry) -> Vec<String> {
        let mut inner = self.lock();
        inner.ops.insert(op_id, entry);
        inner.pending.remove(&op_id).unwrap_or_default()
    }

    /// Buffer an event whose op is not yet registered (the pump calls this on a
    /// miss); [`OpRegistry::register`] later returns it.
    pub fn buffer(&self, op_id: u64, event_json: String) {
        self.lock()
            .pending
            .entry(op_id)
            .or_default()
            .push(event_json);
    }

    /// Snapshot the [`AsyncKind`] of a registered op without removing it (a
    /// progress event does not end the op). `None` means the op is not registered
    /// (the caller buffers). `AsyncKind` is `Copy` - every field is a function
    /// pointer - so this clones nothing of the entry.
    pub fn kind(&self, op_id: u64) -> Option<AsyncKind> {
        self.lock().ops.get(&op_id).map(|entry| entry.kind)
    }

    /// Remove and return a registered op (a terminal ends it exactly once - whoever
    /// wins the take owns the terminal, so a rare concurrent dispatch cannot
    /// double-emit). `None` means not registered (the caller buffers).
    pub fn take(&self, op_id: u64) -> Option<OpEntry> {
        self.lock().ops.remove(&op_id)
    }

    /// Find the in-flight op for `command` and signal its cancellation, returning
    /// whether one was found and signaled (the `cancelUpdate` path, which targets
    /// the single in-flight `startUpdate`). The op stays registered: its CANCELED
    /// terminal still produces the command's reply.
    pub fn cancel_by_command(&self, command: &str) -> bool {
        let token = {
            let inner = self.lock();
            inner
                .ops
                .values()
                .find(|entry| entry.command == command)
                .and_then(|entry| entry.cancel.clone())
        };
        token.map(|t| t.cancel()).unwrap_or(false)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}
