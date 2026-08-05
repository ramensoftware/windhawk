//! The IPC bridge: the `wh_ipc` Tauri command, the injected [`BridgeCtx`]
//! handlers reach the core and the emit sink through, and the drive loop that
//! turns a handler [`Outcome`] into an emitted envelope. The bridge owns the
//! default failure shaping - it is the one place a propagated handler `Err`
//! becomes the standard error `reply`, making the "exactly one reply per
//! messageWithReply" invariant total.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use windhawk_core_host::{GatedCore, HostError, SessionApi, SessionApiExt};

use crate::broker::HANDOVER_REASON;
use crate::broker::ops::HostOps;
use crate::commands::app::announce_app_settings;
use crate::file_dialog::FileDialog;
use crate::ipc::dispatch;
use crate::ipc::emit_sink::EmitSink;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, FollowUp, HostEffect, Outcome, Started};
use crate::ipc::reply;
use crate::logwindow::LogController;
use crate::pump::events::{dispatch_event, fail_terminal};
use crate::pump::ops::{OpEntry, OpRegistry, Registered};
use crate::theme::NativeThemeControl;

/// The single injected context every handler runs against: the stateless
/// [`GatedCore`] (for the session-free `parseModSource`), the long-lived session
/// behind the [`SessionApi`] seam, and the [`EmitSink`]. Held in Tauri managed
/// state and passed to each handler rather than reached through `AppHandle` ad
/// hoc - the seam that makes the headless tests possible (a test fills it with a
/// recording sink and drives handlers directly).
///
/// `Clone` is cheap (every field is an `Arc` or `Copy`), so the `wh_ipc` command
/// clones it out of managed state and moves the clone onto a worker thread; that
/// needs `Send + Sync`, which the host's `GatedCore`/`SessionApi` guarantee and
/// the `EmitSink` bound requires.
#[derive(Clone)]
pub struct BridgeCtx {
    pub(crate) core: Arc<GatedCore>,
    /// The session the handlers invoke, held behind the seam rather than as a
    /// concrete `Session`: what is behind it is the caller's choice, and a handler
    /// cannot tell.
    pub(crate) session: Arc<dyn SessionApi>,
    pub(crate) emit: Arc<dyn EmitSink>,
    /// The single owner of async-op correlation. Shared by cloning the context:
    /// the `wh_ipc` workers (which start ops), the pump thread (which routes
    /// their events), and `cancelUpdate` all reach the one registry.
    pub(crate) ops: Arc<OpRegistry>,
    /// The log pane controller: the dispatch handlers reach it through `show`
    /// (`showLogOutput`/`showAdvancedDebugLogOutput`) and the event dispatcher
    /// routes every failed terminal through `report_op_failure` (the
    /// compiler-output surface). `NoopLogController` in the headless tests.
    pub(crate) log: Arc<dyn LogController>,
    /// The privileged host operations - the workspace preparation and editor
    /// launch, the mod-runtime seed, the UI data folder, the cross-session
    /// debug-output capture - behind the same swap point as the session, so a
    /// handler cannot tell whether they run here or in the elevated helper.
    pub(crate) host: Arc<dyn HostOps>,
    /// Whether the development tools are installed, from `getCoreInfo`'s UI path.
    /// A plain fact rather than a call: both processes read the same core info, and
    /// it is consulted on paths that have nothing to do with launching an editor
    /// (every local compile checks it), which is what would make a round trip for
    /// it wrong.
    pub(crate) dev_tools_installed: bool,
    /// Re-applies the native window theme (title bar, WebView2 surfaces) when the
    /// `updateAppSettings` handler changes the theme setting. `NoopThemeControl`
    /// in the headless tests (no window to theme).
    pub(crate) theme: Arc<dyn NativeThemeControl>,
    /// The native Save/Open pickers the user-data export/import handlers run around
    /// the core call (the host owns the archive file dialogs). A `Win32FileDialog`
    /// in production; a fake in the headless tests, which have no window.
    pub(crate) file_dialog: Arc<dyn FileDialog>,
}

