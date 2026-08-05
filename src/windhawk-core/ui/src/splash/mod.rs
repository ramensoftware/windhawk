//! The startup splash: the Windhawk mark the main window shows while WebView2
//! comes up.
//!
//! The window itself is up early - it is built visible, at its remembered
//! geometry - but Tauri only returns from the build once the webview exists,
//! which takes the better part of a second, and the front-end then has to load
//! and paint. This module fills that gap INSIDE the main window: a child window
//! covering its client area, painted with the Windhawk mark on the front-end's
//! own background color, from before the window is first shown.
//!
//! The hand-over is what keeps the mark from blinking. A webview's output is
//! composited above the window's child windows whatever their z-order, so nothing
//! painted here can survive the webview appearing: it is held back
//! (`shell::set_webview_visible`) until the front-end has rendered
//! ([`ready_init_script`] reporting through [`wh_splash_ready`]), and the overlay
//! then stays until the page reports its first frame drawn with the webview
//! visible ([`wh_splash_presented`]). So the mark is on screen from the window's
//! first frame, and what replaces it is the app itself.
//!
//! The overlay owns a thread with its own message loop, since the main thread is
//! blocked inside the WebView2 creation for that whole stretch and cannot service
//! a paint. Everything here is best effort: if the overlay cannot be created the
//! window simply comes up with its themed background, as it did before.

mod logo;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EndPaint, HBITMAP, HDC, HGDIOBJ, InvalidateRect, PAINTSTRUCT, SRCCOPY, SelectObject,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    FillModeWinding, GdipAddPathBezier, GdipAddPathLine, GdipClosePathFigure, GdipCreateFromHDC,
    GdipCreatePath, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath,
    GdipFillPath, GdipFillRectangle, GdipSetSmoothingMode, GdipStartPathFigure, GdiplusShutdown,
    GdiplusStartup, GdiplusStartupInput, GdiplusStartupOutput, GpBrush, GpGraphics, GpPath,
    SmoothingModeAntiAlias, SmoothingModeNone,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CWPSTRUCT, CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GW_CHILD, GW_HWNDNEXT, GWL_STYLE, GetClientRect, GetMessageW, GetParent, GetWindow,
    GetWindowLongW, HHOOK, HWND_TOP, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetTimer, SetWindowLongW, SetWindowPos, SetWindowsHookExW, TranslateMessage,
    USER_DEFAULT_SCREEN_DPI, UnhookWindowsHookEx, WH_CALLWNDPROC, WM_APP, WM_CREATE, WM_DESTROY,
    WM_ERASEBKGND, WM_NCACTIVATE, WM_PAINT, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_NOPARENTNOTIFY, WS_VISIBLE,
};

use tauri::{AppHandle, Manager};

use crate::lifecycle::window;
use crate::shell;
use logo::{Logo, Segment};

/// The Windhawk mark, the same artwork the installer and the front-end header
/// use. Kept as the SVG rather than a bitmap so it stays crisp at any display
/// scale.
pub const LOGO_SVG: &str = include_str!("../../icons/main-icon-no-background.svg");

/// The overlay's own window class ([`window::SPLASH_WINDOW_CLASS`], which owns it
/// alongside the main window's). The overlay is a child of the main window, so it is
/// not a window of its own as far as the user (or the tray, which finds the UI by the
/// main window's class) is concerned - but a second instance does read it off the
/// window tree, since a main window still carrying it is one that has not finished
/// starting.
const CLASS_NAME: &str = window::SPLASH_WINDOW_CLASS;

/// Posted to the overlay to take it down ([`dismiss`]). A window can only be
/// destroyed from the thread that created it, and the overlay's thread is the one
/// pumping its messages.
const WM_SPLASH_CLOSE: u32 = WM_APP + 1;

/// The overlay's re-sync timer, a backstop under the message-driven sync.
///
/// Everything the overlay is known to have to answer for - its parent's style being
/// rewritten from under it, the webview's window being raised over it, the client
/// area changing size - reaches the main window's tree as a `WM_WINDOWPOSCHANGED`
/// that [`splash_hook`] answers as it happens, and the state the overlay is created
/// into is the same sync run outright ([`create_overlay`]). What the tick is left
/// with is what a per-thread hook cannot see: a window in the tree that belongs to
/// another thread, and a style rewritten with no `SetWindowPos` behind it. Neither is
/// known to happen - it is insurance against a webview runtime that does not behave
/// the way this one does, for a mark that is only up for about a second. Hence a
/// cadence measured against how long a wrong frame may stand, not against the frame
/// rate.
const FIT_TIMER_ID: usize = 1;
const FIT_INTERVAL_MS: u32 = 100;

/// The overlay's closing timer: a few frames between being told to go and going,
/// on top of the page's report that it has drawn. The webview is showing the same
/// mark by then, so the overlap cannot be seen, while closing in the same breath
/// as the report would ride on the frame the compositor is still presenting.
const CLOSE_TIMER_ID: usize = 2;
const CLOSE_DELAY_MS: u32 = 50;

/// How long the overlay thread waits to be handed the main window. It is created
/// within a few milliseconds of the build starting; the bound only keeps the
/// thread from waiting forever when it never is.
const PARENT_WAIT: Duration = Duration::from_secs(10);

/// The live overlay window, as a raw handle value (an `HWND` is a pointer and not
/// `Send`).
static OVERLAY: Mutex<Option<isize>> = Mutex::new(None);

/// The windows [`clip_siblings`] added `WS_CLIPSIBLINGS` to, as raw handle values,
/// to hand their styles back when the overlay goes.
static CLIPPED_SIBLINGS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Set once the webview has taken over the client area, so a [`dismiss`] that
/// beats the overlay's creation still takes it down.
static DISMISSED: AtomicBool = AtomicBool::new(false);

