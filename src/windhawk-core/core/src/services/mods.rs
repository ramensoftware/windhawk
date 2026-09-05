//! `services::mods`: the mod config/settings commands over the single shared
//! `SettingsBackend`, the mod-source file I/O (`getModSource`, `doesModExist`),
//! the composite `listInstalledMods` and its single-mod twin
//! `getInstalledModDetails`, and `removeMod`. The parsing they consume
//! lives in `domain` (a port of `services/modSource.ts`); the profile half of
//! `listInstalledMods` goes through `services::profile`. The install flows live
//! in `services::install`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde_json::{Map, Number, Value};
use windhawk_core_domain::{ModId, extract_metadata, is_valid_flat_key};
use windhawk_core_ports::{Files, SettingsTree, TreeValue};
use windhawk_core_protocol::{
    GetInstalledModDetailsParams, InstalledModListEntry, ListInstalledModsParams,
    ListInstalledModsResult, ModConfig, ModIdParams, ModLoadError, ModMetadata,
    SetModEnabledParams, SetModLoggingEnabledParams, SetModSettingsParams, UpdateModConfigParams,
    is_valid_suppression,
};

use crate::convert::metadata_to_protocol;
use crate::dispatch::{check_storage_id, decode_params};
use crate::error::CoreError;
use crate::services::profile::{mirror_mod, read_modify_write, read_profile};
use crate::services::settings_io::{
    UPDATES_DISABLED_FOR_VERSION, open_tree, read_array, read_bool, read_string, write_bool,
    write_mod_config_patch,
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
        // Absent and empty both collapse to `""`, the "updates are offered"
        // arm - so a mod installed before this field existed and a mod whose
        // updates were reenabled take the same path, whichever of the two a
        // given backend reports for a name that is not there.
        updates_disabled_for_version: read_string(tree, UPDATES_DISABLED_FOR_VERSION)?
            .unwrap_or_default(),
    }))
}

