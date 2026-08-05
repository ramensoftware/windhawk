//! The environment shim: the small, backend-free glue that frees the shared
//! React front-end from its VSCode-webview assumptions. Two pieces live here:
//!
//! - **Theme.** The shared front-end themes itself; this shim only colors the pieces
//!   we inject. [`theme_init_script`] returns a tiny initialization script that sets
//!   `color-scheme` and a small set of `--wh-*` design tokens on `:root` - the colors
//!   the injected log pane, custom scrollbar and broker banner consume (each carries
//!   hard-coded fallbacks covering the first paint).
//!   [`theme_background_color`] gives the matching window/webview background, which
//!   [`apply_background_color`] pushes to both layers so every pixel the host paints
//!   outside the document - the frame before the first document paint, the band a
//!   resize exposes - is the themed color rather than a white flash.
//!   [`apply_frame_theme`] pushes the same palette to the
//!   native window frame via DWM, so the title bar and border match the content
//!   instead of staying a stock-light strip around a dark webview.
//!   [`apply_webview_color_scheme`] sets the WebView2 profile's preferred color scheme
//!   so WebView2's own surfaces (context menus, dialogs) match the content too, rather
//!   than following the OS.
//! - **Scrollbars.** [`scrollbar_init_script`] injects `scrollbar.js`, a custom
//!   overlay scrollbar that replaces WebView2's Edge Fluent bar with the flat VSCode
//!   look, themed from the slider variables the theme script sets. It stands down in
//!   Windows high contrast mode, where the native bar follows the system palette and a
//!   token-themed thumb would not.
//! - **Shortcuts.** A WebView2 window inherits Edge's browser hotkeys.
//!   [`disable_browser_shortcuts`] marks the ones with no place in this app handled on
//!   the controller's accelerator-key event - show downloads (Ctrl+J), print (Ctrl+P),
//!   reload and hard reload (F5, Ctrl+R, Ctrl+Shift+R, Ctrl+F5), and the caret-browsing
//!   toggle (F7) - which cancels WebView2's default action while leaving find, the zoom
//!   keys, and the clipboard keys alone. WebView2 has no per-key switch, and its
//!   all-or-nothing
//!   `AreBrowserAcceleratorKeysEnabled` would also drop the keys we keep.
//! - **Zoom.** The content zoom factor is a WebView2 controller property rather than a
//!   window one, so [`apply_and_track_zoom`] restores the remembered level here and
//!   subscribes to the controller's `ZoomFactorChanged` event, feeding each change back
//!   to the window-state tracker that persists it. It also takes Ctrl+0 over on the
//!   accelerator-key event, since WebView2 would send it to the restored level instead
//!   of to 100%.
//! - **Context menu.** A WebView2 window also inherits Edge's full right-click menu.
//!   [`customize_context_menu`] trims it on the controller's `ContextMenuRequested`
//!   event to just the items this app wants - navigation (back/forward) and the input
//!   context menu (cut/copy/paste/paste-as-plain-text/undo/redo/select all) - dropping
//!   the rest (reload, save as, print, share, web select, emoji, inspect, ...) and the
//!   separators those removals leave dangling. The same keep-list governs both the
//!   page menu (where only back/forward survive) and the editable-field menu (where the
//!   clipboard items survive).
//! - **External links.** Donate/GitHub/homepage links are `<a target="_blank">`, and a mod
//!   README's markdown links are plain same-window anchors; in a webview both must go to
//!   the system rather than load in the app. WebView2 splits the two ways one can arrive,
//!   so both hooks are wired: [`handle_navigation`] takes a same-window top-level
//!   navigation, and [`handle_new_window`] takes the `target="_blank"` / `window.open`
//!   case, which WebView2 raises as a new-window request and never as a navigation. Either
//!   way [`is_external`] decides - a real (non-`localhost`) `http(s)` host or a `mailto:`
//!   address - and the target is handed to the Tauri opener plugin; the in-webview
//!   navigation is cancelled, and the second webview is denied.
//!
//! The `data-content` panel attribute the extension sets is NOT injected here: its
//! value is front-end-internal, so the `tauri` build mode (the sibling front-end
//! changeset) sets it, the same place it selects the transport. Injecting a guessed
//! value from Rust could mis-route the panel.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tauri::webview::{Color, NewWindowResponse};
use tauri::{AppHandle, Url, WebviewWindow, Wry};
use tauri_plugin_opener::OpenerExt;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
    COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
    ICoreWebView2, ICoreWebView2_11, ICoreWebView2_13, ICoreWebView2AcceleratorKeyPressedEventArgs,
    ICoreWebView2ContextMenuItem, ICoreWebView2ContextMenuRequestedEventArgs,
    ICoreWebView2Controller,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, ContextMenuRequestedEventHandler,
    ZoomFactorChangedEventHandler, take_pwstr,
};
use windows_core::{IUnknown, Interface, PWSTR};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::Controls::LoadIconWithScaleDown;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, SM_CYICON,
    SM_CYSMICON, SendMessageW, USER_DEFAULT_SCREEN_DPI, WM_NCACTIVATE, WM_SETICON,
};

/// The appearance the front-end and injected shell pieces are themed to match.
struct Theme {
    /// Whether the dark palette is in effect.
    dark: bool,
}

impl Theme {
    /// `(foreground, background, border)` for the theme.
    fn palette(&self) -> (&'static str, &'static str, &'static str) {
        if self.dark {
            ("#cccccc", "#1e1e1e", "#454545")
        } else {
            ("#1f1f1f", "#ffffff", "#cecece")
        }
    }

    /// The DWM frame colors `(caption background, caption text, border)` as
    /// `COLORREF`s for the given focus state. The caption and border share one frame
    /// color - the elevated surface gray in dark, the window border gray in light - so
    /// the title bar reads as part of the window edge rather than a filled bar in the
    /// editor background.
    ///
    /// Active uses the foreground over the active frame gray. Inactive matches VSCode's
    /// unfocused title bar: a dimmer frame - receded toward the editor background in both
    /// themes (darker in dark, lighter in light) - and a dimmed caption text. DWM keeps a
    /// single set of frame colors regardless of focus, so the caller re-pushes these on
    /// each focus transition. The inactive text is VSCode's `titleBar.inactiveForeground`,
    /// the foreground at 60% alpha, flattened over the inactive frame, since
    /// `DWMWA_TEXT_COLOR` is an opaque `COLORREF`: dark `rgba(204,204,204,0.6)` over
    /// `#323233` -> `#8e8e8f`, light `rgba(31,31,31,0.6)` over `#dedede` -> `#6b6b6b`.
    fn frame_colors(&self, active: bool) -> (COLORREF, COLORREF, COLORREF) {
        if active {
            let (fg, ..) = self.palette();
            let frame = colorref(if self.dark { "#3c3c3c" } else { "#cecece" });
            (frame, colorref(fg), frame)
        } else {
            let (frame, text) = if self.dark {
                ("#323233", "#8e8e8f")
            } else {
                ("#dedede", "#6b6b6b")
            };
            (colorref(frame), colorref(text), colorref(frame))
        }
    }

    /// The warning surface `(background, icon)` for the theme: antd's warning Alert
    /// in the front-end's own two themes. The injected banner paints itself with
    /// these, so a shell notice and the front-end's safe-mode banner are the same
    /// yellow rather than two nearly-alike ones.
    fn warning_colors(&self) -> (&'static str, &'static str) {
        if self.dark {
            ("#2b2111", "#d89614")
        } else {
            ("#fffbe6", "#faad14")
        }
    }

    /// The custom scrollbar's thumb colors `(background, hover, active)` for the
    /// theme, as `rgba()` CSS literals. The values are VSCode's editor scrollbar-slider
    /// defaults; the theme script publishes them as the `--wh-cscroll-*` tokens that
    /// `scrollbar.js` consumes, so the overlay scrollbar reads as part of the editor.
    fn scrollbar_slider_colors(&self) -> (&'static str, &'static str, &'static str) {
        if self.dark {
            (
                "rgba(121, 121, 121, 0.4)",
                "rgba(100, 100, 100, 0.7)",
                "rgba(191, 191, 191, 0.4)",
            )
        } else {
            (
                "rgba(100, 100, 100, 0.4)",
                "rgba(100, 100, 100, 0.7)",
                "rgba(0, 0, 0, 0.6)",
            )
        }
    }
}