/// The theme the overlay paints in, published for the window procedure (which
/// runs on the overlay thread and gets no arguments of its own).
static SPLASH_DARK: AtomicBool = AtomicBool::new(true);

/// Start the overlay for the given theme. Must be called on the thread that is
/// about to build the main window, and just before it: the overlay attaches to
/// that window the moment it is created, which is well before the build returns.
///
/// The handle comes from a hook on this thread rather than by looking the window
/// up, so the overlay thread has nothing to wait for and nothing to guess: the
/// window's own `WM_CREATE`, which runs here, hands it over
/// ([`splash_hook`]).
pub fn show(dark: bool) {
    SPLASH_DARK.store(dark, Ordering::Release);

    let (sender, receiver) = mpsc::channel();
    *HANDOVER.lock().unwrap_or_else(|error| error.into_inner()) = Some(sender);

    // SAFETY: a hook on this thread only (its own id), with a plain fn of the
    // documented signature as the procedure. It is removed exactly once
    // ([`remove_splash_hook`]), when the overlay goes or when it turns out there will
    // not be one; a null return means it was not installed, where the overlay thread
    // times out and this startup simply has no splash.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROC,
            Some(splash_hook),
            std::ptr::null_mut(),
            GetCurrentThreadId(),
        )
    };
    SPLASH_HOOK.store(hook as isize, Ordering::Release);

    let _ = std::thread::Builder::new()
        .name("wh-ui-splash".to_owned())
        .spawn(move || run_overlay(&receiver));
}

/// The channel the creation hook hands the main window's handle over on. Taken by
/// the hook the first time it fires for the right window, which also closes it.
static HANDOVER: Mutex<Option<mpsc::Sender<isize>>> = Mutex::new(None);

/// The installed [`splash_hook`], removed when the splash hands over.
static SPLASH_HOOK: AtomicIsize = AtomicIsize::new(0);

/// Watches the main window's messages for as long as the splash is up: it hands
/// the window over when it is created, and colors its frame for each activation
/// change until the app takes that over.
///
/// `WM_CREATE` is the earliest point the window is usable: it has its class and
/// its client area (the frame has been measured), while `CreateWindowExW` has not
/// returned yet - so the overlay is attached and painted before the window is ever
/// shown, rather than a few frames into its life.
///
/// `WM_NCACTIVATE` is handled here rather than through a subclass because tao
/// answers that message with `DefWindowProcW`, which skips the rest of the
/// subclass chain: a subclass added this early would never see it. It is what
/// paints the frame active from the window's first frame: the window takes the
/// foreground as it is shown, which is before anything else is watching, and the
/// frame would otherwise keep the inactive colors it was seeded with until `run`
/// re-themes it half a second later.
///
/// `WM_WINDOWPOSCHANGED` says a window in the main window's tree has been moved,
/// sized or shown, which is every occasion the overlay has to answer for: a window
/// created under it without `WS_CLIPSIBLINGS` that would copy its pixels away
/// ([`clip_siblings`]), tao rewriting the parent's whole style - dropping the
/// `WS_CLIPCHILDREN` [`clip_children`] put there - which it always follows with a
/// `SWP_FRAMECHANGED` `SetWindowPos`, the webview's host window being raised over the
/// overlay, and the client area changing size. The message is SENT, so answering it
/// here puts the styles back before the frame that prompted it is drawn, rather than
/// leaving the mark flickering until a tick catches up.
unsafe extern "system" fn splash_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A negative code is not ours to inspect; the message must be passed straight on.
    if code >= 0 {
        // SAFETY: for a non-negative code the hook contract makes `lparam` a live
        // CWPSTRUCT for the message being delivered on this thread.
        let message = unsafe { &*(lparam as *const CWPSTRUCT) };
        match message.message {
            WM_CREATE if is_main_window(message.hwnd) => {
                if let Some(sender) = HANDOVER
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                {
                    let _ = sender.send(message.hwnd as isize);
                }
            }
            WM_NCACTIVATE if is_main_window(message.hwnd) => {
                shell::apply_frame_theme_to_hwnd(
                    message.hwnd,
                    SPLASH_DARK.load(Ordering::Acquire),
                    message.wParam != 0,
                );
            }
            WM_WINDOWPOSCHANGED if is_under_main_window(message.hwnd) => resync_overlay(),
            _ => {}
        }
    }

    // SAFETY: the arguments are the ones the hook procedure was handed, passed on
    // to the rest of the chain as the contract requires.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Whether a window is the main one or sits somewhere inside it - the webview's
/// host windows, which are children of a child of it.
fn is_under_main_window(hwnd: HWND) -> bool {
    let mut hwnd = hwnd;
    // The webview is two levels down; the bound only keeps a cycle in a window
    // tree being rearranged from spinning here.
    for _ in 0..8 {
        if hwnd.is_null() {
            return false;
        }
        if is_main_window(hwnd) {
            return true;
        }
        // SAFETY: `hwnd` is a live window (the hook's, then its ancestors); GetParent
        // only reads the window tree and returns null at the top of it.
        hwnd = unsafe { GetParent(hwnd) };
    }
    false
}

