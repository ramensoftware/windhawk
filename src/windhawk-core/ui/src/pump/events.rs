//! The event dispatcher: one generic router with NO per-command `match`. For
//! each `(op_id, event_json)` it runs the host's [`classify_event`], looks the
//! op up in the [`OpRegistry`], and acts on the registered [`AsyncKind`] - the
//! progress mapper for events; a terminal `Shaped` shaper, a `Composite`
//! follow-up-then-merge, or an `Internal` side effect for the reply. The
//! per-command knowledge lives in the handler that built the `AsyncKind`, never
//! here; the `failed -> WireError` decode lives ONCE in the host's
//! `classify_event`, not here.
//!
//! The two impure steps are reached through injected seams, so the routing is
//! exercisable headless against a recording [`EmitSink`] with no Tauri loop: the
//! composite's follow-up core call between `follow_up` and `merge` (an
//! `Fn(&FollowUp) -> Result<Value, HostError>` - the host `Session`/`GatedCore`
//! invoke in production, a canned result in tests), and the [`HostEffect`] a
//! progress event names (an `Fn(HostEffect)` the bridge performs against its
//! context, recorded in tests).

use serde_json::Value;
use windhawk_core_host::{EventClass, HostError, classify_event};

use crate::ipc::emit_sink::EmitSink;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{Completion, FollowUp, HostEffect, Terminal};
use crate::ipc::reply;
use crate::logwindow::LogController;
use crate::pump::ops::{OpEntry, OpRegistry};

/// Route one core operation event to the op's registered handling. An event for an
/// op not yet registered is buffered (the register/event race, [`OpRegistry`]); a
/// malformed event JSON is logged and dropped (it cannot be a terminal we owe a
/// reply for, since it did not decode).
pub fn dispatch_event(
    ops: &OpRegistry,
    emit: &dyn EmitSink,
    log: &dyn LogController,
    follow_up: &dyn Fn(&FollowUp) -> Result<Value, HostError>,
    effect: &dyn Fn(HostEffect),
    op_id: u64,
    event_json: &str,
) {
    let class = match classify_event(event_json) {
        Ok(class) => class,
        Err(error) => {
            eprintln!("windhawk-ui: undecodable operation event for op {op_id}: {error}");
            return;
        }
    };

    match class {
        EventClass::Progress(op_event) => match ops.kind(op_id) {
            Some(kind) => {
                // Registered with a progress mapper: emit its event envelopes. The
                // common case (no mapper) ignores progress.
                if let Some(mapper) = kind.progress {
                    for envelope in mapper(&op_event) {
                        emit.emit(envelope);
                    }
                }
                // A progress event that marks a host-state change names its
                // effect, which the bridge performs - so the change is announced
                // as it happens rather than when the whole op ends.
                if let Some(mapper) = kind.effect
                    && let Some(named) = mapper(&op_event)
                {
                    effect(named);
                }
            }
            None => ops.buffer(op_id, event_json.to_owned()),
        },
        EventClass::Completed(value) => match ops.take(op_id) {
            Some(entry) => handle_terminal(emit, log, follow_up, &entry, Ok(value)),
            None => ops.buffer(op_id, event_json.to_owned()),
        },
        EventClass::Failed(wire) => match ops.take(op_id) {
            Some(entry) => {
                handle_terminal(emit, log, follow_up, &entry, Err(HostError::wire(wire)))
            }
            None => ops.buffer(op_id, event_json.to_owned()),
        },
    }
}

