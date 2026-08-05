//! The launcher contract Rust side and the fatal startup presentation. The
//! single-instance plugin (registered in `lib.rs`) makes a bare re-launch
//! ensure-running-and-foreground; this module holds the small Win32 + window
//! helpers it drives: the process AppUserModelID, the foreground hand-off, the
//! bring-to-front, the `Local\WindhawkUI` mutex the UI reads to spot a second
//! instance, the placement and activation of the main window as it is created and
//! the state it is first seen in,
//! and the native task dialog for a fatal failure (there is no webview to show it
//! in), whose expander carries the diagnostics `lifecycle::diagnostics` collected.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, RECT, S_OK,
    WAIT_OBJECT_0, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
};
use windows_sys::Win32::Security::{
    AllocateAndInitializeSid, CheckTokenMembership, FreeSid, PSID, SECURITY_NT_AUTHORITY,
};
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentProcess, GetCurrentThreadId, OpenEventW, OpenProcess,
    ResetEvent, SetEvent, TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TD_ERROR_ICON, TD_WARNING_ICON,
    TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER, TDF_EXPAND_FOOTER_AREA,
    TDF_SIZE_TO_CONTENT, TDM_CLICK_BUTTON, TDN_CREATED, TDN_TIMER, TaskDialogIndirect,
};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetCurrentProcessExplicitAppUserModelID,
    SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, CWPSTRUCT, CallNextHookEx, FindWindowExW, FindWindowW,
    GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetForegroundWindow, GetMenu, GetShellWindow,
    GetWindowLongW, GetWindowRect, HHOOK, HWND_TOPMOST, IDCANCEL, IsWindowVisible, MB_ICONERROR,
    MB_OK, MB_SYSTEMMODAL, MessageBoxW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowsHookExW,
    USER_DEFAULT_SCREEN_DPI, UnhookWindowsHookEx, WH_CALLWNDPROC, WINDOWPOS, WM_CREATE,
    WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING,
};
use windows_sys::core::HRESULT;

use crate::shell;

/// The explicit AppUserModelID for this process. Windows keys taskbar-button grouping
/// (and jump lists) off this identity; setting it explicitly makes the UI's window
/// group under a stable Windhawk identity rather than one derived from the executable
/// path. Matches the C++ launcher's SetCurrentProcessExplicitAppUserModelID argument.
const APP_USER_MODEL_ID: &str = "RamenSoftware.Windhawk";