impl BridgeCtx {
    /// Build a context with a fresh [`OpRegistry`]. The registry is shared across
    /// every clone of the returned context (it is an `Arc`), so `run` builds one
    /// context and clones it for managed state, the pump thread, and the watcher.
    // The wiring constructor for a context whose whole job is to hold the seams
    // apart: every parameter is one of them, and grouping them into a struct would
    // only move the same list somewhere the call site cannot read it.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Arc<GatedCore>,
        session: Arc<dyn SessionApi>,
        emit: Arc<dyn EmitSink>,
        log: Arc<dyn LogController>,
        host: Arc<dyn HostOps>,
        dev_tools_installed: bool,
        theme: Arc<dyn NativeThemeControl>,
        file_dialog: Arc<dyn FileDialog>,
    ) -> BridgeCtx {
        BridgeCtx {
            core,
            session,
            emit,
            ops: OpRegistry::new(),
            log,
            host,
            dev_tools_installed,
            theme,
            file_dialog,
        }
    }

    /// Start an async op, capturing everything about it that belongs to the session
    /// rather than to the command: the op-id, the generation it was started under,
    /// and its cancel handle.
    ///
    /// The generation is read BEFORE the start, so a swap anywhere in the window
    /// between here and [`BridgeCtx::register_async`] is one the registry can see.
    /// Reading it after would place the op with whichever session happened to be
    /// installed by then, which for an op the swap already declined to drain means
    /// no event can ever end it.
    ///
    /// The cancel handle is taken here for the same reason it is stamped here: a
    /// handle resolved through the seam later could be bound to a session that never
    /// issued the op-id.
    pub(crate) fn start_async<P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<Started, HostError> {
        let generation = self.ops.generation();
        let op_id = self.session.invoke_async(command, params)?;
        Ok(Started {
            op_id,
            generation,
            cancel: self.session.cancel_token(op_id),
        })
    }

    /// Record a started async op against its core op-id and replay any events that
    /// arrived before this call (the register/event race, [`OpRegistry`]). The
    /// `message_id` is the originating `messageWithReply` id the terminal reply
    /// echoes (`0` for an internal background op). Used by the async handlers (via
    /// the bridge) and the startup refresh.
    ///
    /// An op whose session was swapped out while it was starting is ENDED here
    /// rather than recorded, on the same terms as one the swap drained: the reply
    /// its `messageWithReply` is owed is the only thing that keeps the front-end
    /// from waiting on it forever.
    pub(crate) fn register_async(
        &self,
        start: Started,
        command: String,
        message_id: i64,
        kind: AsyncKind,
        context: Value,
    ) {
        let Started {
            op_id,
            generation,
            cancel,
        } = start;
        let entry = OpEntry {
            command,
            message_id,
            kind,
            context,
            cancel: Some(cancel),
        };
        // The buffered events come back with the generation each was produced under,
        // so a replay reaches this op only if it is the same session's.
        match self.ops.register(generation, op_id, entry) {
            Registered::Replay(events) => {
                for (generation, event_json) in events {
                    self.dispatch_event(generation, op_id, &event_json);
                }
            }
            Registered::Orphaned(entry) => fail_terminal(
                self.emit.as_ref(),
                self.log.as_ref(),
                &self.follow_up(),
                &entry,
                HostError::transport(HANDOVER_REASON.to_owned()),
            ),
        }
    }

    /// Route one core operation event to its op (the pump thread and the
    /// register-replay path both call this). The dispatcher's two seams are bound
    /// to this context here: the composite follow-up reaches the host through
    /// `session.invoke` / `core.invoke_stateless` by the [`FollowUp`]'s
    /// `stateless` flag, and a named [`HostEffect`] is performed against the
    /// context the dispatcher deliberately does not hold.
    pub(crate) fn dispatch_event(&self, generation: u64, op_id: u64, event_json: &str) {
        let effect = |effect: HostEffect| match effect {
            HostEffect::AppSettingsChanged => announce_app_settings(self),
        };
        dispatch_event(
            &self.ops,
            self.emit.as_ref(),
            self.log.as_ref(),
            &self.follow_up(),
            &effect,
            generation,
            op_id,
            event_json,
        );
    }

    /// Hand the op registry over to the session carrying `generation`, ending
    /// every op the outgoing session left in flight.
    ///
    /// An op-id is meaningful only to the session that issued it, so an op cannot
    /// survive the session it belongs to: no event will ever end it, and its
    /// `messageWithReply` would hang forever. Each one therefore runs the terminal
    /// path a `failed` event would have run, with `reason` as the failure - which
    /// the front-end already renders.
    ///
    /// Runs on the PUMP thread, which is what makes the drain and the install one
    /// operation rather than two a caller could interleave (the registry clears its
    /// buffered events and installs the incoming generation in the same step).
    pub(crate) fn hand_over_ops(&self, generation: u64, reason: &str) {
        for (_op_id, entry) in self.ops.drain_and_install(generation) {
            fail_terminal(
                self.emit.as_ref(),
                self.log.as_ref(),
                &self.follow_up(),
                &entry,
                HostError::transport(reason.to_owned()),
            );
        }
    }

    /// The composite follow-up seam: one invoke, against this context's core.
    fn follow_up(&self) -> impl Fn(&FollowUp) -> Result<Value, HostError> + '_ {
        |request: &FollowUp| {
            if request.stateless {
                self.core.invoke_stateless(request.command, &request.params)
            } else {
                self.session.invoke(request.command, &request.params)
            }
        }
    }
}

