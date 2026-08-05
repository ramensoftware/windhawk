//! The async op pump: the op-id registry, the generic event dispatcher, the
//! profile watcher, and the startup catalog refresh. The bridge starts ops and
//! records them in [`ops::OpRegistry`]; a session's event callback feeds
//! `(generation, op_id, event_json)` to [`events::dispatch_event`], which routes
//! each event to the op's registered handling. Per-command knowledge lives in the
//! `commands/` handler that built the `AsyncKind`, never here.
//!
//! The pump thread also owns the moments when the SESSION changes, because ending
//! an op needs the same three seams a terminal event needs (the emit sink, the log
//! controller, the follow-up invoke) and because doing it here makes a swap a
//! single-threaded critical section rather than three threads agreeing about
//! ordering. Whoever notices - a channel's reader thread, the elevation ladder,
//! the Retry - posts the work; this thread runs it, in order, between two events.

pub mod events;
pub mod ops;
pub mod profile_watch;
pub mod startup;

#[cfg(test)]
pub mod test_support;

use std::sync::mpsc::Receiver;

use crate::ipc::bridge::BridgeCtx;

/// What reaches the pump thread.
pub enum PumpMessage {
    /// One core operation event, stamped with the generation of the session that
    /// produced it: an op-id identifies an op only within one session, and two can
    /// be live at once (the retained local session and the broker's), so the pump
    /// routes on the pair.
    Event {
        generation: u64,
        op_id: u64,
        event_json: String,
    },
    /// Work that must run ON this thread, against the bridge context. The session
    /// swap and the op drain that goes with it are the only users, and what they
    /// need is exactly what this thread has.
    Deferred(Box<dyn FnOnce(&BridgeCtx) + Send>),
}

impl PumpMessage {
    pub fn deferred(work: impl FnOnce(&BridgeCtx) + Send + 'static) -> PumpMessage {
        PumpMessage::Deferred(Box::new(work))
    }
}

/// Drain the pump until every sender is gone (the app is exiting).
pub fn run(ctx: BridgeCtx, messages: Receiver<PumpMessage>) {
    while let Ok(message) = messages.recv() {
        match message {
            PumpMessage::Event {
                generation,
                op_id,
                event_json,
            } => {
                // Isolate a panic in one op's dispatch: without this a single
                // shaper bug would kill the pump thread, after which NO async
                // reply is ever emitted (every pending messageWithReply hangs).
                // The registry's locks recover from poisoning (into_inner), so the
                // shared state stays usable after a caught panic - which is what
                // makes AssertUnwindSafe sound here. The default panic hook still
                // prints the panic; we add which op it was.
                let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ctx.dispatch_event(generation, op_id, &event_json)
                }));
                if dispatched.is_err() {
                    eprintln!(
                        "windhawk-ui: event pump recovered from a panic dispatching op {op_id}"
                    );
                }
            }
            // The same isolation for the same reason: a swap that panics must not
            // take the pump - and with it every future reply - down with it.
            PumpMessage::Deferred(work) => {
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ctx)));
                if done.is_err() {
                    eprintln!("windhawk-ui: event pump recovered from a panic changing sessions");
                }
            }
        }
    }
}
