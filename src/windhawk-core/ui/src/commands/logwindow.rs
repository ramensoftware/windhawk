//! The log-output affordances: `showLogOutput` and `showAdvancedDebugLogOutput`
//! reveal the native log pane that tails Windhawk's live `[WH] ` debug output.
//! Both are fire-and-forget `message`s (no `messageId`), so the handler reveals
//! the pane and returns `Outcome::Done`. The extension drives one
//! `WindhawkLogOutput` singleton from both entry points, so there is no
//! separate verbosity to model - they reveal the same pane.

use serde_json::Value;
use windhawk_core_host::HostError;

use crate::ipc::bridge::BridgeCtx;
use crate::ipc::outcome::Outcome;

/// Reveal the log pane. Used by both `showLogOutput` and
/// `showAdvancedDebugLogOutput`.
///
/// Capture has two halves and they start together: the pane's own per-session
/// `Local\` loop, and the cross-session `Global\` one that needs a privilege this
/// process may not have and so belongs to the host operations. Only the local
/// half gates the reveal - a pane that waited for the elevated helper would be
/// un-openable in degraded mode, which is exactly when someone wants to read it.
pub fn show(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    ctx.log.show();
    ctx.host.dbwin_start();
    Ok(Outcome::Done)
}