/// `getModConfig`: the full config, or `null` when the mod is not installed.
pub fn get_mod_config(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModConfig", params)?;
    check_storage_id("getModConfig", "modId", &params.mod_id)?;
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
///
/// `updatesDisabledForVersion` is checked against its grammar before anything is
/// written, so a mistyped value cannot be stored and then honored by nothing.
/// The check is here rather than down in the shared writer because the import
/// restore drives that one directly and is deliberately exempt: the archive is
/// the authority on user-owned config, and a value outside the grammar reads as
/// "not suppressed" either way.
pub fn update_mod_config(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: UpdateModConfigParams = decode_params("updateModConfig", params)?;
    check_storage_id("updateModConfig", "modId", &params.mod_id)?;
    if let Some(value) = &params.patch.updates_disabled_for_version
        && !is_valid_suppression(value)
    {
        return Err(CoreError::invalid_request(format!(
            "updateModConfig: invalid updatesDisabledForVersion {value:?}; \
             expected \"\" (updates on), \"*\" (all versions), or \"=<version>\""
        )));
    }
    apply_mod_config_patch(session, &params.mod_id, &params.patch)?;
    Ok(Value::Null)
}

/// The write half of `updateModConfig` without the envelope decode: an empty
/// patch is a no-op (opens no tree), otherwise the present fields are written to
/// the mod-config tree, and the mirrored ones are copied into the profile
/// ([`mirror_config_fields`]).
///
/// [`write_config_patch`] is the same write WITHOUT the mirror, for a caller
/// that discharges the obligation in a profile write of its own.
pub(crate) fn apply_mod_config_patch(
    session: &SessionInner,
    mod_id: &str,
    patch: &windhawk_core_protocol::ModConfigPatch,
) -> Result<(), CoreError> {
    write_config_patch(session, mod_id, patch)?;
    mirror_config_fields(
        session,
        mod_id,
        patch.disabled,
        patch.updates_disabled_for_version.as_deref(),
    );
    Ok(())
}

/// The config-tree half alone. `services::user_data`'s import drives this
/// directly (under its own keyed `Mod` lock, rather than through the dispatch
/// that decodes the params and resolves that lock): it takes a profile write
/// per restored mod anyway, and folds the suppression mirror into that one
/// rather than paying a second read-modify-write for it.
pub(crate) fn write_config_patch(
    session: &SessionInner,
    mod_id: &str,
    patch: &windhawk_core_protocol::ModConfigPatch,
) -> Result<(), CoreError> {
    if !patch.has_any() {
        return Ok(());
    }
    let storage = session.storage();
    let mut tree = open_tree(storage, &storage.mod_config_tree(mod_id), true)?;
    write_mod_config_patch(tree.as_mut(), patch)
}

/// Copy the mod-config fields the profile keeps its own copy of into a mod's
/// profile entry: `disabled` and `updatesDisabledForVersion`, two of the config
/// tree's thirteen. An absent one is left as the profile holds it, and both go in
/// ONE read-modify-write - a config write that moves both is one change, and two
/// writes would leave a window where the profile agreed with neither the old tree
/// nor the new one. The tree is written first and stays authoritative.
///
/// The copy is not only a convenience for a reader that cannot reach the config
/// tree (the app's mod-update count is one). It is also what tells a GUI that
/// somebody else disabled a mod or turned an offer down: the profile watcher fires
/// on the profile FILE changing, so a write from another process reaches the
/// screen only because this one lands beside it. Do not drop the copy as
/// redundant on the strength of the config tree owning the value - the tree is not
/// watched.
///
/// The profile's third mirrored field, a mod's `version`, is deliberately not
/// here: recording it is install bookkeeping (it drops the cached `latestVersion`
/// with it, [`Profile::set_mod_version`](windhawk_core_domain::Profile::set_mod_version)),
/// so the install commit owns that write and a config patch is not its writer.
pub(crate) fn mirror_config_fields(
    session: &SessionInner,
    mod_id: &str,
    disabled: Option<bool>,
    suppression: Option<&str>,
) {
    // What a failure to take the copy names in the log: the fields it was of, so a
    // warning says which copy is behind until the next sync converges it.
    let what = match (disabled.is_some(), suppression.is_some()) {
        (true, true) => "enabled state and update suppression",
        (true, false) => "enabled state",
        (false, true) => "update suppression",
        (false, false) => return,
    };
    mirror_mod(session, mod_id, what, |profile| {
        let mut changed = false;
        if let Some(disabled) = disabled {
            // Unconditionally, as `setModEnabled`'s own mirror writes: the profile
            // setter reports no change to gate on.
            profile.set_mod_disabled(mod_id, disabled);
            changed = true;
        }
        if let Some(stored) = suppression {
            // Reports whether it moved, which is what keeps a suppression the
            // profile already holds from costing a write.
            changed |= profile.set_mod_updates_disabled_for_version(mod_id, stored);
        }
        changed
    });
}

/// The suppression alone, for the install commits that write that one field and
/// mirror it on its own (`services::install`).
pub(crate) fn mirror_update_suppression(session: &SessionInner, mod_id: &str, stored: &str) {
    mirror_config_fields(session, mod_id, None, Some(stored));
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
/// `preserve_order`).
pub(crate) fn write_mod_settings(
    session: &SessionInner,
    mod_id: &str,
    settings: &Map<String, Value>,
) -> Result<(), CoreError> {
    let storage = session.storage();

    // Check the WHOLE map - names and values alike - before the clear below: the
    // write replaces the tree wholesale, so an entry rejected part way through
    // would leave the mod with a truncated settings section.
    let values = settings
        .iter()
        .map(|(name, value)| {
            check_setting_name(name)?;
            Ok((name.as_str(), stored_value(name, value)?))
        })
        .collect::<Result<Vec<_>, CoreError>>()?;

    // Clear the existing settings (registry: delete the subkey; INI: remove
    // the section), matching the TS deleteTree / whole-section replacement.
    storage
        .backend()
        .remove_tree(&storage.mod_settings_tree(mod_id))
        .wire()?;

    {
        let mut tree = open_tree(storage, &storage.mod_settings_tree(mod_id), true)?;
        for (name, value) in values {
            match value {
                StoredValue::Str(s) => tree.set_string(name, s).wire()?,
                StoredValue::Int(i) => tree.set_int(name, i).wire()?,
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
    check_storage_id("getModSettings", "modId", &params.mod_id)?;
    Ok(Value::Object(read_mod_settings(session, &params.mod_id)?))
}

/// `setModSettings`: replace the whole `[Settings]` tree and stamp
/// `SettingsChangeTime`.
pub fn set_mod_settings(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModSettingsParams = decode_params("setModSettings", params)?;
    check_storage_id("setModSettings", "modId", &params.mod_id)?;
    write_mod_settings(session, &params.mod_id, &params.settings)?;
    Ok(Value::Null)
}

/// `setModLoggingEnabled`: the scoped single-field `LoggingEnabled` write
/// (the editor sidebar's logging toggle).
pub fn set_mod_logging_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModLoggingEnabledParams = decode_params("setModLoggingEnabled", params)?;
    check_storage_id("setModLoggingEnabled", "modId", &params.mod_id)?;
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

/// One settings name checked for the store: the engine's flat notation
/// (`Scalar`, `Group.child`, `List[0]`, `Matrix[0].cell`), which is every name a
/// mod's settings can flatten to. A name outside it is not merely unknown - the
/// portable INI emits a value name verbatim ahead of the `=`, so one carrying a
/// line break or a leading `[` writes extra lines into the file that also holds
/// the mod's `[Mod]` config. That backend refuses such a name, but only AFTER
/// the clear, leaving the mod with no settings at all; the registry backend
/// takes it and stores a key nothing reads. Checking here is what gets both
/// storage modes to the same answer, before anything is destroyed.
fn check_setting_name(name: &str) -> Result<(), CoreError> {
    if is_valid_flat_key(name) {
        return Ok(());
    }
    Err(CoreError::invalid_request(format!(
        "setting name {name:?} is not a valid settings key"
    )))
}

/// A settings value in the two forms the store holds: `REG_SZ` / an INI string,
/// or `REG_DWORD` / an INI decimal.
enum StoredValue<'a> {
    Str(&'a str),
    Int(i32),
}

/// One settings value typed for the store: a string verbatim, or a number that
/// is an integer in the DWORD range. Nothing else is representable there, and
/// because the write replaces the whole tree, accepting an unrepresentable value
/// would not merely mistype the key - it would DELETE it (a float silently
/// truncating, an out-of-range integer wrapping, a `null` writing nothing at
/// all). The same string-or-int32 rule the user-data archive enforces on an
/// imported settings map (`domain::user_data::validate`), applied to every
/// caller that writes the tree.
fn stored_value<'a>(name: &str, value: &'a Value) -> Result<StoredValue<'a>, CoreError> {
    match value {
        Value::String(s) => Ok(StoredValue::Str(s)),
        Value::Number(n) => n
            .as_i64()
            .and_then(|i| i32::try_from(i).ok())
            .map(StoredValue::Int)
            .ok_or_else(|| {
                CoreError::invalid_request(format!(
                    "setting {name:?} must be a 32-bit integer; {n} is not"
                ))
            }),
        _ => Err(CoreError::invalid_request(format!(
            "setting {name:?} must be a string or a 32-bit integer"
        ))),
    }
}

/// The mod-source decode: UTF-8 or nothing. A source file that is not valid
/// UTF-8 is REJECTED rather than repaired with replacement characters, because
/// every consumer either compiles the text or writes it back (the editor round
/// trip), and a lossy decode would silently make the mod something other than
/// what is on disk. `None` is the caller's to classify - a command fails, the
/// installed-mod scan records a per-mod load error.
fn decode_mod_source(bytes: Vec<u8>) -> Option<String> {
    String::from_utf8(bytes).ok()
}

/// The bare cause a non-UTF-8 source file is reported with, shared by the
/// command and the scan so the two cannot drift.
const NOT_UTF8: &str = "source file is not valid UTF-8";

/// `getModSource`: the stored source file of a mod. A missing file maps to
/// `MOD_NOT_INSTALLED` (the TS path rejected with the raw ENOENT; the native
/// backend maps it); a file that is not valid UTF-8 maps to `IO_FAILED`.
pub fn get_mod_source(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModSource", params)?;
    check_storage_id("getModSource", "modId", &params.mod_id)?;
    let path = session.storage().mod_source_file(&params.mod_id);
    match session.deps().files.read(&path) {
        Ok(bytes) => decode_mod_source(bytes).map(Value::String).ok_or_else(|| {
            CoreError::io_failed(
                format!("Mod '{}': {NOT_UTF8}", params.mod_id),
                path.display().to_string(),
                None,
            )
        }),
        Err(e) if e.is_not_found() => Err(CoreError::mod_not_installed(params.mod_id)),
        Err(e) => Err(file_err(e)),
    }
}

/// `doesModExist`: whether a storage id is occupied by a source file or a
/// config entry (the TS `doesSourceExist || doesConfigExist`).
pub fn does_mod_exist(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("doesModExist", params)?;
    check_storage_id("doesModExist", "modId", &params.mod_id)?;
    let source_exists = session
        .deps()
        .files
        .exists(&session.storage().mod_source_file(&params.mod_id));
    let exists = source_exists || does_config_exist(session, &params.mod_id)?;
    Ok(Value::Bool(exists))
}

/// Scan the mods-source directory and extract each mod's metadata (the TS
/// `getMetadataOfInstalled`). A missing directory yields no mods; a per-file
/// read, decode, or parse failure becomes a `loadError` carrying the bare
/// cause rather than failing the command. Every consumer names the mod it
/// belongs to when rendering it, so the cause carries no label of its own.
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
        match read_mod_metadata(session, mod_id, language) {
            Ok(metadata) => {
                mods.insert(mod_id.to_owned(), metadata);
            }
            Err(error) => load_errors.push(ModLoadError {
                mod_id: mod_id.to_owned(),
                error,
            }),
        }
    }
    Ok((mods, load_errors))
}

/// One installed mod's source metadata, or the bare cause of the read, decode,
/// or parse failure that stood in the way - the string the listing carries as
/// that mod's `loadError`, and the reason a single-mod read has no metadata to
/// report.
fn read_mod_metadata(
    session: &SessionInner,
    mod_id: &str,
    language: &str,
) -> Result<ModMetadata, String> {
    let path = session.storage().mod_source_file(mod_id);
    let bytes = session
        .deps()
        .files
        .read(&path)
        .map_err(|e| e.message().to_owned())?;
    let source = decode_mod_source(bytes).ok_or_else(|| NOT_UTF8.to_owned())?;
    extract_metadata(&source, language)
        .map(metadata_to_protocol)
        .map_err(|e| e.to_string())
}

/// One mod's listing entry: what it is and how it is configured, decorated with
/// the two things only the user profile knows - the cached repository version
/// and the rating.
///
/// Built here for both readers, whether the entry came from the listing (which
/// calls this per mod) or from `getInstalledModDetails` (which calls it once),
/// so a single-mod read cannot answer differently from the listing that covers
/// it. The update answer is not among what it carries: an entry holds the terms,
/// and every consumer reaches the answer from them
/// ([`InstalledModListEntry::is_update_available`]).
fn installed_mod_entry(
    profile: &windhawk_core_domain::Profile,
    mod_id: &str,
    metadata: Option<ModMetadata>,
    config: Option<ModConfig>,
    check_for_updates: bool,
) -> InstalledModListEntry {
    InstalledModListEntry {
        metadata,
        config,
        // An empty cached version names nothing, so it is dropped where the
        // version is read rather than ruled out inside the update rule.
        latest_version: check_for_updates
            .then(|| profile.mod_latest_version(mod_id))
            .flatten()
            .filter(|latest| !latest.is_empty())
            .map(str::to_owned),
        // A rating is stored only when nonzero, so an absent one IS zero.
        user_rating: profile.mod_rating(mod_id).unwrap_or(0),
    }
}

/// `getInstalledModDetails`: the listing's entry for ONE mod, without the scan.
/// Three reads - the mod's source, its config, the profile - where the listing
/// reads every installed mod's source and every config to answer about one.
///
/// A mod with no source, or one whose source will not parse, has no metadata
/// here, exactly as its entry in the listing has none; the listing reports the
/// cause under `loadErrors`, which is a report about a scan and has no meaning
/// for a caller that named the mod itself.
///
/// The answer is the whole entry rather than the subset a caller asked about:
/// a caller reads one shape whichever way it got there, and a field the listing
/// gains arrives here without a decision.
pub fn get_installed_mod_details(
    session: &SessionInner,
    params: Value,
) -> Result<Value, CoreError> {
    let params: GetInstalledModDetailsParams = decode_params("getInstalledModDetails", params)?;
    check_storage_id("getInstalledModDetails", "modId", &params.mod_id)?;
    let metadata = read_mod_metadata(session, &params.mod_id, &params.language).ok();
    let config = read_mod_config(session, &params.mod_id)?;
    let entry = installed_mod_entry(
        &read_profile(session)?,
        &params.mod_id,
        metadata,
        config,
        params.check_for_updates,
    );
    to_value_result("getInstalledModDetails", &entry)
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
/// profile-derived `latestVersion`/`userRating`, and - when `syncProfile` - the
/// profile reconciliation (per-mod version/disabled refresh and removed-mod
/// cleanup, persisted as an external update). The decorations read values the
/// reconciliation does not touch, so they are independent of the write.
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
    let suppression_of = |id: &str| -> &str {
        config
            .get(id)
            .map_or("", |c| c.updates_disabled_for_version.as_str())
    };

    let build_entries =
        |profile: &windhawk_core_domain::Profile| -> BTreeMap<String, InstalledModListEntry> {
            let mut mods = BTreeMap::new();
            for id in &union {
                mods.insert(
                    id.clone(),
                    installed_mod_entry(
                        profile,
                        id,
                        metadata.get(id).cloned(),
                        config.get(id).cloned(),
                        params.check_for_updates,
                    ),
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
                // Converge the update-suppression mirror on the config tree,
                // which owns the value. The per-write mirrors keep it current
                // from here on; this is what carries a mod whose suppression
                // predates the mirror, or one written by a path that bypassed
                // it. Only a genuine difference marks the profile dirty, so a
                // sync over unsuppressed mods still writes nothing.
                if profile.set_mod_updates_disabled_for_version(id, suppression_of(id)) {
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
/// `enableMod`), then mirror the new state into the user profile so GUI and CLI
/// reads stay consistent. Always writes; existence/already-in-state checks are
/// the caller's job.
pub fn set_mod_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModEnabledParams = decode_params("setModEnabled", params)?;
    check_storage_id("setModEnabled", "modId", &params.mod_id)?;

    {
        let storage = session.storage();
        let mut tree = open_tree(storage, &storage.mod_config_tree(&params.mod_id), true)?;
        // enableMod writes Disabled = enable ? 0 : 1.
        write_bool(tree.as_mut(), "Disabled", !params.enable)?;
    }

    mirror_mod(session, &params.mod_id, "enabled state", |profile| {
        profile.set_mod_disabled(&params.mod_id, !params.enable);
        true
    });

    Ok(Value::Null)
}

/// `removeMod`: uninstall a mod - delete its config, source, and DLLs, and drop
/// its profile entry for non-local mods (the TS `removeMod`). Data only: the
/// extension's editor-draft cleanup for `local@` mods is the caller's job, as
/// are existence checks and confirmation gates.
pub fn remove_mod(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("removeMod", params)?;
    check_storage_id("removeMod", "modId", &params.mod_id)?;
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
