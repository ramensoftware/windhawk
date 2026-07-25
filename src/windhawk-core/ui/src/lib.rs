//! The native Windhawk UI library: a protocol adapter over the C ABI that hosts
//! the shared React front-end in a WebView2 window and translates the webview
//! envelope protocol into core invokes. `main.rs` is a thin shell over `run`.
//! The policy - dispatch, the command handlers, the pure shapers - lives here
//! and is exercisable headless through the [`EmitSink`]/[`BridgeCtx`] seams
//! with no Tauri loop.

// The UI has three located Win32 touchpoints (the DBWIN log capture, the detect
// mutex + fatal-startup box, and the theme read + native-frame theming), so the
// crate can no longer `forbid(unsafe_code)`. Instead it follows the `windows/`
// adapter convention: deny unsafe ops outside an `unsafe` block and require a
// multi-line `// SAFETY:` note on every block. Unsafe stays confined to
// `logwindow/capture.rs`, `lifecycle/window.rs`, and `shell.rs`; the rest of
// the crate is safe.
#![deny(unsafe_op_in_unsafe_fn)]

mod commands;
// The launch-into-VSCode subsystem: the workspace manager, the VSCodium
// launcher, and the [`editor::Editor`] the development handlers reach through
// the bridge context. Exposed as public API so the handler orchestration tests
// construct an `Editor` over a recording launch seam.
pub mod editor;
mod file_dialog;
mod ipc;
mod lifecycle;
mod logwindow;
mod pump;
mod shape;
mod shell;
mod theme;

use std::sync::Arc;

use serde_json::json;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use windhawk_core_host::Session;
use windhawk_core_protocol::AppSettings;

// Internal handles `run` wires together.
use ipc::bridge::{wh_ipc, wh_log_backlog, wh_log_stop_capture};
use ipc::emit_sink::AppHandleSink;
use lifecycle::CoreHandles;
use lifecycle::window;
use lifecycle::window_state;
use logwindow::AppLogController;
use theme::AppThemeControl;

// The headless test surface (the integration smoke drives dispatch against a real
// session and a recording sink, with no Tauri loop): re-exported so a test binary
// can build a context and call into the bridge. These names are also what `run`
// uses below.
pub use commands::app::announce_app_settings;
pub use file_dialog::{DialogOutcome, FileDialog};
pub use ipc::bridge::{BridgeCtx, handle_envelope};
pub use ipc::emit_sink::EmitSink;
pub use ipc::envelope::{Envelope, EnvelopeType};
pub use logwindow::{LogController, NoopLogController};
pub use pump::profile_watch::refresh_installed_mods_details;
pub use shell::ThemeSetting;
pub use theme::{NativeThemeControl, NoopThemeControl};

/// The main UI window's private data subfolder under the Windhawk AppData
/// directory (`CoreHandles::app_data_path`): it holds the WebView2 browser profile
/// (cache, cookies, Local Storage, IndexedDB - WebView2 creates its own `EBWebView`
/// tree inside) and the window-state file (`window_state::FILE_NAME`). Rooting the
/// UI's on-disk data here keeps it with the rest of Windhawk's data, and inside the
/// install tree for a portable copy, instead of the Tauri Windows defaults (a
/// `<identifier>` folder under `%LOCALAPPDATA%` for the WebView2 profile, `%APPDATA%`
/// for window state) which a portable copy would leave behind.
const UI_DATA_SUBDIR: &str = "UIMainData";

/// WebView2's fixed browser-profile folder name, which it creates inside the data
/// directory (`UI_DATA_SUBDIR`). Named here so startup can pre-create it and make
/// it writable before handing the data directory to WebView2
/// (`window::ensure_webview_profile_writable`).
const WEBVIEW_PROFILE_SUBDIR: &str = "EBWebView";

