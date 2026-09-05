//! Storage resolution (`storage/paths.ts`): read `<appRoot>\windhawk.ini`
//! `[Storage]`, expand environment variables, resolve the filesystem paths,
//! parse `RegistryKey`, and construct the mode-specific `SettingsBackend`. Also
//! the installer-language registry write (`applyAppSettings`, non-portable),
//! the one write that does not go through the resolved backend.

use std::path::Path;
use std::sync::Arc;

use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath};
use windows_sys::core::PWSTR;

use windhawk_core_ports::{
    InstallerLanguage, OsError, ResolvedStorage, SettingsBackend, StorageInfo, StorageProvider,
    StorageResolveError, TreeLocation, os_message,
};

use crate::ini::IniBackend;
use crate::registry::{Hive, RegistryBackend, RegistryView, set_dword_value};
use crate::wide::{from_wide_nul, from_wide_ptr, to_wide};

pub struct WindowsStorageProvider;

/// Resolve an optional `[Storage]` path (the development-tools paths `UIPath` /
/// `CompilerPath`). A missing OR empty raw value means the component is not
/// installed, so the result is empty - deliberately NOT run through `resolve`,
/// which would join an empty value onto the app root. A present, non-empty value
/// resolves normally. Mirrors the C++ `PathFromStorage(..., optional=true)`.
fn optional_resolved(raw: Option<String>, resolve: impl Fn(&str) -> String) -> String {
    match raw {
        Some(raw) if !raw.is_empty() => resolve(&raw),
        _ => String::new(),
    }
}

/// Split a `HIVE\sub\key` string into its hive and the remaining subkey,
/// accepting both the short and long hive spellings (`parseRegistryKey`).
fn parse_registry_key(registry_key: &str) -> Option<(Hive, String)> {
    let (prefix, sub_key) = match registry_key.find('\\') {
        Some(i) => (&registry_key[..i], registry_key[i + 1..].to_owned()),
        None => (registry_key, String::new()),
    };
    Hive::parse(prefix).map(|hive| (hive, sub_key))
}

impl StorageProvider for WindowsStorageProvider {
    fn resolve(&self, app_root_path: &str) -> Result<ResolvedStorage, StorageResolveError> {
        let ini_path = Path::new(app_root_path).join("windhawk.ini");
        let reader = IniBackend::new();
        let storage_tree = TreeLocation::Ini {
            file: ini_path.clone(),
            section: "Storage".to_owned(),
        };
        let tree = reader.open(&storage_tree, false).map_err(|e| {
            StorageResolveError::new(format!(
                "could not read {}: {}",
                ini_path.display(),
                e.message()
            ))
        })?;

        let read_string = |name: &str| -> Result<Option<String>, StorageResolveError> {
            tree.get_string(name)
                .map_err(|e| StorageResolveError::new(e.message()))
        };
        let read_required = |name: &str| -> Result<String, StorageResolveError> {
            read_string(name)?.ok_or_else(|| {
                StorageResolveError::new(format!(
                    "{} is missing the [Storage] {name} key",
                    ini_path.display()
                ))
            })
        };

        // `!!parseInt(Portable, 10)`: an absent or non-numeric value is 0
        // (non-portable).
        let portable = tree
            .get_int("Portable")
            .map_err(|e| StorageResolveError::new(e.message()))?
            .unwrap_or(0)
            != 0;

        let resolve_path = |raw: &str| -> String {
            let expanded = substitute_program_data(&expand_env(raw));
            Path::new(app_root_path)
                .join(&expanded)
                .to_string_lossy()
                .into_owned()
        };

        let app_data_path = resolve_path(&read_required("AppDataPath")?);
        let engine_path = resolve_path(&read_required("EnginePath")?);
        // The development tools (compiler + VSCodium UI) are an optional install
        // component. When absent, the installer writes their [Storage] keys empty,
        // so a missing or empty value means "not installed" and stays empty rather
        // than resolving to the app root. Mirrors the C++ PathFromStorage(optional).
        let compiler_path = optional_resolved(read_string("CompilerPath")?, resolve_path);
        let ui_path = optional_resolved(read_string("UIPath")?, resolve_path);

        let info = StorageInfo {
            portable,
            app_root_path: app_root_path.to_owned(),
            app_data_path: app_data_path.clone(),
            engine_path,
            compiler_path,
            ui_path,
        };

        let backend: Arc<dyn SettingsBackend> = if portable {
            Arc::new(IniBackend::new())
        } else {
            let registry_key = read_required("RegistryKey")?;
            let (hive, sub_key) = parse_registry_key(&registry_key).ok_or_else(|| {
                StorageResolveError::new(format!("unsupported registry path: {registry_key}"))
            })?;
            Arc::new(RegistryBackend::new(hive, sub_key))
        };

        Ok(ResolvedStorage { info, backend })
    }
}

