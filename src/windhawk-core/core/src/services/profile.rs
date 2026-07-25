//! `services::profile`: the user-profile read-modify-write primitives and the
//! profile-touching commands (`setModRating`, `getAppUpdateStatus`,
//! `getProfileWatchInfo`, `syncCatalogToProfile`), a port of
//! `services/userProfile.ts`. Other services mutate the profile only through
//! this module.
//!
//! Each read-modify-write runs under the rank-2 artifact lock (in-process RMW
//! serialization) and the cross-process profile `NamedLock`. The named lock is
//! new behavior, scoped to core sessions and best effort: a timeout degrades to
//! last-write-wins, never a command failure. The write itself is best effort
//! too (the TS `UserProfile.write` logs and swallows failures), and the
//! last-own-write mtime is recorded only for the session's own (non-external)
//! writes, for the extension's profile watcher (`getProfileWatchInfo`).

use std::sync::Mutex;

use serde_json::Value;
use windhawk_core_domain::{
    Profile, coerce_version, higher_version, is_pre_release, is_update_available,
};
use windhawk_core_protocol::{
    AppUpdateStatus, ProfileWatchInfo, SetModRatingParams, SyncCatalogToProfileParams,
    SyncCatalogToProfileResult,
};

use crate::callbacks::LogLevel;
use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::services::wire::{file_err, to_value_result};
use crate::session::SessionInner;

/// The cross-process profile mutex name. A constant defined by the core; the TS
/// backend and the C++ app do not take it during the migration window.
const PROFILE_LOCK_NAME: &str = "Windhawk_UserProfileLock";
/// How long to wait for the cross-process lock before proceeding unlocked.
const PROFILE_LOCK_TIMEOUT_MS: u32 = 10_000;

/// Session-scoped profile coordination: the rank-2 read-modify-write lock and
/// the last-own-write mtime. Holds no durable data - the file is the single
/// source of truth.
pub struct ProfileState {
    rmw: Mutex<()>,
    last_modified_by_user_ms: Mutex<Option<f64>>,
}

impl ProfileState {
    pub fn new() -> Self {
        Self {
            rmw: Mutex::new(()),
            last_modified_by_user_ms: Mutex::new(None),
        }
    }
}

