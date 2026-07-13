//! Storage resolution (`storage/paths.ts`): read `<appRoot>\windhawk.ini`
//! `[Storage]`, expand environment variables, resolve the filesystem paths,
//! parse `RegistryKey`, and construct the mode-specific `SettingsBackend`. Also
//! the installer-language registry write (`applyAppSettings`, non-portable),
//! the one write that does not go through the resolved backend.

use std::path::Path;
use std::sync::Arc;

use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;

use windhawk_core_ports::{
    InstallerLanguage, OsError, ResolvedStorage, SettingsBackend, StorageInfo, StorageProvider,
    StorageResolveError, TreeLocation,
};

use crate::ini::IniBackend;
use crate::registry::{Hive, RegistryBackend, RegistryView, set_dword_value};
use crate::wide::{from_wide_nul, to_wide};

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
            let expanded = expand_env(raw);
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
        // failure surface the registry rc as the OsError's OS code.
        set_dword_value(hive, &sub_key, "language", lcid, RegistryView::Bit32).map_err(|rc| {
            OsError::new(
                "set_installer_language",
                rc,
                "installer language registry write failed",
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
