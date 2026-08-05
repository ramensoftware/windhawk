//! The profile watcher: the extension watches the user-profile JSON and pushes
//! `updateInstalledModsDetails` when an EXTERNAL change (the tray's update
//! check, another instance) alters update-availability or ratings. The UI
//! reproduces this: `getProfileWatchInfo` yields the path; a background thread
//! detects changes and re-derives the details through the SAME
//! `shape::installed` projection the `getInstalledMods` handler uses, so the
//! watcher and the list handler cannot drift.
//!
//! External-vs-own distinction: the core records the mtime of its OWN profile
//! writes in `getProfileWatchInfo().lastModifiedByUserMtimeMs`, so a change whose
//! mtime equals that value is the core's own write and is ignored (exactly the
//! extension's `mtimeMs !== lastModifiedByUserMtimeMs` guard). The mtime is
//! computed with the core's formula (`modified().duration_since(UNIX_EPOCH) *
//! 1000`) so the two compare equal for the core's own writes.
//!
//! Mechanism: a 2-second poll, not the `notify` crate the plan names. A poll keeps
//! the change self-contained with no new external dependency (notify's transitive
//! tree and CC0 license would need cargo-deny/allowlist work) at the cost of up to
//! one poll-interval of latency - immaterial for an update-availability indicator.
//! Swapping in `notify` later is a localized change behind this module.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;
use windhawk_core_host::SessionApiExt;
use windhawk_core_protocol::{ListInstalledModsParams, ProfileWatchInfo};

use crate::commands::app::emit_new_app_settings;
use crate::commands::{app_settings, check_for_updates, language};
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::shape::installed::installed_mods_details;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Spawn the watcher thread. Best effort: if the thread cannot start the UI simply
/// has no live update-availability refresh (the reads still report it on demand).
pub fn spawn(ctx: BridgeCtx) {
    let _ = std::thread::Builder::new()
        .name("wh-profile-watch".to_owned())
        .spawn(move || run(ctx));
}

fn run(ctx: BridgeCtx) {
    let path = match resolve_profile_path(&ctx) {
        Some(path) => path,
        None => return,
    };

    // Seed with the current mtime so the pre-existing file does not look "changed"
    // on the first poll.
    let mut last_seen = modified_ms(&path);
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let Some(mtime) = modified_ms(&path) else {
            continue;
        };
        if Some(mtime) == last_seen {
            continue;
        }
        last_seen = Some(mtime);
        if is_external_change(&ctx, mtime) {
            // Match the extension's `_userProfileChanged` order: refresh the
            // app-level indicators (the Windhawk-update badge) first, then the
            // per-mod update/rating details.
            emit_new_app_settings(&ctx);
            refresh_installed_mods_details(&ctx);
        }
    }
}

fn resolve_profile_path(ctx: &BridgeCtx) -> Option<PathBuf> {
    match ctx
        .session
        .invoke_as::<ProfileWatchInfo, _>("getProfileWatchInfo", &json!({}))
    {
        Ok(info) => Some(PathBuf::from(info.file_path)),
        Err(error) => {
            eprintln!("windhawk-ui: profile watcher could not resolve the profile path: {error}");
            None
        }
    }
}

/// Whether the observed mtime differs from the core's last-own-write mtime - i.e.
/// the change was external (the tray, another instance), not this session's own
/// profile write. A read failure is treated as "not external" (skip) rather than
/// firing a spurious refresh.
fn is_external_change(ctx: &BridgeCtx, mtime: f64) -> bool {
    match ctx
        .session
        .invoke_as::<ProfileWatchInfo, _>("getProfileWatchInfo", &json!({}))
    {
        Ok(info) => is_external(
            mtime,
            info.last_modified_by_user_mtime_ms
                .and_then(|last| last.as_f64()),
        ),
        Err(_) => false,
    }
}

/// The pure external-vs-own decision: an observed profile mtime is external unless it
/// equals the core's recorded last-own-write mtime. `None` (the core never wrote the
/// profile this session) means any change is external.
fn is_external(observed_mtime: f64, last_own_mtime: Option<f64>) -> bool {
    last_own_mtime.is_none_or(|last| last != observed_mtime)
}

/// Re-derive the update-availability + ratings subset and push it as
/// `updateInstalledModsDetails`. No profile sync (`syncProfile: false`): this
/// reacts to a change, it does not cause one, matching the extension. Public as the
/// headless test surface (the integration smoke drives it against a real session to
/// assert it re-derives through the shared `shape::installed` projection); the
/// watcher loop calls it on every external change.
pub fn refresh_installed_mods_details(ctx: &BridgeCtx) {
    let settings = app_settings(ctx).ok();
    let language = settings
        .as_ref()
        .map(language)
        .unwrap_or_else(|| "en".to_owned());
    let check = settings.as_ref().map(check_for_updates).unwrap_or(false);

    let params = ListInstalledModsParams {
        language,
        check_for_updates: check,
        sync_profile: false,
    };
    match ctx.session.invoke("listInstalledMods", &params) {
        Ok(result) => {
            log_load_errors(&result);
            ctx.emit.emit(Envelope::event(
                "updateInstalledModsDetails",
                installed_mods_details(&result),
            ));
        }
        Err(error) => eprintln!("windhawk-ui: profile watcher listInstalledMods failed: {error}"),
    }
}

/// Log each per-mod metadata load error (the extension shows an error message box;
/// the UI logs, as the list handler does).
fn log_load_errors(list_result: &serde_json::Value) {
    let Some(errors) = list_result.get("loadErrors").and_then(|v| v.as_array()) else {
        return;
    };
    for error in errors {
        let mod_id = error.get("modId").and_then(|v| v.as_str()).unwrap_or("?");
        let message = error.get("error").and_then(|v| v.as_str()).unwrap_or("");
        eprintln!("windhawk-ui: failed to load metadata for mod '{mod_id}': {message}");
    }
}

/// Last-modified time in milliseconds since the Unix epoch, using the core's
/// formula (`windhawk-core-windows` `modified_ms`) so the value compares equal to
/// the core's recorded last-own-write mtime.
fn modified_ms(path: &Path) -> Option<f64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_external_distinguishes_own_writes_from_external_ones() {
        // The core records the mtime of its OWN profile write, so an equal observed
        // mtime is this session's write - not external (do not re-push).
        assert!(!is_external(100.0, Some(100.0)));
        // A different mtime is an external change (the tray, another instance).
        assert!(is_external(100.5, Some(100.0)));
        // The core never wrote the profile this session: any change is external.
        assert!(is_external(100.0, None));
    }

    #[test]
    fn modified_ms_uses_the_cores_epoch_millis_formula() {
        // The watcher's mtime must equal the core's recorded last-own-write mtime for
        // the own-vs-external comparison to be exact, so it uses the SAME formula
        // (`modified().duration_since(UNIX_EPOCH) * 1000`).
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("profile.json");
        std::fs::write(&path, "{}").expect("write profile");

        let expected = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
            * 1000.0;
        assert_eq!(modified_ms(&path), Some(expected));
        // A missing file yields None (no spurious change), not a panic.
        assert_eq!(modified_ms(&dir.path().join("absent.json")), None);
    }
}
