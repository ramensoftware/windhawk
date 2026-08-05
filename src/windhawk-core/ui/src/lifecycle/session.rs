//! Session bring-up. Discovers the app root exe-relative, loads + gates
//! `windhawk-core.dll`, creates this process's own core session, and wires that
//! session's event callback to the channel the async pump drains. A startup
//! failure is fatal; `lib.rs` presents it as a native message box
//! (`window::show_fatal`) and exits.
//!
//! The session created here is the LOCAL one, and it is not necessarily the one
//! the handlers end up running against: an unelevated window hands its commands to
//! the elevated broker's session instead (`crate::broker`), and keeps this one for
//! the process lifetime as what serves the startup reads and what a lost channel
//! falls back to.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use serde_json::json;
use windhawk_core_host::windhawk_ini::has_windhawk_ini;
use windhawk_core_host::{
    GatedCore, HostError, Session, SessionApi, SessionApiExt, SessionCallbacks, SessionConfig,
    resolve_dll_path,
};
use windhawk_core_protocol::{CoreInfo, SourceLocation};

use crate::pump::PumpMessage;
use crate::pump::ops::FIRST_GENERATION;

/// The loaded core, the live session, and the resolved Windhawk AppData
/// directory, shared into `run`. The `GatedCore` is retained alongside the
/// `Session` for the session-free `parseModSource` path.
pub struct CoreHandles {
    pub core: Arc<GatedCore>,
    /// The session this process hosts itself. It stays CONCRETE and is kept for
    /// the process lifetime: it is what serves the startup reads, and what a lost
    /// broker channel falls back to (`crate::broker`).
    pub session: Arc<Session>,
    /// The Windhawk installation directory (`getCoreInfo` `fsPaths.appRootPath`, where
    /// `windhawk.ini` lives). Its `ModsRuntime` subfolder is the source the startup
    /// mod-runtime seed copies into the engine's `Engine\Mods` under appData
    /// (`lifecycle::mods_runtime`).
    pub app_root_path: PathBuf,
    /// The resolved Windhawk AppData directory (`getCoreInfo`
    /// `fsPaths.appDataPath`): the tree the whole machine shares on a system
    /// install, and a folder in the install tree for a portable copy. The parent
    /// of the editor's `EditorWorkspaces` container and its `UIData` VSCodium
    /// profile, and of the engine the mod-runtime seed copies into.
    pub app_data_path: PathBuf,
    /// Whether this is a portable install (`getCoreInfo` `portable`). It decides
    /// where the window's own data goes: a portable copy keeps it inside the
    /// install directory so it travels with the copy, and a system install puts
    /// it in this user's own profile (`lifecycle::ui_data`).
    pub portable: bool,
    /// `getCoreInfo` `fsPaths.uiPath`, where the VSCodium exe and clangd live;
    /// the editor launcher opens the exe from here and hands the child
    /// `WINDHAWK_UI_PATH`.
    pub ui_path: PathBuf,
    /// `getCoreInfo` `fsPaths.compilerPath`, the compiler root handed to the child as
    /// `WINDHAWK_COMPILER_PATH`.
    pub compiler_path: PathBuf,
}

/// A fatal startup failure: the message to present before exit and the source
/// origin it was raised at (DIAGNOSTIC). `lib.rs` renders both in the fatal box;
/// the message stays clean (the location is a separate line).
pub struct StartupError {
    pub message: String,
    pub location: Option<SourceLocation>,
}

impl StartupError {
    /// A failure from a host call, ADOPTING the host error's origin (the DLL-load
    /// site in core-client's `api.rs`, the contract gate, the core's wire origin)
    /// over this call site. `context`, when non-empty, prefixes the host message
    /// with the step that failed; the DLL-load message is self-describing, so it
    /// passes an empty context.
    fn from_host(context: &str, error: HostError) -> StartupError {
        let location = error.location().cloned();
        let message = if context.is_empty() {
            error.to_string()
        } else {
            format!("{context}: {error}")
        };
        StartupError { message, location }
    }
}