/// Set this process's explicit AppUserModelID (`set_app_user_model_id`) so the taskbar
/// groups the UI window under a stable Windhawk identity. Called once at startup before
/// any window exists. Best effort: on failure the taskbar falls back to its default
/// path-derived grouping.
pub fn set_app_user_model_id() {
    let app_id = wide(APP_USER_MODEL_ID);
    // SAFETY: app_id is a NUL-terminated wide string that outlives the call, and the
    // API copies it. The returned HRESULT is advisory (best effort) and unused.
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

/// Whether this process is running with administrator rights: a check of the
/// current token against the built-in Administrators alias (S-1-5-32-544), the
/// same membership test `IsUserAnAdmin` performs.
///
/// A SID that cannot be built or checked reports false, which is the safe
/// direction to be wrong in: it claims fewer rights than the process may hold,
/// where the other answer claims rights it may not have and is found out later,
/// somewhere with less to say about it.
pub fn is_running_as_admin() -> bool {
    // Sub-authorities of the built-in Administrators alias SID S-1-5-32-544:
    // SECURITY_BUILTIN_DOMAIN_RID (32) then DOMAIN_ALIAS_RID_ADMINS (544). Defined
    // here rather than pulling the windows-sys Win32_System_SystemServices feature
    // in for two well-known constants.
    const SECURITY_BUILTIN_DOMAIN_RID: u32 = 32;
    const DOMAIN_ALIAS_RID_ADMINS: u32 = 544;

    let nt_authority = SECURITY_NT_AUTHORITY;
    let mut admins_sid: PSID = std::ptr::null_mut();
    // SAFETY: nt_authority outlives the call; on success admins_sid receives an
    // allocated SID freed below. Two sub-authorities are supplied (BUILTIN then
    // ADMINS), matching the count of 2, with the remaining six passed as 0.
    let allocated = unsafe {
        AllocateAndInitializeSid(
            &nt_authority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID,
            DOMAIN_ALIAS_RID_ADMINS,
            0,
            0,
            0,
            0,
            0,
            0,
            &mut admins_sid,
        )
    };
    if allocated == 0 {
        return false;
    }

    let mut is_member = 0;
    // SAFETY: admins_sid is the SID just allocated; a null token handle checks the
    // calling thread's effective token (the process token when not impersonating).
    // is_member receives the BOOL result.
    let ok = unsafe { CheckTokenMembership(std::ptr::null_mut(), admins_sid, &mut is_member) };

    // SAFETY: admins_sid was allocated by AllocateAndInitializeSid above and is
    // freed exactly once here.
    unsafe { FreeSid(admins_sid) };

    ok != 0 && is_member != 0
}

/// The named mutex the UI holds for its lifetime so it can tell at startup
/// whether another instance already exists (`already_existed`), which drives
/// the foreground hand-off. `Local\` (session) scope: one UI per session. The
/// tray does NOT probe it (it detects the UI by its window class); if a
/// tray-side probe is ever added, a permissive cross-integrity DACL becomes
/// relevant (the same refinement the DBWIN capture defers). Only the object's
/// existence matters.
const DETECT_MUTEX_NAME: &str = r"Local\WindhawkUI";

/// A held detect-running mutex; `Drop` closes the handle. Held for the process
/// lifetime so the named object exists exactly while the UI runs. Lives on the
/// thread that runs the app (it never crosses threads). Also records whether the
/// named object already existed when we created it, which tells a starting process
/// that a UI is already running (`another_instance_running`).
pub struct DetectMutex {
    handle: HANDLE,
    already_existed: bool,
}

impl DetectMutex {
    /// Whether a UI was already running when we created the detect mutex - i.e. this
    /// process is a second instance the single-instance plugin will forward and exit.
    /// Read at startup to decide whether to grant the foreground hand-off.
    pub fn another_instance_running(&self) -> bool {
        self.already_existed
    }
}

impl Drop for DetectMutex {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: handle was created by CreateMutexW in `hold_detect_mutex` and
            // is closed exactly once here.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

/// Create and hold the detect-running mutex. Best effort: a failure just means the
/// tray cannot detect via the mutex (it can still launch the exe). Not acquired as
/// an owner - only its existence matters.
pub fn hold_detect_mutex() -> DetectMutex {
    let name = wide(DETECT_MUTEX_NAME);
    // SAFETY: null attributes, no initial owner (0), NUL-terminated name. The
    // returned handle (NULL on failure) is held and closed on Drop.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    // SAFETY: GetLastError reads this thread's last-error code, which CreateMutexW
    // above just set; ERROR_ALREADY_EXISTS means a running instance already created
    // the named object. CreateMutexW still returns a valid handle in that case, so
    // reading it here (before any other call clobbers the code) is correct.
    let already_existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    DetectMutex {
        handle,
        already_existed,
    }
}

/// Grant the foreground right to any process, so this second instance hands it to the
/// running primary. Called at startup once the detect mutex shows a UI is already
/// running (`another_instance_running`): the single-instance plugin then forwards our
/// argv to the primary and exits, and the primary's re-launch callback calls
/// `show_and_focus_main`. That `SetForegroundWindow` only succeeds if a
/// foreground-capable process granted the primary permission first - the background
/// primary cannot grant it to itself, so this freshly launched (foreground-eligible)
/// instance does it here before the forward. Without it the primary's window only
/// flashes its taskbar button.
pub fn allow_foreground_handoff() {
    allow_foreground_for(None);
}

/// Let `pid` - or any process, when the caller has no id to name - put a window in
/// the foreground on this process's behalf.
///
/// The other caller is the editor launch. VSCodium is spawned by the broker, which
/// has never received input and owns no window, so Windows gives its child no
/// reason to take the foreground; this window is the one the user just clicked in,
/// so it is the one that can grant it.
pub fn allow_foreground_for(pid: Option<u32>) {
    // SAFETY: AllowSetForegroundWindow just takes a process id (ASFW_ANY = any
    // process) and has no other preconditions; the BOOL result (whether a grant was
    // recorded) is advisory and unused.
    unsafe {
        AllowSetForegroundWindow(pid.unwrap_or(ASFW_ANY));
    }
}

/// The Win32 class name of the main UI window, fixed on the builder when the window
/// is created (`run`). The tray/launcher locates the window by this class, and a
/// second instance checks it ([`main_window_ready`]) to confirm the running primary
/// actually has a window before handing off. Must match the class the launcher's
/// `FindWindow` uses.
pub const MAIN_WINDOW_CLASS: &str = "WindhawkTauriMainUI";

/// The Win32 class name of the startup splash (`splash`), a child of the main window
/// for as long as the startup lasts. It sits here beside [`MAIN_WINDOW_CLASS`] because
/// it is the other window identity this app is recognized by from outside: a second
/// instance reads it off the primary's window tree to tell a startup still in progress
/// from a finished one ([`main_window_ready`]).
pub const SPLASH_WINDOW_CLASS: &str = "WindhawkTauriStartupLogo";

/// Whether a window's class is `name`, compared as UTF-16 rather than through a
/// `String`. Both callers are message hooks, which see every message sent to any of
/// the main thread's windows, so this does no allocating.
pub fn class_name_is(hwnd: HWND, name: &str) -> bool {
    let mut class = [0u16; 64];
    // SAFETY: `hwnd` is the live window the hook is reporting on; the length is the
    // buffer's own, and the call writes at most that many characters, returning the
    // count written (0 on failure).
    let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    length > 0
        && class[..length as usize]
            .iter()
            .copied()
            .eq(name.encode_utf16())
}

/// Where the main window is to be put, in the pixels the displays are laid out in:
/// where its frame starts, and how big its client area is
/// (`window_state::OpeningGeometry`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: PhysicalPosition<i32>,
    pub inner_size: PhysicalSize<u32>,
}

/// The placement the next main window is to be given, taken by the `WM_CREATE` that
/// applies it, and the hook watching for it.
static PENDING_PLACEMENT: Mutex<Option<Placement>> = Mutex::new(None);
static CREATION_HOOK: AtomicIsize = AtomicIsize::new(0);

/// Whether the window being built opens maximized, which is what puts the subclass
/// that holds its first appearance back ([`hold_the_window_back_until_maximized`]) on it.
static PENDING_MAXIMIZED: AtomicBool = AtomicBool::new(false);

/// The window that subclass is on, so it can be taken off the same one, and zero when
/// there is none - a launch that does not open maximized, or one whose subclass would
/// not install.
static HELD_BACK_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// How many shows that subclass has held back, against [`SHOWS_TO_HOLD_BACK`].
static SHOWS_HELD_BACK: AtomicU32 = AtomicU32::new(0);

/// How many shows may be held back before they are simply let through.
///
/// Two, which is how many a maximized launch makes and undoes on its way up. It is a
/// ceiling rather than a count of a known sequence: what is being held back is a show
/// that is about to be taken back, and being wrong about that costs a window nobody
/// ever shows - so past the shows there is reason to expect, the window goes up.
const SHOWS_TO_HOLD_BACK: u32 = 2;

/// The id [`hold_the_window_back_until_maximized`] registers its subclass under, which
/// only has to be unique among the subclasses this module puts on a window.
const SHOW_SUBCLASS_ID: usize = 1;

/// Whether the main window has been seen as the foreground window, which is where
/// [`take_foreground`] stops asking for it.
static FOREGROUND_TAKEN: AtomicBool = AtomicBool::new(false);

/// Watch the main window's creation to give it the four things the builder cannot: its
/// exact rectangle (`placement`, when there is one to hold it to), the icons it is shown
/// with, the activation that puts the launch in front of the user, and - for a launch
/// that opens `maximized` - a first appearance that is already maximized.
///
/// Must be called on the thread that is about to build it, and just before - after
/// `splash::show`, whose hook watches for the same `WM_CREATE`, so that this one (the
/// more recently installed, which Windows calls first) has moved the window before the
/// splash takes its size from it. [`finish_main_window_creation`] takes it back out
/// once the build returns.
///
/// # The rectangle
///
/// The window builder only takes LOGICAL coordinates, which tao resolves to a display
/// by a search of its own - the first display on which the position, scaled by THAT
/// display's factor, lands. Where two displays run at different scales that can be a
/// different display than the position was derived from, and it can be no display at
/// all (a window remembered half off an edge), which falls back to the default cascade
/// position. Either way the window opens somewhere it was not meant to.
///
/// A rectangle in physical pixels has no such ambiguity, and `WM_CREATE` is where it
/// can still be applied for free: tao creates the window WITHOUT `WS_VISIBLE` and only
/// shows it at the end of the build, after reading back the DPI of the display the
/// window ended up on and after maximizing it. So the move lands before anyone can see
/// it, before tao caches a scale factor, and before a maximize that would otherwise
/// claim the wrong display - and what is finally shown is the remembered rectangle,
/// with nothing to correct afterwards.
///
/// # The icons
///
/// tao gives the window one icon of its own, the 256x256 image Tauri decodes out of
/// `icon.ico`, which Windows then squeezes into the caption's ~16px slot
/// (`shell::apply_window_icons_to`). That set happens after `WM_CREATE` and before the
/// show, so the show is the first point where crisper ones stick - and it is still
/// ahead of the frame and the taskbar button that would otherwise have carried tao's.
/// `WM_WINDOWPOSCHANGING` with `SWP_SHOWWINDOW` is that point: the hook sees it before
/// the window procedure makes the window visible.
///
/// # The activation
///
/// The window is built unfocused, which is what keeps wry's `MoveFocus` off the webview
/// build (`run`), and tao reads that as a window to show without activating
/// (`SW_SHOWNOACTIVATE`). The activation a launch is expected to carry is asked for
/// here instead, on the show itself, so the app is in front of the user for the whole
/// of the WebView2 build rather than from half a second after it.
///
/// # The maximized state
///
/// A window the builder is asked to open maximized reaches the screen three times, and
/// the first two are taken back. tao maximizes it and makes it visible in two separate
/// flag updates, and the maximize comes first: `SW_MAXIMIZE` on a window whose flags do
/// not carry `VISIBLE` yet, which SHOWS it - every `ShowWindow` command but `SW_HIDE`
/// does - followed immediately by the `SW_HIDE` that its own flags then ask for. The
/// update behind it shows the window with `SW_SHOWNOACTIVATE`, which carries
/// `SW_SHOWNORMAL`'s meaning of "in its most recent size and position", so it RESTORES
/// the window it is showing, and only then maximizes it again.
///
/// What the user is shown is the window flashing up maximized, going away, and coming
/// back at its restored size to be animated up to fill the screen - with each of the
/// first two appearances carrying the start of the window-open animation the one after
/// it abandons.
///
/// So the shows that lead up to the last one are shows that are about to be undone, and
/// a window that opens maximized is subclassed as it is created to hold them back
/// ([`hold_the_window_back_until_maximized`]). What is left is the maximize at the end,
/// which is then the window's first appearance and its only one.
pub fn prepare_main_window_creation(placement: Option<Placement>, maximized: bool) {
    *PENDING_PLACEMENT
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = placement;
    PENDING_MAXIMIZED.store(maximized, Ordering::Release);
    HELD_BACK_WINDOW.store(0, Ordering::Release);
    SHOWS_HELD_BACK.store(0, Ordering::Release);
    FOREGROUND_TAKEN.store(false, Ordering::Release);

    // SAFETY: a hook on this thread only (its own id), with a plain fn of the
    // documented signature as the procedure. It is removed exactly once, by
    // `finish_main_window_creation`; a null return means it was not installed, where
    // the window simply opens where the builder put it and unactivated.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_CALLWNDPROC,
            Some(main_window_creation_hook),
            std::ptr::null_mut(),
            GetCurrentThreadId(),
        )
    };
    CREATION_HOOK.store(hook as isize, Ordering::Release);
}

