//! In-memory `StorageProvider`: hands the core a configured `StorageInfo`
//! and a `FakeSettings` backend, so command-level tests choose the storage
//! mode and paths without a real `windhawk.ini`. Records installer-language
//! writes for assertions.

use std::sync::{Arc, Mutex};

use windhawk_core_ports::{
    InstallerLanguage, OsError, ResolvedStorage, StorageInfo, StorageProvider, StorageResolveError,
};

use crate::settings::FakeSettings;

/// A recorded `set_installer_language` call: (lcid, reg_key_override).
pub type InstallerLanguageCall = (u32, Option<String>);

#[derive(Clone)]
pub struct FakeStorageProvider {
    info: StorageInfo,
    backend: FakeSettings,
    installer_language_calls: Arc<Mutex<Vec<InstallerLanguageCall>>>,
    /// What resolve returns; an error lets tests drive APP_ROOT_INVALID.
    resolve_error: Arc<Mutex<Option<String>>>,
}

impl FakeStorageProvider {
    /// A portable provider rooted at the given app-data path; the rest of the
    /// fs paths are derived under the app root for `getCoreInfo` tests.
    pub fn portable(app_root: &str) -> Self {
        Self::new(StorageInfo {
            portable: true,
            app_root_path: app_root.to_owned(),
            app_data_path: format!("{app_root}\\AppData"),
            engine_path: format!("{app_root}\\Engine"),
            compiler_path: format!("{app_root}\\Compiler"),
            ui_path: format!("{app_root}\\UI"),
        })
    }

    /// A registry-mode provider rooted at the given app root.
    pub fn registry(app_root: &str) -> Self {
        Self::new(StorageInfo {
            portable: false,
            app_root_path: app_root.to_owned(),
            app_data_path: format!("{app_root}\\AppData"),
            engine_path: format!("{app_root}\\Engine"),
            compiler_path: format!("{app_root}\\Compiler"),
            ui_path: format!("{app_root}\\UI"),
        })
    }

    pub fn new(info: StorageInfo) -> Self {
        Self {
            info,
            backend: FakeSettings::new(),
            installer_language_calls: Arc::new(Mutex::new(Vec::new())),
            resolve_error: Arc::new(Mutex::new(None)),
        }
    }

    /// The shared backend, for seeding state and inspecting writes.
    pub fn backend(&self) -> FakeSettings {
        self.backend.clone()
    }

    pub fn set_resolve_error(&self, message: &str) {
        *self.resolve_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message.to_owned());
    }

    pub fn installer_language_calls(&self) -> Vec<InstallerLanguageCall> {
        self.installer_language_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl StorageProvider for FakeStorageProvider {
    fn resolve(&self, _app_root_path: &str) -> Result<ResolvedStorage, StorageResolveError> {
        if let Some(message) = self
            .resolve_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(StorageResolveError::new(message));
        }
        Ok(ResolvedStorage {
            info: self.info.clone(),
            backend: Arc::new(self.backend.clone()),
        })
    }
}

impl InstallerLanguage for FakeStorageProvider {
    fn set_installer_language(
        &self,
        lcid: u32,
        reg_key_override: Option<&str>,
    ) -> Result<(), OsError> {
        self.installer_language_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((lcid, reg_key_override.map(str::to_owned)));
        Ok(())
    }
}
