//! `services::mods`: the mod config/settings commands over the single shared
//! `SettingsBackend`, the mod-source file I/O (`getModSource`, `doesModExist`),
//! the composite `listInstalledMods`, and `removeMod`. The parsing they consume
//! lives in `domain` (a port of `services/modSource.ts`); the profile half of
//! `listInstalledMods` goes through `services::profile`. The install flows live
//! in `services::install`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Map, Number, Value};
use windhawk_core_domain::{ModId, extract_metadata};
use windhawk_core_ports::{Files, SettingsTree, TreeValue};
use windhawk_core_protocol::{
    InstalledModListEntry, ListInstalledModsParams, ListInstalledModsResult, ModConfig,
    ModIdParams, ModLoadError, ModMetadata, SetModEnabledParams, SetModLoggingEnabledParams,
    SetModSettingsParams, UpdateModConfigParams,
};

use crate::convert::metadata_to_protocol;
use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::services::profile::{read_modify_write, read_profile};
use crate::services::settings_io::{
    open_tree, read_array, read_bool, read_string, write_bool, write_mod_config_patch,
};
use crate::services::wire::{WireResultExt, file_err, to_value_result};
use crate::session::SessionInner;

/// Read a mod's full config, or `None` when it is not installed (no, or empty,
/// `LibraryFileName` - the TS `!libraryFileName` gate, falsy for the empty
/// string too). Shared with `services::install` (the `compileInstalledMod`
/// read-back).
pub(crate) fn read_mod_config(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Option<ModConfig>, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_config_tree(mod_id), false)?;
    let tree: &dyn SettingsTree = &*tree;

    let library_file_name = match read_string(tree, "LibraryFileName")? {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(None),
    };

    Ok(Some(ModConfig {
        library_file_name,
        disabled: read_bool(tree, "Disabled")?,
        logging_enabled: read_bool(tree, "LoggingEnabled")?,
        debug_logging_enabled: read_bool(tree, "DebugLoggingEnabled")?,
        include: read_array(tree, "Include")?,
        exclude: read_array(tree, "Exclude")?,
        include_custom: read_array(tree, "IncludeCustom")?,
        exclude_custom: read_array(tree, "ExcludeCustom")?,
        include_exclude_custom_only: read_bool(tree, "IncludeExcludeCustomOnly")?,
        patterns_match_critical_system_processes: read_bool(
            tree,
            "PatternsMatchCriticalSystemProcesses",
        )?,
        architecture: read_array(tree, "Architecture")?,
        version: read_string(tree, "Version")?.unwrap_or_default(),
    }))
}

/// `getModConfig`: the full config, or `null` when the mod is not installed.
pub fn get_mod_config(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModConfig", params)?;
    match read_mod_config(session, &params.mod_id)? {
        Some(config) => to_value_result("getModConfig", &config),
        None => Ok(Value::Null),
    }
}

/// Whether a mod has a config (`!!LibraryFileName`, the TS `configExists`).
pub(crate) fn does_config_exist(session: &SessionInner, mod_id: &str) -> Result<bool, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_config_tree(mod_id), false)?;
    let tree: &dyn SettingsTree = &*tree;
    Ok(read_string(tree, "LibraryFileName")?.is_some_and(|s| !s.is_empty()))
}

/// `updateModConfig`: patch semantics (absent field = preserve). Fields are
/// written in the TS `CONFIG_FIELDS` order for human consistency only; per-key
/// write order is non-observable (see `write_mod_config_patch`). An empty patch
/// opens (and so creates) no tree, matching `applyAppSettings`'
/// open-only-when-non-empty policy.
pub fn update_mod_config(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: UpdateModConfigParams = decode_params("updateModConfig", params)?;
    apply_mod_config_patch(session, &params.mod_id, &params.patch)?;
    Ok(Value::Null)
}

