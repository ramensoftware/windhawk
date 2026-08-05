//! Test-only helpers for the headless pump/registry tests: a recording
//! [`EmitSink`] that captures emitted envelopes so a test can assert what would
//! have reached the front-end, with no Tauri loop, and the uneventful
//! registration most of those tests set up with.

use std::sync::{Arc, Mutex};

use crate::ipc::emit_sink::EmitSink;
use crate::ipc::envelope::Envelope;
use crate::pump::ops::{OpEntry, OpRegistry, Registered};

/// Records the envelopes emitted through it (the headless stand-in for the
/// `wh-ipc` channel).
#[derive(Clone, Default)]
pub struct Recorder {
    emitted: Arc<Mutex<Vec<Envelope>>>,
}

impl EmitSink for Recorder {
    fn emit(&self, envelope: Envelope) {
        self.emitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(envelope);
    }
}

impl Recorder {
    /// Take and clear the recorded envelopes.
    pub fn take(&self) -> Vec<Envelope> {
        std::mem::take(&mut self.emitted.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Register an op the way a start with no swap in the middle of it does: under
/// the generation the registry has installed. Returns the buffered events, as
/// [`OpRegistry::register`] does. Panics on an orphan, which is a swap no
/// single-threaded test performed.
pub fn register(ops: &OpRegistry, op_id: u64, entry: OpEntry) -> Vec<(u64, String)> {
    match ops.register(ops.generation(), op_id, entry) {
        Registered::Replay(events) => events,
        Registered::Orphaned(_) => panic!("nothing swapped the session between these two calls"),
    }
}