impl InstallerLanguage for WindowsStorageProvider {
    fn set_installer_language(
        &self,
        lcid: u32,
        reg_key_override: Option<&str>,
    ) -> Result<(), OsError> {
        // The override is a raw `HIVE\sub\key`; default is HKLM\SOFTWARE\Windhawk.
        let (hive, sub_key) = match reg_key_override {
            Some(raw) => match parse_registry_key(raw) {
                Some(parsed) => parsed,
                // No OS call was made, so the OsError carries no code (None);
                // this is exactly the case Option<NonZeroU32> is the right shape
                // for (operation set, no rc).
                None => {
                    return Err(OsError::new(
                        "set_installer_language",
                        0,
                        format!("invalid installer registry key override: {raw}"),
                    ));
                }
            },
            None => (Hive::LocalMachine, "SOFTWARE\\Windhawk".to_owned()),
        };
        // The 32-bit view, matching the TS WOW64_32KEY installer write. On
        // failure surface the registry rc as the OsError's OS code, and the
        // system's own text for it as the cause: the Reg* status is not rendered
        // for us the way a file call's is.
        set_dword_value(hive, &sub_key, "language", lcid, RegistryView::Bit32).map_err(|rc| {
            OsError::new(
                "set_installer_language",
                rc,
                format!(
                    "installer language registry write failed: {}",
                    os_message(rc)
                ),
            )
        })
    }
}

/// `ExpandEnvironmentStringsW`: replaces `%VAR%` with its value, leaving
/// undefined variables unchanged - the same result as the TS
/// `process.env[matched] ?? original`.
fn expand_env(s: &str) -> String {
    let src = to_wide(s);
    // SAFETY: src is NUL-terminated; a zero size requests the needed length.
    let needed = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return s.to_owned();
    }
    let mut buf = vec![0u16; needed as usize];
    // SAFETY: buf has `needed` units; src is NUL-terminated.
    let written = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), needed) };
    if written == 0 {
        return s.to_owned();
    }
    from_wide_nul(&buf)
}

/// The one spelling of the variable, shared by the detection and the
/// substitution so the two cannot drift.
const PROGRAM_DATA_VAR: &str = "%ProgramData%";

/// Substitute `%ProgramData%` left behind by [`expand_env`], which returns an
/// undefined variable unchanged.
///
/// A process can be started with a trimmed environment block that omits the
/// variable, and the default `AppDataPath` is written in terms of it. Left
/// alone, the literal would be joined onto the app root as a relative path and
/// every read under it would come back empty instead of failing, so recover the
/// directory by other means. Mirrors the C++ engine's `PathFromStorage`
/// (`engine/storage_manager.cpp`), fallback chain included.
fn substitute_program_data(expanded: &str) -> String {
    // ASCII lowercasing is length-preserving, so offsets into the lowered copy
    // index the original. The variable is matched case-insensitively because
    // the environment block is.
    let lowered = expanded.to_ascii_lowercase();
    let needle = PROGRAM_DATA_VAR.to_ascii_lowercase();
    if !lowered.contains(&needle) {
        return expanded.to_owned();
    }

    let program_data = program_data_dir();
    let mut out = String::new();
    let mut rest = 0;
    while let Some(hit) = lowered[rest..].find(&needle) {
        let at = rest + hit;
        out.push_str(&expanded[rest..at]);
        out.push_str(&program_data);
        rest = at + needle.len();
    }
    out.push_str(&expanded[rest..]);
    out
}

