//! The dual-backend keyed value store, deliberately mirroring the proven C++
//! abstraction (`shared/portable_settings.cpp` in the main repository). Two
//! adapters implement it: `RegistryBackend` and `IniBackend`
//! (windhawk-core-windows); tests substitute an in-memory fake.
//!
//! Services never see `TreeLocation`; they go through the session's
//! `Storage` accessor, which owns the resolved mode and base location and
//! produces the locations below from the same rules the C++ `StorageManager`
//! uses.

use std::path::PathBuf;

use crate::os_error::{OsError, render};

/// A typed value as it lives in a settings tree. The three storage types:
/// `REG_SZ` / strings, `REG_DWORD` / decimal ints, `REG_BINARY` /
/// uppercase-hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeValue {
    Str(String),
    Int(i32),
    Binary(Vec<u8>),
}

/// Where a settings tree lives. The `Storage` accessor (which knows the
/// resolved mode) builds these; a registry backend handles only `Registry`
/// locations and an INI backend only `Ini` locations, so the variant always
/// matches the backend the session resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeLocation {
    /// A registry subkey relative to the backend's resolved root
    /// (`<hive>\<rootSubKey>`), e.g. `Settings` or `Engine\Mods\<modId>`.
    Registry { sub_key: String },
    /// An INI file plus the section within it, e.g.
    /// (`<appData>\settings.ini`, `Settings`).
    Ini { file: PathBuf, section: String },
}

/// Which backend produced a `SettingsError`, so services pick `REGISTRY_FAILED`
/// vs `IO_FAILED`. A PER-BACKEND CONSTANT - `RegistryBackend` only ever yields
/// `Registry`, `IniBackend` only `Ini` - set through the named constructors
/// below, so the kind is not threaded as an argument at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsErrorKind {
    Registry,
    Ini,
}

/// A settings failure carrying the OS-call triple (the embedded `OsError`), the
/// typed `location` the adapter touched, and which backend produced it.
/// Services map this onto the wire codes; adapters never choose user-facing
/// codes.
#[derive(Debug, Clone)]
pub struct SettingsError {
    /// The shared OS-call triple (operation, raw code, message).
    pub os: OsError,
    /// A human description of the location (registry subkey or INI path).
    pub location: String,
    pub kind: SettingsErrorKind,
}

impl SettingsError {
    /// A `RegistryBackend` failure. `os_error` is the raw Win32 code; `0` means
    /// "not from a Win32 call" and renders no `(os error N)` suffix.
    pub fn registry(
        operation: &'static str,
        location: impl Into<String>,
        os_error: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            os: OsError::new(operation, os_error, message),
            location: location.into(),
            kind: SettingsErrorKind::Registry,
        }
    }

    /// An `IniBackend` failure. See `registry` for the `os_error` convention.
    pub fn ini(
        operation: &'static str,
        location: impl Into<String>,
        os_error: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            os: OsError::new(operation, os_error, message),
            location: location.into(),
            kind: SettingsErrorKind::Ini,
        }
    }

    /// The bare OS message, WITHOUT the decorated prefix or the `(os error N)`
    /// suffix. Consumers that forward just the cause use this; `to_string()`
    /// appends `(os error N)` and the two are NOT interchangeable (the
    /// bare-vs-decorated discipline the FileError inventory documents).
    pub fn message(&self) -> &str {
        &self.os.message
    }
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&render(&self.location, &self.os))
    }
}

impl std::error::Error for SettingsError {}

/// The keyed value store. Opening a tree for write creates it if absent
/// (registry: `RegCreateKeyEx`; INI: the file with a UTF-16LE BOM), matching
/// the C++ side.
pub trait SettingsBackend: Send + Sync {
    fn open(
        &self,
        tree: &TreeLocation,
        write: bool,
    ) -> Result<Box<dyn SettingsTree>, SettingsError>;

    /// Remove a whole tree: in registry mode the subkey and its values; in
    /// INI mode the section. Absent trees are a no-op.
    fn remove_tree(&self, tree: &TreeLocation) -> Result<(), SettingsError>;

    /// Rename a tree (the mod-id rename of the editor install flow,
    /// `changeModId`). In registry mode the subkey and its whole descendant
    /// tree are renamed in place (`RegRenameKey`, so `Engine\Mods\<from>` and
    /// its `Settings` child move together); in INI mode the backing file is
    /// renamed (both the `[Mod]` and `[Settings]` sections live in one file, so
    /// renaming the file moves both). `from` and `to` must be the same kind of
    /// location. An absent source is a no-op.
    fn rename_tree(&self, from: &TreeLocation, to: &TreeLocation) -> Result<(), SettingsError>;

    /// Enumerate the immediate child trees of a location: in registry mode the
    /// subkey names under `parent` (the only enumeration the mod listing
    /// needs - `getConfigOfInstalled`, registry mode). INI mode has no nested
    /// trees (mods are separate files), so the INI backend returns an empty
    /// list and the portable listing comes from the `Files` port instead. An
    /// absent parent enumerates as empty.
    fn list_subtrees(&self, parent: &TreeLocation) -> Result<Vec<String>, SettingsError>;
}

/// A single open tree (a registry key or an INI section). Reads return
/// `None` for absent values; the adapter applies no defaults (that is the
/// service's job).
pub trait SettingsTree {
    fn get_string(&self, name: &str) -> Result<Option<String>, SettingsError>;
    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError>;
    fn get_int(&self, name: &str) -> Result<Option<i32>, SettingsError>;
    fn set_int(&mut self, name: &str, value: i32) -> Result<(), SettingsError>;
    fn get_binary(&self, name: &str) -> Result<Option<Vec<u8>>, SettingsError>;
    fn set_binary(&mut self, name: &str, value: &[u8]) -> Result<(), SettingsError>;
    fn remove(&mut self, name: &str) -> Result<(), SettingsError>;

    /// Every value in the tree, with its stored type. The order is the
    /// backend's natural enumeration order (registry: unordered; INI: file
    /// order).
    fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError>;
}