/// The theme SETTING as chosen by the user: an explicit theme, or `Auto` to follow the OS
/// light/dark preference. It resolves to a concrete `dark` flag at the point of rendering
/// the native frame, background, and injected tokens; the WebView2 color scheme is the
/// one surface that consumes `Auto` directly (WebView2 has a matching auto scheme that
/// follows the OS, so its context menus and the webview's `prefers-color-scheme` track the
/// system without a resolve here).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeSetting {
    Dark,
    Light,
    Auto,
}

impl ThemeSetting {
    /// Parse the stored setting string. Anything other than `"light"`/`"auto"` - including
    /// `"dark"` and any unrecognized value - is the dark default.
    pub fn parse(value: &str) -> ThemeSetting {
        match value {
            "light" => ThemeSetting::Light,
            "auto" => ThemeSetting::Auto,
            _ => ThemeSetting::Dark,
        }
    }

    /// The stored spelling, which is what crosses the runtime-broker channel when
    /// the editor theme is synced by the elevated helper: the setting travels as
    /// the string the core stores rather than as a second encoding of it.
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeSetting::Dark => "dark",
            ThemeSetting::Light => "light",
            ThemeSetting::Auto => "auto",
        }
    }

    /// Resolve to a concrete dark flag, reading the OS preference for `Auto`.
    pub fn resolved_dark(self) -> bool {
        match self {
            ThemeSetting::Dark => true,
            ThemeSetting::Light => false,
            ThemeSetting::Auto => os_theme_is_dark(),
        }
    }
}

/// Whether the OS currently prefers dark app windows, read from the Personalize registry
/// key. Resolves the `Auto` setting where a concrete theme is needed (the native frame,
/// the first-frame background, the injected shell tokens). A missing or unreadable value
/// is treated as dark - the app's default.
fn os_theme_is_dark() -> bool {
    // HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize : AppsUseLightTheme
    // is a REG_DWORD, 1 = light apps, 0 = dark apps.
    let sub_key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value_name = wide("AppsUseLightTheme");
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `sub_key`/`value_name` are valid null-terminated UTF-16 buffers; `data`/
    // `size` point to a u32 and its byte length. RRF_RT_REG_DWORD restricts the read to a
    // 4-byte DWORD, so RegGetValueW writes at most `size` bytes through `data` and returns
    // a nonzero error rather than overrunning; on any error we fall through to the dark
    // default below. The type-out pointer is null (the DWORD restriction fixes the type).
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::from_mut(&mut data).cast(),
            &mut size,
        )
    };
    // Dark unless the read succeeded AND reported light apps (1).
    !(status == 0 && data == 1)
}

/// A `&str` as a null-terminated UTF-16 buffer for the wide Win32 registry calls.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The initialization script that themes the injected shell pieces. Emits a script for
/// the given theme (`dark`) that sets `color-scheme` and the `--wh-*` design tokens the
/// log pane and custom scrollbar consume on `:root`, and publishes the initial theme as
/// a global the front-end reads before its first paint (so a light-theme user does not
/// flash the default dark bundle while the settings load over IPC).
pub fn theme_init_script(dark: bool) -> String {
    theme_init_script_for(&Theme { dark })
}

/// The front-end's page background per theme (its `--whui-background-color`), as
/// `(red, green, blue)`. Every surface the user sees before the document paints
/// takes this one value - the window's own background, WebView2's default color,
/// and the startup splash the mark is drawn on - so nothing changes shade as the
/// app comes up.
pub const fn page_background(dark: bool) -> (u8, u8, u8) {
    if dark {
        (0x1e, 0x1e, 0x1e)
    } else {
        (0xf5, 0xf5, 0xf5)
    }
}

/// The background color for the window and webview in the given theme. Set on the window
/// at build time so the first frame matches the theme instead of flashing white while
/// the webview attaches and the document paints (the init script only governs the
/// document, which paints after that frame), and re-applied by
/// [`apply_background_color`] whenever the theme changes at runtime. Alpha is full: on
/// Windows the window layer ignores alpha and the webview layer ignores a non-zero
/// alpha, so opaque is the only meaningful value.
pub fn theme_background_color(dark: bool) -> Color {
    let (red, green, blue) = page_background(dark);
    Color(red, green, blue, 255)
}

/// Re-point the window's and the webview's background at the given theme's page
/// background. Both layers hold a color of their own that outlives the first frame - the
/// window's background fill and WebView2's default background - and each shows through
/// wherever the host paints before the document does, most visibly in the band a resize
/// exposes. Left at the build-time color they would keep the startup theme's shade after
/// a switch, so this pushes the current one to both.
///
/// Best effort, like the other surface applies here: a failure leaves the previous color.
pub fn apply_background_color(window: &WebviewWindow, dark: bool) {
    let _ = window.set_background_color(Some(theme_background_color(dark)));
}

/// Theme the native window frame (title bar + border) to match the content theme.
/// Pushes the matching caption, text, and border colors plus the immersive dark-mode
/// flag to DWM, so the frame is not a stock-light strip around a dark webview. Called at
/// startup and again whenever the theme setting changes at runtime.
///
/// Best effort: the per-color attributes need Windows 11 (build 22000+) and the
/// call simply fails on older systems, where the dark-mode flag (Windows 10 2004+)
/// still flips the caption and its buttons between light and dark.
pub fn apply_frame_theme(window: &WebviewWindow, dark: bool) {
    // A missing handle just leaves the frame at the OS default; nothing else depends
    // on this. The Tauri `HWND` wraps the same `*mut c_void` as the windows-sys one.
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // The window is on screen by now (it is built visible, carrying the splash) and
    // has asked for the foreground, so whether it is the active one is Windows' answer
    // to give rather than an assumption to make here: a launch the user made was
    // granted it, one made in the background was not. [`track_activation`] keeps it
    // right from there.
    let active = is_foreground_hwnd(hwnd.0);
    apply_frame_theme_to(hwnd.0, &Theme { dark }, active);
}

/// Re-push the frame colors for the given activation state to the main window. DWM
/// keeps a single set of frame colors regardless of activation, so the dimmed inactive
/// look (and the restore on reactivation) only happens if the colors are re-pushed on
/// each transition ([`track_activation`] reports them). Same best-effort and handle
/// handling as [`apply_frame_theme`]; `dark` is the current theme.
pub fn apply_frame_focus(window: &WebviewWindow, active: bool, dark: bool) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    apply_frame_theme_to(hwnd.0, &Theme { dark }, active);
}

/// Theme a window that is not a Tauri window - the startup splash, which opens
/// before the main window exists - so its title bar and border match the frame
/// the main window will carry.
pub fn apply_frame_theme_to_hwnd(hwnd: HWND, dark: bool, active: bool) {
    apply_frame_theme_to(hwnd, &Theme { dark }, active);
}

/// Whether the window is the active (foreground) one - the state its frame colors
/// follow. Read wherever the frame is re-pushed outside an activation change (a
/// theme switch, an OS light/dark switch), since the window's own focus flag says
/// something else: the webview holds the keyboard focus, so the window reads as
/// unfocused while being the very window the user is working in.
pub fn is_active(window: &WebviewWindow) -> bool {
    window.hwnd().is_ok_and(|hwnd| is_foreground_hwnd(hwnd.0))
}

/// [`is_active`] for a raw handle, for the startup splash - it colors the frame
/// before Tauri hands the window over, and from there [`track_activation`] reports
/// every change.
pub fn is_foreground_hwnd(hwnd: HWND) -> bool {
    // SAFETY: GetForegroundWindow takes no arguments and only reads the current
    // foreground window; the handles are compared, not dereferenced.
    unsafe { GetForegroundWindow() == hwnd }
}