/// Put the overlay back over its parent and mark it for repainting, from the main
/// thread, which is where the change that called for it happened ([`splash_hook`]).
///
/// Running here rather than on the overlay's own thread is what makes the styles
/// stick: the parent and the webview's windows belong to this thread, so the
/// read-modify-write [`clip_children`] and [`clip_siblings`] perform cannot be
/// interleaved with tao's own rewrite of the same style.
///
/// The paint itself stays the overlay thread's: this only invalidates, and that
/// thread is idle and picks it up as soon as this one lets go.
fn resync_overlay() {
    let Some(hwnd) = overlay_window() else {
        return;
    };
    // SAFETY: `hwnd` is the overlay window, whose thread outlives this call - the
    // dismissal clears the handle before the window is destroyed. GetParent only reads
    // the window tree; a null parent means it is already on its way out. Erasing is
    // off, since the paint covers every pixel; the invalidation result is advisory.
    unsafe {
        let parent = GetParent(hwnd);
        if parent.is_null() {
            return;
        }
        sync_to_parent(parent, hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

/// Whether a window is the main one, by the class `run` gives it.
fn is_main_window(hwnd: HWND) -> bool {
    window::class_name_is(hwnd, window::MAIN_WINDOW_CLASS)
}

/// Take the hook back out, once the splash has handed the screen over.
fn remove_splash_hook() {
    let hook = SPLASH_HOOK.swap(0, Ordering::AcqRel);
    if hook != 0 {
        // SAFETY: `hook` is the handle `show` stored for the hook it installed, and
        // is removed exactly once (the swap hands it to a single caller).
        unsafe { UnhookWindowsHookEx(hook as HHOOK) };
    }
}

/// The front-end has rendered, so the webview has the app to show and can be
/// brought on screen. Reported from [`ready_init_script`] on the first content
/// under the front-end's root element - NOT on a painted frame: the webview is
/// still hidden at that point, and a hidden webview does not render, so a report
/// waiting on a paint would wait for the very thing it is meant to trigger.
#[tauri::command]
pub fn wh_splash_ready(app: AppHandle) {
    hand_off(&app);
}

/// Take the overlay down after [`DISMISS_FALLBACK`] whatever the page does, so a
/// document that never loads or paints cannot leave the mark up over a webview
/// the user could otherwise be looking at.
pub fn arm_dismiss_fallback(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("wh-ui-splash-fallback".to_owned())
        .spawn(move || {
            std::thread::sleep(DISMISS_FALLBACK);
            hand_off(&app);
        });
}

/// The page has drawn a frame with the webview visible, so the app is on screen
/// and the overlay in front of it can go. Reported from [`ready_init_script`].
#[tauri::command]
pub fn wh_splash_presented() {
    dismiss();
}

/// Give the screen back to the webview: show it (it is held back while the splash
/// is up, see `run`) and leave the overlay standing until the page reports that it
/// has drawn with it visible.
///
/// That report is what makes the swap safe. Showing the webview does not put
/// anything on screen by itself: it was held invisible while the page loaded - and
/// a hidden webview does not render - so its first frame still has to be drawn and
/// composited. Taking the overlay away on a timer instead would leave the window
/// bare for however long that takes. If the page never reports,
/// [`PRESENTED_FALLBACK`] takes the overlay down anyway.
fn hand_off(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        dismiss();
        return;
    };
    if HANDING_OFF.swap(true, Ordering::AcqRel) {
        return;
    }

    shell::set_webview_visible(&window, true);

    // The first moment the webview is both built and visible, which is what the
    // startup's one chance to focus it waits for: the window took the focus as it was
    // shown, and a webview held invisible cannot be given it. Only for a window the
    // user is actually in - a launch that did not get the foreground has no focus to
    // put anywhere, and wry moves it in on the WM_SETFOCUS that arrives when the user
    // turns to the window.
    if shell::is_active(&window) {
        shell::focus_webview(&window);
    }

    let _ = std::thread::Builder::new()
        .name("wh-ui-splash-presented-fallback".to_owned())
        .spawn(|| {
            std::thread::sleep(PRESENTED_FALLBACK);
            dismiss();
        });
}

/// How long the overlay stays up after the webview was shown without the page
/// reporting that it drew. Long enough to cover a compositor that takes its time,
/// short enough that a page which cannot report does not hold the mark in front of
/// a webview that is already showing the app.
const PRESENTED_FALLBACK: Duration = Duration::from_millis(400);

/// Whether the hand-off has run, so the webview is shown (and the page asked to
/// report) exactly once - the ready report and the fallback both land here.
static HANDING_OFF: AtomicBool = AtomicBool::new(false);

/// How long the overlay stays up without a report from the page at all. Well past
/// a normal startup (the front-end renders within a second of the window opening),
/// and short enough that a front-end which never renders still gives way to
/// whatever the webview has.
const DISMISS_FALLBACK: Duration = Duration::from_secs(5);

/// Take the overlay down: the webview is showing the app, so the mark has nothing
/// left to cover. Idempotent, and safe to call before the overlay is up.
///
/// This is also where the startup ends as far as anything watching it is concerned:
/// the app is on screen, so the watchdog has nothing left to watch and a second
/// instance can hand the foreground over. Both read the same moment - the watchdog
/// through [`window::mark_startup_handed_over`], a second instance by the overlay
/// leaving the main window's tree.
pub fn dismiss() {
    DISMISSED.store(true, Ordering::Release);
    window::mark_startup_handed_over();
    // The window's frame is `run`'s to keep from here (shell::track_activation).
    remove_splash_hook();
    // Taken, not read and then cleared: the page's report and both fallbacks can land
    // here at once, and a handle two of them each saw would be sent two closing
    // messages - the second of which could arrive after the window has gone.
    let handle = OVERLAY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(hwnd) = handle {
        // SAFETY: `hwnd` is the overlay window and the message is posted to the
        // thread that owns it, which destroys it in its own window procedure.
        unsafe { PostMessageW(hwnd as HWND, WM_SPLASH_CLOSE, 0, 0) };
    }
}

/// The initialization script the front-end reports its progress through. It draws
/// nothing: the mark on screen is the native overlay's, all the way until the app
/// is there to replace it.
///
/// Drawing it a second time in the page was the obvious way to cover the webview,
/// and the wrong one. The two are rasterized by different engines and centered
/// against slightly different rectangles - the page's viewport loses width to a
/// scrollbar the moment the app has content - so the mark moved and shimmered as
/// the screen changed hands. Keeping the webview hidden until the app itself is
/// ready means the hand-over is the only change the user sees.
///
/// The two reports are what drive it: the app's first rendered content brings the
/// webview on screen ([`wh_splash_ready`]), and the first frame drawn once it is
/// visible retires the overlay ([`wh_splash_presented`]). The second report waits
/// for the frame AFTER the one it is about, since an animation frame callback runs
/// before its frame is composited, and watches for the page becoming visible
/// itself rather than being told - a report the shell has to ask for would race
/// the listener the page registers to hear the question.
pub fn ready_init_script() -> &'static str {
    "(function(){\
       function invoke(command){\
         var api=window.__TAURI__;\
         if(!api){setTimeout(function(){invoke(command);},4);return;}\
         try{api.core.invoke(command);}catch(e){}\
       }\
       function presented(){invoke('wh_splash_presented');}\
       function reportWhenDrawn(){\
         function drawn(){requestAnimationFrame(function(){requestAnimationFrame(presented);});}\
         if(document.visibilityState==='visible'){drawn();return;}\
         document.addEventListener('visibilitychange',function once(){\
           if(document.visibilityState!=='visible'){return;}\
           document.removeEventListener('visibilitychange',once);\
           drawn();\
         });\
       }\
       function ready(){invoke('wh_splash_ready');reportWhenDrawn();}\
       function watch(){\
         var root=document.getElementById('root');\
         if(root&&root.children.length){ready();return;}\
         var observer=new MutationObserver(function(){\
           var root=document.getElementById('root');\
           if(root&&root.children.length){observer.disconnect();ready();}\
         });\
         observer.observe(document.documentElement,{childList:true,subtree:true});\
       }\
       if(document.documentElement){watch();}\
       else{document.addEventListener('DOMContentLoaded',watch);}\
     })();"
}

/// The live overlay handle, if any.
fn overlay_window() -> Option<HWND> {
    (*OVERLAY.lock().unwrap_or_else(|error| error.into_inner())).map(|hwnd| hwnd as HWND)
}

/// The overlay thread: attach to the main window, keep the mark painted, and go
/// away when the webview takes over.
fn run_overlay(handover: &mpsc::Receiver<isize>) {
    // GDI+ is what draws the mark; without it the overlay would have nothing to
    // paint, so there is no point putting it up. Started - and the mark parsed -
    // BEFORE the window arrives, so both are ready the moment it does and the
    // first paint is only a paint.
    let Some(token) = start_gdiplus() else {
        // Nothing will be painted, so nothing is left for the hook to answer: take it
        // back out here rather than leave it on the main thread until the dismissal
        // fallback gets to it.
        remove_splash_hook();
        return;
    };
    let _ = shipped_logo();

    // Blocks until the creation hook hands the window over. The bound is only a
    // safety valve for a startup that never gets that far (a window that fails to
    // build, a hook that could not be installed), where this thread has nothing
    // left to do.
    let Ok(parent) = handover.recv_timeout(PARENT_WAIT) else {
        remove_splash_hook();
        // SAFETY: `token` came from the GdiplusStartup just above, on this thread.
        unsafe { GdiplusShutdown(token) };
        return;
    };
    let parent = parent as HWND;

    // The window is on screen for the whole build, so its frame has to be themed
    // now rather than when the build returns - otherwise it wears a stock-light
    // title bar around the dark splash until then. This runs off the window's
    // WM_CREATE, before it has been shown, so the activation read here is a seed
    // rather than an answer: it is all but always inactive, and the WM_NCACTIVATE
    // from the show that follows is what corrects it (`splash_hook`). `run`
    // re-applies the frame itself (this path is best effort).
    let active = shell::is_foreground_hwnd(parent);
    shell::apply_frame_theme_to_hwnd(parent, SPLASH_DARK.load(Ordering::Acquire), active);

    match create_overlay(parent) {
        Some(hwnd) => {
            *OVERLAY.lock().unwrap_or_else(|error| error.into_inner()) = Some(hwnd as isize);
            // The dismissal may already have happened, in which case the overlay must
            // not linger over a webview that is already showing.
            if DISMISSED.load(Ordering::Acquire) {
                dismiss();
            }
            pump_messages();
        }
        // No overlay to keep on its parent, and the frame is themed above, so the hook
        // has nothing left to answer: `run` re-applies the frame and takes the
        // activation over from there.
        None => remove_splash_hook(),
    }

    // SAFETY: `token` came from the matching GdiplusStartup on this thread, and
    // every GDI+ object the painting created was deleted before returning.
    unsafe { GdiplusShutdown(token) };
}

/// Start GDI+ for this thread, returning its token.
fn start_gdiplus() -> Option<usize> {
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        DebugEventCallback: 0,
        SuppressBackgroundThread: 0,
        SuppressExternalCodecs: 0,
    };
    let mut token = 0usize;
    let mut output = GdiplusStartupOutput::default();
    // SAFETY: `input` is a fully initialized startup record, and `token`/`output`
    // are slots the call writes into. A non-zero status means GDI+ is unavailable,
    // where the token must not be used (and is not, below).
    let status = unsafe { GdiplusStartup(&mut token, &input, &mut output) };
    (status == 0).then_some(token)
}

