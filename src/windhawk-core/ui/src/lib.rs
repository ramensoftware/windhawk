//! The native Windhawk UI library: a protocol adapter over the C ABI that hosts
//! the shared React front-end in a WebView2 window and translates the webview
//! envelope protocol into core invokes. `main.rs` is a thin shell over `run`.
//! The policy - dispatch, the command handlers, the pure shapers - lives here
//! and is exercisable headless through the [`EmitSink`]/[`BridgeCtx`] seams
//! with no Tauri loop.

// The UI has a handful of located Win32 touchpoints (the DBWIN log capture, the
// detect mutex + fatal-startup box, the theme read + native-frame theming, the
// startup splash window, the WebView2 environment probe behind a fatal window
// failure, and the runtime broker's elevation ladder and process-lifetime
// objects), so the crate can no longer `forbid(unsafe_code)`. Instead it follows
// the `windows/` adapter convention: deny unsafe ops outside an `unsafe` block
// and require a multi-line `// SAFETY:` note on every block. Unsafe stays
// confined to `logwindow/capture.rs`, `lifecycle/diagnostics.rs`,
// `lifecycle/window.rs`, `shell.rs`, `splash/`, `broker/launch.rs`, and
// `broker/serve.rs`; the rest of the crate is safe.
#![deny(unsafe_op_in_unsafe_fn)]

// The elevated helper that owns the privileged core session, and the UI side of
// the channel to it. Public because `main.rs` dispatches the `--runtime-broker`
// mode before it builds anything, and because the two-process test drives the
// same entry point.
pub mod broker;
mod commands;
// The launch-into-VSCode subsystem: the workspace manager, the VSCodium
// launcher, and the [`editor::Editor`] the privileged host operations act
// through. Exposed as public API so the handler orchestration tests build the
// in-process host operations over a recording launch seam.
pub mod editor;
mod file_dialog;
mod ipc;
mod lifecycle;
mod logwindow;
mod pump;
mod shape;
mod shell;
mod splash;
mod theme;

use std::sync::Arc;

use serde_json::json;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use windhawk_core_host::{SessionApi, SessionApiExt};
use windhawk_core_protocol::AppSettings;

// Internal handles `run` wires together.
use ipc::bridge::{wh_ipc, wh_log_backlog, wh_log_stop_capture};
use ipc::emit_sink::AppHandleSink;
use lifecycle::CoreHandles;
use lifecycle::diagnostics;
use lifecycle::taskbar_list;
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

/// The environment variable that adds browser arguments to the main window's
/// WebView2, for a launch that has to be observed rather than only run.
///
/// It exists for the window failures this side cannot see: what the UI is told
/// about one is a single `HRESULT`, and the browser's own logging
/// (`--enable-logging --v=1`, which writes into the WebView2 data folder) says
/// what happened around it. Unset, which is every ordinary launch, the window is
/// built with [`DEFAULT_WEBVIEW_BROWSER_ARGS`] alone.
const WEBVIEW_BROWSER_ARGS_VAR: &str = "WINDHAWK_UI_WEBVIEW2_ARGS";