/// Build and run the Tauri application. The single-instance plugin makes the
/// first process authoritative: a bare re-launch
/// ensures-running-and-foreground, forwarded to the primary's callback, which
/// shows and focuses the window (the tray closes the UI with a window message,
/// not a re-launch, so there is no quit intent to forward). The core session is
/// brought up in `setup` so only the primary creates one; a startup failure is
/// fatal and presented natively. Development is always on for the native build:
/// VSCodium ships with the install, so the launch entry points route to real
/// handlers and a missing editor is a launch-time error, not a hidden button.
pub fn run() {
    // Give the process a stable AppUserModelID before any window opens so the
    // taskbar groups the UI window under a single Windhawk identity rather than
    // one derived from the executable path.
    window::set_app_user_model_id();

    // Held for the process lifetime so the detect-running named mutex exists
    // exactly while the UI runs; `_detect` drops at app exit. Creating it also
    // tells us whether a UI is already running.
    let _detect = window::hold_detect_mutex();

    // If a UI is already running we are a second instance: the single-instance plugin
    // will forward our argv to the primary and exit, and the primary's callback brings
    // its window to front. A background process cannot SetForegroundWindow on its own -
    // the arriving (foreground-eligible) instance has to grant it - so we grant it here
    // before the forward; without it the primary only flashes its taskbar button. The
    // primary itself has no one to grant to, so it skips this.
    //
    // But the detect mutex only proves a UI process is alive. One wedged holding the
    // single-instance lock without ever showing its window swallows every relaunch: the
    // plugin hands off to it and we exit, and nothing appears. So wait briefly for the
    // window - covering a normal startup we may be racing, since the tray only launches
    // us once it already sees no visible window - and if it never shows, tell the user
    // how to clear the stuck process instead of vanishing.
    if _detect.another_instance_running() {
        if window::wait_for_main_window_visible() {
            window::allow_foreground_handoff();
        } else {
            window::show_stuck_background_instance();
            return;
        }
    } else {
        // We are the primary (first) instance. The main thread is about to do the
        // startup work that can wedge - session bring-up, and above all the WebView2
        // window creation - so watch it from a side thread: if our window never
        // appears, offer to keep waiting or end the process, rather than leaving a hung,
        // windowless Windhawk that then makes every relaunch hand off into the void (the
        // state the second-instance check above only mitigates after the fact).
        window::spawn_startup_watchdog();
    }

    tauri::Builder::default()
        // Single-instance MUST be the first plugin. On a second launch it
        // forwards the new argv to this (primary) instance and exits the second
        // process; the callback brings the primary's window to front. A launch
        // always means ensure-running-and-foreground; the tray closes the UI
        // with a window message (SC_CLOSE), not a re-launch, so there is no
        // intent to parse.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::show_and_focus_main(app);
        }))
        // The external-link shim: the navigation handler routes external links
        // through this plugin.
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // Bring up the core before the window opens. A failure is fatal and
            // shown natively, since there is no webview yet to render it.
            let CoreHandles {
                core,
                session,
                events,
                app_root_path,
                app_data_path,
                portable,
                ui_path,
                compiler_path,
            } = match lifecycle::start_core() {
                Ok(handles) => handles,
                Err(error) => {
                    // The message is the diagnostic second paragraph; the origin
                    // location (DIAGNOSTIC) follows on its own line when present.
                    let detail = match &error.location {
                        Some(location) => format!("{}\n\n(at {location})", error.message),
                        None => error.message,
                    };
                    fail_startup(&detail);
                }
            };

            // The stored UI theme, read once to seed the native frame, the first-frame
            // background, and the injected initial-theme global before the window opens.
            // The front-end applies the theme to its own document once the settings
            // arrive over IPC; this only covers the pieces the native shell owns.
            // `theme_dark` is the resolved value (the OS preference when the setting is
            // "auto"); `theme_setting` keeps the raw choice, which the WebView2 color scheme
            // consumes directly (its auto scheme follows the OS).
            let theme_setting = startup_theme_setting(&session);
            let theme_dark = theme_setting.resolved_dark();

            // Build the main window in Rust (not tauri.conf.json) so the theme
            // background color, the theme + scrollbar + log-pane initialization
            // scripts, and the external-link navigation handler attach. The
            // background color is the system theme's so the first frame matches
            // it instead of flashing white before the document paints. The
            // scrollbar script replaces WebView2's Edge Fluent scrollbars with
            // flat themed overlay ones; the log-pane script injects the output
            // pane into the shared front-end (neither can ship in the
            // separate-repo bundle, and an init script is exempt from the
            // `script-src 'self'` CSP). The CSP and `withGlobalTauri` stay in
            // config.
            //
            // Built hidden: the remembered geometry is restored below while the window
            // is invisible, then it is shown - so it opens directly at its saved
            // size/position instead of opening at the default size and visibly
            // resizing. `inner_size`/`min_inner_size` are the first-run fallback the
            // restore keeps when there is no saved state.
            // A normal (non-portable) install expects to run elevated so it can
            // manage the system-wide engine; when it is not, say so in the title so
            // the user understands why admin-only actions may fail. A portable copy
            // makes no such demand, so it stays a plain "Windhawk".
            let title = if !portable && !window::is_running_as_admin() {
                "Windhawk (not running as administrator)"
            } else {
                "Windhawk"
            };

            // The UI data folder (<appData>\UIMainData) holds the WebView2 profile
            // and the window-state file. Make sure the WebView2 profile subtree
            // exists and the current user can write to it before handing the folder
            // to WebView2: a profile first created by an elevated or different-user
            // run can otherwise deny the current user write access and break WebView2
            // startup (see window::ensure_webview_profile_writable).
            //
            // A create failure (as opposed to the DACL grant, which is best effort)
            // is fatal and surfaced natively: a non-elevated launch against a system
            // install whose ProgramData grants Users only read+execute cannot make
            // the folder, and without this WebView2's own create below raises the same
            // denial as an opaque exit-101 setup panic.
            let ui_data_dir = app_data_path.join(UI_DATA_SUBDIR);
            if let Err(error) =
                window::ensure_webview_profile_writable(&ui_data_dir.join(WEBVIEW_PROFILE_SUBDIR))
            {
                fail_startup(&data_dir_failure_detail(&ui_data_dir, portable, &error));
            }

            let nav_handle = app.handle().clone();
            let built =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title(title)
                    // Give the native window a stable Win32 class name so external
                    // tools (and our own native launcher/tray) can locate it via
                    // FindWindow, rather than tao's generic default ("Tauri Window").
                    // The class name is fixed at window creation, so it must be set on
                    // the builder here. The same constant backs the second-instance
                    // window-visibility check above.
                    .window_classname(window::MAIN_WINDOW_CLASS)
                    // Store the WebView2 profile in the UI data folder under the
                    // Windhawk AppData (UI_DATA_SUBDIR) rather than Tauri's
                    // %LOCALAPPDATA% default; see the constant for why. An absolute
                    // path set on the builder is used verbatim (Tauri only forces its
                    // default when none is given), and Tauri creates the directory.
                    .data_directory(ui_data_dir.clone())
                    .visible(false)
                    .inner_size(1280.0, 768.0)
                    .min_inner_size(
                        window_state::MIN_INNER_WIDTH as f64,
                        window_state::MIN_INNER_HEIGHT as f64,
                    )
                    // WebView2 disables page zoom by default (wry sets
                    // IsZoomControlEnabled from this flag); turn it on so Ctrl+/-/0,
                    // Ctrl+wheel, and pinch zoom the content. The browser shortcuts we
                    // do not want are removed separately (shell::disable_browser_shortcuts).
                    .zoom_hotkeys_enabled(true)
                    // Pin the window theme to the stored setting (`None` under "auto"). An
                    // explicit theme has to be pinned here so tao suppresses the OS
                    // `ThemeChanged` event that would otherwise make tauri-runtime-wry reset
                    // WebView2's color scheme (its context menus, dialogs) back to the OS.
                    .theme(shell::window_theme(theme_setting))
                    .background_color(shell::theme_background_color(theme_dark))
                    .initialization_script(shell::theme_init_script(theme_dark))
                    .initialization_script(shell::scrollbar_init_script())
                    .on_navigation(move |url| shell::handle_navigation(&nav_handle, url))
                    .build();
            let main_window = match built {
                Ok(window) => window,
                Err(error) => {
                    // Surface a window-build failure natively instead of the opaque
                    // exit-101 panic Tauri raises when the setup hook returns Err. The
                    // data folder was ensured writable above, so a denial here is a
                    // different WebView2 startup fault (a missing runtime, a profile
                    // locked by another instance); present it as-is.
                    fail_startup(&format!("The main window could not be created.\n\n{error}"));
                }
            };

            // Theme the native title bar and border to match the stored content
            // theme, while the window is still hidden so the first painted frame is
            // themed rather than a stock-light strip.
            shell::apply_frame_theme(&main_window, theme_dark);

            // Replace tao's single oversized title-bar icon with crisp ones
            // sized for the window's DPI, loaded from the executable's
            // multi-resolution icon group, while still hidden so the first
            // frame shows them.
            shell::apply_window_icons(&main_window);

            // Disable the WebView2 browser shortcuts that have no place in this
            // app window - show downloads, print, reload/hard reload, and the
            // caret-browsing toggle - while keeping find, zoom, and clipboard
            // keys.
            shell::disable_browser_shortcuts(&main_window);

            // Trim the WebView2 context menu to the items this app wants:
            // back/forward and the input context menu
            // (cut/copy/paste/undo/redo/select all), dropping the rest -
            // reload, save as, print, share, web select, inspect, ...
            shell::customize_context_menu(&main_window);

            // Theme WebView2's own surfaces (context menus, dialogs) to the stored setting.
            // An explicit theme pins them; "auto" leaves WebView2's auto scheme, which
            // follows the OS (so they do not pop light on a light OS while the app is dark).
            shell::apply_webview_color_scheme(&main_window, theme_setting);

            // Persist the main window's state across runs in the UI data folder
            // (<appData>\UIMainData\window-state.json), beside the WebView2 profile,
            // so a portable copy carries it too. Restore the saved state while the
            // window is still hidden - it then opens directly at its remembered
            // size/position and zoom level instead of at the builder default and
            // visibly jumping - and seed the live tracker from the saved state (or the
            // current geometry on first run) so a window closed while maximized still
            // persists sensible restore bounds. A missing or unreadable file is a
            // benign first run: the builder defaults stand.
            let window_state_path = ui_data_dir.join(window_state::FILE_NAME);
            let saved_state = window_state::load(&window_state_path);
            if let Some(state) = &saved_state {
                window_state::restore_geometry(&main_window, state);
            }
            let state_tracker = Arc::new(window_state::Tracker::new(
                window_state_path,
                saved_state
                    .or_else(|| window_state::capture(&main_window))
                    .unwrap_or_default(),
            ));

            // The zoom factor is the one persisted facet the window does not own: apply
            // it to the WebView2 controller and track the user's later zooming there.
            let saved_zoom = saved_state.unwrap_or_default().zoom();
            let zoom_tracker = state_tracker.clone();
            shell::apply_and_track_zoom(&main_window, saved_zoom, move |zoom| {
                zoom_tracker.on_zoom_changed(zoom)
            });

            let _ = main_window.show();
            let _ = main_window.set_focus();

            let emit: Arc<dyn EmitSink> = Arc::new(AppHandleSink::new(app.handle().clone()));
            let log: Arc<dyn LogController> = Arc::new(AppLogController::new(app.handle().clone()));

            // Owns the current theme setting and re-applies the native window surfaces when
            // the setting changes (through the bridge context), the window's focus changes,
            // or - under "auto" - the OS switches light/dark (the event handlers below).
            let theme_control =
                Arc::new(AppThemeControl::new(app.handle().clone(), theme_setting));

            // Stop DBWIN capture when the (only) window closes: capture is
            // scoped to while the log pane is open, since it contends for the
            // single-owner DBWIN buffer. The pane's Close button does the same
            // mid-session.
            let log_on_close = log.clone();
            let event_app = app.handle().clone();
            let theme_on_focus = theme_control.clone();
            main_window.on_window_event(move |event| match event {
                tauri::WindowEvent::Destroyed => log_on_close.stop_capture(),
                // Track the window geometry live and persist the state (geometry plus
                // the separately tracked zoom factor) on close. Live tracking is what
                // lets the normal (non-maximized) bounds survive a maximize: while
                // maximized the OS reports the maximized rect, so the tracker keeps the
                // last restored size/position for the next launch.
                tauri::WindowEvent::Moved(position) => {
                    if let Some(window) = event_app.get_webview_window("main") {
                        state_tracker.on_moved(&window, *position);
                    }
                }
                tauri::WindowEvent::Resized(size) => {
                    if let Some(window) = event_app.get_webview_window("main") {
                        state_tracker.on_resized(&window, *size);
                    }
                }
                tauri::WindowEvent::CloseRequested { .. } => {
                    if let Some(window) = event_app.get_webview_window("main") {
                        state_tracker.save(&window);
                    }
                }
                // DWM has no separate inactive-frame color, so re-push the frame colors
                // for the new focus state on each transition: dimmed when the window
                // loses focus, restored when it regains it. The control supplies the
                // current theme (which the runtime setting may have changed).
                tauri::WindowEvent::Focused(active) => {
                    theme_on_focus.apply_focus(*active);
                }
                // The OS switched light/dark. This fires only under "auto": an explicit theme
                // pins the window theme (shell::window_theme), which makes tao suppress the
                // event on an OS switch - and with it tauri-runtime-wry's ThemeChanged handler
                // that would otherwise reset WebView2's color scheme to the OS. Under "auto"
                // the native frame and injected tokens are re-pushed here (the webview content
                // follows via the front-end's matchMedia, WebView2's surfaces via that handler).
                tauri::WindowEvent::ThemeChanged(_) => {
                    theme_on_focus.reapply_for_os_change();
                }
                _ => {}
            });

            // The launch-into-VSCode environment: the shared workspace manager
            // (its process-local lock serializes allocate and sweep across
            // handlers, so it is one instance) and the VSCodium launcher, both
            // rooted at the `getCoreInfo` paths. Always present: development is
            // on for the native build.
            let editor: Arc<editor::Editor> = Arc::new(editor::Editor::new(
                &app_data_path,
                ui_path,
                compiler_path,
            ));

            let ctx = BridgeCtx::new(
                core,
                session,
                emit,
                log,
                editor,
                theme_control,
                Arc::new(file_dialog::Win32FileDialog),
            );

            // The handlers run on `wh_ipc`'s blocking workers and reach the context
            // from managed state.
            app.manage(ctx.clone());

            // Seed the engine's mod runtime libraries at startup (the install-tree
            // ModsRuntime -> Engine\Mods copy of libc++/libunwind/the mod shim, for
            // files not already present), off the setup thread so copying never delays
            // the window. Best-effort, like the workspace sweep below.
            let seed_app_data = app_data_path.clone();
            std::thread::Builder::new()
                .name("wh-mods-runtime-seed".to_owned())
                .spawn(move || {
                    lifecycle::mods_runtime::copy_mods_runtime_libs(&app_root_path, &seed_app_data)
                })
                .expect("spawn the mod runtime seed thread");

            // Garbage-collect abandoned editor workspaces at startup, off the
            // setup thread so a slow rename probe never delays the window. The
            // manager's lock serializes it against any allocate a handler
            // starts.
            let sweep_ctx = ctx.clone();
            std::thread::Builder::new()
                .name("wh-editor-sweep".to_owned())
                .spawn(move || commands::dev::sweep_abandoned_workspaces(&sweep_ctx))
                .expect("spawn the editor workspace sweep thread");

            // The pump thread owns the event receiver and routes each operation
            // event to its op through the bridge (off the core callback thread,
            // so a composite follow-up may re-enter the session).
            let pump_ctx = ctx.clone();
            std::thread::Builder::new()
                .name("wh-event-pump".to_owned())
                .spawn(move || {
                    while let Ok((op_id, event_json)) = events.recv() {
                        // Isolate a panic in one op's dispatch: without this a single
                        // shaper bug would kill the pump thread, after which NO async
                        // reply is ever emitted (every pending messageWithReply hangs).
                        // The registry's locks recover from poisoning (into_inner), so
                        // the shared state stays usable after a caught panic - which is
                        // what makes AssertUnwindSafe sound here. The default panic hook
                        // still prints the panic; we add which op it was.
                        let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || pump_ctx.dispatch_event(op_id, &event_json),
                        ));
                        if dispatched.is_err() {
                            eprintln!(
                                "windhawk-ui: event pump recovered from a panic dispatching op {op_id}"
                            );
                        }
                    }
                })
                .expect("spawn the event pump thread");

            // The profile watcher (update-availability / ratings refresh) and
            // the background catalog refresh.
            pump::profile_watch::spawn(ctx.clone());
            pump::startup::kick(&ctx);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            wh_ipc,
            wh_log_backlog,
            wh_log_stop_capture
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Windhawk UI");
}

