//! The startup catalog refresh: a background `fetchCatalog` ->
//! `syncCatalogToProfile` so the profile's recorded latest versions - and thus
//! the per-mod update-availability the reads report - are current for the
//! session. The extension folds this into every
//! `getFeaturedMods`/`getRepositoryMods` call; the UI moves it here so those
//! composites stay a single follow-up, and the profile watcher keeps it fresh
//! thereafter.
//!
//! It is an INTERNAL async op: a `Terminal::Internal` whose handler runs the
//! follow-up `syncCatalogToProfile` (and the extension's `newUpdatesFound` tray
//! notification when the sync changed the profile) through the injected seam and
//! emits NO front-end reply.

use serde_json::{Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    FetchCatalogParams, NotifyTrayParams, SyncCatalogToProfileRequest, TrayAction,
};

use crate::commands::{app_settings, check_for_updates, language};
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::outcome::{AsyncKind, FollowUp, Terminal};

/// Kick off the background catalog refresh. Best effort: a failure to start (or
/// later to sync) is logged, never fatal - the tray and the profile watcher keep
/// update availability current regardless.
pub fn kick(ctx: &BridgeCtx) {
    let settings = app_settings(ctx).ok();
    let language = settings
        .as_ref()
        .map(language)
        .unwrap_or_else(|| "en".to_owned());
    let check = settings.as_ref().map(check_for_updates).unwrap_or(false);

    let params = FetchCatalogParams { language };
    match ctx.session.invoke_async("fetchCatalog", &params) {
        Ok(op_id) => {
            let kind = AsyncKind {
                terminal: Terminal::Internal(refresh_terminal),
                progress: None,
            };
            ctx.register_async(
                op_id,
                "fetchCatalog".to_owned(),
                0,
                kind,
                json!({ "checkForUpdates": check }),
            );
        }
        Err(error) => eprintln!("windhawk-ui: startup catalog refresh could not start: {error}"),
    }
}

/// The internal terminal: on a fetched catalog, sync it to the profile, then post
/// the `newUpdatesFound` tray notification when the sync changed something and
/// update checks are enabled (the extension's `_updateUserProfileJson`). Emits no
/// reply.
fn refresh_terminal(
    outcome: Result<Value, HostError>,
    context: &Value,
    invoke: &dyn Fn(&FollowUp) -> Result<Value, HostError>,
) {
    let catalog = match outcome {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("windhawk-ui: startup catalog fetch failed: {error}");
            return;
        }
    };

    let sync = SyncCatalogToProfileRequest { catalog };
    let sync_params = serde_json::to_value(&sync).unwrap_or(Value::Null);
    let result = match invoke(&FollowUp {
        command: "syncCatalogToProfile",
        params: sync_params,
        stateless: false,
    }) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("windhawk-ui: startup catalog sync failed: {error}");
            return;
        }
    };

    let profile_updated = result
        .get("profileUpdated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let check = context
        .get("checkForUpdates")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if profile_updated && check {
        let notify = NotifyTrayParams {
            action: TrayAction::NewUpdatesFound,
        };
        let params = serde_json::to_value(notify).unwrap_or(Value::Null);
        if let Err(error) = invoke(&FollowUp {
            command: "notifyTray",
            params,
            stateless: false,
        }) {
            eprintln!("windhawk-ui: startup tray notify failed: {error}");
        }
    }
}