/// Run the overlay's message loop until its window is gone.
fn pump_messages() {
    let mut message = MSG::default();
    loop {
        // SAFETY: `message` is a valid MSG slot; a null window filter takes every
        // message posted to this thread. GetMessageW returns 0 on WM_QUIT (posted
        // when the window is destroyed) and -1 on error, both of which end the loop.
        let received = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if received <= 0 {
            return;
        }
        // SAFETY: `message` was just filled by GetMessageW and is dispatched to
        // the window procedure it names.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Register the overlay class (once per process) and create the overlay filling
/// the main window's client area.
fn create_overlay(parent: HWND) -> Option<HWND> {
    let class = wide(CLASS_NAME);
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let registered = *REGISTERED.get_or_init(|| {
        // SAFETY: the class name and the window procedure outlive the process; the
        // remaining fields are the documented defaults (no background brush, since
        // the window procedure paints every pixel itself). A zero atom means the
        // class could not be registered, which is reported as-is.
        unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: GetModuleHandleW(std::ptr::null()),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&class) != 0
        }
    });
    if !registered {
        return None;
    }

    let size = client_size(parent);
    // SAFETY: the class is registered above, and `parent` is the main window (a
    // window of this process, which is what makes it a legal parent). No window
    // name, menu, or creation parameter is passed; the call returns NULL on
    // failure, which is checked. WS_EX_NOPARENTNOTIFY keeps the creation from
    // sending WM_PARENTNOTIFY to the parent: that window belongs to the main
    // thread, which is inside its own creation right now and not taking messages,
    // so the send would block this thread until it is.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOPARENTNOTIFY,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            size.0,
            size.1,
            parent,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }

    // The same sync every later change gets, so the overlay does not depend on one
    // arriving: the webview's host window is created before the overlay and already
    // needs `WS_CLIPSIBLINGS`, and the parent already needs `WS_CLIPCHILDREN`.
    sync_to_parent(parent, hwnd);

    // SAFETY: `hwnd` is the overlay window just created; the timer is killed when
    // it is destroyed. A null callback routes the ticks to the window procedure.
    unsafe { SetTimer(hwnd, FIT_TIMER_ID, FIT_INTERVAL_MS, None) };

    Some(hwnd)
}