impl Default for ProfileState {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the profile fresh from disk. A missing file (or unparseable JSON)
/// yields an empty profile, like the TS constructor; any other read failure is
/// `IO_FAILED`.
pub(crate) fn read_profile(session: &SessionInner) -> Result<Profile, CoreError> {
    let path = session.storage().user_profile_path();
    match session.deps().files.read(&path) {
        Ok(bytes) => Ok(Profile::parse(Some(&String::from_utf8_lossy(&bytes)))),
        Err(e) if e.is_not_found() => Ok(Profile::parse(None)),
        Err(e) => Err(file_err(e)),
    }
}

/// Run a read-modify-write against the profile under both the in-process RMW
/// lock and the cross-process named lock. The closure returns
/// `(should_write, result)`; the profile is written (best effort) only when
/// `should_write`. `external` mirrors the TS `asExternalUpdate`: an external
/// write does not advance the last-own-write mtime.
pub(crate) fn read_modify_write<R>(
    session: &SessionInner,
    external: bool,
    f: impl FnOnce(&mut Profile) -> (bool, R),
) -> Result<R, CoreError> {
    let state = session.profile_state();
    let _rmw = state.rmw.lock().unwrap_or_else(|e| e.into_inner());
    // Best effort cross-process exclusion; the guard releases on scope exit.
    let _named = session
        .deps()
        .named_lock
        .acquire(PROFILE_LOCK_NAME, PROFILE_LOCK_TIMEOUT_MS);

    let mut profile = read_profile(session)?;
    let (should_write, result) = f(&mut profile);
    if should_write {
        write_profile(session, &profile, external);
    }
    Ok(result)
}

/// Write the profile via the atomic-replace `Files` primitive. Best effort,
/// matching the TS `write`: a failure is logged as a warning and never fails
/// the command. On a non-external success the last-own-write mtime is recorded.
fn write_profile(session: &SessionInner, profile: &Profile, external: bool) {
    let path = session.storage().user_profile_path();
    let bytes = profile.to_pretty().into_bytes();
    match session.deps().files.write_atomic(&path, &bytes) {
        Ok(()) => {
            if !external && let Ok(mtime) = session.deps().files.modified_ms(&path) {
                *session
                    .profile_state()
                    .last_modified_by_user_ms
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(mtime);
            }
        }
        Err(e) => {
            session.log(
                LogLevel::Warn,
                format!("failed to write user profile: {}", e.message()),
            );
        }
    }
}

/// `setModRating`: a profile write (a nonzero rating is stored, 0 clears it),
/// tracked as an own write so the watcher ignores it.
pub fn set_mod_rating(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModRatingParams = decode_params("setModRating", params)?;
    read_modify_write(session, false, |profile| {
        profile.set_mod_rating(&params.mod_id, params.rating);
        (true, ())
    })?;
    Ok(Value::Null)
}

/// The pre-release-folded cached `(stable, bleeding-edge)` latest versions,
/// exactly as [`get_app_update_status`] reports them: on a pre-release build the
/// cached pre-release version (`latestVersionPreRelease`) is folded into both
/// channels, replacing each with the higher of the two, so an alpha/beta tester
/// is offered the next pre-release rather than only the next stable/bleeding-edge
/// release. A stable build ignores it (it stays on its own channel), and it is a
/// no-op when the field is absent or not a coercible version. Single-sourced so
/// the update-check read and the self-update installer URL cannot drift.
pub(crate) fn resolved_latest_versions(
    session: &SessionInner,
) -> Result<(Option<String>, Option<String>), CoreError> {
    let profile = read_profile(session)?;
    let current = session.config().windhawk_version.as_deref();

    let mut latest = profile.app_latest_version();
    let mut latest_be = profile.app_latest_version_bleeding_edge();
    // Fold the pre-release channel into both offers, but only for a coercible
    // value: a malformed cached pre-release (empty or non-numeric, e.g. from a
    // backend glitch) must not replace a valid offer, nor - since the self-update
    // installer URL is pinned to this result - become a bad download target. The
    // C++ GetUpdateStatus is likewise robust here.
    if current.is_some_and(is_pre_release)
        && let Some(pre) = profile
            .app_latest_version_pre_release()
            .filter(|pre| coerce_version(pre).is_some())
    {
        latest = Some(higher_version(latest, pre));
        latest_be = Some(higher_version(latest_be, pre));
    }

    Ok((latest.map(str::to_owned), latest_be.map(str::to_owned)))
}

/// `getAppUpdateStatus`: the cached latest versions and their npm-semver
/// comparison against the session's installed version. The versions are the
/// pre-release-folded pair from [`resolved_latest_versions`].
pub fn get_app_update_status(session: &SessionInner, _params: Value) -> Result<Value, CoreError> {
    let current = session.config().windhawk_version.as_deref();
    let (latest, latest_be) = resolved_latest_versions(session)?;

    let dto = AppUpdateStatus {
        update_available: is_update_available(current, latest.as_deref()),
        update_available_bleeding_edge: is_update_available(current, latest_be.as_deref()),
        latest_version: latest,
        latest_version_bleeding_edge: latest_be,
    };
    to_value_result("getAppUpdateStatus", &dto)
}

/// `getProfileWatchInfo`: the profile path plus the last-own-write mtime, for
/// the extension's external-change watcher.
pub fn get_profile_watch_info(session: &SessionInner, _params: Value) -> Result<Value, CoreError> {
    let path = session.storage().user_profile_path();
    let last = *session
        .profile_state()
        .last_modified_by_user_ms
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dto = ProfileWatchInfo {
        file_path: path.to_string_lossy().into_owned(),
        last_modified_by_user_mtime_ms: last.and_then(serde_json::Number::from_f64),
    };
    to_value_result("getProfileWatchInfo", &dto)
}

/// `syncCatalogToProfile`: record the catalog's latest app and per-mod versions
/// (an external write, so the watcher forwards it), returning whether anything
/// changed.
pub fn sync_catalog_to_profile(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SyncCatalogToProfileParams = decode_params("syncCatalogToProfile", params)?;
    let app_latest = params.catalog.app.version.clone();
    let app_latest_be = params.catalog.app.version_bleeding_edge.clone();
    let app_latest_pre = params.catalog.app.version_pre_release.clone();
    // Only catalog mods with a truthy version are recorded (the TS
    // `if (version)` filter).
    let mod_latest: Vec<(String, String)> = params
        .catalog
        .mods
        .iter()
        .filter_map(|(id, m)| {
            m.metadata
                .version
                .clone()
                .filter(|v| !v.is_empty())
                .map(|v| (id.clone(), v))
        })
        .collect();

    let updated = read_modify_write(session, true, |profile| {
        let changed = profile.update_latest_versions(
            app_latest.as_deref(),
            app_latest_be.as_deref(),
            app_latest_pre.as_deref(),
            &mod_latest,
        );
        (changed, changed)
    })?;

    to_value_result(
        "syncCatalogToProfile",
        &SyncCatalogToProfileResult {
            profile_updated: updated,
        },
    )
}
