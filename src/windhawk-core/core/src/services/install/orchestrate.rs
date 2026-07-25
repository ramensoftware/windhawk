//! The install/compile orchestration: the `compileInstalledMod` and
//! `installMod` prepare/body pairs, the locked `installMod` commit section
//! (`commit_install`), the rename-install storage-tree rename
//! (`change_mod_id`), and the install/recompile config write
//! (`write_install_config`). The slow compile/download runs unlocked; the
//! commit takes the exclusive keyed lock(s).

use std::sync::Arc;

use serde_json::{Map, Value};
use windhawk_core_domain::{ModId, extract_initial_settings_for_engine};
use windhawk_core_protocol::{
    CompileInstalledModParams, CompileInstalledModResult, InstallModParams, InstallModResult,
    ModConfigPatch, ModMetadata,
};

use super::cleanup::delete_old_mod_files;
use super::download::download_precompiled_mod;
use super::migrate::{engine_items_to_map, migrate_mod_settings, read_previous_engine_settings};
use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::pending::PendingHandle;
use crate::runtime::{OpContext, PreparedOp};
use crate::services::compiler::{self, CompileOutput};
use crate::services::mods::{delete_source, does_config_exist, read_mod_config, set_source};
use crate::services::settings_io::{open_tree, write_mod_config_patch};
use crate::services::wire::{WireResultExt, to_value_result};
use crate::session::SessionInner;

/// `compileInstalledMod`: validate synchronously, then run the compile + commit
/// on the operation thread. The keyed `Mod` lock is the session's (services do
/// not construct command locks); it is held only across the commit.
pub fn prepare_compile_installed_mod(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: CompileInstalledModParams = decode_params("compileInstalledMod", params)?;
    let session = session.clone();
    Ok(PreparedOp(Box::new(move |ctx| {
        compile_installed_mod_body(&session, params, ctx)
    })))
}

fn compile_installed_mod_body(
    session: &Arc<SessionInner>,
    params: CompileInstalledModParams,
    ctx: &OpContext,
) -> Result<Value, CoreError> {
    let CompileInstalledModParams {
        storage_id,
        source,
        metadata,
    } = params;

    // Slow phase (no command lock): compile into operation-private DLLs, kept
    // in the pending-artifact set until the commit drops `pending`.
    // `compileInstalledMod` has no precompiled-headers folder (that is the
    // editor `installMod` flow's input).
    let CompileOutput {
        target_dll_name,
        pending,
        warnings,
    } = compiler::compile_mod(session, &storage_id, &metadata, &source, None, ctx)?;

    // A cancel observed before the commit ends the operation; unlink the
    // freshly compiled DLLs nothing points at yet. Once the commit begins it
    // runs to completion.
    if ctx.cancel_token().is_canceled() {
        pending.unlink_all(session.deps().files.as_ref());
        return Err(CoreError::canceled());
    }

    // Commit (exclusive keyed `Mod` lock): config write, old-DLL cleanup,
    // read-back - all against the state as it is now.
    let mod_lock = session.mod_lock(&storage_id);
    let _commit = mod_lock.write().unwrap_or_else(|e| e.into_inner());

    // The recompile config write goes through the SAME shared patch writer as
    // installMod, passing disabled/logging as None so a recompile PRESERVES
    // them (the former write_compiled_config / write_install_config subset/
    // superset pair, now one function).
    write_install_config(
        session,
        &storage_id,
        &metadata,
        &target_dll_name,
        None,
        None,
    )?;
    delete_old_mod_files(
        session,
        &storage_id,
        metadata.architecture.as_deref().unwrap_or(&[]),
        Some(&target_dll_name),
    );

    let config = read_mod_config(session, &storage_id)?
        .ok_or_else(|| CoreError::internal("Failed to query compiled mod details"))?;

    // The DLLs are committed now (the config points at the new one); release
    // them from the cleanup-exclusion set.
    drop(pending);

    to_value_result(
        "compileInstalledMod",
        &CompileInstalledModResult {
            config,
            target_dll_name,
            warnings,
        },
    )
}

////////////////////////////////////////////////////////////////////////////
// installMod: the full install/reinstall flow - settings migration,
// compile-or-download, config + source writes, old-DLL cleanup, and the
// user-profile version record. A staged keyed-`Mod` operation: the slow
// compile/download runs unlocked (its DLLs in the pending set); the commit
// takes the exclusive keyed lock(s) for every read and write of stored mod
// state. `renameFromStorageId` targets two storage ids and takes both keyed
// locks (lexicographic order, the only two-lock command).