/// Whether the main window had to be given `WS_CLIPCHILDREN`, so the overlay can
/// take it away again when it goes.
static CLIPPED_CHILDREN: AtomicBool = AtomicBool::new(false);

/// Keep the main window from painting over the overlay.
///
/// tao creates it without `WS_CLIPCHILDREN`, so its background fill covers the
/// whole client area - the overlay's rectangle included. Every fill therefore
/// erases the mark, and the overlay repaints it on the WM_PAINT that follows: a
/// flicker, once per fill, and the window fills repeatedly while the webview is
/// being set up. With the style on, the child's rectangle is excluded from the
/// parent's drawing and the mark simply stays.
fn clip_children(parent: HWND) {
    // SAFETY: `parent` is the live main window. Reading and writing its style is a
    // plain get/set; adding a bit leaves every other flag as it was. A failure just
    // leaves the flicker in place.
    unsafe {
        let style = GetWindowLongW(parent, GWL_STYLE);
        if style & WS_CLIPCHILDREN as i32 == 0 {
            SetWindowLongW(parent, GWL_STYLE, style | WS_CLIPCHILDREN as i32);
            CLIPPED_CHILDREN.store(true, Ordering::Release);
        }
    }
}

/// Give the main window its own style back when the overlay goes, so the window is
/// left exactly as tao made it.
fn unclip_children(parent: HWND) {
    if !CLIPPED_CHILDREN.swap(false, Ordering::AcqRel) {
        return;
    }
    // SAFETY: `parent` is the live main window; the bit removed is the one added in
    // `clip_children`, leaving every other flag as it was.
    unsafe {
        let style = GetWindowLongW(parent, GWL_STYLE);
        SetWindowLongW(parent, GWL_STYLE, style & !(WS_CLIPCHILDREN as i32));
    }
}

/// Keep the webview's windows from taking the overlay's pixels with them.
///
/// WebView2's host window is a sibling of the overlay, under it and over the same
/// client area, and it is created without `WS_CLIPSIBLINGS`. While the webview is
/// set up that window is moved, sized and shown, and each of those copies what is
/// on screen in its old rectangle - the overlay's mark, drawn on top of it - into
/// the new one, so the mark ends up displaced sideways with nothing telling the
/// overlay to draw it again. With the style on, the overlay's rectangle is outside
/// what the webview's windows may draw into or copy from, and the mark stays where
/// it was put.
///
/// Re-applied rather than applied once, since the window the style belongs on is
/// created after the overlay ([`sync_to_parent`]).
fn clip_siblings(parent: HWND, overlay: HWND) {
    // SAFETY: `parent` is the live main window. GetWindow only reads the window
    // tree - GW_CHILD gives the topmost child, GW_HWNDNEXT the next one down, and
    // null ends the walk - and reading and writing a style is a plain get/set that
    // leaves every other flag as it was. A failure just leaves the copying in place.
    unsafe {
        let mut child = GetWindow(parent, GW_CHILD);
        while !child.is_null() {
            if child != overlay {
                let style = GetWindowLongW(child, GWL_STYLE);
                if style & WS_CLIPSIBLINGS as i32 == 0 {
                    SetWindowLongW(child, GWL_STYLE, style | WS_CLIPSIBLINGS as i32);
                    // Recorded once per window: a style cleared and re-applied is
                    // still one style to hand back.
                    let mut clipped = CLIPPED_SIBLINGS
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if !clipped.contains(&(child as isize)) {
                        clipped.push(child as isize);
                    }
                }
            }
            child = GetWindow(child, GW_HWNDNEXT);
        }
    }
}