/// The browser command line the main window is built with: the browser
/// components this window has no use for, the autoplay policy Tauri's default
/// asks for, and the switches that keep WebView2 off the network on its own
/// account.
///
/// The first two mirror what wry passes when the builder sets none. Setting
/// arguments REPLACES wry's rather than adding to them, so they have to be
/// carried here for the window to keep them - and so does anything
/// [`WEBVIEW_BROWSER_ARGS_VAR`] contributes ([`webview_browser_args`]). wry
/// exposes those defaults nowhere, so the copy is by hand and nothing links it to
/// the original; the version pin in the tests below is what makes a wry upgrade
/// re-read them instead of leaving this window on a stale mirror.
///
/// The networking pair is what stops an embedded browser from behaving like a
/// browser. Left alone, WebView2 fetches an Edge experiment configuration as it
/// starts and then runs its component updater for as long as the window is open,
/// pulling CRLSet, PKI metadata, origin trials and the rest over BITS into the
/// WebView2 profile - which on a system install sits under `%ProgramData%`. This
/// window renders local app UI plus a few known origins and needs none of it.
/// `--disable-background-networking` turns off the browser-initiated background
/// services as a class, and `--disable-component-update` names the component
/// updater outright rather than leaving it to that class. Requests the page makes
/// (the mod catalog, changelogs, readme images) are a different mechanism and are
/// unaffected. The cost is that the revocation and PKI component data no longer
/// refresh.
const DEFAULT_WEBVIEW_BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
     --autoplay-policy=no-user-gesture-required \
     --disable-background-networking --disable-component-update";

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
    // Start keeping the records the window stack emits, before anything can emit
    // one. A window or webview that fails to build is reported through the `log`
    // facade and then discarded, so this capture is the only place the reason
    // survives to be shown (lifecycle/diagnostics.rs).
    diagnostics::install_log_capture();

    // When this process was started to replace a stuck instance (the startup-stuck
    // prompt's Relaunch), let that instance finish exiting first: it holds the
    // single-instance state below until it is gone, and racing it would make this
    // process a second instance of the very one it replaces.
    window::await_relaunch_predecessor();

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
    // single-instance lock without ever getting its app on screen swallows every
    // relaunch: the plugin hands off to it and we exit, and nothing appears. So wait
    // briefly for it to finish starting - covering a normal startup we may be racing,
    // since the tray only launches us once it already sees no visible window - and if it
    // never does, tell the user how to clear the stuck process instead of vanishing.
    // Finishing STARTING, not merely having a window: the window is up carrying the
    // startup splash from the moment it is created, which is before the webview whose
    // creation is the likeliest thing to hang.
    if _detect.another_instance_running() {
        match window::wait_for_main_window_ready() {
            window::PrimaryState::Ready => window::allow_foreground_handoff(),
            // The primary is waiting for a consent dialog to be answered, which
            // the user is looking at: it is finishing its startup behind the
            // prompt, and saying anything here would be a second message about the
            // first one.
            window::PrimaryState::WaitingForElevation => return,
            window::PrimaryState::Stuck => {
                window::show_stuck_background_instance();
                return;
            }
        }
    } else {
        // We are the primary (first) instance. The main thread is about to do the
        // startup work that can wedge - session bring-up, and above all the WebView2
        // window creation - so watch it from a side thread: if the app never comes up,
        // offer to keep waiting or end the process, rather than leaving a hung Windhawk
        // stuck on its splash that then makes every relaunch hand off into the void (the
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
        // The external-link shim: the navigation and new-window handlers route
        // external links through this plugin.
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // The elevation ladder starts FIRST, before the core is even loaded.
            // Nothing about the decision needs a session - it is one flag in
            // windhawk.ini and one token check - so starting here is what lets the
            // whole elevation round trip overlap the DLL load, the session create,
            // and the WebView2 build instead of queueing behind them. What does
            // NOT overlap the build is the ladder's consent dialog: it waits for
            // the window that owns it (`broker::elevation_prompt_gate`).
            let app_root = match lifecycle::discover_app_root() {
                Some(app_root) => app_root,
                None => fail_startup(
                    "Could not locate the Windhawk installation: no windhawk.ini was found \
                     walking up from windhawk-ui.exe.",
                ),
            };
            let needs_broker = broker::needs_broker(&app_root);
            let ladder = needs_broker.then(broker::Ladder::start);

            // Bring up the core before the window opens. A failure is fatal and
            // shown natively, since there is no webview yet to render it.
            let (pump_sender, pump_messages) = std::sync::mpsc::channel::<pump::PumpMessage>();
            let CoreHandles {
                core,
                session,
                app_root_path,
                app_data_path,
                portable,
                ui_path,
                compiler_path,
            } = match lifecycle::start_core(&app_root, pump_sender.clone()) {
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

            // The launch-into-VSCode environment: the shared workspace manager
            // (its process-local lock serializes allocate and sweep across
            // handlers, so it is one instance) and the VSCodium launcher, both
            // rooted at the `getCoreInfo` paths. Built here rather than beside the
            // window because it is what this process's own host operations act
            // through, and those exist before the window does.
            let editor: Arc<editor::Editor> = Arc::new(editor::Editor::new(
                &app_data_path,
                &ui_path,
                &compiler_path,
            ));
            // Whether there is a code editor to launch at all. An install fact,
            // read once here, never asked of the elevated helper.
            let dev_tools_installed = editor::launch::dev_tools_installed(&ui_path);

            // The privileged work this process would do for itself. It stays the
            // implementation in a portable install and in an already elevated
            // window, and it is what a lost channel falls back to.
            let local_host: Arc<dyn broker::ops::HostOps> =
                Arc::new(broker::ops::LocalHostOps::for_ui(
                    core.clone(),
                    session.clone(),
                    app_root_path,
                    app_data_path.clone(),
                    editor,
                    pump_sender.clone(),
                ));

            // The two seams every handler runs against. They start on this
            // process's own session and operations - which is what serves the
            // startup reads below - and the broker's take over behind the splash,
            // once the channel is verified.
            let link = broker::BrokerLink::new(session.clone(), local_host, pump_sender, ladder);

            // The stored UI theme, read once to seed the native frame, the first-frame
            // background, and the injected initial-theme global before the window opens.
            // The front-end applies the theme to its own document once the settings
            // arrive over IPC; this only covers the pieces the native shell owns.
            // `theme_dark` is the resolved value (the OS preference when the setting is
            // "auto"); `theme_setting` keeps the raw choice, which the WebView2 color scheme
            // consumes directly (its auto scheme follows the OS).
            let theme_setting = startup_theme_setting(session.as_ref());
            let theme_dark = theme_setting.resolved_dark();

            // Build the main window in Rust (not tauri.conf.json) so the theme
            // background color, the theme + scrollbar + splash + banner
            // initialization scripts, and the external-link navigation handler
            // attach. The background color is the system theme's so the first frame
            // matches it instead of flashing white before the document paints. The
            // scrollbar script replaces WebView2's Edge Fluent scrollbars with
            // flat themed overlay ones; the banner script says when the window is
            // running without its elevated helper (neither can ship in the
            // separate-repo bundle, and an init script is exempt from the
            // `script-src 'self'` CSP). The CSP and `withGlobalTauri` stay in
            // config.

            // The UI data folder holds the WebView2 profile and the window-state
            // file, and it is the ONE thing the window cannot be built without,
            // since WebView2 is handed it directly. It is this user's own - under
            // %LOCALAPPDATA% on a system install, inside the install tree for a
            // portable copy - so creating it asks for no rights this process does
            // not already have, and the elevation ladder has nothing to do with it
            // (lifecycle/ui_data.rs).
            let ui_data_dir = ensure_ui_data_dir(&app_data_path, portable);

            // The window state is read before the window is built, not restored onto
            // it afterwards: the geometry goes into the builder, so the window is
            // created where it belongs and can be shown from its first frame.
            let window_state_path = ui_data_dir.join(window_state::FILE_NAME);
            let saved_state = window_state::load(&window_state_path);
            let opening = window_state::opening_geometry(app.handle(), saved_state.as_ref());

            // Start the startup splash before the build: creating WebView2 takes the
            // better part of a second, and the window is visible for all of it. The
            // splash attaches itself to the window as soon as it exists and fills its
            // client area with the Windhawk mark, so what opens is a themed, branded
            // window rather than an empty frame (splash/).
            splash::show(theme_dark);

            // Ask for the window to be put on its remembered rectangle in the pixels
            // the displays are laid out in, as it is created and before it is shown,
            // for the foreground as it is shown, and - where the launch opens
            // maximized - for it to be shown only once it is. The builder only takes
            // logical coordinates, which tao resolves to a display by a search of its
            // own that can land on a different one where two displays run at different
            // scales; it shows the window without activating it; and it shows a
            // maximized window at its restored size first (see
            // window_state::OpeningGeometry::prepare_creation).
            //
            // After the splash, not before: both watch for the window's WM_CREATE
            // through a hook, and Windows calls the most recently installed one first,
            // so this way the window is already on its rectangle when the splash takes
            // its size from it.
            opening.prepare_creation();

            let nav_handle = app.handle().clone();
            let new_window_handle = app.handle().clone();
            let builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    // Unconditionally "Windhawk". The suffix that used to name a
                    // non-portable install running without administrator rights
                    // described the normal case once the window stopped being
                    // elevated at all, so it would have become permanent furniture
                    // saying nothing; what it stood in for - writes will fail - is
                    // now said by the degraded-mode banner, and only when true.
                    .title("Windhawk")
                    // Give the native window a stable Win32 class name so external
                    // tools (and our own native launcher/tray) can locate it via
                    // FindWindow, rather than tao's generic default ("Tauri Window").
                    // The class name is fixed at window creation, so it must be set on
                    // the builder here. The same constant backs the second-instance
                    // window-visibility check above.
                    .window_classname(window::MAIN_WINDOW_CLASS)
                    // Store the WebView2 profile in the UI data folder rather than
                    // in Tauri's default `<identifier>` folder, which a portable
                    // copy would leave behind on the machine it ran on
                    // (lifecycle/ui_data.rs). An absolute path set on the builder is
                    // used verbatim - Tauri only forces its default when none is
                    // given.
                    .data_directory(ui_data_dir.clone())
                    // Visible from the start, at the remembered geometry the builder
                    // is given below: the window is up while WebView2 is created
                    // (roughly half a second) instead of after, showing the splash.
                    .visible(true)
                    // Unfocused, which is what keeps wry's `MoveFocus` off the
                    // webview build. wry makes that call for a focused webview
                    // as the last step of the build and propagates what it
                    // answers, and WebView2 fails it with E_INVALIDARG for a
                    // window that cannot take focus - a window minimized while
                    // the splash is up is enough, and the build is visible and
                    // minimizable for the second or so it runs. So a launch
                    // someone minimized lost the whole webview, and with it the
                    // window, to a call about focus.
                    //
                    // Both halves of the launch's focus are put back where the
                    // failure of either is not fatal. Tauri applies this flag to
                    // the window as well, which is what makes tao show the window
                    // without activating it, so the activation is asked for on the
                    // show itself (window::prepare_main_window_creation); and the
                    // focus wry would have moved into the webview is moved there
                    // when the splash hands the screen over (splash::hand_off),
                    // the first moment the webview is both built and visible.
                    .focused(false)
                    .inner_size(
                        opening.inner_size.width as f64,
                        opening.inner_size.height as f64,
                    )
                    .maximized(opening.maximized)
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
                    // The degraded-mode banner: what a window running without its
                    // elevated helper says for itself.
                    .initialization_script(broker::banner_init_script())
                    // Reports the front-end's progress to the splash: what brings
                    // the webview on screen, and what retires the splash once it
                    // has drawn there.
                    .initialization_script(splash::ready_init_script())
                    .on_navigation(move |url| shell::handle_navigation(&nav_handle, url))
                    // The sibling hook: WebView2 raises a new-window request - not a
                    // navigation - for `<a target="_blank">` and `window.open`, so
                    // without this one those links reach nothing at all.
                    .on_new_window(move |url, _features| {
                        shell::handle_new_window(&new_window_handle, &url)
                    });
            // A remembered position that still lands on a display is reused; anything
            // else (a first run, a display that is gone) opens centered.
            let builder = match opening.position {
                Some(position) => builder.position(position.x, position.y),
                None => builder.center(),
            };
            // The window's browser command line: the arguments every launch is
            // built with, plus whatever an observed launch adds
            // (WEBVIEW_BROWSER_ARGS_VAR), which is normally nothing.
            let extra_browser_args = std::env::var(WEBVIEW_BROWSER_ARGS_VAR).ok();
            let builder = builder
                .additional_browser_args(&webview_browser_args(extra_browser_args.as_deref()));

            // Built with CLSID_TaskbarList taken over for the duration: tao ends
            // every window creation with an ITaskbarList::AddTab this window has
            // no use for, and that call is a SendMessage into Explorer with no
            // timeout, so a shell that has stopped pumping would park the launch
            // here with the splash on screen and nothing behind it
            // (lifecycle/taskbar_list.rs). The guard is dropped before the
            // outcome is examined - the rest of the process, including the
            // failure paths below, sees the real class.
            let built = {
                let _taskbar_list = taskbar_list::suppress();
                builder.build()
            };

            let main_window = match built {
                Ok(window) => window,
                Err(error) => {
                    // Surface a window-build failure natively instead of the opaque
                    // exit-101 panic Tauri raises when the setup hook returns Err. The
                    // data folder is one this user owns and was created above, so what
                    // is left is a WebView2 startup fault of its own (a missing
                    // runtime, a profile locked by another instance, a folder that will
                    // not take the profile); present it as-is.
                    fail_startup(&format!("The main window could not be created.\n\n{error}"));
                }
            };

            // A window Tauri hands back is not necessarily a window that exists.
            // This hook runs from INSIDE the event loop, so the build goes through
            // the runtime handle rather than the runtime: that path posts the
            // creation to the loop, logs a failure through the `log` facade instead
            // of returning it, and answers `Ok` either way. A failed build therefore
            // arrives here as a window that was never registered, whose native
            // window is already queued for destruction - and because the loop is
            // only asked to exit for a window the runtime knows, the destruction
            // that follows would take the window off the screen and leave this
            // process running with nothing on it, saying nothing.
            //
            // Any round trip to the runtime tells the two apart: a message for an
            // unregistered window is dropped and its reply channel closes.
            if main_window.hwnd().is_err() {
                fail_startup(&diagnostics::window_creation_detail(&ui_data_dir));
            }

            // The window is built, so the hook that watched it being created has
            // nothing left to catch.
            window::finish_main_window_creation();

            // There is a window, so the ladder may put its consent dialog up. Both
            // halves of that matter: the dialog has an owner to be modal to, and
            // this launch has got far enough that a prompt asks to elevate
            // something the user is going to see, rather than going up behind the
            // fatal box a failed build ends in. What the ladder has been free to do
            // meanwhile is its silent rung, which needs no window and has been
            // running since the first line of this hook.
            broker::elevation_prompt_gate().allow();

            // The fallback behind the placement above, for a build that did not open on
            // the remembered rectangle after all. Normally a no-op; a correction here is
            // visible, since the window has been on screen for the whole build.
            opening.place_exactly(&main_window);

            // Keep the webview from showing until the front-end has rendered
            // (splash::ready_init_script). A webview's output is composited above
            // the window's child windows whatever their z-order, so a visible one
            // would cover the splash with whatever the page has - the browser's
            // blank canvas while it loads - instead of the mark. Held back, the
            // mark stays until the app itself is there to replace it
            // (splash::wh_splash_ready); the fallback covers a page that never
            // reports.
            shell::set_webview_visible(&main_window, false);
            splash::arm_dismiss_fallback(app.handle().clone());

            // Theme the native title bar and border to match the stored content
            // theme, so the frame around the splash is themed rather than a
            // stock-light strip.
            shell::apply_frame_theme(&main_window, theme_dark);

            // The fallback behind the icons the window is shown with
            // (window::prepare_main_window_creation), and normally a no-op. For a
            // build whose creation hook was never installed, this is what replaces
            // tao's single oversized title-bar icon with crisp ones sized for the
            // window's DPI, loaded from the executable's multi-resolution icon group.
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
            // (window-state.json, read above), beside the WebView2 profile, so a
            // portable copy carries it too. The saved geometry went into the
            // builder; here the live tracker is seeded from the saved state (or the
            // current geometry on first run) so a window closed while maximized
            // still persists sensible restore bounds.
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

            let emit: Arc<dyn EmitSink> = Arc::new(AppHandleSink::new(app.handle().clone()));
            let log: Arc<dyn LogController> = Arc::new(AppLogController::new(app.handle().clone()));

            // Owns the current theme setting and re-applies the native window surfaces when
            // the setting changes (through the bridge context), the window's focus changes,
            // or - under "auto" - the OS switches light/dark (the event handlers below).
            let theme_control = Arc::new(AppThemeControl::new(app.handle().clone(), theme_setting));

            // Stop DBWIN capture when the (only) window closes: capture is
            // scoped to while the log pane is open, since it contends for the
            // single-owner DBWIN buffer. The pane's Close button does the same
            // mid-session.
            // DWM has no separate inactive-frame color, so the frame colors are
            // re-pushed on each activation change: dimmed when the window stops being
            // the active one, restored when it becomes it again. The control supplies
            // the current theme (which the runtime setting may have changed).
            //
            // Activation rather than Tauri's `Focused` event: the webview takes the
            // keyboard focus off the window and keeps it, after which no focus event
            // is raised at all (see shell::track_activation).
            let theme_on_activation = theme_control.clone();
            shell::track_activation(&main_window, move |active| {
                theme_on_activation.apply_activation(active);
            });

            let log_on_close = log.clone();
            // The `Global\` half of the capture, which the elevated helper runs
            // for this window when there is one.
            let host_on_close = link.host();
            let event_app = app.handle().clone();
            let theme_on_os_change = theme_control.clone();
            let link_on_close = link.clone();
            main_window.on_window_event(move |event| match event {
                tauri::WindowEvent::Destroyed => {
                    log_on_close.stop_capture();
                    host_on_close.dbwin_stop();
                    // Tell the broker to go, and give it a moment. Its read loop
                    // ends when this process does anyway; asking first is what
                    // keeps an elevated process from being momentarily orphaned.
                    link_on_close.shutdown();
                    // The window can also be taken away without anyone asking for
                    // it: WebView2 destroys the window it draws into when its page
                    // or its browser process asks it to, which is a failure and not
                    // an exit. It reads as one, though - the window goes, and what
                    // the user is left with is a Windhawk that vanished - so say so
                    // rather than let it pass for a close. The teardown above runs
                    // first: the message box waits on a person, and nothing should
                    // hold an elevated helper open while it does.
                    if !window::close_was_requested() {
                        fail_unexpected_close(&diagnostics::unexpected_close_detail());
                    }
                }
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
                // The window's icons are drawn for a DPI (shell::apply_window_icons), so
                // a display whose scale changed under the window - or a move to one at
                // another scale - calls for the pair that display draws, in place of the
                // one it would otherwise stretch.
                tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    if let Some(window) = event_app.get_webview_window("main") {
                        shell::rescale_window_icons(&window, *scale_factor);
                    }
                }
                tauri::WindowEvent::CloseRequested { .. } => {
                    // The close was asked for, which is what makes the destruction
                    // that follows an exit rather than the failure the `Destroyed`
                    // arm reports.
                    window::note_close_requested();
                    if let Some(window) = event_app.get_webview_window("main") {
                        state_tracker.save(&window);
                    }
                }
                // The OS switched light/dark. This fires only under "auto": an explicit theme
                // pins the window theme (shell::window_theme), which makes tao suppress the
                // event on an OS switch - and with it tauri-runtime-wry's ThemeChanged handler
                // that would otherwise reset WebView2's color scheme to the OS. Under "auto"
                // the native frame and injected tokens are re-pushed here (the webview content
                // follows via the front-end's matchMedia, WebView2's surfaces via that handler).
                tauri::WindowEvent::ThemeChanged(_) => {
                    theme_on_os_change.reapply_for_os_change();
                }
                _ => {}
            });

            let ctx = BridgeCtx::new(
                core,
                // Whichever session and operations are behind the seams, now and
                // after every swap.
                link.session(),
                emit,
                log,
                link.host(),
                dev_tools_installed,
                theme_control,
                Arc::new(file_dialog::Win32FileDialog),
            );

            // The handlers run on `wh_ipc`'s blocking workers and reach the context
            // from managed state; the banner commands reach the link the same way.
            app.manage(ctx.clone());
            app.manage(link.clone());

            // Now that there is a window, the link can report what it is doing.
            link.attach(app.handle().clone());

            // The pump thread owns the message channel: it routes each operation
            // event to its op through the bridge (off the core callback thread, so
            // a composite follow-up may re-enter the session), and it runs the
            // session swaps, which need the same seams and the same thread.
            let pump_ctx = ctx.clone();
            std::thread::Builder::new()
                .name("wh-event-pump".to_owned())
                .spawn(move || pump::run(pump_ctx, pump_messages))
                .expect("spawn the event pump thread");

            // Everything the UI starts for ITSELF waits for the session to settle -
            // the swap to the broker's session, or degraded mode, whichever comes
            // first. The startup catalog refresh is why: its terminal writes the
            // user profile, so issued against the local session in the window
            // before the broker arrives it would either fail unelevated or be
            // drained by the swap, on every single launch. Deferring it also leaves
            // the swap-point drain empty in the normal case, which is what keeps
            // that path a rare-path concern rather than a per-launch one.
            //
            // Off the setup thread, as the seed and the sweep already were: none of
            // it may delay the window.
            let background_ctx = ctx.clone();
            let background_link = link.clone();
            std::thread::Builder::new()
                .name("wh-ui-background".to_owned())
                .spawn(move || {
                    background_link.wait_until_settled();
                    // The install-tree ModsRuntime -> Engine\Mods copy of
                    // libc++/libunwind/the mod shim, for files not already present.
                    background_ctx.host.seed_mods_runtime();
                    // Garbage-collect abandoned editor workspaces. The manager's
                    // lock serializes it against any allocate a handler starts.
                    commands::dev::sweep_abandoned_workspaces(&background_ctx);
                    // The profile watcher (update-availability / ratings refresh)
                    // and the background catalog refresh.
                    pump::profile_watch::spawn(background_ctx.clone());
                    pump::startup::kick(&background_ctx);
                })
                .expect("spawn the background startup thread");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            wh_ipc,
            wh_log_backlog,
            wh_log_stop_capture,
            broker::wh_broker_state,
            broker::wh_broker_retry,
            splash::wh_splash_ready,
            splash::wh_splash_presented
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Windhawk UI");
}

