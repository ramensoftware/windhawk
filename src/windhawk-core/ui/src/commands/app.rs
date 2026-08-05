//! App-settings handlers: the reads `getAppSettings` (the full settings object,
//! forwarded verbatim) and `getInitialAppSettings` (the `appUISettings` subset
//! with the forced `devModeOptOut`), and the write `updateAppSettings`
//! (`applyAppSettings`, the `setNewAppSettings` event, and the restart/notify
//! `notifyTray`). The reads mirror the extension's `try/catch` by representing
//! a core failure inline as an empty object; the write represents it as
//! `succeeded: false`.
//!
//! The write's announcement half is also reachable on its own
//! ([`announce_app_settings`]), for the settings changes this host does not drive:
//! a user-data import applies them inside the core, and the front-end and the
//! native shell would otherwise show the old ones.

use serde::Deserialize;
use serde_json::{Value, json};
use windhawk_core_host::{HostError, SessionApiExt};
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
use crate::shape::webview_ipc::{
    AppUiSettings, GetInitialAppSettingsReply, SetNewAppSettings, UpdateAppSettingsReply,
    WEBVIEW_IPC_CONTRACT_VERSION, to_wire,
};
use crate::shell::ThemeSetting;

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
        Ok(settings) => serde_json::to_value(GetInitialAppSettingsReply {
            contract_version: WEBVIEW_IPC_CONTRACT_VERSION.to_owned(),
            app_ui_settings: app_ui_settings_now(ctx, &settings),
        })
        .expect("GetInitialAppSettingsReply serializes"),
        Err(error) => {
            eprintln!("windhawk-ui: getInitialAppSettings failed: {error}");
            // Still carry contractVersion so a settings hiccup does not read as a
            // contract mismatch on the handshake; appUISettings degrades to an empty
            // Partial.
            let mut data = json!({
                "contractVersion": WEBVIEW_IPC_CONTRACT_VERSION,
                "appUISettings": {},
            });
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
    let mut reply = to_wire(UpdateAppSettingsReply {
        app_settings: req.app_settings,
        succeeded: result.is_ok(),
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

    // Gated on the patch touching the theme so an unrelated settings write does not
    // re-push the frame or rewrite the editor settings.
    if params.patch.theme.is_some() {
        apply_theme_to_shell(ctx, &new_settings.theme);
    }

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

/// Re-theme the parts of the app the webview does not own: the native window (title
/// bar/border via DWM, WebView2's own surfaces, and the injected log-pane/scrollbar
/// tokens), plus the shared VSCodium user settings, so an open editor re-themes live
/// (VSCodium watches the file) and the next launch opens matching. The webview content
/// itself follows the `setNewAppSettings` event. The raw setting
/// ("dark"/"light"/"auto") is parsed native-side; "auto" resolves against the OS theme
/// there. The editor sync is best-effort: a write failure must not fail the settings
/// change, which has already applied. Both applies are no-ops when the theme they are
/// handed is the one already in effect.
fn apply_theme_to_shell(ctx: &BridgeCtx, theme: &str) {
    ctx.theme.set_theme(theme);
    ctx.host.editor_sync_theme(ThemeSetting::parse(theme));
}

/// Build the `setNewAppSettings` event from a settings object: the recomputed
/// `appUISettings` the front-end's app-level indicators read. Single-sources the
/// event shape across `updateAppSettings` (the apply path) and the profile watcher.
fn new_app_settings_event(ctx: &BridgeCtx, settings: &AppSettings) -> Envelope {
    let data = serde_json::to_value(SetNewAppSettings {
        app_ui_settings: app_ui_settings_now(ctx, settings),
    })
    .expect("SetNewAppSettings serializes");
    Envelope::event("setNewAppSettings", data)
}

/// Re-read the app settings and emit `setNewAppSettings`, so the front-end's
/// app-level indicators - the Windhawk-update badge and the logging state - refresh.
/// The profile watcher calls this on an external profile change (including the
/// startup catalog sync, which the core classifies as an external write), so the
/// app-update badge tracks the freshly-synced availability the same way the
/// extension's `_userProfileChanged` does. A read failure is logged and skipped.
pub(crate) fn emit_new_app_settings(ctx: &BridgeCtx) {
    if let Some(settings) = settings_to_announce(ctx) {
        ctx.emit.emit(new_app_settings_event(ctx, &settings));
    }
}

/// Announce app settings that a write this host did not drive has changed - a
/// user-data import applying the archive's settings - so the app does not keep
/// showing the old ones until it is restarted. The full announcement
/// `updateAppSettings` makes after its own write: the `setNewAppSettings` push (the
/// language and the theme the front-end applies, plus the app-level indicators) and
/// the native window/editor re-theme. Unlike that path this cannot gate the theme on
/// a patch, so it re-applies unconditionally - both applies no-op on an unchanged
/// theme.
///
/// The tray is deliberately NOT notified here: the core fires the restart/notify
/// action itself the moment it applies an import's settings, so the engine restart
/// overlaps the rest of the import instead of waiting for it.
///
/// Public as the headless test surface (the integration smoke asserts the push and
/// the editor re-theme against a real session); the event pump calls it for the
/// [`HostEffect::AppSettingsChanged`](crate::ipc::outcome::HostEffect) an import's
/// app-settings step names.
pub fn announce_app_settings(ctx: &BridgeCtx) {
    let Some(settings) = settings_to_announce(ctx) else {
        return;
    };
    ctx.emit.emit(new_app_settings_event(ctx, &settings));
    apply_theme_to_shell(ctx, &settings.theme);
}

/// Re-read the app settings for an announcement, logging and yielding `None` on a
/// read failure: an announcement reports a change, it does not make one, so there is
/// nothing to fail.
fn settings_to_announce(ctx: &BridgeCtx) -> Option<AppSettings> {
    match app_settings(ctx) {
        Ok(settings) => Some(settings),
        Err(error) => {
            eprintln!(
                "windhawk-ui: could not recompute appUISettings for setNewAppSettings: {error}"
            );
            None
        }
    }
}

/// Project the current settings into the `appUISettings` subset (the
/// `getInitialAppSettings` reply and the `setNewAppSettings` event share this),
/// with the stored `devModeOptOut` reported as-is (development is always on for
/// the native build, so there is no override) and the cached update
/// availability.
fn app_ui_settings_now(ctx: &BridgeCtx, settings: &AppSettings) -> AppUiSettings {
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
