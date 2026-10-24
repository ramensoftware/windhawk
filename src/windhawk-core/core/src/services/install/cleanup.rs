//! Removal of a mod's files: the old-version DLL sweep (`delete_old_mod_files`,
//! per the installed mod's architectures), the full DLL sweep
//! (`delete_mod_files`, over the supported-target subfolders), and the mod's
//! storage folder (`delete_mod_storage`). Both sweeps delegate to
//! `delete_mod_dlls`, which recognizes the random-suffix DLL names via
//! `domain::ends_with_random_suffix`; the subfolder sets come from
//! `domain::subfolders_for_arch` / `CompilationTarget::all`. What a process
//! still holds after the first attempt is left to the session's deferred
//! removals.

use std::path::PathBuf;
use std::time::Instant;

use windhawk_core_domain::{CompilationTarget, ends_with_random_suffix, subfolders_for_arch};
use windhawk_core_ports::FileError;

use crate::services::mods::{configured_dll_name, does_config_exist};
use crate::session::SessionInner;

/// Whether a removal failed only because a process holds what it removes, so
/// another attempt can succeed once the engine unloads the mod.
fn still_in_use(result: Result<(), FileError>) -> bool {
    result.is_err_and(|e| e.is_in_use())
}

/// Hand a removal whose first attempt left something in use to the deferred
/// removals. Each retry runs under the mod's exclusive lock, the one its
/// commits take, so the mod's config that `attempt` checks stays as read until
/// the attempt is done.
fn retry_under_mod_lock(
    session: &SessionInner,
    mod_id: &str,
    config_changed: Instant,
    mut attempt: impl FnMut(&SessionInner, &str) -> bool + Send + 'static,
) {
    let mod_id = mod_id.to_owned();
    session.deferred_removals().push(
        config_changed,
        Box::new(move |session| {
            let lock = session.mod_lock(&mod_id);
            let _commit = lock.write().unwrap_or_else(|e| e.into_inner());
            attempt(session, &mod_id)
        }),
    );
}

/// Delete a mod's compiled DLLs of prior versions across the given subfolders,
/// shared by `delete_old_mod_files` (per-architecture, keeping the current DLL)
/// and `delete_mod_files` (the full sweep). Best effort throughout; a
/// not-yet-committed pending artifact of a concurrent operation is skipped.
/// `config_changed` is the write that stopped naming the DLLs.
fn delete_mod_dlls(
    session: &SessionInner,
    mod_id: &str,
    subfolders: &[&str],
    current_dll_name: Option<&str>,
    config_changed: Instant,
) {
    let engine_mods_dir = session.storage().engine_mods_dir();
    let files = session.deps().files.clone();
    let pending = session.pending();
    let prefix = format!("{mod_id}_");

    let mut to_delete: Vec<PathBuf> = Vec::new();
    for subfolder in subfolders {
        let folder = engine_mods_dir.join(subfolder);
        let entries = match files.list_dir(&folder) {
            Ok(entries) => entries,
            // A missing folder (or any listing error) means nothing to clean.
            Err(_) => continue,
        };
        for entry in entries {
            if !entry.is_file {
                continue;
            }
            let name = &entry.name;
            if current_dll_name == Some(name.as_str()) {
                continue;
            }
            let Some(rest) = name
                .strip_prefix(&prefix)
                .and_then(|r| r.strip_suffix(".dll"))
            else {
                continue;
            };
            if !ends_with_random_suffix(rest) {
                continue;
            }
            to_delete.push(folder.join(name));
        }
    }

    // Another in-flight operation's not-yet-committed DLL is never deleted.
    to_delete.retain(|path| !pending.contains(path) && still_in_use(files.delete_file(path)));
    if to_delete.is_empty() {
        return;
    }
    retry_under_mod_lock(session, mod_id, config_changed, move |session, mod_id| {
        // A DLL the config names again by now is the mod's, and while the
        // config cannot be read, every DLL is kept for the next attempt.
        let Ok(current_dll_name) = configured_dll_name(session, mod_id) else {
            return false;
        };
        let files = &session.deps().files;
        let pending = session.pending();
        to_delete.retain(|path| {
            path.file_name().and_then(|name| name.to_str()) != current_dll_name.as_deref()
                && !pending.contains(path)
                && still_in_use(files.delete_file(path))
        });
        to_delete.is_empty()
    });
}

