//! The startup catalog refresh: a background `fetchCatalog` ->
//! `syncCatalogToProfile` so the profile's recorded latest versions - and thus
//! the per-mod update-availability the reads report - are current for the
//! session. The extension folds this into every
//! `getFeaturedMods`/`getRepositoryMods` call; the UI moves it here so those
//! composites stay a single follow-up, and the profile watcher keeps it fresh
//! thereafter.
//!
//! It is an INTERNAL async op: a `Terminal::Internal` whose handler runs the
//! follow-up `syncCatalogToProfile` through the injected seam and emits NO
//! front-end reply. The native tray observes the profile write directly (its own
//! file-change watcher), so no tray notification is posted here.

use serde_json::Value;
use windhawk_core_host::HostError;
use windhawk_core_protocol::{FetchCatalogParams, SyncCatalogToProfileRequest};

use crate::commands::{app_settings, language};
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

    let params = FetchCatalogParams { language };
    match ctx.start_async("fetchCatalog", &params) {
        Ok(start) => {
            let kind = AsyncKind {
                terminal: Terminal::Internal(refresh_terminal),
                progress: None,
                effect: None,
            };
            ctx.register_async(start, "fetchCatalog".to_owned(), 0, kind, Value::Null);
        }
        Err(error) => eprintln!("windhawk-ui: startup catalog refresh could not start: {error}"),
    }
}

/// The internal terminal: on a fetched catalog, sync it to the profile. The
/// synced latest versions drive the per-mod update-availability the reads report,
/// and the native tray's file watcher picks up the profile write. Emits no reply.
fn refresh_terminal(
    outcome: Result<Value, HostError>,
    _context: &Value,
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
    if let Err(error) = invoke(&FollowUp {
        command: "syncCatalogToProfile",
        params: sync_params,
        stateless: false,
    }) {
        eprintln!("windhawk-ui: startup catalog sync failed: {error}");
    }
}