/// The stored UI theme setting, read from `getAppSettings` once at startup to seed the
/// native shell before the window opens. A read failure is the dark default (as is any
/// unrecognized value, per `ThemeSetting::parse`), matching the core's stored default and
/// the front-end.
fn startup_theme_setting(session: &Session) -> ThemeSetting {
    match session.invoke_as::<AppSettings, _>("getAppSettings", &json!({})) {
        Ok(settings) => ThemeSetting::parse(&settings.theme),
        Err(_) => ThemeSetting::Dark,
    }
}

/// Present a fatal startup failure as a native modal box, then terminate. Used in
/// the setup hook, before any webview exists to render a reply into: returning Err
/// from setup would surface only as Tauri's opaque exit-101 panic. The lead line is
/// fixed; `detail` is the diagnostic paragraph shown beneath it.
fn fail_startup(detail: &str) -> ! {
    // Stand the startup watchdog down first: it only sees "no window yet", which a
    // fatal failure also produces, and this path owns the message and the exit.
    window::suppress_startup_watchdog();
    window::show_fatal(&format!("Windhawk could not start.\n\n{detail}"));
    std::process::exit(1);
}

/// The diagnostic detail for a UI-data-folder creation failure, tailored to the
/// error and install kind. A permission denial on a non-portable install almost
/// always means the UI was launched unelevated against a system ProgramData folder
/// only administrators can write, so point at elevation; a portable copy writes
/// beside the install, where a denial is environmental, so keep the hint generic.
/// Any non-permission error carries no hint - just the folder and the error.
fn data_dir_failure_detail(
    ui_data_dir: &std::path::Path,
    portable: bool,
    error: &std::io::Error,
) -> String {
    let hint = match (error.kind(), portable) {
        (std::io::ErrorKind::PermissionDenied, false) => {
            "\n\nA system-wide Windhawk install keeps its data under this folder, \
             which only an administrator can create. Start Windhawk the usual way \
             (it runs elevated), or run windhawk-ui.exe as administrator."
        }
        (std::io::ErrorKind::PermissionDenied, true) => {
            "\n\nMake sure this folder can be created and written to."
        }
        _ => "",
    };
    format!(
        "Windhawk could not create its data folder:\n{}\n\n{error}{hint}",
        ui_data_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::path::Path;

    // A permission denial on a system (non-portable) install is the reported
    // scenario: the detail names the folder, echoes the OS error, and points at
    // elevation - the actionable fix.
    #[test]
    fn permission_denied_non_portable_points_at_elevation() {
        let error = Error::from(ErrorKind::PermissionDenied);
        let detail = data_dir_failure_detail(
            Path::new(r"C:\ProgramData\Windhawk\UIMainData"),
            false,
            &error,
        );

        assert!(detail.contains(r"C:\ProgramData\Windhawk\UIMainData"));
        assert!(detail.contains(&error.to_string()));
        assert!(detail.contains("administrator"));
    }

    // A portable copy writes beside the install, so a denial is environmental, not
    // an elevation problem: keep the hint generic and never tell the user to run as
    // administrator (a portable copy makes no such demand).
    #[test]
    fn permission_denied_portable_stays_generic() {
        let error = Error::from(ErrorKind::PermissionDenied);
        let detail = data_dir_failure_detail(Path::new(r"D:\Windhawk\UIMainData"), true, &error);

        assert!(detail.contains("Make sure this folder"));
        assert!(!detail.contains("administrator"));
    }

    // A non-permission failure (a stale file where the folder should be, a bad
    // path) has no elevation or writability remedy, so it carries no hint - just
    // the folder and the raw error.
    #[test]
    fn other_error_carries_no_hint() {
        let error = Error::from(ErrorKind::NotFound);
        let detail = data_dir_failure_detail(
            Path::new(r"C:\ProgramData\Windhawk\UIMainData"),
            false,
            &error,
        );

        assert!(detail.contains(&error.to_string()));
        assert!(!detail.contains("administrator"));
        assert!(!detail.contains("Make sure this folder"));
    }
}