/// Take the creation hook back out, once the window it watches for has been built and
/// there is nothing left for it to catch. Idempotent, and a no-op for a hook that was
/// never installed.
pub fn finish_main_window_creation() {
    let hook = CREATION_HOOK.swap(0, Ordering::AcqRel);
    if hook != 0 {
        // SAFETY: `hook` is the handle `prepare_main_window_creation` stored for the
        // hook it installed, and is removed exactly once (the swap hands it to a single
        // caller).
        unsafe { UnhookWindowsHookEx(hook as HHOOK) };
    }
    PENDING_MAXIMIZED.store(false, Ordering::Release);
    let held_back = HELD_BACK_WINDOW.swap(0, Ordering::AcqRel) as HWND;
    if !held_back.is_null() {
        // SAFETY: `held_back` is the window `hold_the_window_back_until_maximized`
        // subclassed, under the same procedure and id it registered, and the swap hands
        // it to a single caller so it is removed exactly once. A window already gone
        // took its subclass with it, which the call reports and nothing acts on.
        unsafe { RemoveWindowSubclass(held_back, Some(maximized_show_subclass), SHOW_SUBCLASS_ID) };
        show_a_window_left_held_back(held_back);
    }
}

/// The answer for a maximized launch whose held-back show ([`hold_the_show_back`]) was
/// never followed by the maximizing one: the build is over, so a window still off the
/// screen is one that nothing else is going to show. Put it up as it stands - the state
/// it is in is the state it was built for.
///
/// A no-op for the window that is up, which is every launch that went as it should.
fn show_a_window_left_held_back(hwnd: HWND) {
    // SAFETY: `hwnd` is the window that was subclassed as it was created. IsWindowVisible
    // only reads the window's style, and the NOMOVE/NOSIZE flags make the position and
    // size arguments unused, so the call does nothing but put the window on the screen.
    // Both report their outcome, and there is nothing to do about either failing here.
    unsafe {
        if IsWindowVisible(hwnd) != 0 {
            return;
        }
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Place the main window as it is created, subclass it there if the state it is to open
/// in is one the builder cannot show it in, give it its icons as it is shown, and ask for
/// the foreground with it.
unsafe extern "system" fn main_window_creation_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut positioned = None;
    // A negative code is not ours to inspect; the message must be passed straight on.
    if code >= 0 {
        // SAFETY: for a non-negative code the hook contract makes `lparam` a live
        // CWPSTRUCT for the message being delivered on this thread.
        let message = unsafe { &*(lparam as *const CWPSTRUCT) };
        match message.message {
            WM_CREATE if class_name_is(message.hwnd, MAIN_WINDOW_CLASS) => {
                let placement = PENDING_PLACEMENT
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take();
                if let Some(placement) = placement {
                    place_window(message.hwnd, placement);
                }
                if PENDING_MAXIMIZED.swap(false, Ordering::AcqRel) {
                    hold_the_window_back_until_maximized(message.hwnd);
                }
            }
            // SAFETY: for WM_WINDOWPOSCHANGING the message's lParam is a live
            // WINDOWPOS describing the change about to be made.
            WM_WINDOWPOSCHANGING
                if class_name_is(message.hwnd, MAIN_WINDOW_CLASS)
                    && unsafe { shows_the_window(message.lParam) } =>
            {
                shell::apply_window_icons_to(message.hwnd);
            }
            WM_WINDOWPOSCHANGED if class_name_is(message.hwnd, MAIN_WINDOW_CLASS) => {
                positioned = Some(message.hwnd);
            }
            _ => {}
        }
    }

    // SAFETY: the arguments are the ones the hook procedure was handed, passed on to
    // the rest of the chain as the contract requires.
    let result = unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    if let Some(hwnd) = positioned {
        // Outside the message match, so no lock is held: taking the foreground sends
        // the window its activation messages, which re-enter this procedure, and a
        // `std` mutex is not reentrant.
        take_foreground(hwnd);
    }
    result
}

/// Whether a `WM_WINDOWPOSCHANGING` is the one that puts its window on the screen.
///
/// # Safety
///
/// `lparam` must be that message's, so a live `WINDOWPOS`.
unsafe fn shows_the_window(lparam: LPARAM) -> bool {
    // SAFETY: forwarded from this fn's contract.
    let position = unsafe { &*(lparam as *const WINDOWPOS) };
    position.flags & SWP_SHOWWINDOW != 0
}

/// Subclass the main window as it is created so that its first appearance is a maximized
/// one (`prepare_main_window_creation` writes down what it is holding back and why).
/// [`finish_main_window_creation`] takes the subclass back off.
///
/// A subclass rather than the creation hook this is called from, because a
/// `WM_WINDOWPOSCHANGING` can only be ANSWERED from the window procedure: a
/// `WH_CALLWNDPROC` hook is handed a copy of the `WINDOWPOS`, so it can read the change
/// coming but what it writes there is thrown away.
///
/// Best effort: a subclass that will not install leaves the window opening the way tao
/// shows it.
fn hold_the_window_back_until_maximized(hwnd: HWND) {
    // SAFETY: `hwnd` is the window being created, which the hook is reporting on. The
    // procedure is a plain fn of the documented signature and takes no reference data,
    // registered under an id this module owns and removed under the same pair.
    let installed =
        unsafe { SetWindowSubclass(hwnd, Some(maximized_show_subclass), SHOW_SUBCLASS_ID, 0) };
    if installed != 0 {
        HELD_BACK_WINDOW.store(hwnd as isize, Ordering::Release);
    }
}

/// Keep the window off the screen until the rectangle it is being put on is a maximized
/// one, then hand every message on to the rest of the chain.
unsafe extern "system" fn maximized_show_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING {
        // SAFETY: for WM_WINDOWPOSCHANGING the message's lParam is a live WINDOWPOS
        // describing the change about to be made, which the caller reads back once the
        // message has been answered - so this is where clearing a flag takes effect.
        unsafe { hold_the_show_back(hwnd, lparam) };
    }

    // SAFETY: the arguments are the ones this procedure was handed, passed on to the
    // rest of the subclass chain as the contract requires.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Take the show out of a `WM_WINDOWPOSCHANGING` that would put the window on the screen
/// short of maximized, leaving the rest of the change to be applied to a window that
/// stays off it.
///
/// What "maximized" is read off is the rectangle the change lands on, not the window's
/// style: the rectangle is what the person in front of it sees, and it says the same
/// thing whether Windows has updated `WS_MAXIMIZE` before this message or after it.
///
/// The rectangle alone cannot say WHICH maximized show this is, since the first and the
/// last land on the same one. What separates them is that the last comes after the
/// restore, so the show to let through is a maximized one with something already held
/// back behind it - and past [`SHOWS_TO_HOLD_BACK`] every show goes up regardless.
///
/// # Safety
///
/// `lparam` must be that message's, so a live `WINDOWPOS`.
unsafe fn hold_the_show_back(hwnd: HWND, lparam: LPARAM) {
    // SAFETY: forwarded from this fn's contract.
    let position = unsafe { &mut *(lparam as *mut WINDOWPOS) };
    if position.flags & SWP_SHOWWINDOW == 0 {
        return;
    }

    // Read and written from the window's own thread alone, which is where every message
    // it answers is delivered.
    let held_back = SHOWS_HELD_BACK.load(Ordering::Acquire);
    // SAFETY: `hwnd` is the window the message is for, and `position` the change it
    // carries, which is all `fills_the_work_area` reads.
    let maximized = unsafe { fills_the_work_area(hwnd, position) };
    if !holds_the_show_back(maximized, held_back) {
        return;
    }

    SHOWS_HELD_BACK.store(held_back + 1, Ordering::Release);
    position.flags &= !SWP_SHOWWINDOW;
}

/// The rule behind [`hold_the_show_back`], over what it reads: whether the show puts the
/// window on a `maximized` rectangle, and how many shows have been `held_back` already.
fn holds_the_show_back(maximized: bool, held_back: u32) -> bool {
    let last_appearance = maximized && held_back > 0;
    !last_appearance && held_back < SHOWS_TO_HOLD_BACK
}

/// Whether the rectangle a `WM_WINDOWPOSCHANGING` leaves the window on covers the work
/// area of the display it is on - which a maximized window's frame does, since it is the
/// work area grown by the invisible resize border, and a restored one does not.
///
/// A change that cannot be resolved to a display reads as maximized, so an answer that
/// could not be worked out lets the window be seen rather than hiding it on a guess.
///
/// # Safety
///
/// `hwnd` must be a live window and `position` the change it is being given.
unsafe fn fills_the_work_area(hwnd: HWND, position: &WINDOWPOS) -> bool {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: forwarded; GetWindowRect only writes through the RECT it is given, and
    // reports whether it did.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return true;
    }
    // The change carries only what it is not told to leave alone, so the rest of the
    // rectangle is the one the window is on now.
    if position.flags & SWP_NOMOVE == 0 {
        rect.right += position.x - rect.left;
        rect.bottom += position.y - rect.top;
        rect.left = position.x;
        rect.top = position.y;
    }
    if position.flags & SWP_NOSIZE == 0 {
        rect.right = rect.left + position.cx;
        rect.bottom = rect.top + position.cy;
    }

    // SAFETY: MonitorFromRect reads the rectangle it is given and returns the display
    // nearest it, which is never null under DEFAULTTONEAREST; GetMonitorInfoW writes
    // through the MONITORINFO it is given, whose cbSize tells it which one it is.
    let work_area = unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut monitor_info) == 0 {
            return true;
        }
        monitor_info.rcWork
    };

    rect.left <= work_area.left
        && rect.top <= work_area.top
        && rect.right >= work_area.right
        && rect.bottom >= work_area.bottom
}

