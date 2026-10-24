//! `services::mods`: the mod config/settings commands over the single shared
//! `SettingsBackend`, the mod-source file I/O (`getModSource`, `doesModExist`,
//! and `getModDefaultSettings`, the settings the stored source declares),
//! the composite `listInstalledMods` and its single-mod twin
//! `getInstalledModDetails`, and `removeMod`. The parsing they consume
//! lives in `domain` (a port of `services/modSource.ts`); the profile half of
//! `listInstalledMods` goes through `services::profile`. The install flows live
//! in `services::install`.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use serde_json::{Map, Number, Value};
use windhawk_core_domain::{
    ModId, extract_initial_settings_for_engine, extract_metadata, is_valid_flat_key,
};
use windhawk_core_ports::{Files, SettingsTree, TreeLocation, TreeValue};
use windhawk_core_protocol::{
    DynamicSelectOption, GetInstalledModDetailsParams, InstalledModListEntry,
    ListInstalledModsParams, ListInstalledModsResult, ModConfig, ModConfigPatch, ModIdParams,
    ModLoadError, ModMetadata, SetModEnabledParams, SetModLoggingEnabledParams,
    SetModSettingsParams, UpdateModConfigParams, UserScope, is_valid_suppression,
};

use crate::convert::metadata_to_protocol;
use crate::dispatch::{check_storage_id, decode_params};
use crate::error::CoreError;
use crate::services::install::engine_items_to_map;
use crate::services::nuke;
use crate::services::profile::{mirror_mod, read_modify_write, read_profile};
use crate::services::settings_io::{
    DISABLED, UPDATES_DISABLED_FOR_VERSION, open_tree, read_array, read_bool, read_string,
    write_bool, write_mod_config_patch,
};
use crate::services::storage::{Storage, mod_config_ids};
use crate::services::wire::{WireResultExt, file_err, to_value_result};
use crate::session::SessionInner;

////////////////////////////////////////////////////////////////////////////
// The configuration scope: a mod's global configuration, or the session user's
// own configuration of it (`Users\<SID>` under the global one). While the user
// has theirs switched on, the engine uses it in that user's processes and the
// reads and writes below address it; switched off, it is kept, and both use the
// global one. Portable storage has no user configurations: there every mod's
// scope is the global one, and a `Users` tree in a mod's file is ignored, as the
// engine ignores it.

/// The switch of a user configuration: whether it, rather than the global one,
/// is in use, by the engine in the user's processes and by the core.
pub(crate) const USER_SCOPED: &str = "UserScoped";

/// The settings change marker, stamped on the configuration whose settings a
/// write replaced.
pub(crate) const SETTINGS_CHANGE_TIME: &str = "SettingsChangeTime";

/// Which configuration of a mod the session's user has, and which one is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModConfigScope {
    /// No user configuration: the global one is in use.
    Global,
    /// The user has a configuration; `active` is whether it is switched on, and
    /// so in use rather than the global one.
    User { sid: String, active: bool },
}

impl ModConfigScope {
    pub(crate) fn user_scope(&self) -> UserScope {
        match self {
            ModConfigScope::Global => UserScope::None,
            ModConfigScope::User { active: false, .. } => UserScope::Inactive,
            ModConfigScope::User { active: true, .. } => UserScope::Active,
        }
    }

    /// The configuration the reads and writes of the user-scopeable values
    /// address: the one in use.
    pub(crate) fn edited(&self) -> ConfigTarget<'_> {
        match self {
            ModConfigScope::User { sid, active: true } => ConfigTarget::User(sid),
            _ => ConfigTarget::Global,
        }
    }
}

/// One of a mod's configurations, as a place to read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigTarget<'a> {
    Global,
    /// The configuration of the user with this (validated) SID.
    User(&'a str),
}

impl ConfigTarget<'_> {
    pub(crate) fn config_tree(self, storage: &Storage, mod_id: &str) -> TreeLocation {
        match self {
            ConfigTarget::Global => storage.mod_config_tree(mod_id),
            ConfigTarget::User(sid) => storage.mod_user_config_tree(mod_id, sid),
        }
    }

    pub(crate) fn settings_tree(self, storage: &Storage, mod_id: &str) -> TreeLocation {
        match self {
            ConfigTarget::Global => storage.mod_settings_tree(mod_id),
            ConfigTarget::User(sid) => storage.mod_user_settings_tree(mod_id, sid),
        }
    }
}

/// The session user's scope for a mod. The ONE reader of the two values that
/// decide it, by the engine's rule: a user configuration exists exactly when its
/// tree holds `Disabled` (what makes a half-written copy count as none), and
/// `UserScoped` says whether it is switched on.
pub(crate) fn resolve_scope(
    session: &SessionInner,
    mod_id: &str,
) -> Result<ModConfigScope, CoreError> {
    let sid = &session.user().sid;
    let storage = session.storage();
    if storage.portable() {
        return Ok(ModConfigScope::Global);
    }
    let tree = open_tree(storage, &storage.mod_user_config_tree(mod_id, sid), false)?;
    if tree.get_int(DISABLED).wire()?.is_none() {
        return Ok(ModConfigScope::Global);
    }
    Ok(ModConfigScope::User {
        sid: sid.clone(),
        active: read_bool(&*tree, USER_SCOPED)?,
    })
}