/// The stored UI theme setting, read from `getAppSettings` once at startup to seed the
/// native shell before the window opens. A read failure is the dark default (as is any
/// unrecognized value, per `ThemeSetting::parse`), matching the core's stored default and
/// the front-end.
fn startup_theme_setting(session: &dyn SessionApi) -> ThemeSetting {
    match session.invoke_as::<AppSettings, _>("getAppSettings", &json!({})) {
        Ok(settings) => ThemeSetting::parse(&settings.theme),
        Err(_) => ThemeSetting::Dark,
    }
}

/// Present a fatal failure as a native modal box, then terminate. Used where
/// there is no webview to render a reply into - which is every case here, since
/// what has failed is the window itself. `lead` is the sentence the box opens
/// with; `detail` is the explanation that follows it. Whatever the collectors
/// hold goes behind the box's expander, so the message stays a message and the
/// codes are still there to be read out.
fn fail_fatal(lead: &str, detail: &str) -> ! {
    // Stand the startup watchdog down first: it only sees "no window yet", which a
    // fatal failure also produces, and this path owns the message and the exit.
    window::suppress_startup_watchdog();
    // Nothing here is going to own a consent dialog, so make sure the ladder does
    // not raise one alongside the box below. Ignored where the launch already
    // asked for a prompt (a window that came up and was then lost): by then the
    // dialog may be on screen, and it is not this path's to withdraw.
    broker::elevation_prompt_gate().abandon();
    window::show_fatal(lead, detail, diagnostics::diagnostic_lines().as_deref());
    std::process::exit(1);
}