/// `installMod`: validate synchronously, then run compile-or-download + commit
/// on the operation thread.
pub fn prepare_install_mod(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: InstallModParams = decode_params("installMod", params)?;
    let session = session.clone();
    Ok(PreparedOp(Box::new(move |ctx| {
        install_mod_body(&session, params, ctx)
    })))
}

/// Run one install directly on the caller's operation thread and context (the
/// same body `installMod` runs), for `services::user_data`'s import: import runs
/// many single-mod installs under ONE operation, so it invokes this per mod
/// rather than issuing a nested `installMod` envelope. The install self-acquires
/// the exclusive keyed `Mod` lock for its own commit (the `ModStaged`
/// discipline), so the caller must NOT hold that lock across this call.
pub(crate) fn run_install(
    session: &Arc<SessionInner>,
    params: InstallModParams,
    ctx: &OpContext,
) -> Result<Value, CoreError> {
    install_mod_body(session, params, ctx)
}

fn install_mod_body(
    session: &Arc<SessionInner>,
    params: InstallModParams,
    ctx: &OpContext,
) -> Result<Value, CoreError> {
    // The new source's engine settings, parsed up front (the TS reads it before
    // the compile, with no try/catch): a malformed settings block fails the
    // install before any slow work. `None` (no block) becomes an empty map for
    // the migration, matching the TS `initialSettings || {}`.
    //
    // The per-version settings workarounds (domain::apply_settings_workarounds)
    // run only for store-installed mods; a locally-authored mod (`local@` storage
    // id) is parsed as written, so the author sees the real validation error
    // rather than a silent compatibility fixup of a shipped version.
    let apply_workarounds = !ModId::str_is_local(&params.storage_id);
    let initial_settings =
        match extract_initial_settings_for_engine(&params.source, apply_workarounds) {
            Ok(items) => engine_items_to_map(items.unwrap_or_default()),
            // The error names itself ("Failed to parse settings: ..."), so it
            // is surfaced verbatim rather than labeled again.
            Err(e) => return Err(CoreError::internal(e.to_string())),
        };

    let version = params.metadata.version.clone().unwrap_or_default();
    let architecture = params.metadata.architecture.clone().unwrap_or_default();

    // Slow phase (no command lock): compile locally or download the precompiled
    // DLLs into the operation-private pending set.
    let CompileOutput {
        target_dll_name,
        pending,
        warnings,
    } = if params.compile_locally {
        compiler::compile_mod(
            session,
            &params.storage_id,
            &params.metadata,
            &params.source,
            params.pch_folder.as_deref(),
            ctx,
        )?
    } else {
        download_precompiled_mod(session, &params.storage_id, &version, &architecture, ctx)?
    };

    // A cancel observed before the commit ends the operation; unlink the
    // freshly produced DLLs nothing points at yet.
    if ctx.cancel_token().is_canceled() {
        pending.unlink_all(session.deps().files.as_ref());
        return Err(CoreError::canceled());
    }

    // Commit (exclusive keyed `Mod` lock(s)): a rename targets two ids; acquire
    // both keyed locks in lexicographic order. The guards live to the end of
    // the body, holding across the commit call below.
    let mut lock_keys: Vec<String> = vec![params.storage_id.clone()];
    if let Some(rename) = &params.rename_from_storage_id {
        lock_keys.push(rename.clone());
    }
    lock_keys.sort();
    lock_keys.dedup();
    let locks: Vec<_> = lock_keys.iter().map(|k| session.mod_lock(k)).collect();
    let _guards: Vec<_> = locks
        .iter()
        .map(|l| l.write().unwrap_or_else(|e| e.into_inner()))
        .collect();

    commit_install(
        session,
        params,
        target_dll_name,
        warnings,
        initial_settings,
        pending,
    )
}

