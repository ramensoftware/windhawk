//! The command dispatch table: declared in one place, as const data, so tests
//! can enumerate it and diff it against the frozen inventory. This is the
//! per-command routing point; a command not in this table does not exist.
//!
//! Lock declarations: `parseModSource` is pure and `_diagEmitEvents` touches no
//! stored state, so both take `LockSpec::None`. The settings/config commands
//! take the keyed `Mod` RW lock (keyed on `params.modId`) and the app-settings
//! commands take the single `AppSettings` RW lock; dispatch acquires them
//! around the handler.

use serde_json::Value;
use std::sync::Arc;

use crate::commands;
use crate::error::CoreError;
use crate::runtime::PreparedOp;
use crate::services;
use crate::session::SessionInner;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Sync,
    Async,
}

/// Command-level lock declaration. `write` selects the exclusive side of the RW
/// lock; `Mod` is keyed on the request's `modId` param. `Update` is the
/// try-acquire busy flag for `startUpdate` (acquired by the async path, not
/// held by a sync handler). `ModStaged` is the staged keyed-`Mod` lock of an
/// async command (`compileInstalledMod`): the slow phase runs unlocked and the
/// operation body takes the exclusive side only across its commit (keyed on the
/// request's `storageId`), so dispatch resolves no lock for it (the body asks
/// the session for the handle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockSpec {
    None,
    AppSettings { write: bool },
    Mod { write: bool },
    Update,
    ModStaged,
}

pub enum Handler {
    Sync(fn(&SessionInner, Value) -> Result<Value, CoreError>),
    /// A pure, session-free command: it reads no session state, is always
    /// `LockSpec::None`, and is reachable through BOTH transports - the session
    /// `WhCoreInvoke` (so the extension's `parseModSource` keeps working) and
    /// the session-free `WhCoreInvokeStateless`. The pure helpers
    /// (`parseModSource`, `appendToModIdAndName`, `getCompileFlags`, and
    /// `inspectUserData`) carry it; taking no session parameter makes the
    /// statelessness type-enforced.
    Stateless(fn(Value) -> Result<Value, CoreError>),
    /// Decodes and validates synchronously (failures are reported before
    /// an operation id exists, per the ABI), returning the operation body.
    Async(fn(&Arc<SessionInner>, Value) -> Result<PreparedOp, CoreError>),
}

pub struct CommandSpec {
    pub name: &'static str,
    pub kind: CommandKind,
    pub locks: LockSpec,
    /// True for commands of the frozen contract inventory; false for
    /// `_`-prefixed internal diagnostics, which carry no compatibility
    /// promise and are excluded from the inventory tests.
    pub contract: bool,
    pub handler: Handler,
}