/// The write half of `updateModConfig` without the envelope decode: an empty
/// patch is a no-op (opens no tree), otherwise the present fields are written to
/// the mod-config tree. Shared with `services::user_data`'s import, which drives
/// the config restore directly (under its own keyed `Mod` lock) rather than
/// through the dispatch that decodes the params and resolves that lock.
pub(crate) fn apply_mod_config_patch(
    session: &SessionInner,
    mod_id: &str,
    patch: &windhawk_core_protocol::ModConfigPatch,
) -> Result<(), CoreError> {
    if !patch.has_any() {
        return Ok(());
    }
    let storage = session.storage();
    let mut tree = open_tree(storage, &storage.mod_config_tree(mod_id), true)?;
    write_mod_config_patch(tree.as_mut(), patch)?;
    Ok(())
}

/// Read a mod's `[Settings]` tree as a name->(string|number) map. Registry
/// DWORDs come back as (signed) numbers and `REG_SZ` as strings; the portable
/// INI returns every value as a string (the TS asymmetry). Binary values are
/// skipped (the TS reads only DWORD/SZ). Shared with `services::install` (the
/// install settings migration's "current settings" read).
pub(crate) fn read_mod_settings(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Map<String, Value>, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_settings_tree(mod_id), false)?;
    let values = tree.enum_values().wire()?;
    let mut map = Map::new();
    for (name, value) in values {
        match value {
            TreeValue::Str(s) => {
                map.insert(name, Value::String(s));
            }
            TreeValue::Int(i) => {
                map.insert(name, Value::Number(Number::from(i)));
            }
            TreeValue::Binary(_) => {}
        }
    }
    Ok(map)
}

/// Replace a mod's whole `[Settings]` tree (clear then write), then stamp
/// `SettingsChangeTime` on the mod-config tree - the TS `writeAllSettings`.
/// Shared by `setModSettings` and the install settings migration. Values are
/// written in the map's order (insertion-ordered via `serde_json`'s
/// `preserve_order`); non-string/number values are ignored (the TS handles only
/// string and number).
pub(crate) fn write_mod_settings(
    session: &SessionInner,
    mod_id: &str,
    settings: &Map<String, Value>,
) -> Result<(), CoreError> {
    let storage = session.storage();

    // Clear the existing settings (registry: delete the subkey; INI: remove
    // the section), matching the TS deleteTree / whole-section replacement.
    storage
        .backend()
        .remove_tree(&storage.mod_settings_tree(mod_id))
        .wire()?;

    {
        let mut tree = open_tree(storage, &storage.mod_settings_tree(mod_id), true)?;
        for (name, value) in settings {
            if let Some(s) = value.as_str() {
                tree.set_string(name, s).wire()?;
            } else if value.is_number() {
                tree.set_int(name, json_number_to_i32(value)).wire()?;
            }
        }
    }

    let change_time = settings_change_time(session.deps().clock.now_ms());
    let mut config_tree = open_tree(storage, &storage.mod_config_tree(mod_id), true)?;
    config_tree
        .set_int("SettingsChangeTime", change_time)
        .wire()?;

    Ok(())
}

/// `getModSettings`: the `[Settings]` tree as a name->(string|number) object.
pub fn get_mod_settings(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModSettings", params)?;
    Ok(Value::Object(read_mod_settings(session, &params.mod_id)?))
}

/// `setModSettings`: replace the whole `[Settings]` tree and stamp
/// `SettingsChangeTime`.
pub fn set_mod_settings(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModSettingsParams = decode_params("setModSettings", params)?;
    write_mod_settings(session, &params.mod_id, &params.settings)?;
    Ok(Value::Null)
}

/// `setModLoggingEnabled`: the scoped single-field `LoggingEnabled` write
/// (the editor sidebar's logging toggle).
pub fn set_mod_logging_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModLoggingEnabledParams = decode_params("setModLoggingEnabled", params)?;
    let storage = session.storage();
    let mut tree = open_tree(storage, &storage.mod_config_tree(&params.mod_id), true)?;
    write_bool(tree.as_mut(), "LoggingEnabled", params.enable)?;
    Ok(Value::Null)
}