/// Report the window's activation changes, so the caller can re-push the frame
/// colors DWM keeps for both states (see [`apply_frame_focus`]).
///
/// This watches `WM_NCACTIVATE` - the message that tells a window to draw its
/// non-client area active or inactive - rather than Tauri's `Focused` event, which
/// carries KEYBOARD focus. The two part company as soon as the webview takes the
/// focus off the window, which it does at startup and keeps: from that point tao
/// considers the window unfocused, and since it only reports a change of
/// "active AND focused", no further focus event is ever raised - switching away
/// from the app and back would leave the frame stuck as it was.
///
/// The subclass is added after Tauri's own, so it sees the message first and then
/// passes it on unchanged. It lives for the window's lifetime (single window, open
/// until exit), which is what makes leaking the boxed callback sound. Best effort:
/// a window without a handle, or a subclass the system declines, just leaves the
/// frame at the colors last pushed.
pub fn track_activation<F>(window: &WebviewWindow, on_change: F)
where
    F: Fn(bool) + 'static,
{
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let callback: Box<dyn Fn(bool)> = Box::new(on_change);
    let callback = Box::into_raw(Box::new(callback));
    // SAFETY: `hwnd` is the live main window and this runs on the thread that owns
    // it (the setup hook), as subclassing requires. `callback` is a live pointer to
    // a leaked box the subclass procedure only ever borrows, and it outlives the
    // window. A failure leaves the box leaked and nothing subclassed, which is
    // harmless.
    unsafe {
        SetWindowSubclass(
            hwnd.0,
            Some(activation_proc),
            ACTIVATION_SUBCLASS_ID,
            callback as usize,
        );
    }
}

/// The subclass id for [`track_activation`], distinguishing our subclass from
/// tao's on the same window.
const ACTIVATION_SUBCLASS_ID: usize = 1;

/// The [`track_activation`] subclass: report each activation change, then let the
/// message take its normal course (tao's own handling included).
unsafe extern "system" fn activation_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    reference: usize,
) -> LRESULT {
    if message == WM_NCACTIVATE {
        // SAFETY: `reference` is the pointer `track_activation` leaked for this
        // window, which outlives it; the box is only borrowed here. `wparam` is the
        // active flag the message carries.
        let callback = unsafe { &*(reference as *const Box<dyn Fn(bool)>) };
        callback(wparam != 0);
    }
    // SAFETY: the arguments are the ones the subclass procedure was handed, passed
    // on to the rest of the chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Push a theme's frame colors and dark-mode flag to a window handle via DWM, using the
/// active or inactive frame colors per `active`.
fn apply_frame_theme_to(hwnd: HWND, theme: &Theme, active: bool) {
    let (caption, text, border) = theme.frame_colors(active);
    let dark = i32::from(theme.dark);

    // SAFETY: `hwnd` is the live main-window handle from Tauri. Each attribute is
    // paired with a pointer to a value of the size DWM expects (a 4-byte BOOL for the
    // dark-mode flag, a 4-byte COLORREF for each color); DwmSetWindowAttribute copies
    // `cbattribute` bytes and returns an error rather than overrunning on a mismatch.
    // Results are ignored: the color attributes are unsupported before Windows 11 and
    // fail harmlessly there.
    unsafe {
        set_dwm_attribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark);
        set_dwm_attribute(hwnd, DWMWA_CAPTION_COLOR, &caption);
        set_dwm_attribute(hwnd, DWMWA_TEXT_COLOR, &text);
        set_dwm_attribute(hwnd, DWMWA_BORDER_COLOR, &border);
    }
}

/// Set one DWM window attribute from a borrowed value, ignoring the result.
///
/// SAFETY: the caller must pass a valid `hwnd` and a `value` whose type matches the
/// size and layout DWM expects for `attribute`.
unsafe fn set_dwm_attribute<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: &T) {
    // SAFETY: forwarded from this fn's contract; `value` points to one `T`, and
    // `size_of::<T>()` is the exact byte count handed to DWM.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute as u32,
            std::ptr::from_ref(value).cast(),
            std::mem::size_of::<T>() as u32,
        );
    }
}

/// Give the main window crisp icons sized for its current DPI.
///
/// tao sets the window's small (title-bar) icon from a single RGBA image Tauri
/// decodes from the FIRST entry of `icon.ico` - the 256x256 one - so Windows squeezes
/// a 256px bitmap into the ~16px caption slot and the title-bar icon looks blurry.
/// The same `icon.ico` is also embedded in the executable as a multi-resolution icon
/// group (by `tauri-build`, resource id 32512) carrying 16/24/32/48/64/256 px images;
/// `LoadIconWithScaleDown` picks the entry nearest a requested size and scales it down
/// with high quality. Loading at the DPI's small- and large-icon metrics therefore
/// yields the native 16/24/32... rather than a downscaled 256, set as the window's
/// ICON_SMALL (caption, taskbar) and ICON_BIG (alt-tab).
///
/// The icons go on as the window is shown, from the creation hook
/// (`window::prepare_main_window_creation`), so the caption and the taskbar button
/// carry them from the window's first frame, and are loaded again whenever the window's
/// DPI changes ([`rescale_window_icons`]). This is the fallback behind the first of
/// those, for a launch whose hook was never installed: it runs after the build, where
/// the window has been on screen for a while and the correction is visible.
///
/// Best effort: a missing handle or icon resource just leaves tao's icon in place.
pub fn apply_window_icons(window: &WebviewWindow) {
    // A missing handle just leaves tao's oversized icon; nothing else depends on this.
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    apply_window_icons_to(hwnd.0);
}

/// Load the window's icons again for the scale factor it has just changed to, so a
/// window whose display scale changed under it - or that was moved to a display at
/// another scale - carries images drawn for that scale rather than the old pair
/// stretched to fit.
///
/// The scale factor is the one the change reported (`WindowEvent::ScaleFactorChanged`,
/// which tao raises from the DPI-change message and reads the new DPI out of), rather
/// than anything read back off a window that is in the middle of the change.
pub fn rescale_window_icons(window: &WebviewWindow, scale_factor: f64) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // A scale factor is the display's DPI over the 96-DPI baseline (tao divides one by
    // the other), so multiplying it back gives the DPI the icon metrics are asked for.
    // A factor that does not answer as one at all leaves the window to say.
    let dpi = (scale_factor * f64::from(USER_DEFAULT_SCREEN_DPI)).round();
    let dpi = if dpi >= 1.0 {
        dpi as u32
    } else {
        window_dpi(hwnd.0)
    };
    set_window_icons(hwnd.0, dpi);
}

/// The DPI the icons on the window were loaded for, and 0 while it carries none of
/// ours. What keeps a pair to one load per DPI: the creation hook and the fallback
/// behind it ask for the same one, and only a real change loads again.
static WINDOW_ICONS_DPI: AtomicU32 = AtomicU32::new(0);

/// The executable's icon-group resource id. `tauri-build` embeds `icon.ico` under this
/// name id (the default its `set_icon` uses), so the running module owns the full
/// multi-resolution group that `LoadIconWithScaleDown` selects a size from.
const ICON_RESOURCE_ID: u16 = 32512;

/// Set a window handle's small and big icons to the right native sizes for its DPI.
pub fn apply_window_icons_to(hwnd: HWND) {
    set_window_icons(hwnd, window_dpi(hwnd));
}

/// The DPI of the display a window is on, or the 96-DPI baseline for one that cannot be
/// resolved, so the metric lookups still give the standard sizes.
fn window_dpi(hwnd: HWND) -> u32 {
    // SAFETY: `hwnd` is a live window handle - Tauri's, or the one the creation hook is
    // reporting on; GetDpiForWindow only reads it, and answers 0 for a window it cannot
    // resolve.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        USER_DEFAULT_SCREEN_DPI
    } else {
        dpi
    }
}

/// Load the pair of icons `dpi` calls for and set them on the window - unless the pair
/// it already carries was loaded for that same DPI, which every caller but a real DPI
/// change is asking for.
///
/// The images a reload replaces are left where they are rather than destroyed. The
/// loader answers a repeat request for a size with the handle it answered before, so
/// one of them is liable to be an icon still in use - the window's other slot, at the
/// size these two swap through as the DPI moves - or one a later load will be handed
/// again. A session is left holding one pair per distinct DPI its window has been shown
/// at, and the loader charges nothing for the sizes it has already answered for.
fn set_window_icons(hwnd: HWND, dpi: u32) {
    if WINDOW_ICONS_DPI.swap(dpi, Ordering::AcqRel) == dpi {
        return;
    }

    // The caption/taskbar (small) and alt-tab (big) icon dimensions at this DPI.
    // SAFETY: each argument is a valid SYSTEM_METRICS_INDEX; GetSystemMetricsForDpi
    // reads no handle and the DPI is non-zero.
    let (small_cx, small_cy, big_cx, big_cy) = unsafe {
        (
            GetSystemMetricsForDpi(SM_CXSMICON, dpi),
            GetSystemMetricsForDpi(SM_CYSMICON, dpi),
            GetSystemMetricsForDpi(SM_CXICON, dpi),
            GetSystemMetricsForDpi(SM_CYICON, dpi),
        )
    };

    set_window_icon(hwnd, ICON_SMALL, small_cx, small_cy);
    set_window_icon(hwnd, ICON_BIG, big_cx, big_cy);
}

