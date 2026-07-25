//! DLL cleanup: the old-version sweep (`delete_old_mod_files`, per the
//! installed mod's architectures) and the full uninstall sweep
//! (`delete_mod_files`, over the supported-target subfolders). Both delegate to
//! `delete_mod_dlls`, which recognizes the random-suffix DLL names via
//! `domain::ends_with_random_suffix`; the subfolder sets come from
//! `domain::subfolders_for_arch` / `CompilationTarget::all`.

use windhawk_core_domain::{CompilationTarget, ends_with_random_suffix, subfolders_for_arch};

use crate::session::SessionInner;

/// Delete a mod's compiled DLLs of prior versions across the given subfolders,
/// shared by `delete_old_mod_files` (per-architecture, keeping the current DLL)
/// and `delete_mod_files` (the full uninstall sweep). Best effort throughout; a
/// not-yet-committed pending artifact of a concurrent operation is skipped.
fn delete_mod_dlls(
    session: &SessionInner,
    mod_id: &str,
    subfolders: &[&str],
    current_dll_name: Option<&str>,
) {
    let engine_mods_dir = session.storage().engine_mods_dir();
    let files = session.deps().files.clone();
    let pending = session.pending();
    let prefix = format!("{mod_id}_");

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
            let path = folder.join(name);
            // Never delete another in-flight operation's not-yet-committed DLL.
            if pending.contains(&path) {
                continue;
            }
            let _ = files.delete_file(&path);
        }
    }
}

/// Remove all of a mod's compiled DLLs (the TS `modFiles.deleteModFiles`): the
/// uninstall sweep over the full supported-target subfolders, with no current
/// DLL to keep. Best effort. Called by `services::mods::remove_mod`. The
/// subfolder set is derived from `CompilationTarget::all` (the one home for the
/// `32`/`64`(/`arm64`) supported set), not a third hardcoded literal.
pub(crate) fn delete_mod_files(session: &SessionInner, mod_id: &str) {
    let subfolders: Vec<&'static str> = CompilationTarget::all(session.arm64_enabled())
        .iter()
        .map(|t| t.subfolder())
        .collect();
    delete_mod_dlls(session, mod_id, &subfolders, None);
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
) {
    let arm64_enabled = session.arm64_enabled();
    let subfolders = subfolders_for_arch(architectures, arm64_enabled);
    delete_mod_dlls(session, mod_id, &subfolders, current_dll_name);
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