/// The dispatch table. The only non-constant static in the workspace is
/// intentionally absent: the table is const data built from function pointers.
static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "parseModSource",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Stateless(commands::parse_mod_source::run),
    },
    // The new-mod / fork source transform: a pure helper, dispatch-direct into
    // domain (the pure-helper set).
    CommandSpec {
        name: "appendToModIdAndName",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Stateless(commands::parse_mod_source::append_mod_id_and_name),
    },
    // Meta / storage.
    CommandSpec {
        name: "getCoreInfo",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::storage::get_core_info),
    },
    // App settings.
    CommandSpec {
        name: "getAppSettings",
        kind: CommandKind::Sync,
        locks: LockSpec::AppSettings { write: false },
        contract: true,
        handler: Handler::Sync(services::app_settings::get),
    },
    CommandSpec {
        name: "applyAppSettings",
        kind: CommandKind::Sync,
        locks: LockSpec::AppSettings { write: true },
        contract: true,
        handler: Handler::Sync(services::app_settings::apply),
    },
    CommandSpec {
        name: "previewAppSettingsEffects",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::app_settings::preview),
    },
    // Mod config / settings.
    CommandSpec {
        name: "getModConfig",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: false },
        contract: true,
        handler: Handler::Sync(services::mods::get_mod_config),
    },
    CommandSpec {
        name: "updateModConfig",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::mods::update_mod_config),
    },
    CommandSpec {
        name: "getModSettings",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: false },
        contract: true,
        handler: Handler::Sync(services::mods::get_mod_settings),
    },
    CommandSpec {
        name: "setModSettings",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::mods::set_mod_settings),
    },
    CommandSpec {
        name: "setModLoggingEnabled",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::mods::set_mod_logging_enabled),
    },
    // Use-case lifecycle: the enable/disable and uninstall flows. Both take the
    // exclusive keyed `Mod` lock and mirror into the profile (rank 2, internal)
    // for non-local mods.
    CommandSpec {
        name: "setModEnabled",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::mods::set_mod_enabled),
    },
    CommandSpec {
        name: "removeMod",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::mods::remove_mod),
    },
    // Mod source and user profile.
    CommandSpec {
        name: "listInstalledMods",
        kind: CommandKind::Sync,
        // A multi-mod read declares no command lock; its profile write takes
        // the rank-2 artifact lock internally.
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::mods::list_installed_mods),
    },
    CommandSpec {
        name: "getModSource",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: false },
        contract: true,
        handler: Handler::Sync(services::mods::get_mod_source),
    },
    CommandSpec {
        name: "doesModExist",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: false },
        contract: true,
        handler: Handler::Sync(services::mods::does_mod_exist),
    },
    CommandSpec {
        name: "setModRating",
        kind: CommandKind::Sync,
        locks: LockSpec::Mod { write: true },
        contract: true,
        handler: Handler::Sync(services::profile::set_mod_rating),
    },
    CommandSpec {
        name: "syncCatalogToProfile",
        // A multi-mod profile write; no command lock (the rank-2 artifact lock
        // serializes the read-modify-write internally).
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::profile::sync_catalog_to_profile),
    },
    CommandSpec {
        name: "getAppUpdateStatus",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::profile::get_app_update_status),
    },
    CommandSpec {
        name: "getProfileWatchInfo",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::profile::get_profile_watch_info),
    },
    // Repository network commands. Leaf services with no stored state, so no
    // command lock; each runs on its operation thread and terminates with a
    // completed/failed event.
    CommandSpec {
        name: "fetchCatalog",
        kind: CommandKind::Async,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Async(services::repo::prepare_fetch_catalog),
    },
    CommandSpec {
        name: "fetchRepoModSource",
        kind: CommandKind::Async,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Async(services::repo::prepare_fetch_repo_mod_source),
    },
    CommandSpec {
        name: "fetchModVersions",
        kind: CommandKind::Async,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Async(services::repo::prepare_fetch_mod_versions),
    },
    // Update: the single-flight download + detached NSIS launch.
    CommandSpec {
        name: "startUpdate",
        kind: CommandKind::Async,
        locks: LockSpec::Update,
        contract: true,
        handler: Handler::Async(services::update::prepare_start_update),
    },
    // Install the optional development tools: the same installer flow as
    // startUpdate (shares the single-flight `Update` lock), with reinstall +
    // /DEVTOOLS flags instead of the update flags.
    CommandSpec {
        name: "startInstallDevTools",
        kind: CommandKind::Async,
        locks: LockSpec::Update,
        contract: true,
        handler: Handler::Async(services::update::prepare_start_install_devtools),
    },
    // Process execution.
    CommandSpec {
        // Staged keyed-`Mod` lock: the slow compile runs unlocked, the commit
        // takes the exclusive side (keyed on `storageId`) inside the body.
        name: "compileInstalledMod",
        kind: CommandKind::Async,
        locks: LockSpec::ModStaged,
        contract: true,
        handler: Handler::Async(services::install::orchestrate::prepare_compile_installed_mod),
    },
    // Install: the full install/reinstall flow. Staged keyed-`Mod` lock like
    // compileInstalledMod - the slow compile/download runs unlocked, the commit
    // takes the exclusive keyed lock(s) inside the body (two for a
    // `renameFromStorageId` install).
    CommandSpec {
        name: "installMod",
        kind: CommandKind::Async,
        locks: LockSpec::ModStaged,
        contract: true,
        handler: Handler::Async(services::install::orchestrate::prepare_install_mod),
    },
    CommandSpec {
        name: "notifyTray",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::tray::notify_tray),
    },
    CommandSpec {
        name: "getCompileFlags",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Stateless(services::compiler::flags::get_compile_flags),
    },
    // User-data export/import. Export aggregates read-only reads (no command
    // lock, like listInstalledMods); inspect is pure over the archive string, so
    // it is stateless and reachable session-free.
    CommandSpec {
        name: "exportUserData",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Sync(services::user_data::export),
    },
    CommandSpec {
        name: "inspectUserData",
        kind: CommandKind::Sync,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Stateless(services::user_data::inspect),
    },
    // Import is async (it compiles). It declares no command lock: the transaction
    // drives each install (which self-locks its commit) and takes the keyed `Mod`
    // lock itself around the per-mod settings/config writes (per-sub-operation),
    // so a single import-wide lock is deliberately avoided.
    CommandSpec {
        name: "importUserData",
        kind: CommandKind::Async,
        locks: LockSpec::None,
        contract: true,
        handler: Handler::Async(services::user_data::prepare_import),
    },
    CommandSpec {
        name: "_diagEmitEvents",
        kind: CommandKind::Async,
        locks: LockSpec::None,
        contract: false,
        handler: Handler::Async(commands::diag::prepare_emit_events),
    },
];

pub fn command_specs() -> &'static [CommandSpec] {
    COMMANDS
}

pub fn find_command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.name == name)
}

/// Decode typed params out of the request envelope's raw params value.
///
/// `#[track_caller]` + the direct `Err` (not `.map_err(closure)`) so the origin
/// names the service that asked for the decode rather than this line, matching
/// `settings_io`'s read helpers and `services::wire::WireResultExt`.
#[track_caller]
pub fn decode_params<T: serde::de::DeserializeOwned>(
    command: &str,
    params: Value,
) -> Result<T, CoreError> {
    match serde_json::from_value(params) {
        Ok(value) => Ok(value),
        Err(e) => Err(CoreError::invalid_request(format!(
            "invalid params for {command}: {e}"
        ))),
    }
}