/// The SIDs of every user configuration a mod has, whoever it belongs to: the
/// trees under `Users` that hold the marker.
pub(crate) fn user_config_sids(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Vec<String>, CoreError> {
    let storage = session.storage();
    if storage.portable() {
        return Ok(Vec::new());
    }
    let mut sids = Vec::new();
    for sid in storage
        .backend()
        .list_subtrees(&storage.mod_users_tree(mod_id))
        .wire()?
    {
        let tree = open_tree(storage, &storage.mod_user_config_tree(mod_id, &sid), false)?;
        if tree.get_int(DISABLED).wire()?.is_some() {
            sids.push(sid);
        }
    }
    Ok(sids)
}

/// Read a mod's full config as the session's user sees it, or `None` when it is
/// not installed (no, or empty, `LibraryFileName` - the TS `!libraryFileName`
/// gate, falsy for the empty string too). Shared with `services::install` (the
/// `compileInstalledMod` read-back).
pub(crate) fn read_mod_config(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Option<ModConfig>, CoreError> {
    read_mod_config_as(session, mod_id, false)
}

/// [`read_mod_config`] with every value from the global configuration, whatever
/// the user sees - what the machine does for everyone without a configuration
/// of their own. `userScope` still reports the user's scope.
pub(crate) fn read_global_mod_config(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Option<ModConfig>, CoreError> {
    read_mod_config_as(session, mod_id, true)
}

fn read_mod_config_as(
    session: &SessionInner,
    mod_id: &str,
    global_only: bool,
) -> Result<Option<ModConfig>, CoreError> {
    let storage = session.storage();
    let global = open_tree(storage, &storage.mod_config_tree(mod_id), false)?;
    let global: &dyn SettingsTree = &*global;

    let library_file_name = match read_string(global, "LibraryFileName")? {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(None),
    };

    let scope = resolve_scope(session, mod_id)?;
    let user_tree = match scope.edited() {
        ConfigTarget::User(_) if !global_only => Some(open_tree(
            storage,
            &scope.edited().config_tree(storage, mod_id),
            false,
        )?),
        _ => None,
    };
    // The user-scopeable values come from the edited configuration, the ones
    // describing the installed mod from the global one (the split of
    // `ModConfigPatch::split_by_scope`).
    let scopeable: &dyn SettingsTree = user_tree.as_deref().unwrap_or(global);

    Ok(Some(ModConfig {
        library_file_name,
        disabled: read_bool(scopeable, DISABLED)?,
        logging_enabled: read_bool(scopeable, "LoggingEnabled")?,
        debug_logging_enabled: read_bool(scopeable, "DebugLoggingEnabled")?,
        include: read_array(global, "Include")?,
        exclude: read_array(global, "Exclude")?,
        include_custom: read_array(scopeable, "IncludeCustom")?,
        exclude_custom: read_array(scopeable, "ExcludeCustom")?,
        include_exclude_custom_only: read_bool(scopeable, "IncludeExcludeCustomOnly")?,
        patterns_match_critical_system_processes: read_bool(
            scopeable,
            "PatternsMatchCriticalSystemProcesses",
        )?,
        architecture: read_array(global, "Architecture")?,
        version: read_string(global, "Version")?.unwrap_or_default(),
        // Absent and empty both collapse to `""`, the "updates are offered"
        // arm - so a mod installed before this field existed and a mod whose
        // updates were reenabled take the same path, whichever of the two a
        // given backend reports for a name that is not there.
        updates_disabled_for_version: read_string(global, UPDATES_DISABLED_FOR_VERSION)?
            .unwrap_or_default(),
        user_scope: scope.user_scope(),
        nuked: nuke::stored_reason(global)?,
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
    Ok(configured_dll_name(session, mod_id)?.is_some())
}

/// The compiled DLL a mod's config names (a non-empty `LibraryFileName`).
pub(crate) fn configured_dll_name(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Option<String>, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_config_tree(mod_id), false)?;
    let tree: &dyn SettingsTree = &*tree;
    Ok(read_string(tree, "LibraryFileName")?.filter(|s| !s.is_empty()))
}

/// `updateModConfig`: patch semantics (absent field = preserve). Fields are
/// written in the TS `CONFIG_FIELDS` order for human consistency only; per-key
/// write order is non-observable (see `write_mod_config_patch`). An empty patch
/// opens (and so creates) no tree, matching `applyAppSettings`'
/// open-only-when-non-empty policy.
///
/// `updatesDisabledForVersion` is checked against its grammar before anything is
/// written, so a mistyped value cannot be stored and then honored by nothing,
/// and a patch enabling a nuked mod is refused ([`nuke::refuse_enable`]). The
/// checks are here rather than down in the shared writer because the import
/// restore drives that one directly and is deliberately exempt: the archive is
/// the authority on user-owned config, a value outside the grammar reads as
/// "not suppressed" either way, and the engine runs no nuked mod whatever its
/// switch says.
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
    if params.patch.disabled == Some(false) {
        nuke::refuse_enable(session, &params.mod_id)?;
    }
    apply_mod_config_patch(session, &params.mod_id, &params.patch)?;
    Ok(Value::Null)
}

/// The write half of `updateModConfig` without the envelope decode: the patch
/// is written to the configuration the user edits ([`write_config_patch_in`]),
/// and the mirrored fields that landed in the global one are copied into the
/// profile ([`mirror_config_fields`]). A user configuration is the user's alone,
/// while the profile speaks for the machine, so a `disabled` written there is
/// not mirrored.
pub(crate) fn apply_mod_config_patch(
    session: &SessionInner,
    mod_id: &str,
    patch: &ModConfigPatch,
) -> Result<(), CoreError> {
    let scope = resolve_scope(session, mod_id)?;
    let target = scope.edited();
    write_config_patch_in(session, mod_id, patch, target)?;
    let disabled = match target {
        ConfigTarget::Global => patch.disabled,
        ConfigTarget::User(_) => None,
    };
    mirror_config_fields(
        session,
        mod_id,
        disabled,
        patch.updates_disabled_for_version.as_deref(),
    );
    Ok(())
}

/// Write a patch without the profile mirror: its user-scopeable fields to
/// `target`, the fields describing the installed mod to the global
/// configuration ([`ModConfigPatch::split_by_scope`]). A side with nothing to
/// write opens (and so creates) no tree. `services::user_data`'s import drives
/// this directly with the global target (under its own keyed `Mod` lock, rather
/// than through the dispatch that decodes the params and resolves that lock): it
/// takes a profile write per restored mod anyway, and folds the suppression
/// mirror into that one rather than paying a second read-modify-write for it.
pub(crate) fn write_config_patch_in(
    session: &SessionInner,
    mod_id: &str,
    patch: &ModConfigPatch,
    target: ConfigTarget<'_>,
) -> Result<(), CoreError> {
    let storage = session.storage();
    let write = |location: TreeLocation, patch: &ModConfigPatch| {
        if !patch.has_any() {
            return Ok(());
        }
        let mut tree = open_tree(storage, &location, true)?;
        write_mod_config_patch(tree.as_mut(), patch)
    };
    match target {
        ConfigTarget::Global => write(storage.mod_config_tree(mod_id), patch),
        ConfigTarget::User(_) => {
            let (machine, scopeable) = patch.split_by_scope();
            write(storage.mod_config_tree(mod_id), &machine)?;
            write(target.config_tree(storage, mod_id), &scopeable)
        }
    }
}

/// Copy the mod-config fields the profile keeps its own copy of into a mod's
/// profile entry: `disabled` and `updatesDisabledForVersion`, two of the config
/// tree's fourteen. An absent one is left as the profile holds it, and both go in
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

/// Read the settings the session's user sees as a name->(string|number) map
/// ([`read_mod_settings_in`]).
pub(crate) fn read_mod_settings(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Map<String, Value>, CoreError> {
    let scope = resolve_scope(session, mod_id)?;
    read_mod_settings_in(session, mod_id, scope.edited())
}

/// Read one configuration's `Settings` tree as a name->(string|number) map.
/// Registry DWORDs come back as (signed) numbers and `REG_SZ` as strings; the
/// portable INI returns every value as a string (the TS asymmetry). Binary
/// values are skipped (the TS reads only DWORD/SZ). Shared with
/// `services::install` (the install settings migration's "current settings"
/// read).
pub(crate) fn read_mod_settings_in(
    session: &SessionInner,
    mod_id: &str,
    target: ConfigTarget<'_>,
) -> Result<Map<String, Value>, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &target.settings_tree(storage, mod_id), false)?;
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

/// Replace the settings the session's user edits ([`write_mod_settings_in`]).
pub(crate) fn write_mod_settings(
    session: &SessionInner,
    mod_id: &str,
    settings: &Map<String, Value>,
) -> Result<(), CoreError> {
    let scope = resolve_scope(session, mod_id)?;
    write_mod_settings_in(session, mod_id, scope.edited(), settings)
}

/// Replace the values of one configuration's `Settings` tree with `settings`,
/// then stamp `SettingsChangeTime` on that configuration's tree - the TS
/// `writeAllSettings`. Shared by `setModSettings`, the install settings
/// migration and the import.
///
/// A tree that already holds the map is left as it is. Otherwise a store that
/// can replace the whole tree in one write (the portable INI section) takes the
/// map that way: every write to the file waits for its readers, and every
/// injected engine re-reads the file after each one. Elsewhere a value the tree
/// already holds is not written again, and the new values go in before the
/// stale ones come out, so a write cut short - a failing call, the process
/// dying - can leave a setting at its old value or a stale one in place, but
/// does not take away one the map keeps (bar a name whose case changed, see
/// [`plan_settings_write`]). Values are written in the map's order
/// (insertion-ordered via `serde_json`'s `preserve_order`).
pub(crate) fn write_mod_settings_in(
    session: &SessionInner,
    mod_id: &str,
    target: ConfigTarget<'_>,
    settings: &Map<String, Value>,
) -> Result<(), CoreError> {
    let storage = session.storage();
    let settings_tree = target.settings_tree(storage, mod_id);

    let plan = {
        let tree = open_tree(storage, &settings_tree, false)?;
        plan_settings_write(&*tree, settings, storage.portable())?
    };

    {
        let mut tree = open_tree(storage, &settings_tree, true)?;
        let changed = !plan.writes.is_empty() || !plan.stale.is_empty();
        let replaced = changed && tree.replace_values(&plan.tree_values()).wire()?;
        if !replaced {
            for write in plan.writes {
                if let Some(stored) = &write.recased {
                    tree.remove(stored).wire()?;
                }
                match write.value {
                    StoredValue::Str(s) => tree.set_string(write.name, s).wire()?,
                    StoredValue::Int(i) => tree.set_int(write.name, i).wire()?,
                }
            }
            for name in plan.stale {
                tree.remove(&name).wire()?;
            }
        }
    }

    let mut config_tree = open_tree(storage, &target.config_tree(storage, mod_id), true)?;
    let previous = config_tree.get_int(SETTINGS_CHANGE_TIME).wire()?;
    let change_time = settings_change_time(session.deps().clock.now_ms(), previous);
    config_tree
        .set_int(SETTINGS_CHANGE_TIME, change_time)
        .wire()?;

    Ok(())
}

/// One value a settings write sets.
struct SettingWrite<'a> {
    name: &'a str,
    value: StoredValue<'a>,
    /// The stored name `name` matches when the two differ only in case,
    /// removed just before the write.
    recased: Option<String>,
}

/// What replacing a tree's values with a settings map takes.
struct SettingsWritePlan<'a> {
    /// Every setting, typed for the store, one per name ignoring case, in the
    /// map's order.
    values: Vec<(&'a str, StoredValue<'a>)>,
    /// The settings the tree does not already hold, in the map's order.
    writes: Vec<SettingWrite<'a>>,
    /// The stored names the map does not keep, removed after the writes.
    stale: Vec<String>,
}

impl SettingsWritePlan<'_> {
    /// `values` as the store's own values.
    fn tree_values(&self) -> Vec<(&str, TreeValue)> {
        self.values
            .iter()
            .map(|&(name, value)| (name, value.to_tree_value()))
            .collect()
    }
}

/// What replacing `tree`'s values with `settings` takes. Every name and every
/// value written is checked here, so a request the store cannot take is
/// refused before anything changes.
///
/// Both stores match value names ignoring case (every settings name is ASCII),
/// and a write to a name that differs from the stored one only in case replaces
/// the value but keeps the stored casing. So names are matched the same way,
/// and a setting whose casing changed has the stored name removed, then is
/// written under the new one. Map names that differ only in case are one
/// setting, as writing the map in order to an empty tree leaves it: the first
/// name with the last value.
fn plan_settings_write<'a>(
    tree: &dyn SettingsTree,
    settings: &'a Map<String, Value>,
    portable: bool,
) -> Result<SettingsWritePlan<'a>, CoreError> {
    let entries = tree.enum_entries().wire()?;
    // The stored entries by name ignoring case, in enumeration order: a portable
    // section can hold one name on more than one line.
    let mut stored_by_name: HashMap<String, Vec<&(String, Option<TreeValue>)>> =
        HashMap::with_capacity(entries.len());
    for entry in &entries {
        stored_by_name
            .entry(entry.0.to_ascii_lowercase())
            .or_default()
            .push(entry);
    }
    let mut values: Vec<(&str, StoredValue)> = Vec::with_capacity(settings.len());
    let mut kept: HashMap<String, usize> = HashMap::with_capacity(settings.len());
    for (name, value) in settings {
        check_setting_name(name)?;
        let value = stored_value(name, value)?;
        match kept.entry(name.to_ascii_lowercase()) {
            Entry::Occupied(slot) => values[*slot.get()].1 = value,
            Entry::Vacant(slot) => {
                slot.insert(values.len());
                values.push((name.as_str(), value));
            }
        }
    }
    let mut writes = Vec::new();
    for &(name, value) in &values {
        let stored = stored_by_name
            .get(&name.to_ascii_lowercase())
            .map_or(&[][..], Vec::as_slice);
        if stored.iter().any(|(held_name, held)| {
            held_name == name
                && held
                    .as_ref()
                    .is_some_and(|held| value.is_held_as(held, portable))
        }) {
            continue;
        }
        if let StoredValue::Str(s) = value {
            check_storable(tree, name, s)?;
        }
        let recased = stored
            .iter()
            .map(|(stored, _)| stored)
            .find(|stored| *stored != name)
            .cloned();
        writes.push(SettingWrite {
            name,
            value,
            recased,
        });
    }
    let stale = entries
        .into_iter()
        .map(|(stored, _)| stored)
        .filter(|stored| !kept.contains_key(&stored.to_ascii_lowercase()))
        .collect();
    Ok(SettingsWritePlan {
        values,
        writes,
        stale,
    })
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

