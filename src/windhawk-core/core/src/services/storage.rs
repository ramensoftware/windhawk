//! `services::storage`: the `Storage` accessor that owns the resolved mode and
//! base location and turns semantic lookups into `TreeLocation`s (the same
//! rules the C++ `StorageManager` uses), the enumeration of the mod configs on
//! the machine, and `getCoreInfo`. Resolution itself happens at session
//! creation through the `StorageProvider` port (the windows adapter reads
//! `windhawk.ini`); this module holds the result.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use windhawk_core_ports::{SettingsBackend, StorageInfo, TreeLocation};
use windhawk_core_protocol::{CONTRACT_VERSION, CoreFsPaths, CoreInfo};

use crate::error::CoreError;
use crate::services::wire::{WireResultExt, file_err, to_value_result};
use crate::session::SessionInner;

/// The resolved storage handed to every service: the facts (for `getCoreInfo`
/// and tree computation) and the mode-specific backend.
pub struct Storage {
    info: StorageInfo,
    backend: Arc<dyn SettingsBackend>,
}

impl Storage {
    pub fn new(info: StorageInfo, backend: Arc<dyn SettingsBackend>) -> Self {
        Self { info, backend }
    }

    pub fn info(&self) -> &StorageInfo {
        &self.info
    }

    pub fn portable(&self) -> bool {
        self.info.portable
    }

    pub fn backend(&self) -> &Arc<dyn SettingsBackend> {
        &self.backend
    }

    fn app_data(&self) -> &Path {
        Path::new(&self.info.app_data_path)
    }

    /// The mod sources directory: `<appData>\ModsSource` (the TS
    /// `modSource.ts` `modsSourcePath`).
    pub fn mods_source_dir(&self) -> PathBuf {
        self.app_data().join("ModsSource")
    }

    /// A mod's source file: `<appData>\ModsSource\<modId>.wh.cpp`.
    pub fn mod_source_file(&self, mod_id: &str) -> PathBuf {
        self.mods_source_dir().join(format!("{mod_id}.wh.cpp"))
    }

    /// The user profile: `<appData>\userprofile.json` (the TS
    /// `UserProfileFactory` path).
    pub fn user_profile_path(&self) -> PathBuf {
        self.app_data().join("userprofile.json")
    }

    /// The directory holding per-mod config INI files in portable mode:
    /// `<appData>\Engine\Mods` (the TS `IniStorageBackend` `engineModsPath`).
    pub fn engine_mods_dir(&self) -> PathBuf {
        self.app_data().join("Engine").join("Mods")
    }

    /// The mod writable directory: `<appData>\Engine\ModsWritable` (the TS
    /// `engineModsWritablePath`). Holds the per-mod writable INI files
    /// (portable) and the `mod-storage` subtree both modes use.
    pub fn engine_mods_writable_dir(&self) -> PathBuf {
        self.app_data().join("Engine").join("ModsWritable")
    }

    /// A mod's private storage directory, removed on uninstall in both modes
    /// (the TS `deleteModStoragePath`): `<ModsWritable>\mod-storage\<modId>`.
    pub fn mod_storage_dir(&self, mod_id: &str) -> PathBuf {
        self.engine_mods_writable_dir()
            .join("mod-storage")
            .join(mod_id)
    }

    /// A mod's config INI file (portable mode): `<appData>\Engine\Mods\<modId>.ini`.
    pub fn mod_config_ini_path(&self, mod_id: &str) -> PathBuf {
        self.mod_ini(mod_id)
    }

    /// A mod's writable INI file (portable mode):
    /// `<appData>\Engine\ModsWritable\<modId>.ini`.
    pub fn mod_writable_ini_path(&self, mod_id: &str) -> PathBuf {
        self.engine_mods_writable_dir()
            .join(format!("{mod_id}.ini"))
    }

    /// The registry parent of the per-mod config subkeys (registry mode):
    /// `<root>\Engine\Mods`. Used to enumerate installed mods.
    pub fn mods_config_root(&self) -> TreeLocation {
        TreeLocation::Registry {
            sub_key: "Engine\\Mods".to_owned(),
        }
    }

