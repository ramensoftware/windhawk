//! App-settings handlers: the reads `getAppSettings` (the full settings object,
//! forwarded verbatim) and `getInitialAppSettings` (the `appUISettings` subset
//! with the forced `devModeOptOut`), and the write `updateAppSettings`
//! (`applyAppSettings`, the `setNewAppSettings` event, and the restart/notify
//! `notifyTray`). The reads mirror the extension's `try/catch` by representing
//! a core failure inline as an empty object; the write represents it as
//! `succeeded: false`.

use serde::Deserialize;
use serde_json::{Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    AppSettings, AppSettingsIntents, AppSettingsPatch, AppSettingsPatchParams, AppUpdateStatus,
    NotifyTrayParams, TrayAction,
};

use crate::commands::app_settings;
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::Outcome;
use crate::ipc::reply;
use crate::shape::app_ui::app_ui_settings;

/// `getAppSettings`: the full settings object for the settings screen, forwarded
/// untouched (a raw pass-through, so a field the core adds is never dropped). A
/// core error becomes an empty object.
pub fn get_app_settings(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let reply = match ctx.session.invoke("getAppSettings", &json!({})) {
        Ok(app_settings) => json!({ "appSettings": app_settings }),
        Err(error) => {
            eprintln!("windhawk-ui: getAppSettings failed: {error}");
            let mut data = json!({ "appSettings": {} });
            reply::attach_error(&mut data, &error);
            data
        }
    };
    Ok(Outcome::Reply(reply))
}

/// `getInitialAppSettings`: the `appUISettings` subset. `devModeOptOut` is the
/// real stored value (development is always on for the native build), so the
/// front-end shows the authoring affordances unless the user opted out. Any
/// failure building the subset degrades to an empty `appUISettings` (a
/// `Partial`), so a startup hiccup never breaks the shell.
pub fn get_initial_app_settings(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let reply = match app_settings(ctx) {
        Ok(settings) => json!({ "appUISettings": app_ui_settings_now(ctx, &settings) }),
        Err(error) => {
            eprintln!("windhawk-ui: getInitialAppSettings failed: {error}");
            let mut data = json!({ "appUISettings": {} });
            reply::attach_error(&mut data, &error);
            data
        }
    };
    Ok(Outcome::Reply(reply))
}

/// `updateAppSettings`: apply a `Partial<AppSettings>` patch (`applyAppSettings`),
/// then mirror the extension's post-apply work - re-read the settings, push the
/// recomputed `appUISettings` as a `setNewAppSettings` event, and notify the tray
/// for the restart/notify intent the apply reported. The reply echoes the patch that
/// was sent (`data.appSettings`) plus the `succeeded` flag; any failure along the
/// way leaves `succeeded: false` (the apply may already have written, exactly as the
/// extension's single `try` does). `previewAppSettingsEffects` is not invoked: the
/// shared front-end shows no restart-confirm prompt, so the extension's handler
/// applies directly.
pub fn update_app_settings(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let req: UpdateAppSettingsRequest = serde_json::from_value(data.clone())?;
    let patch: AppSettingsPatch = serde_json::from_value(req.app_settings.clone())?;
    let params = AppSettingsPatchParams { patch };
    let result = apply_and_announce(ctx, &params);
    let mut reply = json!({
        "appSettings": req.app_settings,
        "succeeded": result.is_ok(),
    });
    if let Err(error) = &result {
        eprintln!("windhawk-ui: updateAppSettings failed: {error}");
        reply::attach_error(&mut reply, error);
    }
    Ok(Outcome::Reply(reply))
}

