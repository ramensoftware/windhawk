//! The environment shim: the small, backend-free glue that frees the shared
//! React front-end from its VSCode-webview assumptions. Two pieces live here:
//!
//! - **Theme.** The shared front-end themes itself; this shim only colors the pieces
//!   we inject. [`theme_init_script`] returns a tiny initialization script that sets
//!   `color-scheme` and a small set of `--wh-*` design tokens on `:root` - the colors
//!   the injected log pane and custom scrollbar consume (both carry hard-coded
//!   fallbacks covering the first paint).
//!   [`theme_background_color`] gives the matching window/webview background, set on
//!   the window so the frame before that first document paint is the themed color
//!   rather than a white flash. [`apply_frame_theme`] pushes the same palette to the
//!   native window frame via DWM, so the title bar and border match the content
//!   instead of staying a stock-light strip around a dark webview.
//!   [`apply_webview_color_scheme`] sets the WebView2 profile's preferred color scheme
//!   so WebView2's own surfaces (context menus, dialogs) match the content too, rather
//!   than following the OS.
//! - **Scrollbars.** [`scrollbar_init_script`] injects `scrollbar.js`, a custom
//!   overlay scrollbar that replaces WebView2's Edge Fluent bar with the flat VSCode
//!   look, themed from the slider variables the theme script sets.
//! - **Shortcuts.** A WebView2 window inherits Edge's browser hotkeys.
//!   [`disable_browser_shortcuts`] marks the ones with no place in this app handled on
//!   the controller's accelerator-key event - show downloads (Ctrl+J), print (Ctrl+P),
//!   reload and hard reload (F5, Ctrl+R, Ctrl+Shift+R, Ctrl+F5), and the caret-browsing
//!   toggle (F7) - which cancels WebView2's default action while leaving find, the zoom
//!   keys, and the clipboard keys alone. WebView2 has no per-key switch, and its
//!   all-or-nothing
//!   `AreBrowserAcceleratorKeysEnabled` would also drop the keys we keep.
//! - **Context menu.** A WebView2 window also inherits Edge's full right-click menu.
//!   [`customize_context_menu`] trims it on the controller's `ContextMenuRequested`
//!   event to just the items this app wants - navigation (back/forward) and the input
//!   context menu (cut/copy/paste/paste-as-plain-text/undo/redo/select all) - dropping
//!   the rest (reload, save as, print, share, web select, emoji, inspect, ...) and the
//!   separators those removals leave dangling. The same keep-list governs both the
//!   page menu (where only back/forward survive) and the editable-field menu (where the
//!   clipboard items survive).
//! - **External links.** Donate/GitHub/homepage links are `<a target="_blank">`; in
//!   a webview those must open the system browser. [`handle_navigation`] intercepts a
//!   top-level navigation to a real (non-`localhost`) host and hands it to the Tauri
//!   opener plugin, cancelling the in-webview navigation.
//!
//! The `data-content` panel attribute the extension sets is NOT injected here: its
//! value is front-end-internal, so the `tauri` build mode (the sibling front-end
//! changeset) sets it, the same place it selects the transport. Injecting a guessed
//! value from Rust could mis-route the panel.

use tauri::webview::Color;
use tauri::{AppHandle, Url, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
    COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
    ICoreWebView2, ICoreWebView2_11, ICoreWebView2_13, ICoreWebView2AcceleratorKeyPressedEventArgs,
    ICoreWebView2ContextMenuItem, ICoreWebView2ContextMenuRequestedEventArgs,
    ICoreWebView2Controller,
};
use webview2_com::{
    AcceleratorKeyPressedEventHandler, ContextMenuRequestedEventHandler, take_pwstr,
};
use windows_core::{Interface, PWSTR};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::LoadIconWithScaleDown;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HICON, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
    USER_DEFAULT_SCREEN_DPI, WM_SETICON,
};

/// The appearance the front-end and injected shell pieces are themed to match.
struct Theme {
    /// Whether the dark palette is in effect.
    dark: bool,
}

impl Theme {
    /// The configured theme. Fixed to dark; a future setting will make it configurable
    /// (dark, light, or following the system preference).
    fn configured() -> Theme {
        Theme { dark: true }
    }

