//! Storage resolution (`storage/paths.ts`): turning an app-root path into the
//! resolved storage mode, the filesystem paths, and a `SettingsBackend` already
//! rooted at the resolved registry key or app-data directory.
//!
//! This is a port because the resolution reads `windhawk.ini` and expands
//! environment variables - both OS effects that the core must not perform
//! itself (the no-environment-reads rule). The windows adapter does the I/O and
//! hands back plain data plus the constructed backend; the core holds the data
//! (for `getCoreInfo`) and builds tree locations from it.

use std::sync::Arc;

use crate::os_error::OsError;
use crate::settings::SettingsBackend;

/// The resolved storage facts exposed by `getCoreInfo` and used by the
/// `Storage` accessor to compute tree locations. Plain data; carries no
/// `HKEY` (the resolved hive lives inside the constructed backend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageInfo {
    pub portable: bool,
    pub app_root_path: String,
    pub app_data_path: String,
    pub engine_path: String,
    pub compiler_path: String,
    pub ui_path: String,
}

/// A resolution failure, mapped by the core to `APP_ROOT_INVALID`: a missing or
/// unreadable `windhawk.ini`, a missing required `[Storage]` key, or an
/// unparseable `RegistryKey`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct StorageResolveError {
    pub message: String,
}

impl StorageResolveError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// The product of resolution: the facts plus a backend rooted at the
/// resolved location.
pub struct ResolvedStorage {
    pub info: StorageInfo,
    pub backend: Arc<dyn SettingsBackend>,
}

/// The windows storage facade. `resolve` reads `windhawk.ini` and builds the
/// mode-specific backend. (The installer-language registry write was split off
/// to its own `InstallerLanguage` port, so this trait's charter is resolution
/// only.)
pub trait StorageProvider: Send + Sync {
    fn resolve(&self, app_root_path: &str) -> Result<ResolvedStorage, StorageResolveError>;
}

/// The installer-language registry write, split off `StorageProvider`:
/// `applyAppSettings` writes the chosen language LCID under the installer's own
/// registry key (non-portable only). It returns `Result<(), OsError>` rather
/// than a bare bool so the lone consumer can log WHY the write failed - the
/// OS-call context - instead of discarding the reason. Best effort: a failure
/// never fails the command; the consumer logs a warning and continues.
pub trait InstallerLanguage: Send + Sync {
    /// Write the installer language LCID. `reg_key_override` is the raw
    /// `HIVE\sub\key` from the session's `installerRegKey` debug override, or
    /// `None` for the production `HKLM\SOFTWARE\Windhawk`. On failure the
    /// returned `OsError` carries the OS code when there was an OS call (the
    /// registry write rc) and `None` when there was not (an unparsable override
    /// key, no call made).
    fn set_installer_language(
        &self,
        lcid: u32,
        reg_key_override: Option<&str>,
    ) -> Result<(), OsError>;
}