/// Turn an op's terminal outcome into its one reply (or, for an internal op,
/// its side effect), per the op's [`Terminal`]. A failed terminal is ALSO
/// offered to the log controller generically (the compiler-output surface): the
/// controller decides whether it is a local-compile failure worth surfacing, so
/// the dispatcher keeps no per-command match.
fn handle_terminal(
    emit: &dyn EmitSink,
    log: &dyn LogController,
    follow_up: &dyn Fn(&FollowUp) -> Result<Value, HostError>,
    entry: &OpEntry,
    outcome: Result<Value, HostError>,
) {
    if let Err(error) = &outcome {
        log.report_op_failure(&entry.command, error);
    }
    match entry.kind.terminal {
        Terminal::Shaped(shaper) => {
            // Capture the terminal error before `outcome` is moved into the shaper,
            // then attach it to the (failure-shaped) reply so the front-end can
            // surface it generically. The shaper stays a pure success/failure
            // projection; `report_op_failure` above still owns the compiler-output
            // surface, and the front-end skips COMPILER_FAILED here to avoid double
            // surfacing.
            let error = outcome.as_ref().err().map(reply::error_object);
            let mut data = shaper(outcome, &entry.context);
            if let Some(error) = error {
                reply::attach_error_object(&mut data, error);
            }
            emit_reply(emit, entry, data);
        }
        Terminal::Composite(completion) => {
            let data = run_composite(follow_up, &completion, &entry.context, outcome);
            emit_reply(emit, entry, data);
        }
        Terminal::Internal(handler) => handler(outcome, &entry.context, follow_up),
    }
}

/// A composite's terminal: on success, run the one follow-up call and merge; on a
/// terminal failure OR a follow-up that itself errors, the command's failure
/// shaper (the `follow_up`/`merge` are not consulted).
fn run_composite(
    follow_up: &dyn Fn(&FollowUp) -> Result<Value, HostError>,
    completion: &Completion,
    context: &Value,
    outcome: Result<Value, HostError>,
) -> Value {
    let completed = match outcome {
        Ok(completed) => completed,
        Err(error) => {
            eprintln!("windhawk-ui: composite terminal failed: {error}");
            let mut data = (completion.on_failure)(context);
            reply::attach_error(&mut data, &error);
            return data;
        }
    };
    let request = (completion.follow_up)(&completed, context);
    match follow_up(&request) {
        Ok(result) => (completion.merge)(&completed, &result, context),
        Err(error) => {
            eprintln!(
                "windhawk-ui: composite follow-up '{}' failed: {error}",
                request.command
            );
            let mut data = (completion.on_failure)(context);
            reply::attach_error(&mut data, &error);
            data
        }
    }
}