/// `getSettingsChangeTime`: Unix seconds masked to a positive signed 32-bit
/// integer.
fn settings_change_time(now_ms: i64) -> i32 {
    ((now_ms / 1000) & 0x7fff_ffff) as i32
}

/// A settings number to the stored DWORD bit pattern (the TS `value >>> 0`):
/// `value | 0` on read gives it back. Non-integer numbers truncate toward
/// zero like `ToUint32`.
fn json_number_to_i32(value: &Value) -> i32 {
    if let Some(i) = value.as_i64() {
        i as i32
    } else if let Some(f) = value.as_f64() {
        f as i64 as i32
    } else {
        0
    }
}

/// `getModSource`: the stored source file of a mod. A missing file maps to
/// `MOD_NOT_INSTALLED` (the TS path rejected with the raw ENOENT; the native
/// backend maps it).
pub fn get_mod_source(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModSource", params)?;
    let path = session.storage().mod_source_file(&params.mod_id);
    match session.deps().files.read(&path) {
        Ok(bytes) => Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.is_not_found() => Err(CoreError::mod_not_installed(params.mod_id)),
        Err(e) => Err(file_err(e)),
    }
}

/// `doesModExist`: whether a storage id is occupied by a source file or a
/// config entry (the TS `doesSourceExist || doesConfigExist`).
pub fn does_mod_exist(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("doesModExist", params)?;
    let source_exists = session
        .deps()
        .files
        .exists(&session.storage().mod_source_file(&params.mod_id));
    let exists = source_exists || does_config_exist(session, &params.mod_id)?;
    Ok(Value::Bool(exists))
}

/// Scan the mods-source directory and extract each mod's metadata (the TS
/// `getMetadataOfInstalled`). A missing directory yields no mods; a per-file
/// read or parse failure becomes a `loadError` carrying the bare cause rather
/// than failing the command. Every consumer names the mod it belongs to when
/// rendering it, so the cause carries no label of its own.
fn get_metadata_of_installed(
    session: &SessionInner,
    language: &str,
) -> Result<(BTreeMap<String, ModMetadata>, Vec<ModLoadError>), CoreError> {
    let dir = session.storage().mods_source_dir();
    let entries = match session.deps().files.list_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.is_not_found() => return Ok((BTreeMap::new(), Vec::new())),
        Err(e) => return Err(file_err(e)),
    };

    let mut mods = BTreeMap::new();
    let mut load_errors = Vec::new();
    for entry in entries {
        let Some(mod_id) = entry
            .is_file
            .then(|| entry.name.strip_suffix(".wh.cpp"))
            .flatten()
        else {
            continue;
        };
        let path = session.storage().mod_source_file(mod_id);
        let bytes = match session.deps().files.read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                load_errors.push(ModLoadError {
                    mod_id: mod_id.to_owned(),
                    error: e.message().to_owned(),
                });
                continue;
            }
        };
        let source = String::from_utf8_lossy(&bytes);
        match extract_metadata(&source, language) {
            Ok(metadata) => {
                mods.insert(mod_id.to_owned(), metadata_to_protocol(metadata));
            }
            Err(e) => load_errors.push(ModLoadError {
                mod_id: mod_id.to_owned(),
                error: e.to_string(),
            }),
        }
    }
    Ok((mods, load_errors))
}