/// Ask for `hwnd` to be the foreground window, until it is one on screen.
///
/// Answered on every position change rather than on the one that first shows the
/// window, because tao shows the window more than once on the way up: one opening
/// maximized is shown, hidden and shown again, and the styles are pushed through a
/// further `SetWindowPos` after that. A window taken back off the screen has not had
/// the show this is for, so the asking resumes; once it is up and in front, the ones
/// that follow cost two reads.
///
/// Asking is all this does. Windows grants the foreground to a launch the user made and
/// refuses it to one made in the background, which is the wanted answer in both cases,
/// and the window the user turned to while this one was coming up keeps it: by then
/// this has what it asked for and has stopped.
fn take_foreground(hwnd: HWND) {
    // SAFETY: `hwnd` is the live main window the hook is reporting on. The calls only
    // read or request window state. `SetForegroundWindow` reports whether it was
    // granted, which the read after it answers directly instead: the window can be
    // handed the foreground and lose it again in the same breath.
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            FOREGROUND_TAKEN.store(false, Ordering::Release);
            return;
        }
        if FOREGROUND_TAKEN.load(Ordering::Acquire) {
            return;
        }
        SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            FOREGROUND_TAKEN.store(true, Ordering::Release);
        }
    }
}

/// Put a window's frame at `position` with a client area of `inner_size`, both in
/// physical pixels.
///
/// In two steps, because the frame around a client area is a function of the display's
/// DPI: the move comes first so the window takes the DPI of the display it is going to,
/// and the size is then worked out from the frame THAT DPI carries. Doing it the other
/// way would size the window for the display it is leaving.
fn place_window(hwnd: HWND, placement: Placement) {
    // SAFETY: `hwnd` is the window being created, which the hook is reporting on. The
    // NOSIZE flag makes the size arguments unused, and the styles and DPI are plain
    // reads; GetDpiForWindow returns 0 for a window it cannot resolve, where the
    // 96-DPI baseline applies. AdjustWindowRectExForDpi only writes through the RECT it
    // is given, and a failure leaves the window at the size the builder asked for -
    // still on the right display, which is the half that matters most.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            placement.position.x,
            placement.position.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );

        let dpi = GetDpiForWindow(hwnd);
        let dpi = if dpi == 0 {
            USER_DEFAULT_SCREEN_DPI
        } else {
            dpi
        };
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: placement.inner_size.width as i32,
            bottom: placement.inner_size.height as i32,
        };
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let has_menu = i32::from(!GetMenu(hwnd).is_null());
        if AdjustWindowRectExForDpi(&mut frame, style, has_menu, ex_style, dpi) == 0 {
            return;
        }

        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            frame.right - frame.left,
            frame.bottom - frame.top,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// A second instance's grace period for the primary to finish starting before it
/// concludes the primary is stuck, and the primary's own before it asks the user about
/// it. Covers a normal cold start (DLL load, session create, WebView2 window build,
/// front-end render) so a relaunch that races the primary's own startup - a rapid
/// double-launch from the tray - does not misfire the stuck warning.
const MAIN_WINDOW_WAIT: Duration = Duration::from_secs(30);
/// Poll cadence while a second instance waits for the primary.
const MAIN_WINDOW_POLL: Duration = Duration::from_millis(100);

/// Whether the primary instance has FINISHED starting: its main window is visible and
/// no longer carries the startup splash.
///
/// A visible window is not enough on its own. The main window is created VISIBLE, and
/// created before the webview it hosts - so a primary hung inside the WebView2 build
/// (the elevated-without-Explorer case [`shell_missing_while_elevated`] names) has a
/// window on screen for the whole hang, carrying the splash and nothing else. Keying on
/// the window alone would read that as a healthy UI and hand off to a message loop that
/// never runs.
///
/// The splash is a child window of the main one until the app is on screen
/// (`splash::dismiss`), so its absence is the hand-over, observable from another
/// process. `FindWindow`/`FindWindowEx` + `IsWindowVisible` read window state, which
/// crosses integrity levels (UIPI only gates *sending* to a higher-IL window), so an
/// unelevated relaunch still reads an elevated primary's tree. A minimized window keeps
/// `WS_VISIBLE`, so a UI minimized to the taskbar counts as up and takes the normal
/// foreground hand-off.
///
/// A primary whose splash never came up at all (`splash::show` is best effort) shows no
/// child to wait on, which reads as ready - the same answer the window alone gave.
fn main_window_ready() -> bool {
    let class = wide(MAIN_WINDOW_CLASS);
    // SAFETY: class is a NUL-terminated wide string; a null window-name matches any
    // title. FindWindowW returns NULL when no window of that class exists.
    let window = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if window.is_null() {
        return false;
    }
    // SAFETY: `window` is a handle just returned by FindWindowW; IsWindowVisible only
    // reads the window's style.
    if unsafe { IsWindowVisible(window) } == 0 {
        return false;
    }

    let splash = wide(SPLASH_WINDOW_CLASS);
    // SAFETY: `window` is the live main window and `splash` is a NUL-terminated wide
    // string; a null child-after starts the search at the first child and a null window
    // name matches any title. FindWindowExW only reads the window tree and returns NULL
    // when no child of that class is left.
    let splash = unsafe {
        FindWindowExW(
            window,
            std::ptr::null_mut(),
            splash.as_ptr(),
            std::ptr::null(),
        )
    };
    splash.is_null()
}

/// How a second instance found the primary.
pub enum PrimaryState {
    /// It has finished starting and can be handed the foreground.
    Ready,
    /// It is waiting for a consent dialog to be answered, so there is nothing
    /// wrong with it and nothing useful to do: it is finishing its startup behind
    /// the prompt, and the user is looking at the prompt.
    WaitingForElevation,
    /// It never finished starting - wedged while holding the single-instance
    /// lock, which the caller surfaces rather than handing off into the void.
    Stuck,
}

