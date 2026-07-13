//! The command handlers. Each handler computes a typed result implementing
//! [`crate::output::CommandResult`] and never prints; the render seam
//! (output.rs) turns it into text or the `--json` envelope.

pub mod app;
pub mod mods;
pub mod parse;
pub mod render;
pub mod repo;
pub mod source;
pub mod update;

use serde_json::json;
use windhawk_core_protocol::{AppSettings, ListInstalledModsResult};

use crate::Environment;
use crate::error::CliError;

/// Read the app settings once, typed (`getAppSettings`). A NON-caching free
/// function: every call is a fresh read, so the per-command "fetch once" is the
/// caller threading the returned value, not a cached invariant the type system
/// must defend. The single typed read path for the whole CLI - the command
/// inputs `language` / `check_for_updates` derive from its result.
pub(crate) fn app_settings(env: &Environment) -> Result<AppSettings, CliError> {
    Ok(env.core.invoke_as("getAppSettings", &json!({}))?)
}

/// The app language, defaulting to `en` (`appSettings.language || 'en'`),
/// derived from a fetched [`AppSettings`].
pub(crate) fn language(settings: &AppSettings) -> String {
    if settings.language.is_empty() {
        "en".to_owned()
    } else {
        settings.language.clone()
    }
}

/// Whether update checks are enabled (`!disableUpdateCheck`), gating the
/// installed-state update flag, mirroring the GUI.
pub(crate) fn check_for_updates(settings: &AppSettings) -> bool {
    !settings.disable_update_check
}

/// Emit a stderr warning for each mod whose metadata failed to load, shared by
/// `mod list` and `repo list` so the two listings cannot drift in the wording.
pub(crate) fn warn_load_errors(env: &Environment, result: &ListInstalledModsResult) {
    for load_error in &result.load_errors {
        env.logger.warn(&format!(
            "Failed to load metadata for mod '{}': {}",
            load_error.mod_id, load_error.error
        ));
    }
}
