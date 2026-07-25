//! The command handlers. Each is a thin translator: parse the envelope `data`
//! into a typed request DTO, call the host `Session`/`GatedCore`, and shape the
//! reply through the pure `shape/` shapers. No Windhawk logic lives here. The
//! synchronous read commands live here; the development stubs live in
//! `dev_stub`.

pub mod app;
pub mod dev;
pub mod dev_stub;
pub mod devtools;
pub mod logwindow;
pub mod mods;
pub mod repo;
pub mod update;
pub mod userdata;

use serde_json::json;
use windhawk_core_host::HostError;
use windhawk_core_protocol::AppSettings;

use crate::ipc::bridge::BridgeCtx;

/// Read the app settings once, typed (`getAppSettings`). Non-caching, like the
/// CLI's per-call read: the command inputs `language` / `check_for_updates` derive
/// from its result. (The extension caches these from `getInitialAppSettings`; the
/// cached-vs-fresh distinction is an implementation detail, not part of the
/// protocol contract this port preserves.)
pub(crate) fn app_settings(ctx: &BridgeCtx) -> Result<AppSettings, HostError> {
    ctx.session.invoke_as("getAppSettings", &json!({}))
}

/// The app language, defaulting to `en` (`appSettings.language || 'en'`).
pub(crate) fn language(settings: &AppSettings) -> String {
    if settings.language.is_empty() {
        "en".to_owned()
    } else {
        settings.language.clone()
    }
}

/// The app language with the `en` default, reading `getAppSettings` once. For a
/// handler that needs ONLY the language (not the whole settings object); a handler
/// that also needs other fields reads `app_settings` once and reuses it.
pub(crate) fn app_language(ctx: &BridgeCtx) -> String {
    app_settings(ctx)
        .as_ref()
        .map(language)
        .unwrap_or_else(|_| "en".to_owned())
}

/// Whether update checks are enabled (`!disableUpdateCheck`), gating the
/// installed-state update flag, mirroring the GUI.
pub(crate) fn check_for_updates(settings: &AppSettings) -> bool {
    !settings.disable_update_check
}