/// Give the windows [`clip_siblings`] styled their own styles back, so the webview
/// is left as wry made it. Only the ones still parented to the main window, since
/// a handle whose window has gone can name a different one by then.
fn unclip_siblings(parent: HWND) {
    let clipped: Vec<isize> = CLIPPED_SIBLINGS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .drain(..)
        .collect();
    for child in clipped {
        let child = child as HWND;
        // SAFETY: `child` is a handle the walk above returned; GetParent and the
        // style get/set only read or write the window it names, and all three are
        // harmless on a handle that no longer names one.
        unsafe {
            if GetParent(child) != parent {
                continue;
            }
            let style = GetWindowLongW(child, GWL_STYLE);
            SetWindowLongW(child, GWL_STYLE, style & !(WS_CLIPSIBLINGS as i32));
        }
    }
}

/// Put the overlay at the top of its siblings. The webview's own host window is
/// created before the overlay and covers the same client area, so the overlay has
/// to be raised over it - and kept there, since the webview is raised again as it
/// is set up.
///
/// Only when it is not already there: a z-order change invalidates the windows it
/// reorders, and this runs on every sync - once per `WM_WINDOWPOSCHANGED` in the
/// window's tree, which the webview raises a stream of as it starts - so asserting
/// it unconditionally would repaint the overlay (and the webview under it) along
/// with each one.
fn raise(hwnd: HWND) {
    // SAFETY: `hwnd` is the live overlay; GetParent/GetWindow only read the window
    // tree, and GW_CHILD returns the parent's topmost child.
    let already_top = unsafe {
        let parent = GetParent(hwnd);
        !parent.is_null() && GetWindow(parent, GW_CHILD) == hwnd
    };
    if already_top {
        return;
    }

    // SAFETY: `hwnd` is the live overlay. The flags make the position and size
    // arguments unused; the result is advisory.
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

/// A window's client size in physical pixels.
fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is a live window and `rect` is the slot the call fills in;
    // on failure it stays zeroed, which the caller treats as nothing to paint.
    unsafe { GetClientRect(hwnd, &mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// The overlay's window procedure.
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        // Every pixel is painted below, so erasing first would only flicker.
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_TIMER if wparam == CLOSE_TIMER_ID => {
            // SAFETY: `hwnd` is this thread's window, destroyed exactly once (the
            // dismissal clears the handle before posting, so no second closing
            // message can arrive).
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_TIMER => {
            fit_to_parent(hwnd);
            0
        }
        WM_SPLASH_CLOSE => {
            // SAFETY: `hwnd` is this thread's window; the timer fires once
            // (CLOSE_DELAY_MS) and is killed with the window.
            unsafe { SetTimer(hwnd, CLOSE_TIMER_ID, CLOSE_DELAY_MS, None) };
            0
        }
        WM_DESTROY => {
            // SAFETY: `hwnd` is this thread's window, still parented while it is
            // being destroyed, so GetParent gives the main window whose own and
            // whose children's styles are handed back; the timers are the ones set
            // on it, and the quit message ends this thread's own loop.
            unsafe {
                let parent = GetParent(hwnd);
                unclip_children(parent);
                unclip_siblings(parent);
                KillTimer(hwnd, FIT_TIMER_ID);
                KillTimer(hwnd, CLOSE_TIMER_ID);
                PostQuitMessage(0);
            }
            0
        }
        // SAFETY: the arguments are the ones the window procedure was handed.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// [`sync_to_parent`] for a caller that only has the overlay - the tick and the
/// paint, both on the overlay's own thread.
fn fit_to_parent(hwnd: HWND) {
    // SAFETY: `hwnd` is the live overlay; GetParent only reads it and returns the
    // main window, which outlives the overlay.
    let parent = unsafe { GetParent(hwnd) };
    if !parent.is_null() {
        sync_to_parent(parent, hwnd);
    }
}

/// Keep the overlay on top of its siblings and covering all of the main window's
/// client area, and keep the two windows styled so neither draws over it.
///
/// Called from both threads: from the main one as the change happens
/// ([`resync_overlay`]), and from the overlay's own tick as a backstop
/// ([`FIT_INTERVAL_MS`]). Safe from either. The window-tree reads and the style
/// writes send nothing; the two `SetWindowPos` calls (the raise and the resize) do,
/// but both target the overlay, and the overlay's thread only ever waits on its own
/// message queue - it never blocks on the main thread - so neither can wait on the
/// other.
fn sync_to_parent(parent: HWND, overlay: HWND) {
    // Keep the parent clipping its children: tao rewrites the window's style from
    // its own flags as it finishes creating it (and on every state change after),
    // which drops the bit the overlay put there.
    clip_children(parent);
    // Keep the webview's own windows off the overlay's pixels.
    clip_siblings(parent, overlay);
    // Stay over the webview's host window, which is raised as the webview is set
    // up, and keep covering all of the parent's client area (the window can still
    // be sized while the overlay is up, and a child does not hear that).
    raise(overlay);
    let wanted = client_size(parent);
    if wanted == client_size(overlay) {
        return;
    }

    // SAFETY: `overlay` is the live overlay window. The flags make the z-order and
    // activation arguments unused; the invalidation redraws the mark centered in
    // the new size. Both results are advisory.
    unsafe {
        SetWindowPos(
            overlay,
            std::ptr::null_mut(),
            0,
            0,
            wanted.0,
            wanted.1,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        InvalidateRect(overlay, std::ptr::null(), 0);
    }
}

/// Paint the overlay: the theme's background, with the Windhawk mark centered on
/// it. Both colors are the front-end's own tokens, so what the user is looking at
/// is the surface the web UI hands back to.
///
/// The whole client area comes from [`Surface`] in one blit: the mark is filled
/// over the background it is blended with, so painting straight into the window
/// would show the bare background for the moment between the two - a flicker on
/// every repaint.
fn paint(hwnd: HWND) {
    let dark = SPLASH_DARK.load(Ordering::Acquire);

    // The main window can still be resized under the overlay - it is given its
    // maximized state moments after it is created, which is around when the overlay
    // attaches - so every paint starts by covering whatever the client area is now.
    fit_to_parent(hwnd);

    let mut paint_struct = PAINTSTRUCT::default();
    // SAFETY: `hwnd` is the live overlay window and `paint_struct` is a valid slot
    // the call fills in. The device context is released by EndPaint below.
    let hdc = unsafe { BeginPaint(hwnd, &mut paint_struct) };

    let (width, height) = client_size(hwnd);
    let logo_box = logo_box(hwnd);
    if width > 0 && height > 0 {
        SURFACE.with_borrow_mut(|slot| {
            let drawn = slot
                .as_ref()
                .is_some_and(|surface| surface.matches(width, height, dark, logo_box));
            if !drawn {
                // SAFETY: `hdc` is the device context BeginPaint returned, which the
                // new surface is made compatible with; the old one, if any, is
                // dropped (and its objects released) as it is replaced.
                *slot = unsafe { Surface::draw(hdc, width, height, dark, logo_box) };
            }
            if let Some(surface) = slot.as_ref() {
                // SAFETY: `hdc` and the surface's own context are both live, and the
                // surface holds a bitmap of exactly this size. The result is
                // advisory.
                unsafe { BitBlt(hdc, 0, 0, width, height, surface.memory, 0, 0, SRCCOPY) };
            }
        });
    }

    // SAFETY: `paint_struct` is the record BeginPaint filled in for `hwnd`.
    unsafe { EndPaint(hwnd, &paint_struct) };
}

thread_local! {
    /// The overlay's drawn client area, kept for as long as the overlay thread is.
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
}

/// The mark on its background, drawn into an off-screen bitmap and kept.
///
/// The overlay is repainted whenever something in the window has moved over it
/// ([`splash_hook`]), which the webview does repeatedly as it starts, and the mark
/// is the same picture each time. It is rendered again only when what it shows
/// would differ - the window was resized, the theme changed, or the mark is being
/// fitted into a different square (the window moved to a display at another scale) -
/// and blitted otherwise.
struct Surface {
    memory: HDC,
    bitmap: HBITMAP,
    /// The bitmap the memory context had before this one, to select back into it
    /// before the context is deleted, as the API requires.
    previous: HGDIOBJ,
    width: i32,
    height: i32,
    dark: bool,
    logo_box: f32,
}

impl Surface {
    /// Draw the splash into a bitmap compatible with `hdc`. `None` if GDI would not
    /// give out the context or the bitmap, which leaves the overlay unpainted.
    ///
    /// SAFETY: the caller must pass a live device context.
    unsafe fn draw(
        hdc: HDC,
        width: i32,
        height: i32,
        dark: bool,
        logo_box: f32,
    ) -> Option<Surface> {
        // SAFETY: forwarded from this fn's contract; the context and bitmap are
        // created from `hdc`, so they are compatible, and both are released by the
        // Drop below - or here, if only one of the two was handed out.
        unsafe {
            let memory = CreateCompatibleDC(hdc);
            let bitmap = CreateCompatibleBitmap(hdc, width, height);
            if memory.is_null() || bitmap.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !memory.is_null() {
                    DeleteDC(memory);
                }
                return None;
            }
            let previous = SelectObject(memory, bitmap);
            draw(memory, width as f32, height as f32, dark, logo_box);
            Some(Surface {
                memory,
                bitmap,
                previous,
                width,
                height,
                dark,
                logo_box,
            })
        }
    }

    /// Whether this surface still shows what a paint now would draw.
    fn matches(&self, width: i32, height: i32, dark: bool, logo_box: f32) -> bool {
        self.width == width
            && self.height == height
            && self.dark == dark
            && self.logo_box == logo_box
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the objects are this surface's own, released exactly once, with
        // the bitmap selected out of the context before either goes.
        unsafe {
            SelectObject(self.memory, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.memory);
        }
    }
}

/// The square the mark is fitted into, in this window's physical pixels - the
/// logical [`logo::LOGO_BOX`] scaled for the display it is on, so the mark is the
/// same apparent size at any display scale.
fn logo_box(hwnd: HWND) -> f32 {
    // SAFETY: `hwnd` is the live overlay window; GetDpiForWindow only reads it and
    // returns 0 for an invalid one, where the 96-DPI baseline applies.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi == 0 {
        USER_DEFAULT_SCREEN_DPI
    } else {
        dpi
    };
    logo::LOGO_BOX * dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32
}

/// Draw the themed background and the centered mark into a device context.
fn draw(hdc: HDC, width: f32, height: f32, dark: bool, logo_box: f32) {
    let mut graphics: *mut GpGraphics = std::ptr::null_mut();
    // SAFETY: `hdc` is the device context BeginPaint returned, and `graphics` is
    // the slot the call writes the new object into (left null on failure).
    unsafe { GdipCreateFromHDC(hdc, &mut graphics) };
    if graphics.is_null() {
        return;
    }

    // SAFETY: `graphics` is the object just created. The background fill covers the
    // whole client area and is pixel-aligned, so it is drawn WITHOUT antialiasing -
    // antialiasing would give its top and left edges half coverage and blend them
    // with the empty bitmap underneath, drawing a darker line down two sides of the
    // window. The mark, whose curves do need it, is drawn with it on.
    unsafe {
        GdipSetSmoothingMode(graphics, SmoothingModeNone);
        fill_rectangle(graphics, background_color(dark), width, height);
        if let Some(logo) = shipped_logo() {
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            fill_logo(graphics, logo, logo_color(dark), width, height, logo_box);
        }
        GdipDeleteGraphics(graphics);
    }
}

/// Fill the whole surface with one color.
///
/// SAFETY: the caller must pass a live `graphics` object.
unsafe fn fill_rectangle(graphics: *mut GpGraphics, color: u32, width: f32, height: f32) {
    let mut brush = std::ptr::null_mut();
    // SAFETY: forwarded from this fn's contract; `brush` is the slot the call
    // fills in (left null on failure) and is deleted once, below.
    unsafe {
        GdipCreateSolidFill(color, &mut brush);
        if !brush.is_null() {
            GdipFillRectangle(graphics, brush.cast::<GpBrush>(), 0.0, 0.0, width, height);
            GdipDeleteBrush(brush.cast::<GpBrush>());
        }
    }
}

/// Fill the mark, scaled and centered in a surface of `width` x `height`.
///
/// SAFETY: the caller must pass a live `graphics` object.
unsafe fn fill_logo(
    graphics: *mut GpGraphics,
    logo: &Logo,
    color: u32,
    width: f32,
    height: f32,
    logo_box: f32,
) {
    let placement = logo.placement(width, height, logo_box);
    let map = |point: logo::Point| {
        (
            point.x * placement.scale + placement.dx,
            point.y * placement.scale + placement.dy,
        )
    };

    let mut path: *mut GpPath = std::ptr::null_mut();
    // SAFETY: `path` is the slot the call fills in (left null on failure). The
    // winding fill mode is SVG's own default rule, so the mark's inner shapes read
    // as the artwork draws them.
    unsafe { GdipCreatePath(FillModeWinding, &mut path) };
    if path.is_null() {
        return;
    }

    // GDI+ takes every segment's start point explicitly, so the outline's current
    // point (and each subpath's start, which a close returns to) is tracked here as
    // the segments are added.
    let mut current = (0.0, 0.0);
    let mut subpath_start = current;
    for segment in logo.segments() {
        // SAFETY: `path` is the object just created; every call only appends to it,
        // and the coordinates are finite (they come from the parsed asset scaled by
        // a finite placement).
        unsafe {
            match *segment {
                Segment::Move(point) => {
                    GdipStartPathFigure(path);
                    current = map(point);
                    subpath_start = current;
                }
                Segment::Line(point) => {
                    let end = map(point);
                    GdipAddPathLine(path, current.0, current.1, end.0, end.1);
                    current = end;
                }
                Segment::Cubic(control1, control2, point) => {
                    let (control1, control2, end) = (map(control1), map(control2), map(point));
                    GdipAddPathBezier(
                        path, current.0, current.1, control1.0, control1.1, control2.0, control2.1,
                        end.0, end.1,
                    );
                    current = end;
                }
                Segment::Close => {
                    GdipClosePathFigure(path);
                    current = subpath_start;
                }
            }
        }
    }

    let mut brush = std::ptr::null_mut();
    // SAFETY: forwarded from this fn's contract for `graphics`; `path` is the
    // outline just built and `brush` is the slot the call fills in (left null on
    // failure). Both objects are deleted once, here.
    unsafe {
        GdipCreateSolidFill(color, &mut brush);
        if !brush.is_null() {
            GdipFillPath(graphics, brush.cast::<GpBrush>(), path);
            GdipDeleteBrush(brush.cast::<GpBrush>());
        }
        GdipDeletePath(path);
    }
}

/// The parsed mark, read from the asset once.
fn shipped_logo() -> Option<&'static Logo> {
    static LOGO: OnceLock<Option<Logo>> = OnceLock::new();
    LOGO.get_or_init(|| Logo::parse(LOGO_SVG)).as_ref()
}

/// The theme's page background, as a GDI+ `0xAARRGGBB` color: the same value the
/// window and the webview carry ([`shell::page_background`]), so the splash is the
/// surface the web UI hands back to rather than a shade of its own.
fn background_color(dark: bool) -> u32 {
    let (red, green, blue) = shell::page_background(dark);
    0xff00_0000 | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}

/// How opaque the mark is drawn: the splash is a place-holder for the app about to
/// replace it, so the mark sits back into the background rather than reading as
/// content.
const LOGO_ALPHA: u32 = 0x33; // 20%

/// The theme's mark color: the front-end's `--whui-logo-color` (white on the dark
/// background, near-black on the light one) at [`LOGO_ALPHA`], as a GDI+
/// `0xAARRGGBB` color.
fn logo_color(dark: bool) -> u32 {
    let rgb = if dark { 0x00ff_ffff } else { 0x0000_0000 };
    (LOGO_ALPHA << 24) | rgb
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_the_front_ends_own_tokens() {
        // --whui-background-color, opaque; the mark is --whui-logo-color held back
        // to LOGO_ALPHA so it reads as a place-holder rather than as content. The
        // alpha is a tunable, so it is carried in rather than spelled out - what is
        // fixed is which color the mark takes in each theme.
        assert_eq!(background_color(true), 0xff1e_1e1e);
        assert_eq!(background_color(false), 0xfff5_f5f5);
        assert_eq!(logo_color(true), (LOGO_ALPHA << 24) | 0x00ff_ffff);
        assert_eq!(logo_color(false), LOGO_ALPHA << 24);
    }

    // The script is the whole hand-off: it draws nothing, and only reports. The
    // front-end's first rendered content brings the webview on screen, and the
    // first frame drawn once it is visible retires the overlay in front of it.
    #[test]
    fn the_page_reports_both_halves_of_the_hand_off_and_draws_nothing() {
        let script = ready_init_script();
        assert!(script.contains("MutationObserver"));
        assert!(script.contains("getElementById('root')"));
        assert!(script.contains("invoke('wh_splash_ready')"));
        assert!(script.contains("visibilitychange"));
        assert!(script.contains("requestAnimationFrame"));
        assert!(script.contains("invoke('wh_splash_presented')"));
        // Nothing is injected into the page: the mark on screen is the overlay's.
        assert!(!script.contains("<svg"));
        assert!(!script.contains("insertAdjacentHTML"));
    }
}