/// Load + gate the DLL and create this process's own session, feeding its events
/// to `pump`.
///
/// `app_root` is discovered separately ([`discover_app_root`]) because the
/// decision that starts the elevation ladder is made from it BEFORE any of this
/// happens - which is what lets the ladder overlap the DLL load, the session
/// create, and the window build rather than queue behind them.
pub fn start_core(app_root: &str, pump: Sender<PumpMessage>) -> Result<CoreHandles, StartupError> {
    // The host error names the DLL path and the OS failure (code and text) and
    // carries its own core-client origin, so surface it as-is (empty context)
    // rather than re-prefixing "Failed to load windhawk-core.dll". The fatal box
    // leads with "Windhawk could not start." (lib.rs), so this is the diagnostic
    // second paragraph.
    let core =
        GatedCore::load(&resolve_dll_path()).map_err(|error| StartupError::from_host("", error))?;
    // No --arch override (auto): the core detects the OS native machine itself.
    let config =
        SessionConfig::resolve(app_root.to_owned(), "windhawk-ui", product_version(), None);
    let session = core
        .create_session(&config, callbacks(pump, FIRST_GENERATION))
        .map_err(|error| StartupError::from_host("Failed to create the core session", error))?;

    let StartupInfo {
        app_root_path,
        app_data_path,
        portable,
        ui_path,
        compiler_path,
    } = resolve_startup_info(&session)?;

    Ok(CoreHandles {
        core: Arc::new(core),
        session: Arc::new(session),
        app_root_path,
        app_data_path,
        portable,
        ui_path,
        compiler_path,
    })
}

/// The `getCoreInfo` fields the UI needs at startup: the install directory (the
/// mod-runtime seed's copy source), the resolved AppData directory (where it
/// roots its WebView2 profile and the editor's workspaces), the portable flag
/// (which, with the process admin state, decides the window title), and the
/// UI/compiler paths the editor launcher needs.
pub struct StartupInfo {
    pub app_root_path: PathBuf,
    pub app_data_path: PathBuf,
    pub portable: bool,
    pub ui_path: PathBuf,
    pub compiler_path: PathBuf,
}

/// Read the startup info from the core (`getCoreInfo`). The core owns storage
/// resolution - portable vs registry, `%VAR%` expansion, relative-to-app-root
/// joins - so the UI asks it for the final paths rather than re-deriving them, and
/// for the portable flag rather than re-reading windhawk.ini. `getCoreInfo` is a
/// synchronous read served straight off the just-created session, so it needs no
/// event pump. A failure is fatal: without the paths the UI cannot place its
/// WebView2 profile or launch the editor where intended.
pub fn resolve_startup_info(session: &dyn SessionApi) -> Result<StartupInfo, StartupError> {
    let info: CoreInfo = session
        .invoke_as("getCoreInfo", &json!({}))
        .map_err(|error| StartupError::from_host("Failed to read the Windhawk core info", error))?;
    Ok(StartupInfo {
        app_root_path: PathBuf::from(info.fs_paths.app_root_path),
        app_data_path: PathBuf::from(info.fs_paths.app_data_path),
        portable: info.portable,
        ui_path: PathBuf::from(info.fs_paths.ui_path),
        compiler_path: PathBuf::from(info.fs_paths.compiler_path),
    })
}

/// The product version embedded at build time (the workspace `version`), feeding
/// the session's `windhawkVersion` and the `windhawk-ui/<version>` user agent.
pub fn product_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Discover the app root exe-relative: the directory holding `windhawk-ui.exe`,
/// which is the installation directory containing `windhawk.ini`. A debug build
/// first honors `WINDHAWK_DEBUG_APP_ROOT` (development against a scratch
/// install), mirroring the DLL-path debug override; release builds ignore it.
pub fn discover_app_root() -> Option<String> {
    if cfg!(debug_assertions)
        && let Ok(path) = std::env::var("WINDHAWK_DEBUG_APP_ROOT")
        && !path.is_empty()
        && has_windhawk_ini(Path::new(&path))
    {
        return Some(path);
    }

    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    has_windhawk_ini(dir).then(|| dir.to_string_lossy().into_owned())
}

/// The session callbacks the core fires on its own threads. The event callback
/// ONLY forwards the event to the pump - it does no work on the core thread and
/// never re-enters the session (the FFI re-entrancy rule); the pump thread drains
/// the channel and dispatches. `generation` is the session these callbacks belong
/// to: the ids the core allocates are unique only within it, so the pump routes on
/// the pair (`pump::ops`). The log callback forwards core diagnostics to stderr.
fn callbacks(pump: Sender<PumpMessage>, generation: u64) -> SessionCallbacks {
    SessionCallbacks {
        log: Box::new(|level, message| eprintln!("[core:{level}] {message}")),
        event: Box::new(move |op_id, event_json| {
            // Best effort: a closed receiver means the pump (and the app) is gone.
            let _ = pump.send(PumpMessage::Event {
                generation,
                op_id,
                event_json,
            });
        }),
    }
}