/// The reserved local-storage name prefix under which a mod writes the options
/// of a `$dynamicSelect` setting: `::wh_select_option::<path>::<value>`, the
/// label as the stored string.
const DYNAMIC_SELECT_OPTION_PREFIX: &str = "::wh_select_option::";

/// Decode a local-storage value name against the dynamic-option pattern:
/// `(path, value)` for a well-formed name, `None` for anything else - a name
/// without the prefix (the mod's other local values), a prefixed name with no
/// `::` after the path, or an empty path. The value may be empty: `""` is a
/// value a selection can store, as an empty `$options` key is. The path is
/// colon-free by the parameter-key charset, so the split is at the FIRST `::`
/// past the prefix, and the value keeps any `::` of its own.
fn parse_dynamic_option_name(name: &str) -> Option<(&str, &str)> {
    let (path, value) = name
        .strip_prefix(DYNAMIC_SELECT_OPTION_PREFIX)?
        .split_once("::")?;
    (!path.is_empty()).then_some((path, value))
}

/// The label a stored value carries: a string verbatim, an int as its decimal
/// text (what the INI backend would have read anyway), nothing for a binary.
fn dynamic_option_label(value: TreeValue) -> Option<String> {
    match value {
        TreeValue::Str(s) => Some(s),
        TreeValue::Int(i) => Some(i.to_string()),
        TreeValue::Binary(_) => None,
    }
}

