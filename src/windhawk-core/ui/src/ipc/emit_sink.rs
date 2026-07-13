//! [`EmitSink`]: the one seam every inbound envelope (a synchronous `reply`, an
//! async terminal/progress, a profile-watcher `event`) is written through. The
//! production impl is the SINGLE `AppHandle::emit("wh-ipc", ...)` call site; a
//! recording impl backs the headless dispatch tests, so the routing is
//! exercisable with no Tauri loop.

use crate::ipc::envelope::Envelope;

/// Writes an inbound [`Envelope`] toward the front-end. `Send + Sync` so the
/// context that holds it can cross to a worker thread (the async pump and the
/// composite follow-up run off the Tauri loop).
pub trait EmitSink: Send + Sync {
    /// Deliver one envelope to the front-end (the `wh-ipc` channel in production).
    fn emit(&self, envelope: Envelope);
}

/// The production sink: the one place that emits on the `wh-ipc` Tauri event
/// channel, which the front-end's bootstrap re-injects into its `window`
/// `message` pipeline.
pub struct AppHandleSink {
    app: tauri::AppHandle,
}

impl AppHandleSink {
    pub fn new(app: tauri::AppHandle) -> AppHandleSink {
        AppHandleSink { app }
    }
}

impl EmitSink for AppHandleSink {
    fn emit(&self, envelope: Envelope) {
        use tauri::Emitter;
        // Best-effort: a closed window (the app is shutting down) is the only
        // expected failure and is not actionable, so it is logged, not propagated.
        if let Err(error) = self.app.emit("wh-ipc", envelope) {
            eprintln!("windhawk-ui: failed to emit wh-ipc envelope: {error}");
        }
    }
}
