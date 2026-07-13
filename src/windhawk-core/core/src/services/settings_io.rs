//! The `SettingsTree`-bound read/write helpers and the one parameterized
//! tree-open helper. The typed READS apply the field descriptors' defaults (the
//! `parseRawValue` logic of the TS codecs); the symmetric WRITES fold the
//! bool/pipe codec (`windhawk_core_domain::settings_codec`) together with the
//! `set_*` call and the `.wire()` mapping, so a write site and the read it
//! inverts cannot drift. The encode/decode LOGIC has ONE home (the pure codec
//! in `domain`); these helpers are its `SettingsTree` adapter. The field
//! MAPPING stays as explicit greppable loops at the call sites (and in
//! `write_mod_config_patch`), not a serde-bridged descriptor table.
//!
//! Every helper is `#[track_caller]` and maps errors through `.wire()` (not
//! `.map_err(settings_err)`), so a settings failure's captured origin is the
//! SERVICE call site that invoked the helper, not this funnel (see `wire.rs`).

use windhawk_core_domain::{bool_to_int, int_to_bool, join_pipe, split_pipe};
use windhawk_core_ports::{SettingsTree, TreeLocation};
use windhawk_core_protocol::ModConfigPatch;

use crate::error::CoreError;
use crate::services::storage::Storage;
use crate::services::wire::WireResultExt;

// --- reads (the field descriptors' defaults, the TS parseRawValue) ----------

/// A string value, or `None` when absent (registry: only `REG_SZ`; INI: any
/// present value).
#[track_caller]
pub fn read_string(tree: &dyn SettingsTree, name: &str) -> Result<Option<String>, CoreError> {
    tree.get_string(name).wire()
}

/// A boolean: stored as a 0/1 int; absent reads as `false` (`!!undefined`).
/// Decode is the lenient `int_to_bool` (any nonzero is `true`).
#[track_caller]
pub fn read_bool(tree: &dyn SettingsTree, name: &str) -> Result<bool, CoreError> {
    Ok(tree.get_int(name).wire()?.map(int_to_bool).unwrap_or(false))
}

/// A number: stored as an int; absent reads as the field's default. `i64::from`
/// is the symmetric width transform paired with `write_number`'s `as i32`.
#[track_caller]
pub fn read_number(tree: &dyn SettingsTree, name: &str, default: i64) -> Result<i64, CoreError> {
    Ok(tree.get_int(name).wire()?.map(i64::from).unwrap_or(default))
}

/// A string array: a pipe-joined string; absent reads as the empty list.
#[track_caller]
pub fn read_array(tree: &dyn SettingsTree, name: &str) -> Result<Vec<String>, CoreError> {
    Ok(split_pipe(
        &tree.get_string(name).wire()?.unwrap_or_default(),
    ))
}

// --- writes (the symmetric inverses, folding the codec) ---------------------

/// Write a string value verbatim.
#[track_caller]
pub fn write_string(tree: &mut dyn SettingsTree, name: &str, value: &str) -> Result<(), CoreError> {
    tree.set_string(name, value).wire()
}

/// Write a boolean as the stored 0/1 int (`bool_to_int`).
#[track_caller]
pub fn write_bool(tree: &mut dyn SettingsTree, name: &str, value: bool) -> Result<(), CoreError> {
    tree.set_int(name, bool_to_int(value)).wire()
}

/// Write a number, narrowing the DTO's `i64` to the stored `i32` (the symmetric
/// inverse of `read_number`'s `i64::from`, folding the `v as i32` that lived
/// inline at each app-settings write site).
#[track_caller]
pub fn write_number(tree: &mut dyn SettingsTree, name: &str, value: i64) -> Result<(), CoreError> {
    tree.set_int(name, value as i32).wire()
}

/// Write a string array as a pipe-joined string (`join_pipe`).
#[track_caller]
pub fn write_array(
    tree: &mut dyn SettingsTree,
    name: &str,
    value: &[String],
) -> Result<(), CoreError> {
    tree.set_string(name, &join_pipe(value)).wire()
}

/// Write a `ModConfigPatch`'s present fields in the TS `CONFIG_FIELDS`
/// descriptor order (absent field = preserve). The ONE read/write list
/// single-sources - both directions of the SAME patch type - shared by
/// `updateModConfig` and the install config write. Field write-ORDER is
/// non-observable end-to-end: the engine's debounced reload reads the final
/// tree state, and the only non-debounced reader (per-process injection) makes
/// its load decision from the targeting keys BEFORE reading `LibraryFileName`,
/// so no key order - not even "`LibraryFileName` last" - closes its
/// partial-read race. `CONFIG_FIELDS` order is kept for human consistency only.
#[track_caller]
pub fn write_mod_config_patch(
    tree: &mut dyn SettingsTree,
    patch: &ModConfigPatch,
) -> Result<(), CoreError> {
    if let Some(v) = &patch.library_file_name {
        write_string(tree, "LibraryFileName", v)?;
    }
    if let Some(v) = patch.disabled {
        write_bool(tree, "Disabled", v)?;
    }
    if let Some(v) = patch.logging_enabled {
        write_bool(tree, "LoggingEnabled", v)?;
    }
    if let Some(v) = patch.debug_logging_enabled {
        write_bool(tree, "DebugLoggingEnabled", v)?;
    }
    if let Some(v) = &patch.include {
        write_array(tree, "Include", v)?;
    }
    if let Some(v) = &patch.exclude {
        write_array(tree, "Exclude", v)?;
    }
    if let Some(v) = &patch.include_custom {
        write_array(tree, "IncludeCustom", v)?;
    }
    if let Some(v) = &patch.exclude_custom {
        write_array(tree, "ExcludeCustom", v)?;
    }
    if let Some(v) = patch.include_exclude_custom_only {
        write_bool(tree, "IncludeExcludeCustomOnly", v)?;
    }
    if let Some(v) = patch.patterns_match_critical_system_processes {
        write_bool(tree, "PatternsMatchCriticalSystemProcesses", v)?;
    }
    if let Some(v) = &patch.architecture {
        write_array(tree, "Architecture", v)?;
    }
    if let Some(v) = &patch.version {
        write_string(tree, "Version", v)?;
    }
    Ok(())
}

// --- open (one parameterized helper, not per-tree wrappers) -----------------

/// Open a settings tree, folding the `backend().open(...).wire()` the ~14 tree
/// accesses repeat. ONE parameterized helper over `&Storage` (not a set of
/// per-tree-kind wrappers, which would be the parallel-list trap constraint 7
/// warns against); the tree-location selection already lives once in `Storage`.
/// Callers keep the `&*tree`/`as_mut` reborrow.
#[track_caller]
pub fn open_tree(
    storage: &Storage,
    location: &TreeLocation,
    write: bool,
) -> Result<Box<dyn SettingsTree>, CoreError> {
    storage.backend().open(location, write).wire()
}