    /// App-settings tree: `[Settings]` (portable) / `<root>\Settings`
    /// (registry).
    pub fn app_settings_tree(&self) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.app_data().join("settings.ini"),
                section: "Settings".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: "Settings".to_owned(),
            }
        }
    }

    /// Engine-settings tree: `<appData>\engine\settings.ini` `[Settings]`
    /// (note the lowercase `engine`, as in the TS) / `<root>\Engine\Settings`.
    pub fn engine_settings_tree(&self) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.app_data().join("engine").join("settings.ini"),
                section: "Settings".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: "Engine\\Settings".to_owned(),
            }
        }
    }

    fn mod_ini(&self, mod_id: &str) -> PathBuf {
        self.app_data()
            .join("Engine")
            .join("Mods")
            .join(format!("{mod_id}.ini"))
    }

    /// Mod-config tree: `<appData>\Engine\Mods\<modId>.ini` `[Mod]` /
    /// `<root>\Engine\Mods\<modId>`.
    pub fn mod_config_tree(&self, mod_id: &str) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.mod_ini(mod_id),
                section: "Mod".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: format!("Engine\\Mods\\{mod_id}"),
            }
        }
    }

    /// Mod-settings tree: the `[Settings]` section of the same INI file /
    /// `<root>\Engine\Mods\<modId>\Settings`.
    pub fn mod_settings_tree(&self, mod_id: &str) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.mod_ini(mod_id),
                section: "Settings".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: format!("Engine\\Mods\\{mod_id}\\Settings"),
            }
        }
    }

    /// The parent of a mod's user configurations, which `list_subtrees` names by
    /// SID: `<root>\Engine\Mods\<modId>\Users` / the `[Users\<SID>]` sections of
    /// the mod's INI file.
    pub fn mod_users_tree(&self, mod_id: &str) -> TreeLocation {
        self.mod_user_tree(mod_id, "Users".to_owned())
    }

    /// A user's configuration of a mod, nested in the global one:
    /// `<root>\Engine\Mods\<modId>\Users\<sid>` / the `[Users\<sid>]` section of
    /// the mod's INI file. `sid` is a validated canonical SID string.
    pub fn mod_user_config_tree(&self, mod_id: &str, sid: &str) -> TreeLocation {
        self.mod_user_tree(mod_id, format!("Users\\{sid}"))
    }

    /// A user's settings of a mod: `<root>\Engine\Mods\<modId>\Users\<sid>\Settings`
    /// / the `[Users\<sid>\Settings]` section of the mod's INI file.
    pub fn mod_user_settings_tree(&self, mod_id: &str, sid: &str) -> TreeLocation {
        self.mod_user_tree(mod_id, format!("Users\\{sid}\\Settings"))
    }

    /// `path` under the mod's config: a subkey in registry mode, a section of the
    /// same file in portable mode, which is how the engine nests them too.
    fn mod_user_tree(&self, mod_id: &str, path: String) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.mod_ini(mod_id),
                section: path,
            }
        } else {
            TreeLocation::Registry {
                sub_key: format!("Engine\\Mods\\{mod_id}\\{path}"),
            }
        }
    }

    /// Mod-writable tree: `<appData>\Engine\ModsWritable\<modId>.ini` (portable,
    /// the whole file) / `<root>\Engine\ModsWritable\<modId>` (registry). Used by
    /// the mod-id rename and the uninstall, which move/remove it alongside the
    /// config tree (the TS `renameConfig` / `deleteConfig` second subkey). The
    /// section is nominal: a rename moves the file and a remove drops the whole
    /// subkey, neither keying on it.
    pub fn mod_writable_tree(&self, mod_id: &str) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.mod_writable_ini_path(mod_id),
                section: "Mod".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: format!("Engine\\ModsWritable\\{mod_id}"),
            }
        }
    }

    /// Mod local-storage tree: the `[LocalStorage]` section of the mod-writable
    /// INI file / `<root>\Engine\ModsWritable\<modId>\LocalStorage` - where the
    /// engine's `GetModWritableConfig(modName, L"LocalStorage", ..)`
    /// (`engine/storage_manager.cpp`) lands a mod's `Wh_SetStringValue` /
    /// `Wh_SetIntValue` writes. Read-only here (`getModDynamicSelectOptions`);
    /// the rename and the uninstall move or remove the whole parent through
    /// `mod_writable_tree`, so the subtree needs no lifecycle of its own.
    pub fn mod_local_storage_tree(&self, mod_id: &str) -> TreeLocation {
        if self.portable() {
            TreeLocation::Ini {
                file: self.mod_writable_ini_path(mod_id),
                section: "LocalStorage".to_owned(),
            }
        } else {
            TreeLocation::Registry {
                sub_key: format!("Engine\\ModsWritable\\{mod_id}\\LocalStorage"),
            }
        }
    }
}

/// The ids of the mod configs on the machine (the TS `getConfigOfInstalled`'s
/// enumeration): registry subkeys of `Engine\Mods`, or `.ini` files in the
/// portable engine-mods directory. An id here may have no `LibraryFileName`,
/// which is what installed means, so a caller reads that for itself.
/// `#[track_caller]`, so a failure's origin is the service that enumerated.
#[track_caller]
pub(crate) fn mod_config_ids(session: &SessionInner) -> Result<Vec<String>, CoreError> {
    let storage = session.storage();
    if storage.portable() {
        match session.deps().files.list_dir(&storage.engine_mods_dir()) {
            Ok(entries) => Ok(entries
                .into_iter()
                .filter_map(|e| {
                    e.is_file
                        .then(|| e.name.strip_suffix(".ini").map(str::to_owned))
                        .flatten()
                })
                .collect()),
            Err(e) if e.is_not_found() => Ok(Vec::new()),
            Err(e) => Err(file_err(e)),
        }
    } else {
        storage
            .backend()
            .list_subtrees(&storage.mods_config_root())
            .wire()
    }
}

/// `getCoreInfo`: contract/core versions, portable flag, resolved paths,
/// arm64Enabled, windhawkVersion, and the session's user.
pub fn get_core_info(session: &SessionInner, _params: Value) -> Result<Value, CoreError> {
    let info = session.storage().info();
    let config = session.config();
    let dto = CoreInfo {
        contract_version: CONTRACT_VERSION.to_owned(),
        portable: info.portable,
        arm64_enabled: session.arm64_enabled(),
        windhawk_version: config.windhawk_version.clone(),
        fs_paths: CoreFsPaths {
            app_root_path: info.app_root_path.clone(),
            app_data_path: info.app_data_path.clone(),
            engine_path: info.engine_path.clone(),
            compiler_path: info.compiler_path.clone(),
            ui_path: info.ui_path.clone(),
        },
        user: session.user().clone(),
    };
    to_value_result("getCoreInfo", &dto)
}