/// Read the options a mod wrote for its `$dynamicSelect` settings out of its
/// local-storage tree, grouped by setting path. Paths and the entries under
/// each keep the tree's enumeration order - the order the mod wrote them, which
/// is the one ordering a device list and a "Small / Medium / Large" list can
/// both be right under. An empty label is an absent option: it is how a mod
/// retires an entry it wrote earlier, and it keeps a half-written entry (name
/// set, label not yet) out of the dropdown. Nothing here reads the mod's
/// source: whether a path names a `$dynamicSelect` item is the consumer's
/// lookup, so a mod whose settings block fails to parse still answers. An
/// absent tree opens as empty in both backends and reads as no options.
fn read_dynamic_select_options(
    session: &SessionInner,
    mod_id: &str,
) -> Result<Vec<(String, Vec<DynamicSelectOption>)>, CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_local_storage_tree(mod_id), false)?;
    let mut groups: Vec<(String, Vec<DynamicSelectOption>)> = Vec::new();
    for (name, stored) in tree.enum_values().wire()? {
        let Some((path, value)) = parse_dynamic_option_name(&name) else {
            continue;
        };
        let Some(label) = dynamic_option_label(stored).filter(|label| !label.is_empty()) else {
            continue;
        };
        let slot = match groups.iter().position(|(p, _)| p == path) {
            Some(slot) => slot,
            None => {
                groups.push((path.to_owned(), Vec::new()));
                groups.len() - 1
            }
        };
        groups[slot].1.push(DynamicSelectOption {
            value: value.to_owned(),
            label,
        });
    }
    Ok(groups)
}