/// The outbound transport pipe: the front-end's selected transport forwards
/// each envelope here fire-and-forget. The returned promise is ignored -
/// replies come back out of band on the `wh-ipc` event channel (the
/// [`EmitSink`]), mirroring the VSCode webview model. The dispatch runs on a
/// blocking worker so a core invoke never blocks the webview thread.
#[tauri::command]
pub fn wh_ipc(envelope: Envelope, ctx: tauri::State<'_, BridgeCtx>) {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle_envelope(&ctx, envelope));
}

/// The backlog the log pane requests on first reveal: the retained tail of
/// captured `[WH] ` lines plus any compiler output written just before the pane
/// was revealed. An app command, not ACL-gated; the live stream after that
/// arrives on the `wh-log` event channel.
#[tauri::command]
pub fn wh_log_backlog(ctx: tauri::State<'_, BridgeCtx>) -> Vec<String> {
    ctx.log.backlog()
}

/// Stop the live `[WH]` DBWIN capture (the log pane's Close affordance).
/// Capture is scoped to while the pane is open because it contends for the
/// single-owner DBWIN buffer, so closing the pane releases it. An app command,
/// not ACL-gated.
///
/// Both halves: the pane's own `Local\` loop and the `Global\` one the elevated
/// helper runs for it. They start together in the `showLogOutput` handler
/// and stop together here.
///
/// On a blocking worker, like `wh_ipc` and for the same reason: a synchronous
/// `#[tauri::command]` runs on the event-loop thread, and both halves block -
/// the local one joins its capture thread, and the other is a round trip to a
/// helper that serves it from a thread pool the mod compiles share.
#[tauri::command]
pub fn wh_log_stop_capture(ctx: tauri::State<'_, BridgeCtx>) {
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ctx.log.stop_capture();
        ctx.host.dbwin_stop();
    });
}

/// Dispatch one inbound envelope and route its [`Outcome`] to the [`EmitSink`].
/// Public so the headless tests drive it directly with a recording sink (no Tauri
/// loop). On a propagated handler `Err`, applies the default shaper to emit the
/// standard error `reply` (for a `messageWithReply`) or logs it (for a `message`
/// with no reply channel) - the one-reply invariant backstop.
pub fn handle_envelope(ctx: &BridgeCtx, envelope: Envelope) {
    // A reply is only possible when the front-end correlated the request with a
    // messageId (a `messageWithReply`); a `message` is fire-and-forget. The total
    // dispatch uses this to decide whether an out-of-scope / unknown command emits
    // an error reply or only logs.
    let expects_reply = envelope.message_id.is_some();
    match dispatch::dispatch(ctx, &envelope.command, &envelope.data, expects_reply) {
        Ok(Outcome::Reply(data)) => emit_reply(ctx, &envelope, data),
        Ok(Outcome::Async(op)) => {
            // No reply now: the op's terminal event produces it (the pump), echoing
            // this `messageWithReply`'s id. An async command is always a
            // messageWithReply, so the id is present (default 0 only defends a
            // contract violation).
            ctx.register_async(
                op.start,
                envelope.command.clone(),
                envelope.message_id.unwrap_or(0),
                op.kind,
                op.context,
            );
        }
        Ok(Outcome::Done) => {}
        Err(error) => match envelope.message_id {
            Some(message_id) => ctx.emit.emit(Envelope::reply(
                envelope.command,
                message_id,
                reply::default_shaper(Err(error)),
            )),
            None => eprintln!(
                "windhawk-ui: '{}' failed with no reply channel: {error}",
                envelope.command
            ),
        },
    }
}

/// Emit a handler's `Reply` data as a `reply` envelope. A `Reply` outcome for an
/// envelope without a `messageId` is a contract mismatch (a reply-type command
/// sent fire-and-forget); log rather than fabricate a correlation id.
fn emit_reply(ctx: &BridgeCtx, envelope: &Envelope, data: serde_json::Value) {
    match envelope.message_id {
        Some(message_id) => {
            ctx.emit
                .emit(Envelope::reply(envelope.command.clone(), message_id, data));
        }
        None => eprintln!(
            "windhawk-ui: handler produced a reply for '{}' which carries no messageId",
            envelope.command
        ),
    }
}