/// Enumerate installed mod configs (the TS `getConfigOfInstalled`): registry
/// subkeys of `Engine\Mods`, or `.ini` files in the portable engine-mods
/// directory; each is parsed and included only when it has a `LibraryFileName`.
fn get_config_of_installed(
    session: &SessionInner,
) -> Result<BTreeMap<String, ModConfig>, CoreError> {
    let storage = session.storage();
    let ids: Vec<String> = if storage.portable() {
        match session.deps().files.list_dir(&storage.engine_mods_dir()) {
            Ok(entries) => entries
                .into_iter()
                .filter_map(|e| {
                    e.is_file
                        .then(|| e.name.strip_suffix(".ini").map(str::to_owned))
                        .flatten()
                })
                .collect(),
            Err(e) if e.is_not_found() => Vec::new(),
            Err(e) => return Err(file_err(e)),
        }
    } else {
        storage
            .backend()
            .list_subtrees(&storage.mods_config_root())
            .wire()?
    };

    let mut configs = BTreeMap::new();
    for id in ids {
        if let Some(config) = read_mod_config(session, &id)? {
            configs.insert(id, config);
        }
    }
    Ok(configs)
}

/// `listInstalledMods`: the composite installed-mods listing (the TS
/// `getInstalledMods` handler). Source metadata + config, decorated with the
/// profile-derived `updateAvailable`/`userRating`, and - when `syncProfile` -
/// the profile reconciliation (per-mod version/disabled refresh and
/// removed-mod cleanup, persisted as an external update). `updateAvailable`
/// and `userRating` read values the reconciliation does not touch, so they are
/// independent of the write.
pub fn list_installed_mods(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ListInstalledModsParams = decode_params("listInstalledMods", params)?;
    to_value_result("listInstalledMods", &list_installed(session, &params)?)
}

/// The typed listing behind `listInstalledMods`, for in-core callers
/// (`services::user_data`'s export), so composing services do not round-trip
/// through the wire `Value` shape.
pub(crate) fn list_installed(
    session: &SessionInner,
    params: &ListInstalledModsParams,
) -> Result<ListInstalledModsResult, CoreError> {
    let (metadata, load_errors) = get_metadata_of_installed(session, &params.language)?;
    let config = get_config_of_installed(session)?;

    // The union of source-derived and config-derived mod ids.
    let union: Vec<String> = metadata
        .keys()
        .chain(config.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let version_of = |id: &str| -> String {
        metadata
            .get(id)
            .and_then(|m| m.version.clone())
            .unwrap_or_default()
    };
    let disabled_of = |id: &str| -> bool { config.get(id).is_some_and(|c| c.disabled) };

    let build_entries =
        |profile: &windhawk_core_domain::Profile| -> BTreeMap<String, InstalledModListEntry> {
            let mut mods = BTreeMap::new();
            for id in &union {
                let version = version_of(id);
                let update_available = params.check_for_updates
                    && profile
                        .mod_latest_version(id)
                        .is_some_and(|latest| !latest.is_empty() && latest != version);
                let user_rating = profile.mod_rating(id).filter(|r| *r != 0).unwrap_or(0);
                mods.insert(
                    id.clone(),
                    InstalledModListEntry {
                        metadata: metadata.get(id).cloned(),
                        config: config.get(id).cloned(),
                        update_available,
                        user_rating,
                    },
                );
            }
            mods
        };

    let mods = if params.sync_profile {
        read_modify_write(session, true, |profile| {
            let mut updated = false;
            for id in &union {
                if ModId::str_is_local(id) {
                    continue;
                }
                if profile.update_mod_details(id, &version_of(id), disabled_of(id)) {
                    updated = true;
                }
            }
            let mods = build_entries(profile);
            let non_local: HashSet<String> = mods
                .keys()
                .filter(|&id| !ModId::str_is_local(id))
                .cloned()
                .collect();
            if profile.cleanup_removed_mods(&non_local) {
                updated = true;
            }
            (updated, mods)
        })?
    } else {
        build_entries(&read_profile(session)?)
    };

    Ok(ListInstalledModsResult { mods, load_errors })
}

////////////////////////////////////////////////////////////////////////////
// Use-case lifecycle: setModEnabled, removeMod (the flows `services::mods`
// owns). Both run under the exclusive keyed `Mod` write lock (acquired by
// dispatch) and mirror into the user profile for non-local mods through
// `services::profile` (rank 2, acquired internally).

/// `setModEnabled`: write the mod's `Disabled` config field (the TS
/// `enableMod`), then mirror the new state into the user profile for non-local
/// mods so GUI and CLI reads stay consistent (`local@` mods are not tracked).
/// Always writes; existence/already-in-state checks are the caller's job.
pub fn set_mod_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModEnabledParams = decode_params("setModEnabled", params)?;

    {
        let storage = session.storage();
        let mut tree = open_tree(storage, &storage.mod_config_tree(&params.mod_id), true)?;
        // enableMod writes Disabled = enable ? 0 : 1.
        write_bool(tree.as_mut(), "Disabled", !params.enable)?;
    }

    if !ModId::str_is_local(&params.mod_id) {
        read_modify_write(session, false, |profile| {
            profile.set_mod_disabled(&params.mod_id, !params.enable);
            (true, ())
        })?;
    }

    Ok(Value::Null)
}