/// Load the `(cx, cy)`-sized image from the embedded icon group and set it as the
/// window's `which` (ICON_SMALL or ICON_BIG) icon. A failed load is skipped, leaving
/// that slot as it was.
fn set_window_icon(hwnd: HWND, which: u32, cx: i32, cy: i32) {
    let mut icon: HICON = std::ptr::null_mut();
    // SAFETY: a null module name resolves to the running executable, where tauri-build
    // embedded the icon group under ICON_RESOURCE_ID (passed as a MAKEINTRESOURCE
    // ordinal: an integer carried in the pointer's low word, not a real pointer).
    // LoadIconWithScaleDown writes the loaded handle through `icon` and returns a
    // failure HRESULT (leaving `icon` null) if the resource is missing; we check both
    // before using the handle.
    let hr = unsafe {
        LoadIconWithScaleDown(
            GetModuleHandleW(std::ptr::null()),
            ICON_RESOURCE_ID as usize as *const u16,
            cx,
            cy,
            &mut icon,
        )
    };
    if hr < 0 || icon.is_null() {
        return;
    }

    // SAFETY: `hwnd` is the live main window. WM_SETICON takes the icon-type tag
    // (ICON_SMALL/ICON_BIG) in wParam and the HICON in lParam; it neither copies nor
    // takes ownership of the handle, which stays valid for the process lifetime.
    unsafe {
        SendMessageW(hwnd, WM_SETICON, which as WPARAM, icon as LPARAM);
    }
}

/// A `#rrggbb` palette literal as a Win32 `COLORREF` (`0x00bbggrr`).
fn colorref(hex: &str) -> COLORREF {
    // Each literal is one of our own `#rrggbb` strings, so every component parses.
    let component =
        |i: usize| u32::from_str_radix(&hex[i..i + 2], 16).expect("palette color is #rrggbb");
    component(1) | (component(3) << 8) | (component(5) << 16)
}

/// The custom overlay scrollbar (`scrollbar.js`), returned to `run` (`lib.rs`), which
/// attaches it as a main-window initialization script alongside the theme shim. It
/// hides WebView2's Edge Fluent scrollbars and draws flat, themed overlay thumbs - the
/// VSCode look - reading the `--wh-cscroll-*` colors the theme script sets. In Windows
/// high contrast mode it leaves the native scrollbar alone, since that one follows the
/// system palette while the token-themed thumb would not.
/// Injected from Rust, so it runs only in the Tauri app; the shared front-end keeps its
/// host's scrollbars everywhere else.
pub fn scrollbar_init_script() -> &'static str {
    include_str!("scrollbar.js")
}

/// Suppress the WebView2 browser shortcuts that do not belong in this app window: show
/// downloads (Ctrl+J), print (Ctrl+P), reload and hard reload (F5, Ctrl+R, Ctrl+Shift+R,
/// Ctrl+F5), and the caret-browsing toggle (F7). Each unwanted key is marked handled on
/// the controller's
/// AcceleratorKeyPressed event, which cancels WebView2's default action; find (Ctrl+F),
/// the zoom keys, and the clipboard keys are left untouched. WebView2 has no per-key
/// switch, and its all-or-nothing `AreBrowserAcceleratorKeysEnabled` would also drop
/// those keys we keep. (Ctrl+0 stays a zoom key, but is served by
/// [`apply_and_track_zoom`] rather than by WebView2.)
///
/// Best effort: `with_webview` runs the closure once the platform webview is available,
/// and a failure to reach it just leaves the WebView2 defaults in place. The handler
/// lives for the window's lifetime (the app has a single window, open until exit).
pub fn disable_browser_shortcuts(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
        let controller = webview.controller();
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(
            move |_controller: Option<ICoreWebView2Controller>,
                  args: Option<ICoreWebView2AcceleratorKeyPressedEventArgs>| {
                let Some(args) = args else {
                    return Ok(());
                };
                // SAFETY: `args` is the live event argument WebView2 handed to this
                // callback; each accessor writes through the out-pointer we pass and
                // returns an error (propagated by `?`) rather than overrunning.
                unsafe {
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                    args.KeyEventKind(&mut kind)?;
                    let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
                    if down {
                        let mut virtual_key = 0u32;
                        args.VirtualKey(&mut virtual_key)?;
                        if is_suppressed_shortcut(virtual_key, ctrl_down()) {
                            args.SetHandled(true)?;
                        }
                    }
                }
                Ok(())
            },
        ));
        // The subscription lives for the window's lifetime, so the registration cookie
        // is intentionally discarded.
        let mut token = 0i64;
        // SAFETY: `controller` is the live WebView2 controller from Tauri, `handler` is
        // a valid event-handler COM object, and `token` is a stack slot the call writes
        // the cookie into. A failed registration returns an error rather than
        // misbehaving, and we ignore it (best effort).
        unsafe {
            let _ = controller.add_AcceleratorKeyPressed(&handler, &mut token);
        }
    });
}

/// Whether a key-down virtual key (with the current Ctrl state) is a browser shortcut
/// we suppress. F5 reloads (Ctrl+F5 hard-reloads) and F7 toggles caret browsing
/// regardless of modifiers; the letter combos - show downloads (Ctrl+J), print
/// (Ctrl+P), and reload (Ctrl+R, and Ctrl+Shift+R, the same virtual key) - require
/// Ctrl held, so a plain `j`/`p`/`r` keystroke is left to the page.
fn is_suppressed_shortcut(virtual_key: u32, ctrl_held: bool) -> bool {
    const VK_J: u32 = 0x4A;
    const VK_P: u32 = 0x50;
    const VK_R: u32 = 0x52;
    const VK_F5: u32 = 0x74;
    const VK_F7: u32 = 0x76;
    matches!(virtual_key, VK_F5 | VK_F7) || (ctrl_held && matches!(virtual_key, VK_J | VK_P | VK_R))
}

/// Whether Ctrl is held for the key event being processed.
fn ctrl_down() -> bool {
    // SAFETY: GetKeyState has no preconditions; it reads the virtual-key state for the
    // message under processing and returns a plain SHORT whose high bit is set while
    // the key is down.
    let state = unsafe { GetKeyState(VK_CONTROL as i32) };
    (state as u16 & 0x8000) != 0
}

/// Trim the WebView2 context menu to the items this app keeps. On the
/// controller's `ContextMenuRequested` event the handler removes every default
/// item that is not navigation (back/forward) or a text-editing/clipboard
/// command (the input context menu), then drops the separators those removals
/// leave leading, trailing, or doubled. The page menu collapses to just
/// back/forward; the editable-field menu keeps its
/// cut/copy/paste/undo/redo/select-all.
///
/// Best effort, mirroring [`disable_browser_shortcuts`]: `with_webview` runs the closure
/// once the platform webview is available, and a failure to reach it leaves the default
/// menu in place. The handler lives for the window's lifetime (single window, open until
/// exit), so the registration cookie is intentionally discarded.
pub fn customize_context_menu(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
        let controller = webview.controller();
        // SAFETY: `controller` is the live WebView2 controller from Tauri; CoreWebView2
        // writes the core object through an out-pointer and returns an error rather than
        // misbehaving. A failure leaves the default menu in place.
        let Ok(core) = (unsafe { controller.CoreWebView2() }) else {
            return;
        };
        // ICoreWebView2_11 carries ContextMenuRequested; QueryInterface (cast) returns an
        // error rather than misbehaving on the off chance the runtime predates it, where
        // the menu is simply left untrimmed.
        let Ok(core) = core.cast::<ICoreWebView2_11>() else {
            return;
        };

        let handler = ContextMenuRequestedEventHandler::create(Box::new(
            move |_core: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2ContextMenuRequestedEventArgs>| {
                if let Some(args) = args {
                    prune_context_menu(&args);
                }
                Ok(())
            },
        ));
        let mut token = 0i64;
        // SAFETY: `core` is the live ICoreWebView2_11, `handler` is a valid event-handler
        // COM object, and `token` is a stack slot the call writes the cookie into. A
        // failed registration returns an error rather than misbehaving; ignored (best
        // effort).
        unsafe {
            let _ = core.add_ContextMenuRequested(&handler, &mut token);
        }
    });
}

