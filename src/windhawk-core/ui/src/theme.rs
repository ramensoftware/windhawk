//! The native theme state: the current theme setting plus the seam that re-applies the
//! native window surfaces (the title bar and border via DWM, WebView2's own surfaces via
//! the profile color scheme, and the injected `--wh-*` tokens the log pane and scrollbar
//! read) when the theme setting changes at runtime or, under `Auto`, when the OS switches
//! light/dark. The `theme_init_script` / `theme_background_color` first-frame pieces are
//! applied once at window creation (`run`); this seam re-pushes the surfaces that outlive
//! the first paint.
//!
//! [`NativeThemeControl`] is the injected trait the `updateAppSettings` handler reaches
//! through [`BridgeCtx`](crate::BridgeCtx); [`AppThemeControl`] is the production
//! implementation (it owns the [`AppHandle`] and the current setting, which the focus and
//! OS-theme-change handlers also read), and [`NoopThemeControl`] backs the headless tests,
//! which have no window.

use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{AppHandle, Manager};

use crate::shell::{self, ThemeSetting};

/// The main window's Tauri label, matching `WebviewWindowBuilder::new(app, "main", ...)`
/// and the `get_webview_window("main")` lookups in `run`.
const MAIN_WINDOW_LABEL: &str = "main";

/// Apply a new theme setting to the native window surfaces. Injected into
/// [`BridgeCtx`](crate::BridgeCtx) so the `updateAppSettings` handler can push a theme
/// change to the window without reaching an `AppHandle` ad hoc; the headless tests use
/// [`NoopThemeControl`]. `Send + Sync` so the context that holds it can cross to the
/// `wh_ipc` worker thread.
pub trait NativeThemeControl: Send + Sync {
    /// Store the new theme setting (`"dark"`/`"light"`/`"auto"`; anything else is the dark
    /// default) and re-theme the native window frame, WebView2 surfaces, and injected
    /// tokens to match. Best effort - a missing window is a no-op.
    fn set_theme(&self, setting: &str);
}

fn encode(setting: ThemeSetting) -> u8 {
    match setting {
        ThemeSetting::Dark => 0,
        ThemeSetting::Light => 1,
        ThemeSetting::Auto => 2,
    }
}

fn decode(value: u8) -> ThemeSetting {
    match value {
        1 => ThemeSetting::Light,
        2 => ThemeSetting::Auto,
        _ => ThemeSetting::Dark,
    }
}

/// The production control: owns the app handle and the current theme setting. The setting
/// is shared with the focus handler (which re-pushes the DWM frame colors on each focus
/// transition) and the OS-theme-change handler, so it lives behind an atomic.
pub struct AppThemeControl {
    app: AppHandle,
    setting: AtomicU8,
}

impl AppThemeControl {
    /// Seed the control with the setting read at startup.
    pub fn new(app: AppHandle, setting: ThemeSetting) -> AppThemeControl {
        AppThemeControl {
            app,
            setting: AtomicU8::new(encode(setting)),
        }
    }

    /// The current theme setting.
    pub fn current_setting(&self) -> ThemeSetting {
        decode(self.setting.load(Ordering::Relaxed))
    }

    /// Re-push the frame colors for a focus transition using the current (resolved) theme.
    /// DWM keeps one set of frame colors regardless of focus, so this runs on each
    /// `Focused` event.
    pub fn apply_focus(&self, active: bool) {
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW_LABEL) {
            shell::apply_frame_focus(&window, active, self.current_setting().resolved_dark());
        }
    }

    /// Re-apply the resolved theme after the OS switched light/dark. Only `Auto` reaches
    /// here: an explicit theme pins the window theme (see [`shell::window_theme`]), which
    /// makes tao suppress `WindowEvent::ThemeChanged` on an OS switch, so this is not called
    /// for it. Under `Auto` the webview content follows via the front-end's matchMedia
    /// listener and WebView2's surfaces via tauri-runtime-wry's own `ThemeChanged` handler;
    /// this covers the native frame and the injected `--wh-*` tokens (log pane, scrollbar).
    pub fn reapply_for_os_change(&self) {
        if self.current_setting() != ThemeSetting::Auto {
            return;
        }
        let dark = ThemeSetting::Auto.resolved_dark();
        if let Some(window) = self.app.get_webview_window(MAIN_WINDOW_LABEL) {
            let active = window.is_focused().unwrap_or(true);
            shell::apply_frame_focus(&window, active, dark);
            let _ = window.eval(shell::theme_tokens_update_script(dark));
        }
    }
}

impl NativeThemeControl for AppThemeControl {
    fn set_theme(&self, setting: &str) {
        let setting = ThemeSetting::parse(setting);
        self.setting.store(encode(setting), Ordering::Relaxed);
        // Hop to the main thread: this runs on a `wh_ipc` worker, but the WebView2 calls
        // reach STA/UI-thread-affine COM objects (the DWM frame and the eval are moved
        // along for uniformity). Best effort - a failure (the event loop is gone at
        // shutdown) just leaves the frame until the next focus/OS-change/startup apply.
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let dark = setting.resolved_dark();
                let active = window.is_focused().unwrap_or(true);
                // Pin the window theme first so an explicit setting suppresses future OS
                // `ThemeChanged` events (and tauri-runtime-wry's override with them); the
                // color-scheme apply below is then the final word on WebView2's surfaces.
                let _ = window.set_theme(shell::window_theme(setting));
                shell::apply_frame_focus(&window, active, dark);
                shell::apply_webview_color_scheme(&window, setting);
                let _ = window.eval(shell::theme_tokens_update_script(dark));
            }
        });
    }
}

/// The no-op control for the headless tests (no `AppHandle`, no window).
pub struct NoopThemeControl;

impl NativeThemeControl for NoopThemeControl {
    fn set_theme(&self, _setting: &str) {}
}