/// `getModDynamicSelectOptions`: the `LocalStorage` tree's dynamic options as a
/// setting-path -> options object, `{}` when the mod wrote none.
pub fn get_mod_dynamic_select_options(
    session: &SessionInner,
    params: Value,
) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModDynamicSelectOptions", params)?;
    check_storage_id("getModDynamicSelectOptions", "modId", &params.mod_id)?;
    let mut result = Map::new();
    for (path, options) in read_dynamic_select_options(session, &params.mod_id)? {
        result.insert(
            path,
            to_value_result("getModDynamicSelectOptions", &options)?,
        );
    }
    Ok(Value::Object(result))
}

/// `setModLoggingEnabled`: the single-field `LoggingEnabled` write (the editor
/// sidebar's logging toggle), to the configuration the user edits.
pub fn set_mod_logging_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModLoggingEnabledParams = decode_params("setModLoggingEnabled", params)?;
    check_storage_id("setModLoggingEnabled", "modId", &params.mod_id)?;
    let storage = session.storage();
    let scope = resolve_scope(session, &params.mod_id)?;
    let location = scope.edited().config_tree(storage, &params.mod_id);
    let mut tree = open_tree(storage, &location, true)?;
    write_bool(tree.as_mut(), "LoggingEnabled", params.enable)?;
    Ok(Value::Null)
}

/// The `SettingsChangeTime` that replaces `previous`: Unix seconds masked to a
/// positive signed 32-bit integer, or `previous + 1` when that is not later.
/// The engine applies a mod's settings only when the value differs from the
/// last one it read, which may be older than `previous`, so a stamp never
/// repeats an earlier one, not even for two writes in the same second.
fn settings_change_time(now_ms: i64, previous: Option<i32>) -> i32 {
    let now = ((now_ms / 1000) & 0x7fff_ffff) as i32;
    match previous {
        Some(previous) if previous >= now => previous.wrapping_add(1) & 0x7fff_ffff,
        _ => now,
    }
}

/// One settings name checked for the store: the engine's flat notation
/// (`Scalar`, `Group.child`, `List[0]`, `Matrix[0].cell`), which is every name a
/// mod's settings can flatten to. A name outside it is not merely unknown - the
/// portable INI emits a value name verbatim ahead of the `=`, so one carrying a
/// line break or a leading `[` writes extra lines into the file that also holds
/// the mod's `[Mod]` config. That backend refuses such a name, but only when the
/// write reaches it, with the values ahead of it already written; the registry
/// backend takes it and stores a key nothing reads. Checking here is what gets
/// both storage modes to the same answer, before anything is written.
fn check_setting_name(name: &str) -> Result<(), CoreError> {
    if is_valid_flat_key(name) {
        return Ok(());
    }
    Err(CoreError::invalid_request(format!(
        "setting name {name:?} is not a valid settings key"
    )))
}