/// Present a fatal startup failure. Used in the setup hook, before any webview
/// exists: returning Err from setup would surface only as Tauri's opaque
/// exit-101 panic.
fn fail_startup(detail: &str) -> ! {
    fail_fatal("Windhawk could not start.", detail)
}

/// Present the loss of a window nobody asked to close, and end the process.
///
/// The exit is part of the report. A window destroyed behind Tauri's back leaves
/// the loop with nothing to run and no reason to stop (the runtime asks it to
/// exit only for a window it still knows), so without this the UI would linger
/// as a windowless process - alive enough to swallow every relaunch through the
/// single-instance hand-off, and invisible enough that nobody would know to end
/// it.
fn fail_unexpected_close(detail: &str) -> ! {
    fail_fatal("Windhawk has closed unexpectedly.", detail)
}

/// The UI data folder for this install, created if it is not there, or a fatal
/// startup failure.
///
/// Creating it is ordinary unprivileged work: the folder is this user's own
/// (`lifecycle::ui_data`), so there is no ladder to wait for and no consent
/// dialog on the way to the window. What is left is a filesystem that will not
/// take it, which is fatal because WebView2 is handed the folder as the window is
/// built and would fail over it with an HRESULT for a message.
fn ensure_ui_data_dir(app_data: &std::path::Path, portable: bool) -> std::path::PathBuf {
    let Some(ui_data_dir) = lifecycle::ui_data::ui_data_dir(app_data, portable) else {
        fail_startup(
            "Windhawk could not work out where to keep its window data: the \
             LOCALAPPDATA environment variable is not set.",
        );
    };
    if let Err(error) = std::fs::create_dir_all(&ui_data_dir) {
        fail_startup(&data_dir_failure_detail(&ui_data_dir, &error.to_string()));
    }
    ui_data_dir
}

