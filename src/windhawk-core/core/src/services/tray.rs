//! `services::tray`: `notifyTray`, a port of `services/trayProgram.ts`.
//! Mechanism only - when to call is front-end policy. It spawns `windhawk.exe`
//! detached with the action's flag and returns; a spawn failure is logged,
//! never fatal (the contract's `notifyTray` resolves to void).
//!
//! Deliberate parity note: the TS `TrayProgram` spawned (not detached) and
//! logged a warning when the tray's exit code was nonzero ("make sure it's
//! running"). The core uses the `spawn_detached` form for `notifyTray`, so that
//! exit-code warning is dropped - a fire-and-forget IPC ping to an
//! already-running tray, server-visible at most.

use std::path::Path;

use serde_json::Value;
use windhawk_core_ports::DetachedRequest;
use windhawk_core_protocol::{NotifyTrayParams, TrayAction};

use crate::callbacks::LogLevel;
use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::session::SessionInner;

/// `notifyTray`: spawn `windhawk.exe -<flag>` detached for the requested
/// action.
pub fn notify_tray(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: NotifyTrayParams = decode_params("notifyTray", params)?;
    let flag = match params.action {
        TrayAction::RestartBg => "-restart-bg",
        TrayAction::NewUpdatesFound => "-new-updates-found",
        TrayAction::AppSettingsChanged => "-app-settings-changed",
    };
    let program = Path::new(&session.storage().info().app_root_path).join("windhawk.exe");
    let request = DetachedRequest {
        program: program.to_string_lossy().into_owned(),
        raw_args: flag.to_owned(),
    };
    if let Err(e) = session.deps().processes.spawn_detached(&request) {
        session.log(
            LogLevel::Error,
            format!("Failed to notify the Windhawk tray: {}", e.message),
        );
    }
    Ok(Value::Null)
}