/// One string settings value checked against the tree it goes to: a string the
/// backend's `set_string` would refuse is a request error naming the setting.
fn check_storable(tree: &dyn SettingsTree, name: &str, value: &str) -> Result<(), CoreError> {
    match tree.unstorable_string(value) {
        None => Ok(()),
        Some(why) => Err(CoreError::invalid_request(format!(
            "setting {name:?} cannot be stored: its value {why}"
        ))),
    }
}

/// A settings value in the two forms the store holds: `REG_SZ` / an INI string,
/// or `REG_DWORD` / an INI decimal.
#[derive(Clone, Copy)]
enum StoredValue<'a> {
    Str(&'a str),
    Int(i32),
}

impl StoredValue<'_> {
    fn to_tree_value(self) -> TreeValue {
        match self {
            StoredValue::Str(s) => TreeValue::Str(s.to_owned()),
            StoredValue::Int(i) => TreeValue::Int(i),
        }
    }

    /// Whether `held`, a value read back from the tree, is this one as the tree
    /// stores it. A portable INI file holds an int as its decimal text.
    fn is_held_as(&self, held: &TreeValue, portable: bool) -> bool {
        match (self, held) {
            (StoredValue::Str(s), TreeValue::Str(held)) => held == s,
            (StoredValue::Int(i), TreeValue::Int(held)) => held == i,
            (StoredValue::Int(i), TreeValue::Str(held)) => portable && *held == i.to_string(),
            _ => false,
        }
    }
}

/// One settings value typed for the store: a string verbatim, or a number that
/// is an integer in the DWORD range. Nothing else is representable there, and
/// accepting an unrepresentable value would report as stored a value the tree
/// does not hold (a float silently truncating, an out-of-range integer
/// wrapping, a `null` writing nothing at all). The same string-or-int32 rule
/// the user-data archive enforces on an imported settings map
/// (`domain::user_data::validate`), applied to every caller that writes the
/// tree.
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

/// The stored source file of a mod. A missing file maps to `MOD_NOT_INSTALLED`
/// (the TS path rejected with the raw ENOENT; the native backend maps it); a
/// file that is not valid UTF-8 maps to `IO_FAILED`. `#[track_caller]` with
/// direct `Err`s, so an error's origin is the command that read the source.
#[track_caller]
fn read_mod_source(session: &SessionInner, mod_id: &str) -> Result<String, CoreError> {
    let path = session.storage().mod_source_file(mod_id);
    match session.deps().files.read(&path) {
        Ok(bytes) => match decode_mod_source(bytes) {
            Some(source) => Ok(source),
            None => Err(CoreError::io_failed(
                format!("Mod '{mod_id}': {NOT_UTF8}"),
                path.display().to_string(),
                None,
            )),
        },
        Err(e) if e.is_not_found() => Err(CoreError::mod_not_installed(mod_id)),
        Err(e) => Err(file_err(e)),
    }
}

/// `getModSource`: the stored source file of a mod ([`read_mod_source`]).
pub fn get_mod_source(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModSource", params)?;
    check_storage_id("getModSource", "modId", &params.mod_id)?;
    read_mod_source(session, &params.mod_id).map(Value::String)
}