/// Theme WebView2's own surfaces - context menus, dialogs, and the default form-control
/// rendering - to the content theme. Those surfaces follow the profile's
/// `PreferredColorScheme`, which defaults to auto (the OS light/dark preference), so a
/// dark app on a light OS otherwise pops light context menus. Setting the scheme to the
/// content theme keeps them in step with the injected `color-scheme`. Called at startup
/// and again whenever the theme setting changes at runtime.
///
/// Best effort, mirroring [`customize_context_menu`]: `with_webview` runs the closure
/// once the platform webview is available, and any failure to reach the profile (a
/// runtime predating `ICoreWebView2_13` has none) leaves the auto scheme in place.
pub fn apply_webview_color_scheme(window: &WebviewWindow, setting: ThemeSetting) {
    // `move` so the closure owns the `setting` copy: `with_webview` requires a `'static`
    // callback, which cannot borrow a local.
    let _ = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: `controller` is the live WebView2 controller from Tauri; CoreWebView2
        // writes the core object through an out-pointer and returns an error rather than
        // misbehaving. A failure leaves the auto scheme in place.
        let Ok(core) = (unsafe { controller.CoreWebView2() }) else {
            return;
        };
        // ICoreWebView2_13 exposes the profile; QueryInterface (cast) returns an error
        // rather than misbehaving on a runtime that predates it, where the scheme is left
        // at auto.
        let Ok(core) = core.cast::<ICoreWebView2_13>() else {
            return;
        };
        // `Auto` maps to WebView2's own auto scheme, which follows the OS - so its context
        // menus and the webview's prefers-color-scheme track the system with no re-apply.
        let scheme = match setting {
            ThemeSetting::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            ThemeSetting::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            ThemeSetting::Auto => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
        };
        // SAFETY: `core` is the live ICoreWebView2_13; Profile writes the profile object
        // through an out-pointer and SetPreferredColorScheme takes the enum by value.
        // Both return an error rather than misbehaving; ignored (best effort).
        unsafe {
            if let Ok(profile) = core.Profile() {
                let _ = profile.SetPreferredColorScheme(scheme);
            }
        }
    });
}

/// Show or hide the webview inside the window, without touching the window
/// itself. The startup splash holds it back until the page has painted the mark:
/// the webview's output is composited above the window's child windows whatever
/// their z-order, so it would otherwise cover the splash with the browser's blank
/// canvas until the document paints.
///
/// Best effort, mirroring [`disable_browser_shortcuts`]: a failure to reach the
/// controller leaves the webview as it is - visible, which is the pre-splash
/// behavior.
pub fn set_webview_visible(window: &WebviewWindow, visible: bool) {
    let _ = window.with_webview(move |webview| {
        let controller = webview.controller();
        // SAFETY: `controller` is the live WebView2 controller from Tauri;
        // SetIsVisible takes the flag by value and returns an error rather than
        // misbehaving. Ignored (best effort).
        unsafe {
            let _ = controller.SetIsVisible(visible);
        }
    });
}

/// Move the keyboard focus into the webview, so what the user types reaches the page.
///
/// The window is activated as it is shown, before there is a webview to hand the focus
/// to, and the webview is built unfocused (`run`, where the reason is written down).
/// wry moves the focus in on every later `WM_SETFOCUS`, but the window is already the
/// focused one by then, so the startup has no `WM_SETFOCUS` to ride: without this call
/// the focus would sit on the window itself, which swallows the keyboard, until the
/// user clicked into the page or switched away and back.
///
/// Best effort, mirroring [`set_webview_visible`]: WebView2 refuses the move for a
/// window that cannot take focus, which leaves the page unfocused until the user's
/// first click or switch-back brings wry's own call.
pub fn focus_webview(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
        let controller = webview.controller();
        // SAFETY: `controller` is the live WebView2 controller from Tauri; MoveFocus
        // takes the reason by value and returns an error rather than misbehaving.
        // Ignored (best effort).
        unsafe {
            let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
    });
}

/// The zoom factor of unscaled content, which Ctrl+0 restores.
const UNZOOMED: f64 = 1.0;

/// Restore the content zoom factor and report every later change to `on_change`, so
/// the level the user picked with Ctrl+/-, Ctrl+wheel, or a pinch is remembered across
/// runs alongside the window's size and position.
///
/// The zoom factor lives on the WebView2 controller, not the window, so it is applied
/// and observed here rather than in `window_state`. A factor the host sets becomes the
/// webview's new default, which applies across navigations, so applying it once at
/// startup is enough. A factor the *user* sets is only the current page's, which is
/// precisely why it has to be captured and re-applied as the default on the next run
/// rather than left to WebView2.
///
/// That default is also where WebView2 sends Ctrl+0, which would make the restored
/// level - rather than 100% - the reset target, and leave a user who zoomed in one run
/// no key to get back to unscaled content. So Ctrl+0 is taken over here: the key is
/// marked handled to cancel WebView2's own reset, [`UNZOOMED`] is applied in its place,
/// and the new level is reported like any other. Setting the property raises no
/// `ZoomFactorChanged` (WebView2 only raises it for a user zoom, or when normalizing a
/// factor outside its supported range), which is why neither this reset nor the startup
/// restore comes back through the subscription on its own.
///
/// Best effort, mirroring [`disable_browser_shortcuts`]: `with_webview` runs the
/// closure once the platform webview is available, and a failure to reach it leaves
/// the content unzoomed and untracked. Both subscriptions live for the window's
/// lifetime (single window, open until exit), so their cookies are intentionally
/// discarded.
pub fn apply_and_track_zoom<F>(window: &WebviewWindow, zoom: f64, on_change: F)
where
    F: Fn(f64) + Send + Sync + 'static,
{
    let _ = window.with_webview(move |webview| {
        let controller = webview.controller();

        // SAFETY: `controller` is the live WebView2 controller from Tauri;
        // SetZoomFactor takes the factor by value and returns an error rather than
        // misbehaving on a value it rejects. Ignored (best effort).
        unsafe {
            let _ = controller.SetZoomFactor(zoom);
        }

        // Shared by the two subscriptions below, which both report a new level.
        let on_change = Arc::new(on_change);

        let zoomed = Arc::clone(&on_change);
        let zoom_handler = ZoomFactorChangedEventHandler::create(Box::new(
            move |controller: Option<ICoreWebView2Controller>, _args: Option<IUnknown>| {
                let Some(controller) = controller else {
                    return Ok(());
                };
                // The event carries no factor, so read it back from the controller
                // that raised it.
                // SAFETY: `controller` is the live sender WebView2 handed to this
                // callback; ZoomFactor writes one f64 through the out-pointer and
                // returns an error (propagated by `?`) rather than overrunning.
                let mut factor = 0f64;
                unsafe {
                    controller.ZoomFactor(&mut factor)?;
                }
                zoomed(factor);
                Ok(())
            },
        ));

        let reset = Arc::clone(&on_change);
        let key_handler = AcceleratorKeyPressedEventHandler::create(Box::new(
            move |controller: Option<ICoreWebView2Controller>,
                  args: Option<ICoreWebView2AcceleratorKeyPressedEventArgs>| {
                let (Some(controller), Some(args)) = (controller, args) else {
                    return Ok(());
                };
                // SAFETY: `controller` and `args` are the live sender and event
                // argument WebView2 handed to this callback; each accessor writes
                // through the out-pointer we pass, and SetHandled/SetZoomFactor take
                // their value by value. All return an error (propagated by `?`) rather
                // than overrunning or misbehaving.
                unsafe {
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                    args.KeyEventKind(&mut kind)?;
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }
                    let mut virtual_key = 0u32;
                    args.VirtualKey(&mut virtual_key)?;
                    if !is_zoom_reset_shortcut(virtual_key, ctrl_down()) {
                        return Ok(());
                    }
                    args.SetHandled(true)?;
                    controller.SetZoomFactor(UNZOOMED)?;
                }
                reset(UNZOOMED);
                Ok(())
            },
        ));

        let mut token = 0i64;
        // SAFETY: `controller` is the live WebView2 controller, each handler is a valid
        // event-handler COM object, and `token` is a stack slot the calls write their
        // cookie into. A failed registration returns an error rather than misbehaving;
        // ignored (best effort).
        unsafe {
            let _ = controller.add_ZoomFactorChanged(&zoom_handler, &mut token);
            let _ = controller.add_AcceleratorKeyPressed(&key_handler, &mut token);
        }
    });
}

