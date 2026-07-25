//! The IPC bridge: the `wh_ipc` Tauri command, the injected [`BridgeCtx`]
//! handlers reach the core and the emit sink through, and the drive loop that
//! turns a handler [`Outcome`] into an emitted envelope. The bridge owns the
//! default failure shaping - it is the one place a propagated handler `Err`
//! becomes the standard error `reply`, making the "exactly one reply per
//! messageWithReply" invariant total.

use std::sync::Arc;

use serde_json::Value;
use windhawk_core_host::{GatedCore, HostError, Session};

use crate::commands::app::announce_app_settings;
use crate::editor::Editor;
use crate::file_dialog::FileDialog;
use crate::ipc::dispatch;
use crate::ipc::emit_sink::EmitSink;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, FollowUp, HostEffect, Outcome};
use crate::ipc::reply;
use crate::logwindow::LogController;
use crate::pump::events::dispatch_event;
use crate::pump::ops::{OpEntry, OpRegistry};
use crate::theme::NativeThemeControl;

/// The single injected context every handler runs against: the stateless
/// [`GatedCore`] (for the session-free `parseModSource`), the long-lived
/// [`Session`], and the [`EmitSink`]. Held in Tauri managed state and passed to
/// each handler rather than reached through `AppHandle` ad hoc - the seam that
/// makes the headless tests possible (a test fills it with a recording sink and
/// drives handlers directly).
///
/// `Clone` is cheap (every field is an `Arc` or `Copy`), so the `wh_ipc` command
/// clones it out of managed state and moves the clone onto a worker thread; that
/// needs `Send + Sync`, which the host's `GatedCore`/`Session` guarantee and the
/// `EmitSink` bound requires.
#[derive(Clone)]
pub struct BridgeCtx {
    pub(crate) core: Arc<GatedCore>,
    pub(crate) session: Arc<Session>,
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
    /// The launch-into-VSCode environment: the shared workspace manager and
    /// VSCodium launch seam the `commands/dev/` handlers and the
    /// startup/`deleteMod` sweep reach. Always present - `run` wires it and
    /// development is always on for the native build, so there is no
    /// editor-less mode.
    pub(crate) editor: Arc<Editor>,
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
    pub fn new(
        core: Arc<GatedCore>,
        session: Arc<Session>,
        emit: Arc<dyn EmitSink>,
        log: Arc<dyn LogController>,
        editor: Arc<Editor>,
        theme: Arc<dyn NativeThemeControl>,
        file_dialog: Arc<dyn FileDialog>,
    ) -> BridgeCtx {
        BridgeCtx {
            core,
            session,
            emit,
            ops: OpRegistry::new(),
            log,
            editor,
            theme,
            file_dialog,
        }
    }

    /// Record a started async op against its core op-id and replay any events that
    /// arrived before this call (the register/event race, [`OpRegistry`]). The
    /// `message_id` is the originating `messageWithReply` id the terminal reply
    /// echoes (`0` for an internal background op). Used by the async handlers (via
    /// the bridge) and the startup refresh.
    pub(crate) fn register_async(
        &self,
        op_id: u64,
        command: String,
        message_id: i64,
        kind: AsyncKind,
        context: Value,
    ) {
        let entry = OpEntry {
            command,
            message_id,
            kind,
            context,
            cancel: Some(self.session.cancel_token(op_id)),
        };
        for event_json in self.ops.register(op_id, entry) {
            self.dispatch_event(op_id, &event_json);
        }
    }

    /// Route one core operation event to its op (the pump thread and the
    /// register-replay path both call this). The dispatcher's two seams are bound
    /// to this context here: the composite follow-up reaches the host through
    /// `session.invoke` / `core.invoke_stateless` by the [`FollowUp`]'s
    /// `stateless` flag, and a named [`HostEffect`] is performed against the
    /// context the dispatcher deliberately does not hold.
    pub(crate) fn dispatch_event(&self, op_id: u64, event_json: &str) {
        let follow_up = |request: &FollowUp| -> Result<Value, HostError> {
            if request.stateless {
                self.core.invoke_stateless(request.command, &request.params)
            } else {
                self.session.invoke(request.command, &request.params)
            }
        };
        let effect = |effect: HostEffect| match effect {
            HostEffect::AppSettingsChanged => announce_app_settings(self),
        };
        dispatch_event(
            &self.ops,
            self.emit.as_ref(),
            self.log.as_ref(),
            &follow_up,
            &effect,
            op_id,
            event_json,
        );
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
#[tauri::command]
pub fn wh_log_stop_capture(ctx: tauri::State<'_, BridgeCtx>) {
    ctx.log.stop_capture();
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
                op.op_id,
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