/// `getModDefaultSettings`: the settings a fresh install of the stored source
/// writes - its declared defaults in the store's flat name -> value form, with
/// the published-version workarounds for a store mod as at install - or `{}`
/// when it declares none. A settings block that fails to parse is `INTERNAL`,
/// as at install: the stored source parsed when it was installed.
pub fn get_mod_default_settings(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("getModDefaultSettings", params)?;
    check_storage_id("getModDefaultSettings", "modId", &params.mod_id)?;
    let source = read_mod_source(session, &params.mod_id)?;
    let apply_workarounds = !ModId::str_is_local(&params.mod_id);
    let items = match extract_initial_settings_for_engine(&source, apply_workarounds) {
        Ok(items) => items.unwrap_or_default(),
        Err(e) => return Err(CoreError::internal(e.to_string())),
    };
    Ok(Value::Object(engine_items_to_map(items)))
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

/// Enumerate installed mod configs (the TS `getConfigOfInstalled`): each config
/// on the machine ([`mod_config_ids`]) is parsed and included only when it has a
/// `LibraryFileName`.
fn get_config_of_installed(
    session: &SessionInner,
) -> Result<BTreeMap<String, ModConfig>, CoreError> {
    let mut configs = BTreeMap::new();
    for id in mod_config_ids(session)? {
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
    // The profile mirrors the machine's enabled state, so a mod whose
    // configuration in use is the user's own has its global one read for the
    // sync.
    let mut global_disabled: BTreeMap<&str, bool> = BTreeMap::new();
    if params.sync_profile {
        for (id, entry) in &config {
            let disabled = match entry.user_scope {
                UserScope::Active => {
                    read_global_mod_config(session, id)?.is_some_and(|c| c.disabled)
                }
                UserScope::None | UserScope::Inactive => entry.disabled,
            };
            global_disabled.insert(id, disabled);
        }
    }
    let disabled_of = |id: &str| -> bool { global_disabled.get(id).copied().unwrap_or(false) };
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
/// `enableMod`) in the configuration the user edits, then, when that is the
/// global one, mirror the new state into the user profile so GUI and CLI reads
/// stay consistent (see [`apply_mod_config_patch`] for why a user
/// configuration's is not). Always writes; existence/already-in-state checks are
/// the caller's job. Enabling a nuked mod is refused before anything is written
/// ([`nuke::refuse_enable`]); disabling one writes the user's preference, the
/// one that applies once the mod is restored.
pub fn set_mod_enabled(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModEnabledParams = decode_params("setModEnabled", params)?;
    check_storage_id("setModEnabled", "modId", &params.mod_id)?;
    if params.enable {
        nuke::refuse_enable(session, &params.mod_id)?;
    }

    let scope = resolve_scope(session, &params.mod_id)?;
    let target = scope.edited();
    {
        let storage = session.storage();
        let mut tree = open_tree(storage, &target.config_tree(storage, &params.mod_id), true)?;
        // enableMod writes Disabled = enable ? 0 : 1.
        write_bool(tree.as_mut(), DISABLED, !params.enable)?;
    }

    if target == ConfigTarget::Global {
        mirror_mod(session, &params.mod_id, "enabled state", |profile| {
            profile.set_mod_disabled(&params.mod_id, !params.enable);
            true
        });
    }

    Ok(Value::Null)
}

/// `removeMod`: uninstall a mod - delete its config, storage folder, source, and
/// DLLs, and drop its profile entry for non-local mods (the TS `removeMod`).
/// Data only: the extension's editor-draft cleanup for `local@` mods is the
/// caller's job, as are existence checks and confirmation gates.
pub fn remove_mod(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("removeMod", params)?;
    check_storage_id("removeMod", "modId", &params.mod_id)?;
    let mod_id = &params.mod_id;

    delete_mod_config(session, mod_id)?;
    let config_changed = Instant::now();
    crate::services::install::delete_mod_storage(session, mod_id, config_changed);
    delete_source(session, mod_id)?;
    crate::services::install::delete_mod_files(session, mod_id, config_changed);

    if !ModId::str_is_local(mod_id) {
        read_modify_write(session, false, |profile| {
            profile.delete_mod(mod_id);
            (true, ())
        })?;
    }

    Ok(Value::Null)
}

/// Delete a mod's config (the TS `modConfig.deleteMod` -> `deleteConfig`): the
/// config tree and the writable tree. Registry mode removes the two subkey
/// trees; portable mode deletes the two INI files (the section-removing
/// `remove_tree` would leave an empty file, but the TS unlinks the whole file).
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use windhawk_core_ports::SettingsError;

    use super::*;

    #[test]
    fn dynamic_option_names_decode_at_the_first_separator_past_the_prefix() {
        let parse = parse_dynamic_option_name;
        assert_eq!(
            parse("::wh_select_option::outputDevice::speakers"),
            Some(("outputDevice", "speakers"))
        );
        // A nested and an object-array path are one segment each to the split.
        assert_eq!(
            parse("::wh_select_option::audio.device::hp"),
            Some(("audio.device", "hp"))
        );
        // The value keeps every `::` of its own.
        assert_eq!(
            parse("::wh_select_option::dev::usb::0483:5740"),
            Some(("dev", "usb::0483:5740"))
        );
        // An empty value is a value, as an empty `$options` key is.
        assert_eq!(parse("::wh_select_option::dev::"), Some(("dev", "")));

        // Not an option: no prefix (the mod's other local values), the prefix
        // alone, a path with no value separator, an empty path.
        for name in [
            "LastDevice",
            "wh_select_option::dev::a",
            "::wh_select_option::",
            "::wh_select_option::dev",
            "::wh_select_option::::a",
        ] {
            assert_eq!(parse(name), None, "{name:?}");
        }
    }

    #[test]
    fn dynamic_option_labels_follow_the_stored_type() {
        assert_eq!(
            dynamic_option_label(TreeValue::Str("Speakers".into())),
            Some("Speakers".into())
        );
        assert_eq!(dynamic_option_label(TreeValue::Int(-2)), Some("-2".into()));
        assert_eq!(dynamic_option_label(TreeValue::Binary(vec![1])), None);
    }

    #[test]
    fn a_settings_change_time_never_repeats_the_one_it_replaces() {
        let now_ms = 1_700_000_000_500;
        let now = 1_700_000_000;
        assert_eq!(settings_change_time(now_ms, None), now);
        assert_eq!(settings_change_time(now_ms, Some(now - 5)), now);
        // A negative value, which no stamp is, counts as earlier.
        assert_eq!(settings_change_time(now_ms, Some(-1)), now);

        // Two writes in one second, and a stamp ahead of the clock after them.
        assert_eq!(settings_change_time(now_ms, Some(now)), now + 1);
        assert_eq!(settings_change_time(now_ms, Some(now + 3)), now + 4);

        // Past the top of the positive range, the stamp starts over at zero.
        assert_eq!(settings_change_time(now_ms, Some(i32::MAX)), 0);
    }

    /// A tree that enumerates the entries it is given, repeated names and
    /// values of no modeled type included, and is only ever read.
    struct EnumeratedTree(Vec<(&'static str, Option<TreeValue>)>);

    impl SettingsTree for EnumeratedTree {
        fn get_string(&self, _: &str) -> Result<Option<String>, SettingsError> {
            unreachable!()
        }
        fn set_string(&mut self, _: &str, _: &str) -> Result<(), SettingsError> {
            unreachable!()
        }
        fn unstorable_string(&self, _: &str) -> Option<&'static str> {
            None
        }
        fn get_int(&self, _: &str) -> Result<Option<i32>, SettingsError> {
            unreachable!()
        }
        fn set_int(&mut self, _: &str, _: i32) -> Result<(), SettingsError> {
            unreachable!()
        }
        fn get_binary(&self, _: &str) -> Result<Option<Vec<u8>>, SettingsError> {
            unreachable!()
        }
        fn set_binary(&mut self, _: &str, _: &[u8]) -> Result<(), SettingsError> {
            unreachable!()
        }
        fn remove(&mut self, _: &str) -> Result<(), SettingsError> {
            unreachable!()
        }
        fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError> {
            unreachable!()
        }
        fn value_names(&self) -> Result<Vec<String>, SettingsError> {
            unreachable!()
        }
        fn enum_entries(&self) -> Result<Vec<(String, Option<TreeValue>)>, SettingsError> {
            Ok(self
                .0
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect())
        }
    }

    /// The plan for writing `settings` over `tree`: each write with the stored
    /// name it replaces, and the stale names.
    fn plan(
        tree: &EnumeratedTree,
        settings: Value,
        portable: bool,
    ) -> (Vec<(String, Option<String>)>, Vec<String>) {
        let Value::Object(settings) = settings else {
            unreachable!()
        };
        let plan = plan_settings_write(tree, &settings, portable).unwrap();
        let writes = plan
            .writes
            .into_iter()
            .map(|write| (write.name.to_owned(), write.recased))
            .collect();
        (writes, plan.stale)
    }

    fn str_value(s: &str) -> Option<TreeValue> {
        Some(TreeValue::Str(s.to_owned()))
    }

    #[test]
    fn a_settings_write_plan_matches_stored_names_ignoring_case() {
        let tree = EnumeratedTree(vec![
            ("kept", str_value("same")),
            ("Recased", str_value("same")),
            ("changed", str_value("old")),
            ("unmodeled", None),
            ("stale", str_value("x")),
            ("STALE", str_value("x")),
        ]);
        let settings = json!({
            "kept": "same", "recased": "same", "changed": "new",
            "unmodeled": "x", "added": "y"
        });

        assert_eq!(
            plan(&tree, settings, false),
            (
                vec![
                    // Held only under another casing, so written under the new
                    // one after the stored name is removed.
                    ("recased".to_owned(), Some("Recased".to_owned())),
                    ("changed".to_owned(), None),
                    // A stored value of no modeled type holds nothing.
                    ("unmodeled".to_owned(), None),
                    ("added".to_owned(), None),
                ],
                // Every stored name the map keeps under no casing, in the
                // order the tree enumerates them.
                vec!["stale".to_owned(), "STALE".to_owned()],
            )
        );
    }

    #[test]
    fn a_settings_write_plan_reads_a_repeated_name_as_every_entry_under_it() {
        // A portable section can repeat a name, in any casing.
        let tree = EnumeratedTree(vec![
            ("Name", str_value("a")),
            ("name", str_value("a")),
            ("NAME", str_value("a")),
        ]);

        // An entry under the exact name holds the value, whatever the others are.
        assert_eq!(plan(&tree, json!({"name": "a"}), true), (vec![], vec![]));

        // A write replaces the first entry under another casing, and keeps
        // every entry the map names from going stale.
        assert_eq!(
            plan(&tree, json!({"name": "b"}), true),
            (vec![("name".to_owned(), Some("Name".to_owned()))], vec![])
        );
        assert_eq!(
            plan(&tree, json!({"Name": "b"}), true),
            (vec![("Name".to_owned(), Some("name".to_owned()))], vec![])
        );
    }

    #[test]
    fn a_settings_write_plan_reads_an_int_as_its_store_holds_it() {
        let tree = EnumeratedTree(vec![
            ("text", str_value("5")),
            ("dword", Some(TreeValue::Int(5))),
            ("padded", str_value("05")),
        ]);
        let settings = json!({"text": 5, "dword": 5, "padded": 5});
        let written = |portable| -> Vec<String> {
            plan(&tree, settings.clone(), portable)
                .0
                .into_iter()
                .map(|(name, _)| name)
                .collect()
        };

        // A portable file holds an int as its decimal text, and only that text.
        assert_eq!(written(true), ["padded"]);
        // A registry string is not the DWORD, whatever it reads as.
        assert_eq!(written(false), ["text", "padded"]);
    }
}