/// Whether a key-down virtual key (with the current Ctrl state) is the zoom-reset
/// shortcut. Ctrl+0 on the number row and on the numpad both reset the zoom in a
/// browser, so both are taken; a plain `0` keystroke is left to the page.
fn is_zoom_reset_shortcut(virtual_key: u32, ctrl_held: bool) -> bool {
    const VK_0: u32 = 0x30;
    const VK_NUMPAD0: u32 = 0x60;
    ctrl_held && matches!(virtual_key, VK_0 | VK_NUMPAD0)
}

/// The tao/tauri window theme to pin for a setting: an explicit `Dark`/`Light`, or `None`
/// to follow the OS under `Auto`. Pass to `WebviewWindowBuilder::theme` at build and
/// `WebviewWindow::set_theme` on a runtime change, alongside [`apply_webview_color_scheme`].
///
/// Pinning the window theme is what keeps an explicit context menu from drifting on an OS
/// light/dark switch. tauri-runtime-wry force-calls `Webview::set_theme(os_theme)` on every
/// `WindowEvent::ThemeChanged`, which resets WebView2's `PreferredColorScheme` (and with it
/// the context menus and dialogs) to the OS - clobbering the scheme
/// [`apply_webview_color_scheme`] set. tao only raises `ThemeChanged` on an OS switch when
/// the window theme is unpinned, so pinning an explicit theme suppresses the event and the
/// override never runs. `Auto` stays unpinned so the OS switch still drives the webview,
/// WebView2's surfaces (through that same runtime handler), and the native frame.
pub fn window_theme(setting: ThemeSetting) -> Option<tauri::Theme> {
    match setting {
        ThemeSetting::Dark => Some(tauri::Theme::Dark),
        ThemeSetting::Light => Some(tauri::Theme::Light),
        ThemeSetting::Auto => None,
    }
}

/// One menu entry as classified for pruning: a wanted item to keep, a separator (kept
/// only when it still divides two kept runs), or an unwanted item to drop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuSlot {
    Keep,
    Separator,
    Drop,
}

/// Remove the unwanted items from a `ContextMenuRequested` menu in place: classify each
/// entry, then delete the drops plus any separator left dangling, walking high-to-low so
/// each index stays valid as earlier ones go.
fn prune_context_menu(args: &ICoreWebView2ContextMenuRequestedEventArgs) {
    // SAFETY: `args` is the live event argument WebView2 handed to the callback;
    // MenuItems writes the collection through an out-pointer and errors rather than
    // overrunning.
    let Ok(items) = (unsafe { args.MenuItems() }) else {
        return;
    };
    let mut count = 0u32;
    // SAFETY: `items` is the live collection; Count writes one u32 through the
    // out-pointer.
    if unsafe { items.Count(&mut count) }.is_err() {
        return;
    }

    let mut slots = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: `i < count`; GetValueAtIndex writes the item through an out-pointer and
        // errors rather than overrunning on an out-of-range index.
        match unsafe { items.GetValueAtIndex(i) } {
            Ok(item) => slots.push(classify_menu_item(&item)),
            // An item we cannot read is left in place rather than removed.
            Err(_) => slots.push(MenuSlot::Keep),
        }
    }

    for index in menu_items_to_remove(&slots).into_iter().rev() {
        // SAFETY: `index < count` and removals run high-to-low, so it is still in range.
        let _ = unsafe { items.RemoveValueAtIndex(index) };
    }
}

/// Classify one menu item: a separator, a kept command (its unlocalized name is on the
/// keep-list), or a drop. An item whose kind or name cannot be read is kept, so an
/// unreadable entry is never silently removed.
fn classify_menu_item(item: &ICoreWebView2ContextMenuItem) -> MenuSlot {
    let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND::default();
    // SAFETY: `item` is a live menu item; Kind writes one enum value through the
    // out-pointer.
    if unsafe { item.Kind(&mut kind) }.is_err() {
        return MenuSlot::Keep;
    }
    if kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR {
        return MenuSlot::Separator;
    }
    match menu_item_name(item) {
        Some(name) if is_kept_menu_item(&name) => MenuSlot::Keep,
        Some(_) => MenuSlot::Drop,
        None => MenuSlot::Keep,
    }
}

/// The item's unlocalized name (`"back"`, `"copy"`, ...), or `None` if it cannot be read.
fn menu_item_name(item: &ICoreWebView2ContextMenuItem) -> Option<String> {
    let mut name = PWSTR::null();
    // SAFETY: `item` is a live menu item; Name writes a CoTaskMem-allocated PWSTR through
    // the out-pointer, leaving it null on failure (where `?` returns before the take).
    unsafe { item.Name(&mut name) }.ok()?;
    // take_pwstr frees the CoTaskMem buffer WebView2 allocated for the name.
    Some(take_pwstr(name))
}

/// Whether a context-menu item with this unlocalized WebView2 name is one we keep: the
/// navigation items (back/forward) and the text editing/clipboard commands that make up
/// the input context menu. Everything else - reload, save as, print, share, web select,
/// emoji, inspect, ... - is dropped.
fn is_kept_menu_item(name: &str) -> bool {
    matches!(
        name,
        "back"
            | "forward"
            | "undo"
            | "redo"
            | "cut"
            | "copy"
            | "paste"
            | "pasteAndMatchStyle"
            | "selectAll"
    )
}

/// The indices to remove from a menu, given its slots in order: every `Drop`, plus any
/// separator left leading, trailing, or doubled once the drops are gone. Pure, so the
/// separator collapsing is unit-testable without the COM collection.
fn menu_items_to_remove(slots: &[MenuSlot]) -> Vec<u32> {
    let mut remove = vec![false; slots.len()];
    for (i, slot) in slots.iter().enumerate() {
        if *slot == MenuSlot::Drop {
            remove[i] = true;
        }
    }

    // A separator is kept only after a surviving Keep (so leading separators and runs of
    // separators collapse). The last separator kept this way is held as possibly-trailing
    // until a later Keep confirms it divides two runs; if none does, it is dropped too.
    let mut after_keep = false;
    let mut trailing_separator: Option<usize> = None;
    for (i, slot) in slots.iter().enumerate() {
        if remove[i] {
            continue;
        }
        match slot {
            MenuSlot::Keep => {
                after_keep = true;
                trailing_separator = None;
            }
            MenuSlot::Separator => {
                if after_keep {
                    after_keep = false;
                    trailing_separator = Some(i);
                } else {
                    remove[i] = true;
                }
            }
            // Drops are already marked above and skipped via the `continue`.
            MenuSlot::Drop => {}
        }
    }
    if let Some(i) = trailing_separator {
        remove[i] = true;
    }

    remove
        .iter()
        .enumerate()
        .filter_map(|(i, &drop)| drop.then_some(i as u32))
        .collect()
}

/// A script that re-applies the `--wh-*` tokens and `color-scheme` for the given theme to
/// the already-loaded document. Evaled when the theme changes at runtime (a setting change
/// or, under `Auto`, an OS light/dark switch) so the injected log pane, custom scrollbar
/// and broker banner - which read these tokens - re-color live alongside the native frame.
pub fn theme_tokens_update_script(dark: bool) -> String {
    format!("(function(){{{}}})();", token_apply_js(&Theme { dark }))
}

/// The `--wh-*` design tokens for a theme, as a JS object literal. The shell pieces we
/// inject (the log pane, the custom scrollbar and the broker banner) read these; the
/// shared front-end themes itself. A JSON object literal is valid JS and is correctly
/// escaped by serde_json.
fn theme_vars(theme: &Theme) -> serde_json::Value {
    let (fg, bg, border) = theme.palette();
    let (warning_bg, warning) = theme.warning_colors();
    let (slider_bg, slider_hover, slider_active) = theme.scrollbar_slider_colors();
    serde_json::json!({
        "--wh-bg": bg,
        "--wh-fg": fg,
        "--wh-border": border,
        // A fixed accent for the log-pane splitter hover, the same in both themes.
        "--wh-accent": "#0078d4",
        "--wh-warning-bg": warning_bg,
        "--wh-warning": warning,
        "--wh-cscroll-thumb": slider_bg,
        "--wh-cscroll-thumb-hover": slider_hover,
        "--wh-cscroll-thumb-active": slider_active,
    })
}