fn emit_reply(emit: &dyn EmitSink, entry: &OpEntry, data: Value) {
    emit.emit(Envelope::reply(
        entry.command.clone(),
        entry.message_id,
        data,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::envelope::EnvelopeType;
    use crate::ipc::outcome::{AsyncKind, Completion};
    use crate::logwindow::NoopLogController;
    use crate::pump::ops::OpEntry;
    use crate::pump::test_support::Recorder;
    use serde_json::json;
    use windhawk_core_protocol::OperationEvent;

    /// A follow-up seam returning a canned result, so the composite routing is
    /// covered with no session.
    fn canned(result: Value) -> impl Fn(&FollowUp) -> Result<Value, HostError> {
        move |_fu: &FollowUp| Ok(result.clone())
    }

    /// A follow-up seam that always errors (the non-composite tests never reach it).
    fn failing() -> impl Fn(&FollowUp) -> Result<Value, HostError> {
        |_fu: &FollowUp| Err(HostError::decode("no follow-up".to_owned()))
    }

    /// An effect seam for the ops that name no host effect: reaching it is the bug.
    fn no_effect() -> impl Fn(HostEffect) {
        |effect: HostEffect| panic!("this op names no host effect, got {effect:?}")
    }

    /// An effect seam that records what it was asked to perform.
    #[derive(Default)]
    struct EffectRecorder {
        performed: std::cell::RefCell<Vec<HostEffect>>,
    }

    impl EffectRecorder {
        fn seam(&self) -> impl Fn(HostEffect) + '_ {
            |effect: HostEffect| self.performed.borrow_mut().push(effect)
        }

        fn take(&self) -> Vec<HostEffect> {
            std::mem::take(&mut self.performed.borrow_mut())
        }
    }

    fn entry(command: &str, message_id: i64, kind: AsyncKind, context: Value) -> OpEntry {
        OpEntry {
            command: command.to_owned(),
            message_id,
            kind,
            context,
            // The dispatcher never invokes `cancel`; cancel is covered against a
            // real session in the smoke. A token needs a session to construct.
            cancel: None,
        }
    }

    // --- Shaped terminal --------------------------------------------------

    fn shaped(outcome: Result<Value, HostError>, ctx: &Value) -> Value {
        let mod_id = ctx.get("modId").cloned().unwrap_or(Value::Null);
        match outcome {
            Ok(v) => json!({ "ok": v, "modId": mod_id }),
            Err(_) => json!({ "ok": null, "modId": mod_id }),
        }
    }

    fn shaped_kind() -> AsyncKind {
        AsyncKind {
            terminal: Terminal::Shaped(shaped),
            progress: None,
            effect: None,
        }
    }

    #[test]
    fn completed_runs_the_shaper_and_emits_one_reply() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(7, entry("demo", 42, shaped_kind(), json!({ "modId": "m" })));

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            7,
            &completed(json!({ "n": 1 })),
        );

        let emitted = rec.take();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].kind, EnvelopeType::Reply);
        assert_eq!(emitted[0].command, "demo");
        assert_eq!(emitted[0].message_id, Some(42));
        assert_eq!(emitted[0].data, json!({ "ok": { "n": 1 }, "modId": "m" }));
        // The op is removed by the terminal: a second take finds nothing.
        assert!(ops.take(7).is_none());
    }

    #[test]
    fn failed_runs_the_shapers_failure_branch() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(1, entry("demo", 1, shaped_kind(), json!({ "modId": "m" })));

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            1,
            &failed("CANCELED", "stop"),
        );

        let emitted = rec.take();
        assert_eq!(emitted.len(), 1);
        // The failure-shaped reply now also carries the error object the front-end
        // surfaces generically (the shaper's own shape is unchanged underneath).
        assert_eq!(
            emitted[0].data,
            json!({ "ok": null, "modId": "m", "error": { "code": "CANCELED", "message": "stop" } })
        );
    }

    // --- progress ---------------------------------------------------------

    fn progress_mapper(event: &OperationEvent) -> Vec<Envelope> {
        match event {
            OperationEvent::Progress { payload } => vec![Envelope::event("prog", payload.clone())],
            OperationEvent::Installing => vec![Envelope::event("inst", json!({}))],
            _ => vec![],
        }
    }

    #[test]
    fn progress_then_terminal_emits_events_then_reply_in_order() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(
            5,
            entry(
                "demo",
                9,
                AsyncKind {
                    terminal: Terminal::Shaped(shaped),
                    progress: Some(progress_mapper),
                    effect: None,
                },
                json!({}),
            ),
        );

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            5,
            &progress(40),
        );
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            5,
            &installing(),
        );
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            5,
            &completed(json!(true)),
        );

        let emitted = rec.take();
        assert_eq!(emitted.len(), 3);
        assert_eq!(emitted[0].command, "prog");
        assert_eq!(emitted[0].data, json!({ "progress": 40 }));
        assert_eq!(emitted[1].command, "inst");
        assert_eq!(emitted[2].kind, EnvelopeType::Reply);
    }

    // --- host effects -----------------------------------------------------

    /// An effect mapper naming an effect for one distinguishing payload only, so
    /// the test can tell a mapped progress event from an unmapped one.
    fn effect_mapper(event: &OperationEvent) -> Option<HostEffect> {
        match event {
            OperationEvent::Progress { payload } if payload["progress"] == json!(100) => {
                Some(HostEffect::AppSettingsChanged)
            }
            _ => None,
        }
    }

    #[test]
    fn a_progress_event_names_its_effect_to_the_seam() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        let effects = EffectRecorder::default();
        ops.register(
            3,
            entry(
                "importUserData",
                11,
                AsyncKind {
                    terminal: Terminal::Shaped(shaped),
                    progress: Some(progress_mapper),
                    effect: Some(effect_mapper),
                },
                json!({}),
            ),
        );

        // A progress event the mapper does not name leaves the seam untouched; the
        // event envelopes are emitted either way.
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &effects.seam(),
            3,
            &progress(40),
        );
        assert!(effects.take().is_empty());
        assert_eq!(rec.take().len(), 1);

        // The named one reaches the seam, while the op stays registered (a progress
        // event does not end it) so its terminal still produces the one reply.
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &effects.seam(),
            3,
            &progress(100),
        );
        assert_eq!(effects.take(), vec![HostEffect::AppSettingsChanged]);
        assert_eq!(rec.take().len(), 1);

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &effects.seam(),
            3,
            &completed(json!(true)),
        );
        let emitted = rec.take();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].kind, EnvelopeType::Reply);
        // The terminal is not offered to the effect mapper: an effect can never
        // stand in for the reply.
        assert!(effects.take().is_empty());
    }

    // --- composite --------------------------------------------------------

    fn comp_follow_up(_completed: &Value, ctx: &Value) -> FollowUp {
        FollowUp {
            command: "listInstalledMods",
            params: ctx.clone(),
            stateless: false,
        }
    }
    fn comp_merge(completed: &Value, follow_up: &Value, _ctx: &Value) -> Value {
        json!({ "completed": completed, "followUp": follow_up })
    }
    fn comp_failure(_ctx: &Value) -> Value {
        json!({ "mods": null })
    }

    fn composite_kind() -> AsyncKind {
        AsyncKind {
            terminal: Terminal::Composite(Completion {
                follow_up: comp_follow_up,
                merge: comp_merge,
                on_failure: comp_failure,
            }),
            progress: None,
            effect: None,
        }
    }

    #[test]
    fn composite_runs_follow_up_then_merge() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(
            2,
            entry("getRepositoryMods", 3, composite_kind(), json!({})),
        );

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &canned(json!({ "mods": {} })),
            &no_effect(),
            2,
            &completed(json!({ "c": 1 })),
        );

        let emitted = rec.take();
        assert_eq!(emitted.len(), 1);
        assert_eq!(
            emitted[0].data,
            json!({ "completed": { "c": 1 }, "followUp": { "mods": {} } })
        );
    }

    #[test]
    fn composite_failed_terminal_uses_on_failure() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(
            2,
            entry("getRepositoryMods", 3, composite_kind(), json!({})),
        );

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &canned(json!({})),
            &no_effect(),
            2,
            &failed("REPO_UNREACHABLE", "down"),
        );

        assert_eq!(
            rec.take()[0].data,
            json!({ "mods": null, "error": { "code": "REPO_UNREACHABLE", "message": "down" } })
        );
    }

    #[test]
    fn composite_follow_up_error_uses_on_failure() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();
        ops.register(
            2,
            entry("getRepositoryMods", 3, composite_kind(), json!({})),
        );

        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            2,
            &completed(json!({ "c": 1 })),
        );

        // The follow-up's own error is attached (a no-wire decode -> INTERNAL, which
        // also carries a #[track_caller] origin location).
        let data = &rec.take()[0].data;
        assert_eq!(data["mods"], json!(null));
        assert_eq!(data["error"]["code"], json!("INTERNAL"));
        assert_eq!(data["error"]["message"], json!("no follow-up"));
    }

    // --- the register/event race buffer ----------------------------------

    #[test]
    fn event_before_register_is_buffered_and_replayed() {
        let ops = OpRegistry::new();
        let rec = Recorder::default();

        // Terminal arrives before the op is registered: buffered, nothing emitted.
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &failing(),
            &no_effect(),
            8,
            &completed(json!({ "n": 9 })),
        );
        assert!(rec.take().is_empty());

        // Registering returns the buffered event; the registrant replays it.
        let buffered = ops.register(
            8,
            entry("demo", 100, shaped_kind(), json!({ "modId": "x" })),
        );
        assert_eq!(buffered.len(), 1);
        for ev in &buffered {
            dispatch_event(
                &ops,
                &rec,
                &NoopLogController,
                &failing(),
                &no_effect(),
                8,
                ev,
            );
        }

        let emitted = rec.take();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].message_id, Some(100));
        assert_eq!(emitted[0].data, json!({ "ok": { "n": 9 }, "modId": "x" }));
    }

    // --- helpers ----------------------------------------------------------

    fn completed(result: Value) -> String {
        json!({ "type": "completed", "result": result }).to_string()
    }
    fn failed(code: &str, message: &str) -> String {
        json!({ "type": "failed", "error": { "code": code, "message": message } }).to_string()
    }
    fn progress(percent: i64) -> String {
        json!({ "type": "progress", "payload": { "progress": percent } }).to_string()
    }
    fn installing() -> String {
        json!({ "type": "installing" }).to_string()
    }
}