/// This process's main window, or null while there is none.
///
/// A dialog that has an owner is modal to it and cannot be lost behind it, which
/// matters most for the one dialog this process raises that a person has to
/// answer: the elevation prompt. Null is a window that is not there - one lost
/// while the prompt was being raised - so the caller has to be able to go on
/// without an owner.
pub fn main_window_handle() -> HWND {
    let class = wide(MAIN_WINDOW_CLASS);
    // SAFETY: `class` is a NUL-terminated wide string; a null window-name matches
    // any title. FindWindowW returns NULL when no window of that class exists.
    unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
}

/// Wait up to [`MAIN_WINDOW_WAIT`] for the primary instance to finish starting,
/// reporting the moment it has (or immediately if it already had).
pub fn wait_for_main_window_ready() -> PrimaryState {
    let deadline = Instant::now() + MAIN_WINDOW_WAIT;
    loop {
        if main_window_ready() {
            return PrimaryState::Ready;
        }
        // Checked on every poll rather than once up front: the prompt can go up
        // at any point while we wait, and a startup that is waiting on a person
        // has not failed at anything.
        if elevation_prompt_on_screen() {
            return PrimaryState::WaitingForElevation;
        }
        if Instant::now() >= deadline {
            return PrimaryState::Stuck;
        }
        std::thread::sleep(MAIN_WINDOW_POLL);
    }
}

/// The named event that says a consent dialog is on screen, raised for the
/// duration of the call that puts it there.
///
/// One signal, two consumers: this process's startup watchdog, which must not
/// offer to end a startup that is merely waiting for a person, and a second
/// instance, which must not report the primary stuck for the same reason. The
/// second consumer is what makes it
/// a cross-process object rather than an `AtomicBool` - and it cannot ride the
/// detect mutex, since a mutex carries existence and no payload. `Local\`
/// scope, like the detect mutex: both processes are in one logon session, at
/// one integrity level, so it needs no descriptor of its own.
const ELEVATION_EVENT_NAME: &str = r"Local\WindhawkUI.WaitingForElevation";

/// The event handle this process created, kept for its lifetime so the named
/// object exists while the UI runs. Only the primary creates one; a second
/// instance opens it by name.
static ELEVATION_EVENT: AtomicIsize = AtomicIsize::new(0);

/// A consent dialog is on screen for as long as this guard lives.
///
/// What it covers is the DIALOG, not the elevation ladder: a scheduled-task
/// trigger is sub-second and cannot plausibly hold up a startup, while a prompt
/// is bounded only by how long the user takes to click. Pausing the watchdog for
/// the whole ladder would disarm it on the fast path - where the window is up and
/// WebView2 is building, which is the hang it exists to catch - for exactly as
/// long as a prompt sits unanswered.
pub struct ElevationPrompt(());

impl Drop for ElevationPrompt {
    fn drop(&mut self) {
        let handle = ELEVATION_EVENT.load(Ordering::Acquire);
        if handle != 0 {
            // SAFETY: the handle is the manual-reset event created below, which is
            // never closed (it lives for the process).
            unsafe { ResetEvent(handle as HANDLE) };
        }
    }
}

/// Announce a consent dialog until the returned guard drops. Best effort: if the
/// event cannot be created, the watchdog and a second instance simply behave as
/// they did before it existed.
pub fn hold_elevation_prompt() -> ElevationPrompt {
    let name = wide(ELEVATION_EVENT_NAME);
    let mut handle = ELEVATION_EVENT.load(Ordering::Acquire);
    if handle == 0 {
        // SAFETY: a null descriptor takes the default (this user, this session);
        // manual-reset and initially clear; `name` is a NUL-terminated wide string
        // the call copies. The handle is stored below and closed only by process
        // exit, so it cannot be used after being freed.
        let created = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
        if !created.is_null() {
            handle = created as isize;
            ELEVATION_EVENT.store(handle, Ordering::Release);
        }
    }
    if handle != 0 {
        // SAFETY: `handle` is the manual-reset event created above.
        unsafe { SetEvent(handle as HANDLE) };
    }
    ElevationPrompt(())
}