/// The theme's `color-scheme` name as a JSON string literal (`"dark"`/`"light"`), which is
/// valid JS, correctly quoted.
fn scheme_js(theme: &Theme) -> serde_json::Value {
    serde_json::Value::from(if theme.dark { "dark" } else { "light" })
}

/// The statements that set `color-scheme` and the `--wh-*` tokens on `:root`, assuming
/// `document.documentElement` exists. Shared by the init script (which guards/defers it)
/// and the runtime token-update eval (where the document is already loaded).
fn token_apply_js(theme: &Theme) -> String {
    let scheme = scheme_js(theme);
    let vars = theme_vars(theme);
    format!(
        "var r=document.documentElement.style;\
         r.setProperty('color-scheme',{scheme});\
         var v={vars};for(var k in v){{r.setProperty(k,v[k]);}}"
    )
}

/// The pure script builder, factored out so it is unit-testable without the registry.
fn theme_init_script_for(theme: &Theme) -> String {
    let scheme = scheme_js(theme);
    let body = token_apply_js(theme);

    // Set the tokens on `:root`. This script is injected at document creation, before
    // `<html>` is parsed, so `document.documentElement` is null at first; defer to
    // DOMContentLoaded in that case (the injected components' fallbacks cover the paint
    // until then), and apply immediately if the element already exists.
    //
    // Also publish the initial (already-resolved) theme as `window.__WH_INITIAL_THEME__`,
    // synchronously before the front-end bundle runs, so its pre-render theme apply
    // (main.tsx) picks the registry-backed theme instead of the default dark - otherwise a
    // light-theme user flashes the dark bundle until the settings arrive over IPC.
    format!(
        "(function(){{window.__WH_INITIAL_THEME__={scheme};function a(){{{body}}}\
         if(document.documentElement){{a();}}\
         else{{document.addEventListener('DOMContentLoaded',a);}}}})();"
    )
}

/// A navigation handler for the main window: hand a top-level navigation to an
/// externally-openable target ([`is_external`]) to the system (returning `false` to cancel
/// the in-webview navigation), and allow everything in-app (the asset/IPC `*.localhost`
/// origins and hash routing). Wired as the window's `on_navigation` callback.
///
/// The mod README's markdown links arrive here rather than at [`handle_new_window`]: the
/// renderer sets no `target`, so they are same-window navigations.
pub fn handle_navigation(app: &AppHandle, url: &Url) -> bool {
    if !is_external(url) {
        return true;
    }
    open_externally(app, url);
    false
}

/// A new-window handler for the main window: hand a request for an externally-openable
/// target ([`is_external`]) to the system, and deny the new webview in every case. Wired
/// as the window's `on_new_window` callback.
///
/// This is the hook that carries `<a target="_blank">` and `window.open`: WebView2 raises
/// `NewWindowRequested` for those and no `NavigationStarting`, so [`handle_navigation`]
/// never sees them and they would otherwise be silently dead (wry denies a new-window
/// request with no handler registered). The deny is unconditional because the app has a
/// single window: an in-app `window.open` has no more business spawning a second webview
/// than an external link has opening inside this one.
pub fn handle_new_window(app: &AppHandle, url: &Url) -> NewWindowResponse<Wry> {
    if is_external(url) {
        open_externally(app, url);
    }
    NewWindowResponse::Deny
}

/// Hand a URL to its registered system handler - the browser, the mail client - through
/// the Tauri opener plugin, reporting a failure to the diagnostic output. Best effort:
/// there is nowhere else for the link to go.
fn open_externally(app: &AppHandle, url: &Url) {
    if let Err(error) = app.opener().open_url(url.as_str(), None::<&str>) {
        eprintln!("windhawk-ui: failed to open external link '{url}': {error}");
    }
}