    /// `(foreground, background, border)` for the theme.
    fn palette(&self) -> (&'static str, &'static str, &'static str) {
        if self.dark {
            ("#cccccc", "#1e1e1e", "#454545")
        } else {
            ("#1f1f1f", "#ffffff", "#cecece")
        }
    }

    /// The window/webview background as an opaque color, derived from the palette's
    /// editor-background so it stays the single source for the theme's background.
    /// Used as the native window + WebView2 default color so the first frame (before
    /// the document paints) is the themed color rather than a white flash. Alpha is
    /// full: on Windows the window layer ignores alpha and the webview layer ignores a
    /// non-zero alpha, so opaque is the only meaningful value.
    fn background_color(&self) -> Color {
        let (_, bg, _) = self.palette();
        // `bg` is one of our own `#rrggbb` literals, so each component parses.
        let component =
            |i: usize| u8::from_str_radix(&bg[i..i + 2], 16).expect("palette bg is #rrggbb");
        Color(component(1), component(3), component(5), 255)
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

/// The initialization script that themes the injected shell pieces. Emits a script from
/// the configured theme that sets `color-scheme` and the `--wh-*` design tokens the log
/// pane and custom scrollbar consume on `:root`.
pub fn theme_init_script() -> String {
    theme_init_script_for(&Theme::configured())
}

/// The configured-theme background color for the window and webview. Set on the window
/// at build time so the first frame matches the theme instead of flashing white while
/// the webview attaches and the document paints (the init script only governs the
/// document, which paints after that frame).
pub fn theme_background_color() -> Color {
    Theme::configured().background_color()
}

/// Theme the native window frame (title bar + border) to match the configured content
/// theme. Pushes the matching caption, text, and border colors plus the immersive
/// dark-mode flag to DWM, so the frame is not a stock-light strip around a dark webview.
///
/// Best effort: the per-color attributes need Windows 11 (build 22000+) and the
/// call simply fails on older systems, where the dark-mode flag (Windows 10 2004+)
/// still flips the caption and its buttons between light and dark.
pub fn apply_frame_theme(window: &WebviewWindow) {
    // A missing handle just leaves the frame at the OS default; nothing else depends
    // on this. The Tauri `HWND` wraps the same `*mut c_void` as the windows-sys one.
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // Shown with focus at startup, so the active colors match the first painted frame.
    apply_frame_theme_to(hwnd.0, &Theme::configured(), true);
}

/// Re-push the frame colors for the given focus state to the main window. DWM keeps a
/// single set of frame colors regardless of focus, so the dimmed inactive look (and the
/// restore on refocus) only happens if the colors are re-pushed on each
/// `WindowEvent::Focused` transition. Same best-effort and handle handling as
/// [`apply_frame_theme`].
pub fn apply_frame_focus(window: &WebviewWindow, active: bool) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    apply_frame_theme_to(hwnd.0, &Theme::configured(), active);
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
/// Best effort, applied once while the window is hidden (like the frame theme): a
/// missing handle or icon resource just leaves tao's icon in place. The loaded icons
/// live for the process - the app has a single window, open until exit. A later DPI
/// change leaves Windows to rescale them rather than reloading the nearest native size.
pub fn apply_window_icons(window: &WebviewWindow) {
    // A missing handle just leaves tao's oversized icon; nothing else depends on this.
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    apply_window_icons_to(hwnd.0);
}

/// The executable's icon-group resource id. `tauri-build` embeds `icon.ico` under this
/// name id (the default its `set_icon` uses), so the running module owns the full
/// multi-resolution group that `LoadIconWithScaleDown` selects a size from.
const ICON_RESOURCE_ID: u16 = 32512;

/// Set a window handle's small and big icons to the right native sizes for its DPI.
fn apply_window_icons_to(hwnd: HWND) {
    // SAFETY: `hwnd` is the live main-window handle from Tauri; GetDpiForWindow only
    // reads it. It returns 0 for an invalid window, where we fall back to the 96-DPI
    // baseline so the metric lookups still give the standard sizes.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi == 0 {
        USER_DEFAULT_SCREEN_DPI
    } else {
        dpi
    };

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
/// VSCode look - reading the `--wh-cscroll-*` colors the theme script sets.
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
/// those keys we keep.
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
/// rendering - to the configured content theme. Those surfaces follow the profile's
/// `PreferredColorScheme`, which defaults to auto (the OS light/dark preference), so a
/// dark app on a light OS otherwise pops light context menus. Setting the scheme to the
/// configured theme keeps them in step with the injected `color-scheme`.
///
/// Best effort, mirroring [`customize_context_menu`]: `with_webview` runs the closure
/// once the platform webview is available, and any failure to reach the profile (a
/// runtime predating `ICoreWebView2_13` has none) leaves the auto scheme in place.
pub fn apply_webview_color_scheme(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
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
        let scheme = if Theme::configured().dark {
            COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK
        } else {
            COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT
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

/// The pure script builder, factored out so it is unit-testable without the registry.
fn theme_init_script_for(theme: &Theme) -> String {
    let (fg, bg, border) = theme.palette();
    let (slider_bg, slider_hover, slider_active) = theme.scrollbar_slider_colors();
    // A JSON string literal (`"dark"`/`"light"`) is valid JS, correctly quoted.
    let scheme = serde_json::Value::from(if theme.dark { "dark" } else { "light" });

    // Design tokens for the shell pieces we inject (the log pane and the custom
    // scrollbar); the shared front-end themes itself. A JSON object literal is valid
    // JS and is correctly escaped by serde_json.
    let vars = serde_json::json!({
        "--wh-bg": bg,
        "--wh-fg": fg,
        "--wh-border": border,
        // A fixed accent for the log-pane splitter hover, the same in both themes.
        "--wh-accent": "#0078d4",
        "--wh-cscroll-thumb": slider_bg,
        "--wh-cscroll-thumb-hover": slider_hover,
        "--wh-cscroll-thumb-active": slider_active,
    });

    // Set the tokens on `:root`. This script is injected at document creation, before
    // `<html>` is parsed, so `document.documentElement` is null at first; defer to
    // DOMContentLoaded in that case (the injected components' fallbacks cover the paint
    // until then), and apply immediately if the element already exists.
    format!(
        "(function(){{function a(){{\
         var r=document.documentElement.style;\
         r.setProperty('color-scheme',{scheme});\
         var v={vars};for(var k in v){{r.setProperty(k,v[k]);}}}}\
         if(document.documentElement){{a();}}\
         else{{document.addEventListener('DOMContentLoaded',a);}}}})();"
    )
}

/// A navigation handler for the main window: open a top-level navigation to a real
/// external host in the system browser (returning `false` to cancel the in-webview
/// navigation), and allow everything in-app (the asset/IPC `*.localhost` origins and
/// hash routing). Wired as the window's `on_navigation` callback.
pub fn handle_navigation(app: &AppHandle, url: &Url) -> bool {
    if !is_external(url) {
        return true;
    }
    if let Err(error) = app.opener().open_url(url.as_str(), None::<&str>) {
        eprintln!("windhawk-ui: failed to open external link '{url}': {error}");
    }
    false
}

/// Whether a navigation target is an external web URL (an `http(s)` host that is not
/// one of Tauri's `*.localhost` app/IPC origins).
fn is_external(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .map(|host| host != "localhost" && !host.ends_with(".localhost"))
            .unwrap_or(false)
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
    fn scrollbar_script_hides_native_and_draws_a_custom_thumb() {
        let js = scrollbar_init_script();
        assert!(js.contains("wh-cscroll-thumb"));
        assert!(js.contains("scrollbar-width:none"));
        assert!(js.contains("var(--wh-cscroll-thumb"));
    }

    #[test]
    fn configured_theme_is_dark() {
        assert!(Theme::configured().dark);
    }

    #[test]
    fn background_color_is_the_opaque_theme_background() {
        assert_eq!(
            Theme { dark: true }.background_color(),
            Color(0x1e, 0x1e, 0x1e, 255)
        );
        assert_eq!(
            Theme { dark: false }.background_color(),
            Color(0xff, 0xff, 0xff, 255)
        );
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
}