/// Whether a Windhawk consent dialog is on screen in this logon session. Read by
/// the startup watchdog and by a second instance; both open the object by name,
/// so neither has to have created it.
fn elevation_prompt_on_screen() -> bool {
    let name = wide(ELEVATION_EVENT_NAME);
    // SAFETY: `name` is a NUL-terminated wide string; SYNCHRONIZE is all a state
    // read needs. A null return means no such object, that is, no prompt.
    let handle = unsafe { OpenEventW(SYNCHRONIZE, 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: `handle` is the event just opened; a zero timeout polls its state.
    // It is closed exactly once, below.
    let signalled = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
    // SAFETY: `handle` was opened above and is closed exactly once here.
    unsafe { CloseHandle(handle) };
    signalled
}

/// Whether THIS process's startup has handed over - the app is on screen and the
/// splash is gone (`splash::dismiss`) - and the wait for it to.
///
/// The primary watches its own startup through this rather than through
/// [`main_window_ready`]: the signal is raised in this process, so it can be waited on
/// outright instead of sampled, and it does not depend on the splash having come up.
static HANDED_OVER: Mutex<bool> = Mutex::new(false);
static HANDED_OVER_SIGNAL: Condvar = Condvar::new();

/// The startup is over: the window is showing the app rather than the splash. Wakes
/// the watchdog, which has nothing left to watch. Idempotent - the front-end's report
/// and every fallback behind it land here.
pub fn mark_startup_handed_over() {
    *HANDED_OVER
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = true;
    HANDED_OVER_SIGNAL.notify_all();
}

/// Whether this process's startup has handed over.
fn startup_handed_over() -> bool {
    *HANDED_OVER
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// Block until this process's startup hands over or `patience` passes, reporting
/// which.
fn wait_for_startup_handover(patience: Duration) -> bool {
    let handed_over = HANDED_OVER
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (handed_over, _) = HANDED_OVER_SIGNAL
        .wait_timeout_while(handed_over, patience, |handed_over| !*handed_over)
        .unwrap_or_else(|error| error.into_inner());
    *handed_over
}

/// Custom button ids for the startup-stuck prompt. Kept out of the low range the task
/// dialog assigns to its own standard controls (IDOK/IDCANCEL).
const ID_KEEP_WAITING: i32 = 101;
const ID_END_PROCESS: i32 = 102;
const ID_RELAUNCH: i32 = 103;

/// Set once a fatal startup failure has taken over (`suppress_startup_watchdog`), so
/// the startup watchdog - which can only observe "the app is not up yet" - stands down
/// instead of stacking its prompt on top of the fatal box. The fatal path presents its
/// own message and exits.
static WATCHDOG_SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// Silence the startup watchdog. The fatal-startup path calls this before showing its
/// box: that path owns the outcome (its own message, then exit), and the app will never
/// come up, so the watchdog must not also fire.
pub fn suppress_startup_watchdog() {
    WATCHDOG_SUPPRESSED.store(true, Ordering::Release);
}

/// Spawn the primary instance's startup watchdog on a background thread. The main
/// thread does the startup work that can wedge - session bring-up, and above all the
/// WebView2 window creation - so the watch has to run from the side to notice a hang
/// there. Only the primary spawns it (a second instance never builds a window).
///
/// What it waits for is the hand-over ([`mark_startup_handed_over`]), not a window: the
/// main window is on screen carrying the splash from the moment it is created, which is
/// before the webview whose creation is the likeliest thing to hang.
pub fn spawn_startup_watchdog() {
    std::thread::Builder::new()
        .name("wh-ui-startup-watchdog".to_owned())
        .spawn(run_startup_watchdog)
        .expect("spawn the startup watchdog thread");
}

/// Watch the primary's own startup: once it hands over, the thread ends. If it has not
/// within [`MAIN_WINDOW_WAIT`], ask what to do about it, act on the answer, and repeat
/// while the app stays off screen. Stands down if the fatal-startup path has taken over
/// ([`WATCHDOG_SUPPRESSED`]).
fn run_startup_watchdog() {
    while wait_out_the_startup() {
        match show_startup_stuck_prompt() {
            StuckChoice::EndProcess => terminate_current_process(),
            StuckChoice::Relaunch => relaunch_current_process(),
            StuckChoice::KeepWaiting => {}
        }
    }
}

/// How often the watchdog re-examines its own patience. Short enough that a
/// consent dialog which closes part way through does not leave the remainder of a
/// slice standing in for the whole grace period.
const WATCHDOG_SLICE: Duration = Duration::from_millis(500);

/// Wait for the startup to hand over, and report whether it is time to ask the
/// user about it. `false` means there is nothing left to watch: the app is up, or
/// a fatal failure has taken the outcome over.
///
/// The patience is [`MAIN_WINDOW_WAIT`] **from the moment the startup is actually
/// free to proceed**. A consent dialog on screen is a startup waiting on a person,
/// not a stuck one, so the deadline is pushed out for as long as one is up - and it
/// has to be pushed out WHILE it is up, not tested once when the wait expires.
/// Sampling it at the end leaves whatever is left of the window after the answer
/// standing in for the whole grace period, which is how the prompt could appear a
/// second after a consent dialog closed and then dismiss itself the moment the app
/// arrived. What the user saw was the watchdog being right on time about a startup
/// that had barely begun.
///
/// This is a pause, never the suppression above: that one RETURNS and stands the
/// watchdog down for good, which would disarm it for the WebView2 build - the very
/// hang it exists to catch.
fn wait_out_the_startup() -> bool {
    let mut deadline = Instant::now() + MAIN_WINDOW_WAIT;
    loop {
        if wait_for_startup_handover(WATCHDOG_SLICE) {
            return false;
        }
        // A fatal startup failure produces the same symptom - no app - but owns its
        // own message and its own exit, so defer to it.
        if WATCHDOG_SUPPRESSED.load(Ordering::Acquire) {
            return false;
        }
        if elevation_prompt_on_screen() {
            deadline = Instant::now() + MAIN_WINDOW_WAIT;
        }
        if Instant::now() >= deadline {
            return true;
        }
    }
}

/// The answer the startup-stuck prompt returned.
enum StuckChoice {
    /// Wait the startup out: the non-destructive button, the close box, the dialog
    /// dismissing itself because the window appeared, or a failure to show it at all.
    KeepWaiting,
    Relaunch,
    EndProcess,
}

/// The startup-stuck prompt's wording and its non-destructive button.
///
/// A startup that never gets the app on screen has one cause we can name from here,
/// [`shell_missing_while_elevated`], and waiting does not clear it - so that variant
/// explains it and offers the remedy (start Explorer, then relaunch). Any other stall
/// is a start that is slow or wedged for reasons not visible from this side, where
/// waiting is the sensible default.
struct StuckPrompt {
    instruction: &'static str,
    content: &'static str,
    action_label: &'static str,
    action_id: i32,
}

fn stuck_prompt(shell_missing_while_elevated: bool) -> StuckPrompt {
    if shell_missing_while_elevated {
        StuckPrompt {
            instruction: "Windhawk cannot start while Windows Explorer is not running",
            content: "Windhawk is running as administrator. Its window is drawn by \
                      WebView2, which in that case starts its browser process through \
                      Windows Explorer, and Explorer is not running - so that process \
                      is never created and Windhawk cannot finish starting.\n\nStart \
                      Windows Explorer, then relaunch Windhawk.",
            action_label: "Relaunch Windhawk",
            action_id: ID_RELAUNCH,
        }
    } else {
        StuckPrompt {
            instruction: "Windhawk is taking longer than usual to start",
            content: "Windhawk has not finished starting. It may still be coming up, \
                      or the process may be stuck.\n\nKeep waiting, or end the \
                      Windhawk process so you can start it again?",
            action_label: "Keep waiting",
            action_id: ID_KEEP_WAITING,
        }
    }
}

/// Whether this elevated process has no desktop shell to bring the window up through.
///
/// WebView2 will not run its browser process elevated: when the host is elevated it
/// launches `msedgewebview2.exe` de-elevated THROUGH Windows Explorer, so on a desktop
/// with no shell that process is never created and the window never opens
/// (MicrosoftEdge/WebView2Feedback#960). Both halves are required, since a Windhawk
/// that is not elevated launches the browser process directly and does not care about
/// the shell, and both are read afresh on every prompt, since Explorer can come back at
/// any point.
fn shell_missing_while_elevated() -> bool {
    // SAFETY: GetShellWindow takes no arguments and only reads the shell window
    // registered for the calling thread's desktop; NULL means there is none.
    is_running_as_admin() && unsafe { GetShellWindow() }.is_null()
}

/// Task dialog callback for the startup-stuck prompt.
///
/// On `TDN_CREATED` it makes the dialog topmost. The dialog is ownerless (there is no
/// window yet - that is the whole reason it shows), so without this it can end up
/// behind whatever the user is looking at, with nothing on screen explaining why
/// Windhawk never came up. `TASKDIALOGCONFIG` has no topmost flag, hence the
/// `SetWindowPos`.
///
/// On `TDN_TIMER` it closes the dialog once the startup has handed over.
/// A startup that is only slow (WebView2 can take well over [`MAIN_WINDOW_WAIT`])
/// finishes on its own while the prompt is on screen, and the watchdog loop re-checks
/// only once the dialog returns - so without this the prompt would sit there insisting
/// Windhawk is stuck, in front of the app that just came up. Cancelling the dialog
/// rather than clicking a button keeps this distinct from every deliberate answer, so
/// it reads as keep waiting whichever buttons the variant is carrying.
///
/// Returning `S_OK` leaves the default handling of every notification in place.
unsafe extern "system" fn startup_stuck_prompt_callback(
    hwnd: HWND,
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    _ref_data: isize,
) -> HRESULT {
    if msg == TDN_CREATED as u32 {
        // SAFETY: hwnd is the live dialog window the notification is reporting on. The
        // NOMOVE/NOSIZE flags make the position and size arguments ignored, and the
        // result is advisory (best effort z-order).
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    } else if msg == TDN_TIMER as u32 && startup_handed_over() {
        // SAFETY: hwnd is the live dialog window the notification is reporting on.
        // TDM_CLICK_BUTTON takes the button id in wparam and ignores lparam; IDCANCEL is
        // accepted because the dialog carries TDF_ALLOW_DIALOG_CANCELLATION, and it ends
        // the dialog exactly as the close box would. The result is unused.
        unsafe {
            SendMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDCANCEL as WPARAM, 0);
        }
    }

    S_OK
}

/// Ask what to do about a startup with no window, through a task dialog carrying the
/// variant's two explicit buttons (a plain message box cannot relabel its buttons).
/// Anything other than a deliberate Relaunch or End click - the non-destructive button,
/// the close box, the self-dismissal once the window appears, or a failure to show the
/// dialog - reads as keep waiting, so the process is never ended or replaced except on
/// an explicit choice.
fn show_startup_stuck_prompt() -> StuckChoice {
    let prompt = stuck_prompt(shell_missing_while_elevated());

    let title = wide("Windhawk");
    let instruction = wide(prompt.instruction);
    let content = wide(prompt.content);
    let action = wide(prompt.action_label);
    let end = wide("End process");

    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: prompt.action_id,
            pszButtonText: action.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            nButtonID: ID_END_PROCESS,
            pszButtonText: end.as_ptr(),
        },
    ];

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        // The timer ticks are what let the callback close the dialog by itself.
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_CALLBACK_TIMER,
        pszWindowTitle: title.as_ptr(),
        Anonymous1: TASKDIALOGCONFIG_0 {
            pszMainIcon: TD_WARNING_ICON,
        },
        pszMainInstruction: instruction.as_ptr(),
        pszContent: content.as_ptr(),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: prompt.action_id,
        pfCallback: Some(startup_stuck_prompt_callback),
        ..Default::default()
    };

    let mut pressed = 0i32;
    // SAFETY: `config` is fully initialized and its title/content/button string pointers
    // and the `buttons` array all outlive the call; the radio-button and verification
    // out-params are unused (null). The callback is a plain fn item with the ABI the
    // field declares and takes no reference data. TaskDialogIndirect pumps its own modal
    // message loop, so it is safe to call from this background thread.
    let hr = unsafe {
        TaskDialogIndirect(
            &config,
            &mut pressed,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if hr < 0 {
        return StuckChoice::KeepWaiting;
    }

    match pressed {
        ID_END_PROCESS => StuckChoice::EndProcess,
        ID_RELAUNCH => StuckChoice::Relaunch,
        _ => StuckChoice::KeepWaiting,
    }
}

/// Force-terminate this process, for the End choice on a wedged startup. The main
/// thread is stuck, so a normal exit - which would run teardown that may touch the
/// stuck thread's state (WebView2/COM) - could itself hang; `TerminateProcess` is
/// unconditional. If it somehow returns, the watchdog loop simply re-prompts.
fn terminate_current_process() {
    // SAFETY: GetCurrentProcess returns the current-process pseudo-handle; TerminateProcess
    // ends this process with exit code 1.
    unsafe {
        TerminateProcess(GetCurrentProcess(), 1);
    }
}

/// The environment variable a relaunch hands its successor, carrying the process id of
/// the instance being replaced ([`relaunch_current_process`] sets it,
/// [`await_relaunch_predecessor`] takes it back out of the environment).
const RELAUNCH_PREDECESSOR_VAR: &str = "WINDHAWK_UI_RELAUNCH_PREDECESSOR";

/// How long a relaunched instance waits for the instance it replaces to exit. Only a
/// bound on a wait that normally ends in milliseconds: the predecessor terminates
/// itself the moment the successor is spawned, and a predecessor that somehow outlives
/// this simply gets the ordinary second-instance handling.
const PREDECESSOR_WAIT: Duration = Duration::from_secs(10);

/// Start a fresh UI process and end this one, for the Relaunch choice on a startup
/// that cannot produce a window. The successor is told which process it replaces so it
/// can wait this one out rather than mistake it for a live primary to hand off to.
fn relaunch_current_process() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };

    let spawned = std::process::Command::new(exe)
        .env(RELAUNCH_PREDECESSOR_VAR, std::process::id().to_string())
        .spawn();

    // Leave only once the replacement is on its way. A spawn failure keeps this process
    // (and its prompt loop) alive rather than closing the UI with nothing to take over.
    if spawned.is_ok() {
        terminate_current_process();
    }
}