/// The ProgramData directory for a process whose environment does not name it:
/// the known folder, else `%SystemDrive%\ProgramData`, else the location it has
/// on a default-installed Windows.
fn program_data_dir() -> String {
    if let Some(known) = known_folder_program_data() {
        return known;
    }
    if let Ok(system_drive) = std::env::var("SystemDrive")
        && !system_drive.is_empty()
    {
        return format!("{system_drive}\\ProgramData");
    }
    "C:\\ProgramData".to_owned()
}

/// `SHGetKnownFolderPath(FOLDERID_ProgramData)`, the authoritative answer: it
/// reads the folder's registered location, so it also covers an install that
/// moved ProgramData off its default path.
fn known_folder_program_data() -> Option<String> {
    let mut path: PWSTR = std::ptr::null_mut();
    // SAFETY: FOLDERID_ProgramData is a valid known-folder id, the default
    // flags are 0, a null token means the calling user, and `path` is a live
    // out-parameter for the callee-allocated buffer.
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut path) };
    // On failure the out-parameter is set to null, so there is nothing to free.
    if hr < 0 || path.is_null() {
        return None;
    }
    // SAFETY: on success the buffer is a NUL-terminated wide string.
    let resolved = unsafe { from_wide_ptr(path) };
    // SAFETY: the buffer is ours to release, and SHGetKnownFolderPath allocates
    // it with CoTaskMemAlloc.
    unsafe { CoTaskMemFree(path.cast::<std::ffi::c_void>()) };
    (!resolved.is_empty()).then_some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_key_parsing() {
        assert_eq!(
            parse_registry_key("HKEY_CURRENT_USER\\Software\\Windhawk"),
            Some((Hive::CurrentUser, "Software\\Windhawk".to_owned()))
        );
        assert_eq!(
            parse_registry_key("HKLM\\SOFTWARE\\X"),
            Some((Hive::LocalMachine, "SOFTWARE\\X".to_owned()))
        );
        assert_eq!(parse_registry_key("BOGUS\\x"), None);
    }

    /// The test process has the variable, so `expand_env` consumes it and the
    /// substitution never fires through `resolve`; drive it directly to cover
    /// the environment that does not.
    #[test]
    fn unexpanded_program_data_resolves_to_the_real_directory() {
        // The known folder and the environment agree on a machine that has
        // both, which is what makes this an assertion rather than a tautology.
        let expected = std::env::var("ProgramData").unwrap();
        assert_eq!(
            substitute_program_data("%ProgramData%\\Windhawk"),
            format!("{expected}\\Windhawk")
        );
        // The environment block is case-insensitive, so the match is too.
        assert_eq!(
            substitute_program_data("%PROGRAMDATA%\\a\\%programdata%\\b"),
            format!("{expected}\\a\\{expected}\\b")
        );
    }

    #[test]
    fn a_path_without_the_variable_is_untouched() {
        // Including one another variable was left in: only ProgramData has a
        // fallback, and an expanded path never reaches the substitution.
        assert_eq!(substitute_program_data("appdata"), "appdata");
        assert_eq!(substitute_program_data("%Undefined%\\x"), "%Undefined%\\x");
    }

    #[test]
    fn program_data_dir_is_an_absolute_path() {
        let dir = program_data_dir();
        assert!(Path::new(&dir).is_absolute(), "not absolute: {dir}");
        assert!(!dir.contains('%'), "unresolved variable: {dir}");
    }

    #[test]
    fn optional_path_is_empty_when_missing_or_empty() {
        let resolve = |raw: &str| format!("ROOT\\{raw}");
        // A present, non-empty value resolves normally.
        assert_eq!(
            optional_resolved(Some("UI".to_owned()), resolve),
            "ROOT\\UI"
        );
        // Missing (devtools not installed) or an empty value stays empty - it is
        // NOT joined onto the app root.
        assert_eq!(optional_resolved(None, resolve), "");
        assert_eq!(optional_resolved(Some(String::new()), resolve), "");
    }
}