/// Apply the patch and perform the extension's post-apply sequence in order:
/// `applyAppSettings` -> re-read settings -> emit `setNewAppSettings` -> notify the
/// tray for the reported intent. Any step's `HostError` aborts the rest (so a
/// re-read failure suppresses the event and notify, as the extension's single `try`
/// does) and surfaces as `succeeded: false`. The intent is read from the
/// `applyAppSettings` result, not previewed.
fn apply_and_announce(ctx: &BridgeCtx, params: &AppSettingsPatchParams) -> Result<(), HostError> {
    let intents: AppSettingsIntents = ctx.session.invoke_as("applyAppSettings", params)?;

    let new_settings: AppSettings = ctx.session.invoke_as("getAppSettings", &json!({}))?;
    ctx.emit.emit(new_app_settings_event(ctx, &new_settings));

    // Tray notification: restart wins over notify (the extension's if/else if),
    // mirroring updateAppSettings' behavior.
    if intents.requires_restart {
        ctx.session.invoke(
            "notifyTray",
            &NotifyTrayParams {
                action: TrayAction::RestartBg,
            },
        )?;
    } else if intents.requires_notify {
        ctx.session.invoke(
            "notifyTray",
            &NotifyTrayParams {
                action: TrayAction::AppSettingsChanged,
            },
        )?;
    }
    Ok(())
}

/// Build the `setNewAppSettings` event from a settings object: the recomputed
/// `appUISettings` the front-end's app-level indicators read. Single-sources the
/// event shape across `updateAppSettings` (the apply path) and the profile watcher.
fn new_app_settings_event(ctx: &BridgeCtx, settings: &AppSettings) -> Envelope {
    Envelope::event(
        "setNewAppSettings",
        json!({ "appUISettings": app_ui_settings_now(ctx, settings) }),
    )
}

/// Re-read the app settings and emit `setNewAppSettings`, so the front-end's
/// app-level indicators - the Windhawk-update badge and the logging state - refresh.
/// The profile watcher calls this on an external profile change (including the
/// startup catalog sync, which the core classifies as an external write), so the
/// app-update badge tracks the freshly-synced availability the same way the
/// extension's `_userProfileChanged` does. A read failure is logged and skipped.
pub(crate) fn emit_new_app_settings(ctx: &BridgeCtx) {
    match app_settings(ctx) {
        Ok(settings) => ctx.emit.emit(new_app_settings_event(ctx, &settings)),
        Err(error) => {
            eprintln!(
                "windhawk-ui: could not recompute appUISettings for setNewAppSettings: {error}"
            )
        }
    }
}

/// Project the current settings into the `appUISettings` subset (the
/// `getInitialAppSettings` reply and the `setNewAppSettings` event share this),
/// with the stored `devModeOptOut` reported as-is (development is always on for
/// the native build, so there is no override) and the cached update
/// availability.
fn app_ui_settings_now(ctx: &BridgeCtx, settings: &AppSettings) -> Value {
    let (update_available, update_available_bleeding_edge) = update_availability(ctx, settings);
    app_ui_settings(
        settings,
        settings.dev_mode_opt_out,
        update_available,
        update_available_bleeding_edge,
    )
}

/// The `updateAppSettings` envelope `data` (`{ appSettings }`). The front-end names
/// the patch `appSettings`, while the core param is `patch`
/// (`AppSettingsPatchParams`), so the data cannot deserialize straight into the
/// request DTO; this keeps the raw `appSettings` Value to echo in the reply and the
/// typed patch is decoded from it for the invoke.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAppSettingsRequest {
    app_settings: Value,
}

/// Read the cached update availability (`getAppUpdateStatus`), gated by
/// `disableUpdateCheck` exactly as the extension's `_getAppUISettings`: no check
/// means both flags are false; a status read failure degrades to false.
fn update_availability(ctx: &BridgeCtx, settings: &AppSettings) -> (bool, bool) {
    if settings.disable_update_check {
        return (false, false);
    }
    match ctx
        .session
        .invoke_as::<AppUpdateStatus, _>("getAppUpdateStatus", &json!({}))
    {
        Ok(status) => (
            status.update_available,
            status.update_available_bleeding_edge,
        ),
        Err(_) => (false, false),
    }
}