/// `removeMod`: uninstall a mod - delete its config, source, and DLLs, and drop
/// its profile entry for non-local mods (the TS `removeMod`). Data only: the
/// extension's editor-draft cleanup for `local@` mods is the caller's job, as
/// are existence checks and confirmation gates.
pub fn remove_mod(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("removeMod", params)?;
    let mod_id = &params.mod_id;

    delete_mod_config(session, mod_id)?;
    delete_source(session, mod_id)?;
    crate::services::install::delete_mod_files(session, mod_id);

    if !ModId::str_is_local(mod_id) {
        read_modify_write(session, false, |profile| {
            profile.delete_mod(mod_id);
            (true, ())
        })?;
    }

    Ok(Value::Null)
}

/// Delete a mod's config (the TS `modConfig.deleteMod` -> `deleteConfig`): the
/// config tree and the writable tree, plus the per-mod `mod-storage` folder.
/// Registry mode removes the two subkey trees; portable mode deletes the two
/// INI files (the section-removing `remove_tree` would leave an empty file, but
/// the TS unlinks the whole file). The `mod-storage` removal is best effort in
/// both modes (the TS `rmSync` swallows errors).
fn delete_mod_config(session: &SessionInner, mod_id: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    let files = session.deps().files.clone();
    if storage.portable() {
        delete_file_if_present(files.as_ref(), &storage.mod_config_ini_path(mod_id))?;
        delete_file_if_present(files.as_ref(), &storage.mod_writable_ini_path(mod_id))?;
    } else {
        storage
            .backend()
            .remove_tree(&storage.mod_config_tree(mod_id))
            .wire()?;
        storage
            .backend()
            .remove_tree(&storage.mod_writable_tree(mod_id))
            .wire()?;
    }
    let _ = files.remove_dir_all(&storage.mod_storage_dir(mod_id));
    Ok(())
}

/// Delete a mod's source file, tolerating its absence (the TS `deleteSource`
/// ignores ENOENT, rethrows other errors). Shared with `services::install` (the
/// rename flow deletes the old id's source).
pub(crate) fn delete_source(session: &SessionInner, mod_id: &str) -> Result<(), CoreError> {
    let files = session.deps().files.clone();
    delete_file_if_present(files.as_ref(), &session.storage().mod_source_file(mod_id))
}

/// Write a mod's source file durably (the TS `setSource`: mkdir + write, here
/// the atomic-replace primitive that also creates the parent). Shared with
/// `services::install`.
pub(crate) fn set_source(
    session: &SessionInner,
    mod_id: &str,
    source: &str,
) -> Result<(), CoreError> {
    session
        .deps()
        .files
        .write_atomic(
            &session.storage().mod_source_file(mod_id),
            source.as_bytes(),
        )
        .wire()
}

/// Delete a file, treating a missing file as success (the TS unlink paths that
/// ignore ENOENT) and surfacing any other I/O failure as `IO_FAILED`.
fn delete_file_if_present(files: &dyn Files, path: &Path) -> Result<(), CoreError> {
    match files.delete_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.is_not_found() => Ok(()),
        Err(e) => Err(file_err(e)),
    }
}
