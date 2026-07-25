//! Session bring-up. Discovers the app root exe-relative, loads + gates
//! `windhawk-core.dll`, creates the single long-lived core session, and wires
//! the session's event callback to the channel the async pump drains. A startup
//! failure is fatal; `lib.rs` presents it as a native message box
//! (`window::show_fatal`) and exits.

use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use serde_json::json;
use windhawk_core_host::windhawk_ini::has_windhawk_ini;
use windhawk_core_host::{
    GatedCore, HostError, Session, SessionCallbacks, SessionConfig, resolve_dll_path,
};
use windhawk_core_protocol::{CoreInfo, SourceLocation};

/// The loaded core, the live session, the receiving end of the operation-event
/// channel, and the resolved Windhawk AppData directory, shared into `run`. The
/// `GatedCore` is retained alongside the `Session` for the session-free
/// `parseModSource` path; the receiver is moved into the pump thread, which
/// routes each `(op_id, event_json)` to the bridge.
pub struct CoreHandles {
    pub core: Arc<GatedCore>,
    pub session: Arc<Session>,
    pub events: Receiver<(u64, String)>,
    /// The Windhawk installation directory (`getCoreInfo` `fsPaths.appRootPath`, where
    /// `windhawk.ini` lives). Its `ModsRuntime` subfolder is the source the startup
    /// mod-runtime seed copies into the engine's `Engine\Mods` under appData
    /// (`lifecycle::mods_runtime`).
    pub app_root_path: PathBuf,
    /// The resolved Windhawk AppData directory (`getCoreInfo`
    /// `fsPaths.appDataPath`). The UI roots its own on-disk data (the WebView2
    /// profile) under here, so that data lives with the rest of Windhawk's -
    /// inside the install tree for a portable copy - rather than at the Tauri
    /// Windows default under `%LOCALAPPDATA%`. Also the parent of the editor's
    /// `EditorWorkspaces` container and its `UIData` VSCodium profile.
    pub app_data_path: PathBuf,
    /// Whether this is a portable install (`getCoreInfo` `portable`). With the
    /// process admin state it decides the main window title: a non-portable install
    /// not running as admin gets a "(not running as administrator)" suffix.
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
    /// A failure raised in the UI itself (app-root discovery), tagged with its
    /// `#[track_caller]` call site.
    #[track_caller]
    fn here(message: String) -> StartupError {
        StartupError {
            message,
            location: Some(SourceLocation::from(Location::caller())),
        }
    }

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

/// Discover the app root, load + gate the DLL, and create the session.
pub fn start_core() -> Result<CoreHandles, StartupError> {
    let app_root = discover_app_root().ok_or_else(|| {
        StartupError::here(
            "Could not locate the Windhawk installation: no windhawk.ini was found \
             walking up from windhawk-ui.exe."
                .to_owned(),
        )
    })?;

    // The host error names the DLL path and the OS failure (code and text) and
    // carries its own core-client origin, so surface it as-is (empty context)
    // rather than re-prefixing "Failed to load windhawk-core.dll". The fatal box
    // leads with "Windhawk could not start." (lib.rs), so this is the diagnostic
    // second paragraph.
    let core =
        GatedCore::load(&resolve_dll_path()).map_err(|error| StartupError::from_host("", error))?;
    // No --arch override (auto): the core detects the OS native machine itself.
    let config = SessionConfig::resolve(app_root, "windhawk-ui", product_version(), None);
    let (tx, events) = mpsc::channel::<(u64, String)>();
    let session = core
        .create_session(&config, callbacks(tx))
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
        events,
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
struct StartupInfo {
    app_root_path: PathBuf,
    app_data_path: PathBuf,
    portable: bool,
    ui_path: PathBuf,
    compiler_path: PathBuf,
}

/// Read the startup info from the core (`getCoreInfo`). The core owns storage
/// resolution - portable vs registry, `%VAR%` expansion, relative-to-app-root
/// joins - so the UI asks it for the final paths rather than re-deriving them, and
/// for the portable flag rather than re-reading windhawk.ini. `getCoreInfo` is a
/// synchronous read served straight off the just-created session, so it needs no
/// event pump. A failure is fatal: without the paths the UI cannot place its
/// WebView2 profile or launch the editor where intended.
fn resolve_startup_info(session: &Session) -> Result<StartupInfo, StartupError> {
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
fn product_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Discover the app root exe-relative: the directory holding `windhawk-ui.exe`,
/// which is the installation directory containing `windhawk.ini`. A debug build
/// first honors `WINDHAWK_DEBUG_APP_ROOT` (development against a scratch
/// install), mirroring the DLL-path debug override; release builds ignore it.
fn discover_app_root() -> Option<String> {
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
/// ONLY forwards `(op_id, event_json)` to the channel - it does no work on the
/// core thread and never re-enters the session (the FFI re-entrancy rule); the
/// pump thread drains the channel and dispatches. The log callback forwards
/// core diagnostics to stderr.
fn callbacks(events: Sender<(u64, String)>) -> SessionCallbacks {
    SessionCallbacks {
        log: Box::new(|level, message| eprintln!("[core:{level}] {message}")),
        event: Box::new(move |op_id, event_json| {
            // Best effort: a closed receiver means the pump (and the app) is gone.
            let _ = events.send((op_id, event_json));
        }),
    }
}