/// Whether a link target is one the system, not this webview, should open: a real
/// `http(s)` host - any host but Tauri's `*.localhost` app/IPC origins - or a `mailto:`
/// address. Everything else stays in the app.
///
/// This is a scheme ALLOWLIST, and it is the only one on the path. What it admits reaches
/// `ShellExecute` (the opener plugin filters nothing of its own), and the hrefs behind it
/// are mod metadata and mod README markdown - author-supplied text. The front-end
/// sanitizer admits the same three schemes, but it is the untrusted side of the IPC
/// boundary, so this check stands on its own rather than on that one holding.
fn is_external(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => url
            .host_str()
            .is_some_and(|host| host != "localhost" && !host.ends_with(".localhost")),
        // No host to vet: a `mailto:` addresses the mail client, not a server.
        "mailto" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_script_sets_the_color_scheme_and_defers_until_documentelement() {
        let dark = theme_init_script_for(&Theme { dark: true });
        assert!(dark.contains("'color-scheme',\"dark\""));
        // Injected before `<html>` exists, so it must guard on `documentElement` and
        // defer rather than dereference it unconditionally.
        assert!(dark.contains("if(document.documentElement)"));
        assert!(dark.contains("addEventListener('DOMContentLoaded'"));
        // No `--vscode-*` variables are injected; the front-end themes itself.
        assert!(!dark.contains("--vscode-"));

        let light = theme_init_script_for(&Theme { dark: false });
        assert!(light.contains("'color-scheme',\"light\""));
    }

    #[test]
    fn theme_script_publishes_the_initial_theme_global() {
        // The front-end reads this global before its first paint to avoid flashing
        // the default dark bundle, so it must be set synchronously (not deferred).
        assert!(
            theme_init_script_for(&Theme { dark: true })
                .contains("window.__WH_INITIAL_THEME__=\"dark\"")
        );
        assert!(
            theme_init_script_for(&Theme { dark: false })
                .contains("window.__WH_INITIAL_THEME__=\"light\"")
        );
    }

    #[test]
    fn theme_setting_parses_with_dark_as_the_default() {
        assert_eq!(ThemeSetting::parse("dark"), ThemeSetting::Dark);
        assert_eq!(ThemeSetting::parse("light"), ThemeSetting::Light);
        assert_eq!(ThemeSetting::parse("auto"), ThemeSetting::Auto);
        // An unrecognized value is the dark default (matches the front-end).
        assert_eq!(ThemeSetting::parse("nonsense"), ThemeSetting::Dark);
        // Explicit settings resolve without reading the OS; Auto reads it (not asserted
        // here since it depends on the test host's theme).
        assert!(ThemeSetting::Dark.resolved_dark());
        assert!(!ThemeSetting::Light.resolved_dark());
    }

    #[test]
    fn tokens_update_script_reapplies_scheme_and_tokens_only() {
        // The runtime token update targets the already-loaded document: no initial-theme
        // global and no DOMContentLoaded deferral (unlike the init script).
        let dark = theme_tokens_update_script(true);
        assert!(dark.contains("'color-scheme',\"dark\""));
        assert!(dark.contains("--wh-cscroll-thumb"));
        assert!(!dark.contains("__WH_INITIAL_THEME__"));
        assert!(!dark.contains("DOMContentLoaded"));
        assert!(theme_tokens_update_script(false).contains("'color-scheme',\"light\""));
    }

    #[test]
    fn theme_script_publishes_the_custom_scrollbar_tokens() {
        // The custom scrollbar (scrollbar.js) reads these tokens for theming, so the
        // theme script must publish them with the per-theme defaults.
        let dark = theme_init_script_for(&Theme { dark: true });
        assert!(dark.contains("--wh-cscroll-thumb"));
        assert!(dark.contains("rgba(121, 121, 121, 0.4)"));

        let light = theme_init_script_for(&Theme { dark: false });
        assert!(light.contains("rgba(100, 100, 100, 0.4)"));
    }

    #[test]
    fn theme_script_publishes_the_warning_surface() {
        // The broker banner paints itself with these, so both themes must carry the
        // front-end's antd warning colors rather than one of them falling back.
        let dark = theme_init_script_for(&Theme { dark: true });
        assert!(dark.contains("\"--wh-warning-bg\":\"#2b2111\""));
        assert!(dark.contains("\"--wh-warning\":\"#d89614\""));

        let light = theme_init_script_for(&Theme { dark: false });
        assert!(light.contains("\"--wh-warning-bg\":\"#fffbe6\""));
        assert!(light.contains("\"--wh-warning\":\"#faad14\""));
    }

    #[test]
    fn scrollbar_script_hides_native_and_draws_a_custom_thumb() {
        let js = scrollbar_init_script();
        assert!(js.contains("wh-cscroll-thumb"));
        assert!(js.contains("scrollbar-width:none"));
        assert!(js.contains("var(--wh-cscroll-thumb"));
    }

    #[test]
    fn scrollbar_script_stands_down_in_high_contrast() {
        // Windows high contrast surfaces as the forced-colors media query; the script
        // must consult it and stay live for a mid-session toggle.
        let js = scrollbar_init_script();
        assert!(js.contains("(forced-colors: active)"));
        assert!(js.contains("forcedColors.addEventListener('change'"));
    }

    #[test]
    fn background_color_is_the_opaque_page_background() {
        // The front-end's own page color in each theme, opaque - not the palette's
        // editor background, which the log pane (not the page) is drawn on.
        assert_eq!(theme_background_color(true), Color(0x1e, 0x1e, 0x1e, 255));
        assert_eq!(theme_background_color(false), Color(0xf5, 0xf5, 0xf5, 255));
    }

    #[test]
    fn colorref_packs_rgb_into_dwm_bgr_order() {
        // #102030 -> r=0x10 g=0x20 b=0x30 -> COLORREF 0x00bbggrr.
        assert_eq!(colorref("#102030"), 0x0030_2010);
    }

    #[test]
    fn frame_colors_use_one_frame_gray_for_caption_and_border() {
        // active dark frame #3c3c3c (caption + border), text = fg #cccccc.
        assert_eq!(
            Theme { dark: true }.frame_colors(true),
            (0x003c_3c3c, 0x00cc_cccc, 0x003c_3c3c)
        );
        // active light frame #cecece (caption + border), text = fg #1f1f1f.
        assert_eq!(
            Theme { dark: false }.frame_colors(true),
            (0x00ce_cece, 0x001f_1f1f, 0x00ce_cece)
        );
    }

    #[test]
    fn inactive_frame_colors_dim_the_unfocused_window() {
        // dark inactive: frame #323233, text rgba(204,204,204,0.6) over it = #8e8e8f.
        assert_eq!(
            Theme { dark: true }.frame_colors(false),
            (0x0033_3232, 0x008f_8e8e, 0x0033_3232)
        );
        // light inactive: frame #dedede, text rgba(31,31,31,0.6) over it = #6b6b6b.
        assert_eq!(
            Theme { dark: false }.frame_colors(false),
            (0x00de_dede, 0x006b_6b6b, 0x00de_dede)
        );
    }

    #[test]
    fn suppressed_shortcuts_cover_downloads_print_reload_caret_but_spare_plain_and_kept_keys() {
        const VK_J: u32 = 0x4A;
        const VK_P: u32 = 0x50;
        const VK_R: u32 = 0x52;
        const VK_F: u32 = 0x46;
        const VK_A: u32 = 0x41;
        const VK_F5: u32 = 0x74;
        const VK_F7: u32 = 0x76;

        // F5 (reload, Ctrl+F5 hard reload) and F7 (caret) are suppressed with or
        // without Ctrl.
        assert!(is_suppressed_shortcut(VK_F5, false));
        assert!(is_suppressed_shortcut(VK_F5, true));
        assert!(is_suppressed_shortcut(VK_F7, false));

        // Ctrl+J (downloads), Ctrl+P (print), and Ctrl+R / Ctrl+Shift+R (reload, same
        // virtual key) are suppressed, but a plain j/p/r keystroke is left for the page.
        assert!(is_suppressed_shortcut(VK_J, true));
        assert!(is_suppressed_shortcut(VK_P, true));
        assert!(is_suppressed_shortcut(VK_R, true));
        assert!(!is_suppressed_shortcut(VK_J, false));
        assert!(!is_suppressed_shortcut(VK_P, false));
        assert!(!is_suppressed_shortcut(VK_R, false));

        // Kept keys: find (Ctrl+F) and clipboard/select-all (Ctrl+A) stay live.
        assert!(!is_suppressed_shortcut(VK_F, true));
        assert!(!is_suppressed_shortcut(VK_A, true));
    }

    #[test]
    fn zoom_reset_takes_ctrl_zero_from_both_the_number_row_and_the_numpad() {
        const VK_0: u32 = 0x30;
        const VK_NUMPAD0: u32 = 0x60;
        const VK_1: u32 = 0x31;

        assert!(is_zoom_reset_shortcut(VK_0, true));
        assert!(is_zoom_reset_shortcut(VK_NUMPAD0, true));

        // A plain 0 is a page keystroke, and no other digit resets the zoom.
        assert!(!is_zoom_reset_shortcut(VK_0, false));
        assert!(!is_zoom_reset_shortcut(VK_NUMPAD0, false));
        assert!(!is_zoom_reset_shortcut(VK_1, true));

        // Ctrl+0 reaches this handler rather than the suppression one, which must
        // leave it (like the other zoom keys) for WebView2's own accelerator path.
        assert!(!is_suppressed_shortcut(VK_0, true));
    }

    #[test]
    fn kept_menu_items_are_navigation_and_the_input_clipboard_commands() {
        // Navigation and the editable-field clipboard/edit commands survive.
        for name in [
            "back",
            "forward",
            "undo",
            "redo",
            "cut",
            "copy",
            "paste",
            "pasteAndMatchStyle",
            "selectAll",
        ] {
            assert!(is_kept_menu_item(name), "{name} should be kept");
        }
        // The browser-ish items are dropped.
        for name in [
            "reload",
            "saveAs",
            "print",
            "share",
            "webSelect",
            "emoji",
            "inspectElement",
            "createQRCode",
        ] {
            assert!(!is_kept_menu_item(name), "{name} should be dropped");
        }
    }

    #[test]
    fn pruning_a_page_menu_keeps_back_forward_and_drops_dangling_separators() {
        use MenuSlot::{Drop, Keep, Separator};
        // back, forward, reload, |, saveAs, print, |, webSelect, |, inspectElement.
        let slots = [
            Keep, Keep, Drop, Separator, Drop, Drop, Separator, Drop, Separator, Drop,
        ];
        // Only back/forward remain; every drop and every now-dangling separator goes.
        assert_eq!(menu_items_to_remove(&slots), vec![2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn pruning_an_editable_menu_preserves_inner_separators() {
        use MenuSlot::{Drop, Keep, Separator};
        // undo, redo, |, cut, copy, paste, |, selectAll, |, emoji.
        let slots = [
            Keep, Keep, Separator, Keep, Keep, Keep, Separator, Keep, Separator, Drop,
        ];
        // Only emoji and the trailing separator before it are removed; the separators
        // dividing the kept runs stay.
        assert_eq!(menu_items_to_remove(&slots), vec![8, 9]);
    }

    #[test]
    fn pruning_drops_a_menu_that_keeps_nothing() {
        use MenuSlot::{Drop, Separator};
        let slots = [Drop, Separator, Drop, Separator, Drop];
        assert_eq!(menu_items_to_remove(&slots), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn external_links_are_detected_but_app_origins_are_not() {
        assert!(is_external(&Url::parse("https://github.com/x").unwrap()));
        assert!(is_external(
            &Url::parse("http://i.imgur.com/a.png").unwrap()
        ));
        assert!(!is_external(
            &Url::parse("http://tauri.localhost/index.html").unwrap()
        ));
        assert!(!is_external(&Url::parse("http://ipc.localhost/").unwrap()));
        assert!(!is_external(&Url::parse("tauri://localhost/").unwrap()));
    }

    #[test]
    fn mail_links_open_externally_whatever_their_case() {
        // A mod's `@donate`/README can address the mail client; it has no host to vet.
        assert!(is_external(&Url::parse("mailto:dev@example.com").unwrap()));
        assert!(is_external(
            &Url::parse("mailto:dev@example.com?subject=Hi").unwrap()
        ));
        // Url::parse lowercases the scheme, so the match arm sees it however it was written.
        assert!(is_external(&Url::parse("MAILTO:dev@example.com").unwrap()));
    }

    #[test]
    fn no_other_scheme_reaches_the_system_handler() {
        // The allowlist is this function alone: what it admits is handed to ShellExecute,
        // and the hrefs behind it are mod-author text. Anything that is not http(s) or
        // mailto stays in the app, whatever a front-end sanitizer did or did not catch.
        for url in [
            "file:///C:/Windows/System32/calc.exe",
            "ms-msdt:/id%20PCWDiagnostic",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vscode://x/y",
            "ftp://example.com/x",
        ] {
            assert!(
                !is_external(&Url::parse(url).unwrap()),
                "{url} must not be handed to the system"
            );
        }
    }
}
