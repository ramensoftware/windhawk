//! Test-only helpers for the headless pump/registry tests: a recording
//! [`EmitSink`] that captures emitted envelopes so a test can assert what would
//! have reached the front-end, with no Tauri loop.

use std::sync::{Arc, Mutex};

use crate::ipc::emit_sink::EmitSink;
use crate::ipc::envelope::Envelope;

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