/// The locked commit section of `install_mod_body`, extracted so the
/// load-bearing side-effect ORDERING lives in one named place. The fixture and
/// parity checks compare the FINAL state, not the write order, so a silent
/// reorder of these effects would slip past them - it is pinned instead by a
/// dedicated end-to-end ordering test. Runs under the caller's keyed-lock
/// guard(s) (held across this call). The order is load-bearing: read the OLD
/// engine settings BEFORE the rename moves config; rename; the config-existed
/// check AFTER the rename and BEFORE the config write; migrate settings; write
/// the source; delete the old source on a rename; sweep old DLLs; record the
/// profile version; read back the config; then drop `pending` to commit the
/// DLLs.
fn commit_install(
    session: &Arc<SessionInner>,
    params: InstallModParams,
    target_dll_name: String,
    warnings: String,
    initial_settings: Map<String, Value>,
    pending: PendingHandle,
) -> Result<Value, CoreError> {
    let InstallModParams {
        storage_id,
        source,
        metadata,
        disabled,
        logging_enabled,
        track_in_profile,
        rename_from_storage_id,
        ..
    } = params;
    let version = metadata.version.clone().unwrap_or_default();
    let architecture = metadata.architecture.clone().unwrap_or_default();

    // The OLD source's engine settings (read before the rename moves config),
    // for the migration's "previous initial settings".
    let previous_initial_settings = read_previous_engine_settings(session, &storage_id);

    // Move the prior id's config/writable trees onto the new id (editor rename).
    if let Some(rename) = &rename_from_storage_id {
        change_mod_id(session, rename, &storage_id)?;
    }

    // The config-existed check must run AFTER the rename (which may have just
    // created the config at `storage_id`) and BEFORE the config write (the TS
    // `configExists` ordering inside `setModConfig`).
    let config_existed = does_config_exist(session, &storage_id)?;
    write_install_config(
        session,
        &storage_id,
        &metadata,
        &target_dll_name,
        disabled,
        logging_enabled,
    )?;
    migrate_mod_settings(
        session,
        &storage_id,
        &initial_settings,
        previous_initial_settings.as_ref(),
        config_existed,
    )?;

    set_source(session, &storage_id, &source)?;
    if let Some(rename) = &rename_from_storage_id {
        delete_source(session, rename)?;
    }

    delete_old_mod_files(session, &storage_id, &architecture, Some(&target_dll_name));

    if track_in_profile {
        crate::services::profile::read_modify_write(session, false, |profile| {
            profile.set_mod_version(&storage_id, &version, true);
            (true, ())
        })?;
    }

    let config = read_mod_config(session, &storage_id)?
        .ok_or_else(|| CoreError::internal("Failed to query installed mod details"))?;

    // The DLLs are committed now (the config points at the new one); release
    // them from the cleanup-exclusion set.
    drop(pending);

    to_value_result(
        "installMod",
        &InstallModResult {
            config,
            target_dll_name,
            warnings,
        },
    )
}

/// Move a mod's config and writable trees onto a new storage id (the TS
/// `modConfig.changeModId` -> `renameConfig`): registry renames both subkeys in
/// place, portable renames both INI files (the `[Mod]`/`[Settings]` sections
/// ride along in the config file). An absent tree is a no-op. The rename step of
/// the rename-install commit, kept a named function in `orchestrate`.
fn change_mod_id(session: &SessionInner, from: &str, to: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    let backend = storage.backend();
    backend
        .rename_tree(&storage.mod_config_tree(from), &storage.mod_config_tree(to))
        .wire()?;
    backend
        .rename_tree(
            &storage.mod_writable_tree(from),
            &storage.mod_writable_tree(to),
        )
        .wire()?;
    Ok(())
}

/// Write the install/recompile config (the TS `setModConfig` config half)
/// through the shared `write_mod_config_patch` - the SAME path
/// `updateModConfig` uses - collapsing the former subset/superset writer pair
/// onto the typed patch. FIVE fields are FORCED to `Some`, in TWO source
/// categories: the four metadata-derived Include/Exclude/Architecture/Version
/// (`None` -> empty, so a source with no `@exclude` still writes `Exclude=""`,
/// pinned by the storage-registry-fresh-install fixture) and LibraryFileName
/// (sourced from the COMPILE OUTPUT, not the metadata).
/// `disabled`/`logging_enabled` thread through as genuine `Option` (absent =
/// preserve, so the field is skipped); the recompile path passes `None`/`None`
/// so a recompile PRESERVES them while still writing the fresh DLL name. Field
/// write-ORDER is non-observable end-to-end.
fn write_install_config(
    session: &SessionInner,
    storage_id: &str,
    metadata: &ModMetadata,
    target_dll_name: &str,
    disabled: Option<bool>,
    logging_enabled: Option<bool>,
) -> Result<(), CoreError> {
    let storage = session.storage();
    let mut tree = open_tree(storage, &storage.mod_config_tree(storage_id), true)?;
    let patch = ModConfigPatch {
        library_file_name: Some(target_dll_name.to_owned()),
        disabled,
        logging_enabled,
        include: Some(metadata.include.clone().unwrap_or_default()),
        exclude: Some(metadata.exclude.clone().unwrap_or_default()),
        architecture: Some(metadata.architecture.clone().unwrap_or_default()),
        version: Some(metadata.version.clone().unwrap_or_default()),
        ..Default::default()
    };
    write_mod_config_patch(&mut *tree, &patch)
}
