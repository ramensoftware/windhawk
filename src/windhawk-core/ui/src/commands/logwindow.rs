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
pub fn show(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    ctx.log.show();
    Ok(Outcome::Done)
}