/// Wait for the instance this process was launched to replace, when it was started by
/// [`relaunch_current_process`]. Called before anything reads the single-instance
/// state: the predecessor holds the detect mutex (and the single-instance plugin's own
/// mutex and window) until it is gone, so starting the handshake while it is still
/// alive would make this process a second instance of the very instance it replaces -
/// waiting for a window that will never appear. A normal launch has no variable set and
/// returns immediately.
///
/// The variable is TAKEN rather than read, so it is spent by the one process it names a
/// predecessor to. Left in place it would be inherited by everything this UI goes on to
/// launch, and any child that ever reached this function would wait out a process id
/// that means nothing to it - or, once that id has been reused, one that means something
/// else entirely.
///
/// Taking it is only sound while nothing else can be reading the environment, so this
/// must be called before this process has a second thread - which is where it already
/// belongs for its own sake, at the top of `run`, ahead of everything that reads the
/// single-instance state.
pub fn await_relaunch_predecessor() {
    let Ok(pid) = std::env::var(RELAUNCH_PREDECESSOR_VAR) else {
        return;
    };
    // SAFETY: this process is still single-threaded here (see above), so no other
    // thread can be in the environment while it is modified.
    unsafe { std::env::remove_var(RELAUNCH_PREDECESSOR_VAR) };
    let Ok(pid) = pid.parse::<u32>() else {
        return;
    };

    // SAFETY: OpenProcess takes an access mask, an inherit flag, and a process id, and
    // returns NULL when the process is already gone (nothing left to wait for) or
    // cannot be opened. SYNCHRONIZE is all a wait needs.
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return;
    }

    // SAFETY: handle is the process just opened for SYNCHRONIZE; the wait returns when
    // it exits or the timeout elapses (the outcome is advisory either way), and the
    // handle is closed exactly once.
    unsafe {
        WaitForSingleObject(handle, PREDECESSOR_WAIT.as_millis() as u32);
        CloseHandle(handle);
    }
}

/// Whether this process has been asked to close its window, which is what makes
/// the window's destruction an exit rather than a failure.
///
/// Every deliberate close asks first. The title bar's close box and the tray's
/// `SC_CLOSE` both arrive as `WM_CLOSE`, and a close driven from the app itself
/// goes through the runtime's own close message; all of them raise
/// `WindowEvent::CloseRequested` before anything destroys the window. Nothing
/// else does - so a window that is destroyed with this still clear was taken
/// away by something no one asked for (WebView2 destroys the window it draws into
/// when its page or its browser process asks it to), which is a failure to report
/// rather than an exit to follow.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Record that the window has been asked to close.
pub fn note_close_requested() {
    CLOSE_REQUESTED.store(true, Ordering::Release);
}

/// Whether the window was asked to close before it was destroyed.
pub fn close_was_requested() -> bool {
    CLOSE_REQUESTED.load(Ordering::Acquire)
}

/// Bring the main window to the foreground (the single-instance "show" path):
/// restore if minimized, show if hidden, focus.
pub fn show_and_focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The expander's label, the same either way: it names what is behind it rather
/// than which way the button points, which the arrow beside it already says.
const DIAGNOSTICS_LABEL: &str = "Diagnostic details";

/// Task dialog callback for the fatal box: makes it topmost on `TDN_CREATED`.
///
/// The dialog is ownerless - the whole reason it shows is that there is no window
/// to own it - so without this it can end up behind whatever the user is looking
/// at, which for a message that is the app's last word is the same as not showing
/// it. `TASKDIALOGCONFIG` has no topmost flag, hence the `SetWindowPos`; a message
/// box got the same effect from `MB_SYSTEMMODAL`.
///
/// Returning `S_OK` leaves the default handling of every notification in place.
unsafe extern "system" fn fatal_dialog_callback(
    hwnd: HWND,
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    _ref_data: isize,
) -> HRESULT {
    if msg == TDN_CREATED as u32 {
        // SAFETY: hwnd is the live dialog window the notification is reporting on.
        // The NOMOVE/NOSIZE flags make the position and size arguments ignored, and
        // the result is advisory (best effort z-order).
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    S_OK
}

/// Present a fatal failure as a native modal task dialog: used when there is no
/// webview to render a reply into.
///
/// `instruction` is the sentence in the heading, `content` the explanation under
/// it, and `diagnostics` the raw lines - the captured log records and whatever
/// the WebView2 probe found - that sit collapsed at the bottom under
/// [`DIAGNOSTICS_LABEL`]. A task dialog rather than a message box for exactly
/// that: the person in front of it needs the sentence, and whoever they forward
/// it to needs the codes, and an expander is what serves both without making the
/// message a wall of text. It also gets a real heading and a proper error icon,
/// which a message box cannot do.
///
/// A task dialog needs comctl32 v6, so a failure to show one falls back to the
/// message box: the last thing that may fail is the report of a failure.
pub fn show_fatal(instruction: &str, content: &str, diagnostics: Option<&str>) {
    let title = wide("Windhawk");
    let instruction_text = wide(instruction);
    let content_text = wide(content);
    let diagnostics_text = diagnostics.map(wide);
    let expander_label = wide(DIAGNOSTICS_LABEL);

    // Footer area: the details belong at the BOTTOM of the dialog, under the
    // explanation, rather than pushing it down from between it and the button.
    // Size-to-content, because the lines behind the expander are log records and
    // paths, which a default-width dialog would wrap into a column.
    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT;
    if diagnostics_text.is_some() {
        flags |= TDF_EXPAND_FOOTER_AREA;
    }

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: flags,
        dwCommonButtons: TDCBF_OK_BUTTON,
        pszWindowTitle: title.as_ptr(),
        Anonymous1: TASKDIALOGCONFIG_0 {
            pszMainIcon: TD_ERROR_ICON,
        },
        pszMainInstruction: instruction_text.as_ptr(),
        pszContent: content_text.as_ptr(),
        // A null expanded-information pointer leaves the expander out entirely,
        // which is what a failure with nothing collected behind it should show.
        pszExpandedInformation: diagnostics_text
            .as_ref()
            .map_or(std::ptr::null(), |text| text.as_ptr()),
        pszExpandedControlText: expander_label.as_ptr(),
        pszCollapsedControlText: expander_label.as_ptr(),
        pfCallback: Some(fatal_dialog_callback),
        ..Default::default()
    };

    // SAFETY: `config` is fully initialized and every string pointer in it outlives
    // the call; the button, radio-button, and verification out-params are unused
    // (null). The callback is a plain fn item with the ABI the field declares and
    // takes no reference data.
    let shown = unsafe {
        TaskDialogIndirect(
            &config,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if shown >= 0 {
        return;
    }

    // The fallback: one flat message, since a message box has nowhere to put the
    // details but the text itself.
    let mut message = format!("{instruction}\n\n{content}");
    if let Some(diagnostics) = diagnostics {
        message.push_str(&format!("\n\n{DIAGNOSTICS_LABEL}:\n{diagnostics}"));
    }
    let text = wide(&message);
    // SAFETY: both strings are NUL-terminated; a null owner HWND is valid for a
    // standalone message box. The return value (which button) is unused.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SYSTEMMODAL,
        );
    }
}