/// The diagnostic detail for a UI-data-folder failure: the folder and what went
/// wrong.
///
/// The remedy is the same whatever the error, because the folder is one this user
/// can already write: elevation buys nothing, so what is left to say is that
/// something on the machine is stopping it.
fn data_dir_failure_detail(ui_data_dir: &std::path::Path, error: &str) -> String {
    format!(
        "Windhawk could not create its data folder:\n{}\n\n{error}\n\nMake sure \
         this folder can be created and written to.",
        ui_data_dir.display()
    )
}

/// What to set the main window's `additional_browser_args` to.
///
/// `extra` is what [`WEBVIEW_BROWSER_ARGS_VAR`] holds, appended to
/// [`DEFAULT_WEBVIEW_BROWSER_ARGS`] so that asking for one switch does not
/// silently drop the arguments the window is normally built with. A variable set
/// to nothing reads as unset: an empty command line is not what an empty value
/// asks for.
///
/// The value goes through verbatim, and it is the browser's command line, so a
/// launch made with it is a launch configured by whoever set the variable.
fn webview_browser_args(extra: Option<&str>) -> String {
    match extra.map(str::trim).filter(|extra| !extra.is_empty()) {
        Some(extra) => format!("{DEFAULT_WEBVIEW_BROWSER_ARGS} {extra}"),
        None => DEFAULT_WEBVIEW_BROWSER_ARGS.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::path::Path;

    // The folder is one this user can already write, so a failure to create it is
    // about the machine and never about rights: the detail names the folder,
    // echoes the OS error, and must not send anyone off to run as administrator,
    // which would change nothing.
    #[test]
    fn a_data_folder_failure_names_the_folder_and_asks_for_nothing_privileged() {
        let error = Error::from(ErrorKind::PermissionDenied);
        let detail = data_dir_failure_detail(
            Path::new(r"C:\Users\test\AppData\Local\Windhawk\UIMainData"),
            &error.to_string(),
        );

        assert!(detail.contains(r"C:\Users\test\AppData\Local\Windhawk\UIMainData"));
        assert!(detail.contains(&error.to_string()));
        assert!(detail.contains("Make sure this folder"));
        assert!(!detail.contains("administrator"));
    }

    // The ordinary launch: nothing to add, so the window is built with the
    // defaults alone. This is every launch that is not being observed, which is
    // what makes it the case the networking switches have to reach.
    #[test]
    fn an_unset_variable_gives_the_defaults() {
        assert_eq!(webview_browser_args(None), DEFAULT_WEBVIEW_BROWSER_ARGS);
    }

    // A variable set to nothing (or to spaces) is the same as unset: it must not
    // hand the builder an empty command line, which would take the defaults off
    // the window and replace them with none.
    #[test]
    fn an_empty_value_reads_as_unset() {
        assert_eq!(webview_browser_args(Some("")), DEFAULT_WEBVIEW_BROWSER_ARGS);
        assert_eq!(
            webview_browser_args(Some("   ")),
            DEFAULT_WEBVIEW_BROWSER_ARGS
        );
    }

    // What is asked for is added to the defaults rather than put in their place:
    // setting the variable is a request for one more switch, not a request to
    // build the window differently in every other respect.
    #[test]
    fn a_value_is_added_to_the_defaults() {
        let args = webview_browser_args(Some("--enable-logging --v=1"));

        assert!(args.starts_with(DEFAULT_WEBVIEW_BROWSER_ARGS));
        assert!(args.ends_with("--enable-logging --v=1"));
        assert!(args.contains("msSmartScreenProtection"));
        assert!(args.contains("--autoplay-policy=no-user-gesture-required"));
        // The defaults and what follows them stay separate switches.
        assert!(args.contains(&format!("{DEFAULT_WEBVIEW_BROWSER_ARGS} --enable-logging")));
    }

    // WebView2's background networking is off for every launch, observed or not:
    // the switches belong to the defaults, so an observed launch that adds
    // arguments of its own carries them too. Losing them silently would put the
    // experiment fetch and the component updater back on the window, writing into
    // a data folder that on a system install sits under %ProgramData%.
    #[test]
    fn the_defaults_keep_webview2_off_the_network() {
        for args in [
            webview_browser_args(None),
            webview_browser_args(Some("--enable-logging --v=1")),
        ] {
            assert!(args.contains("--disable-background-networking"));
            assert!(args.contains("--disable-component-update"));
        }
    }

    /// The wry release whose defaults the mirrored half of
    /// [`DEFAULT_WEBVIEW_BROWSER_ARGS`] was copied from. Moving it is a claim that
    /// the copy was read against that release, so bump it only with the re-read.
    const REVIEWED_WRY_VERSION: &str = "0.55.1";

    /// The `wry` version the workspace lockfile resolves to, or `None` if the
    /// lockfile has no such package.
    ///
    /// `Cargo.lock` is generated, so its `[[package]]` tables always spell the
    /// name and the version out on their own lines and finding one next to the
    /// other needs no TOML parser. A name inside a `dependencies` list is a bare
    /// `"wry",` and does not match the `name = ` line this looks for.
    fn locked_wry_version(lockfile: &str) -> Option<&str> {
        let mut lines = lockfile
            .lines()
            .map(str::trim)
            .skip_while(|line| *line != r#"name = "wry""#);
        // The name line itself, past which the version of that same package sits.
        lines.next()?;
        lines
            .take_while(|line| !line.starts_with("[["))
            .find_map(|line| line.strip_prefix(r#"version = ""#))?
            .strip_suffix('"')
    }

    // wry builds its default browser command line inline, as a local in the
    // function that creates the WebView2 environment, and neither wry nor Tauri
    // exposes it; a builder that sets arguments replaces it. So this window can
    // only carry a hand-copy, and a wry upgrade that changes the defaults would
    // leave the copy stale with nothing to say so - the window would keep building
    // with arguments that no longer match what the toolkit ships. Pin the version
    // the copy was read from, so the upgrade is what fails, at the fast gate,
    // rather than the window quietly diverging.
    #[test]
    fn the_mirrored_wry_defaults_are_pinned_to_a_reviewed_version() {
        let resolved = locked_wry_version(include_str!("../../Cargo.lock"))
            .expect("the workspace lockfile should resolve a wry package");

        assert_eq!(
            resolved, REVIEWED_WRY_VERSION,
            "wry moved from {REVIEWED_WRY_VERSION} to {resolved}. Re-read the \
             default browser arguments it builds in src/webview2/mod.rs \
             (create_environment), bring DEFAULT_WEBVIEW_BROWSER_ARGS back in \
             line with them, and then move REVIEWED_WRY_VERSION."
        );
    }

    // The pin is only worth having if it reads the right package: the lockfile is
    // alphabetical, so a version picked up by position rather than by package
    // would be some neighbour's, and would go on matching across the very upgrade
    // this is meant to catch.
    #[test]
    fn the_pin_reads_the_version_of_the_wry_package() {
        let lockfile = r#"
[[package]]
name = "wrapcenum-derive"
version = "0.4.1"

[[package]]
name = "wry"
version = "1.2.3"
dependencies = [
 "windows",
]

[[package]]
name = "x11-dl"
version = "2.21.0"
"#;

        assert_eq!(locked_wry_version(lockfile), Some("1.2.3"));
        assert_eq!(locked_wry_version("[[package]]\nname = \"tauri\"\n"), None);
    }
}