/// Remove a mod's storage folder (the TS `deleteModStoragePath`). Best effort,
/// as the TS `rmSync` swallows its errors. `config_changed` is the write that
/// uninstalled the mod.
pub(crate) fn delete_mod_storage(session: &SessionInner, mod_id: &str, config_changed: Instant) {
    let dir = session.storage().mod_storage_dir(mod_id);
    if !still_in_use(session.deps().files.remove_dir_all(&dir)) {
        return;
    }
    retry_under_mod_lock(session, mod_id, config_changed, move |session, mod_id| {
        // A mod installed again by now owns the folder, and while the config
        // cannot be read, the folder is kept for the next attempt.
        match does_config_exist(session, mod_id) {
            Ok(false) => !still_in_use(session.deps().files.remove_dir_all(&dir)),
            Ok(true) => true,
            Err(_) => false,
        }
    });
}

/// Remove all of a mod's compiled DLLs (the TS `modFiles.deleteModFiles`): the
/// sweep over the full supported-target subfolders with no current DLL to keep,
/// for a storage id no installed mod stands behind any more. Best effort. The
/// subfolder set is derived from `CompilationTarget::all` (the one home for the
/// `32`/`64`(/`arm64`) supported set), not a third hardcoded literal.
pub(crate) fn delete_mod_files(session: &SessionInner, mod_id: &str, config_changed: Instant) {
    let subfolders: Vec<&'static str> = CompilationTarget::all(session.arm64_enabled())
        .iter()
        .map(|t| t.subfolder())
        .collect();
    delete_mod_dlls(session, mod_id, &subfolders, None, config_changed);
}

/// Delete a mod's compiled DLLs of prior versions (the TS modFiles
/// `deleteOldModFiles`): for the architecture's subfolders, drop files named
/// `<modId>_..._<digits>.dll` except the current one and any not-yet-committed
/// pending artifact of a concurrent operation. Best effort throughout. The
/// subfolder set is the shared `domain::subfolders_for_arch` (request order,
/// deduped, unknown-arch skipped); `delete_mod_dlls` deletes each subfolder
/// independently, so the order is invisible here.
pub(super) fn delete_old_mod_files(
    session: &SessionInner,
    mod_id: &str,
    architectures: &[String],
    current_dll_name: Option<&str>,
    config_changed: Instant,
) {
    let arm64_enabled = session.arm64_enabled();
    let subfolders = subfolders_for_arch(architectures, arm64_enabled);
    delete_mod_dlls(
        session,
        mod_id,
        &subfolders,
        current_dll_name,
        config_changed,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_subfolders_expand_and_dedup() {
        // Cleanup's subfolder set is `domain::subfolders_for_arch` (request
        // order, deduped). The unknown-arch SKIP here is the best-effort partner
        // of the compile path's REJECT
        // (compiler::orchestrate::tests::target_selection_follows_the_architecture_rules);
        // one shared taxonomy, two callers' policies. Order is invisible to
        // `delete_mod_dlls` (per-folder independent deletion).
        assert_eq!(subfolders_for_arch(&[], false), vec!["32", "64"]);
        assert_eq!(
            subfolders_for_arch(&["x86-64".into()], true),
            vec!["64", "arm64"]
        );
        assert_eq!(
            subfolders_for_arch(&["x86-64".into(), "amd64".into()], false),
            vec!["64"]
        );
        assert_eq!(
            subfolders_for_arch(&["arm64".into()], false),
            Vec::<&str>::new()
        );
        // Unknown architecture: SKIPPED (best effort), leaving an empty set.
        assert_eq!(
            subfolders_for_arch(&["sparc".into()], false),
            Vec::<&str>::new()
        );
    }
}