/// Present the stuck-background-instance message. The detect mutex shows a UI process
/// is alive, but [`wait_for_main_window_ready`] never saw it finish starting: a previous
/// instance wedged holding the single-instance lock, so every relaunch hands off to it
/// and silently exits. We do not kill it (it may be elevated, or mid-shutdown), so tell
/// the user how to clear it themselves.
pub fn show_stuck_background_instance() {
    show_fatal(
        "Windhawk is already running, but it never finished starting.",
        "A previous Windhawk UI process is likely stuck. Open Task Manager, end \
         every \"windhawk-ui.exe\" process on the Details tab, then start Windhawk \
         again.",
        None,
    );
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The no-shell variant is the one that can say why the window is missing, so it
    // names Explorer and offers the relaunch that follows starting it - never the
    // "keep waiting" that would not help.
    #[test]
    fn a_missing_shell_prompts_to_relaunch_after_starting_explorer() {
        let prompt = stuck_prompt(true);

        assert_eq!(prompt.action_id, ID_RELAUNCH);
        assert!(prompt.instruction.contains("Windows Explorer"));
        assert!(prompt.content.contains("Start Windows Explorer"));
    }

    // Any other windowless startup has no diagnosis to offer from here: the prompt
    // stays about the wait, and its non-destructive button keeps waiting rather than
    // replacing a process that may be seconds from showing its window.
    #[test]
    fn an_unexplained_stall_prompts_to_keep_waiting() {
        let prompt = stuck_prompt(false);

        assert_eq!(prompt.action_id, ID_KEEP_WAITING);
        assert!(!prompt.content.contains("Explorer"));
    }

    // The watchdog waits on the hand-over, so a startup that has not reached it must
    // not read as done - and the wait must return the moment it does, rather than
    // holding the thread for the full grace period.
    #[test]
    fn the_watchdog_waits_until_the_startup_hands_over() {
        assert!(!startup_handed_over());

        mark_startup_handed_over();

        assert!(startup_handed_over());
        let waited = Instant::now();
        assert!(wait_for_startup_handover(MAIN_WINDOW_WAIT));
        assert!(waited.elapsed() < MAIN_WINDOW_WAIT);
    }

    // The patience is measured from when the startup is free to proceed, not from
    // when it began: a startup held up by a consent dialog has not had its grace
    // period yet. The regression this guards is a prompt that appears a moment
    // after the dialog closes and then dismisses itself when the app arrives.
    #[test]
    fn a_consent_dialog_buys_the_startup_its_whole_grace_period() {
        // The wait is sliced, so the deadline moves whenever the dialog is seen -
        // rather than being decided once, at the end, by a single sample.
        assert!(
            WATCHDOG_SLICE < MAIN_WINDOW_WAIT,
            "a slice as long as the wait would sample the flag once and learn nothing"
        );

        let _prompt = hold_elevation_prompt();
        assert!(
            elevation_prompt_on_screen(),
            "the flag the watchdog reads is raised while a dialog is up"
        );
        drop(_prompt);
        assert!(
            !elevation_prompt_on_screen(),
            "and cleared the moment it is answered, however it was answered"
        );
    }

    // The three appearances a maximized launch makes, in the order it makes them: the
    // maximize that puts the window up before tao has asked for it to be visible, the
    // restore behind it, and the maximize that is the one to keep. Only the last is a
    // rectangle the window is meant to be seen on AND the end of the sequence, which is
    // what the count separates it by - the first lands on the same rectangle.
    #[test]
    fn a_maximized_launch_is_shown_on_its_last_appearance_alone() {
        assert!(
            holds_the_show_back(true, 0),
            "the maximize tao takes straight back down"
        );
        assert!(holds_the_show_back(false, 1), "the restore behind it");
        assert!(!holds_the_show_back(true, 2), "the maximize that stays");
    }

    // Past the allowance the window goes up whatever the show looks like: being wrong
    // about a show that was going to be undone costs a flicker, and being wrong the
    // other way costs a window nobody ever shows.
    #[test]
    fn the_allowance_bounds_the_holding_back() {
        assert!(!holds_the_show_back(false, SHOWS_TO_HOLD_BACK));
        assert!(!holds_the_show_back(true, SHOWS_TO_HOLD_BACK));
    }

    // The hook outlives what it watches for, so the build is what takes it out - for a
    // launch that had nothing to place as much as for one that did, since the
    // activation it also carries belongs to every launch. What it does while installed
    // is Win32 message ordering that only a real window can exercise; this covers the
    // lifetime around it.
    // One test for the whole lifetime, since what it covers is process-global: a
    // second test running beside it would be preparing and finishing the same
    // creation.
    #[test]
    fn the_creation_hook_lasts_exactly_as_long_as_the_build() {
        prepare_main_window_creation(None, false);
        assert_ne!(
            CREATION_HOOK.load(Ordering::Acquire),
            0,
            "a launch with no remembered rectangle still needs the hook for the activation"
        );
        assert!(
            !PENDING_MAXIMIZED.load(Ordering::Acquire),
            "a launch that does not open maximized has no show to hold back"
        );

        finish_main_window_creation();
        assert_eq!(CREATION_HOOK.load(Ordering::Acquire), 0);

        finish_main_window_creation();
        assert_eq!(
            CREATION_HOOK.load(Ordering::Acquire),
            0,
            "removing a hook that is already gone is a no-op, not a second unhook"
        );

        // A launch that opens maximized carries the state the subclass is installed
        // from, and its own allowance of shows to hold back.
        SHOWS_HELD_BACK.store(SHOWS_TO_HOLD_BACK, Ordering::Release);
        prepare_main_window_creation(None, true);
        assert!(PENDING_MAXIMIZED.load(Ordering::Acquire));
        assert_eq!(
            SHOWS_HELD_BACK.load(Ordering::Acquire),
            0,
            "every launch is given its own, so what the last one spent is not held \
             against it"
        );

        finish_main_window_creation();
        assert!(
            !PENDING_MAXIMIZED.load(Ordering::Acquire),
            "the build takes the state back out, so a launch that never reached its \
             window cannot leave the next one holding a show back"
        );
    }
}
