//! The launcher contract Rust side and the fatal startup presentation. The
//! single-instance plugin (registered in `lib.rs`) makes a bare re-launch
//! ensure-running-and-foreground; this module holds the small Win32 + window
//! helpers it drives: the process AppUserModelID, the foreground hand-off, the
//! bring-to-front, the `Local\WindhawkUI` mutex the UI reads to spot a second
//! instance, and the native message box for a startup failure (there is no
//! webview yet to show it).

use std::path::Path;

use tauri::{AppHandle, Manager};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetEffectiveRightsFromAclW, GetNamedSecurityInfoW,
    NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID,
    TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    ACL, AllocateAndInitializeSid, CONTAINER_INHERIT_ACE, CheckTokenMembership,
    DACL_SECURITY_INFORMATION, FreeSid, GetTokenInformation, OBJECT_INHERIT_ACE,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_NT_AUTHORITY, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, MB_ICONERROR, MB_OK, MB_SYSTEMMODAL, MessageBoxW,
};

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
/// same membership test `IsUserAnAdmin` performs. Drives the main window title -
/// a non-portable install that is not elevated cannot fully manage the
/// system-wide engine, so the title flags it. On any failure building or
/// checking the SID, reports false (treat an indeterminate state as "not
/// admin").
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

/// Make sure WebView2's browser-profile folder exists and is writable by the
/// current user before Tauri hands the data directory to WebView2.
///
/// WebView2 keeps its profile in an `EBWebView` subtree of the UI data directory
/// (`UI_DATA_SUBDIR`). That directory sits under Windhawk's AppData, which a
/// system install shares across users, and a non-portable install is expected to
/// run elevated - so the profile folder can first be created by an elevated or
/// different-user run whose DACL leaves the current user without write access,
/// after which WebView2 fails to initialize against the existing profile. Create
/// the folder when it is missing and grant the current user write access so the
/// profile is usable whoever created it.
///
/// The writability test is on the user's SID in the folder's DACL, not on this
/// process's access. Windhawk normally runs elevated, and an elevated process can
/// write to a folder that the same user's non-elevated (standard) token cannot -
/// yet WebView2's browser process runs at the window's integrity and needs the
/// standard-user access. Probing this process would report writable and skip the
/// fix that a later non-elevated run depends on.
///
/// Returns the create error when the folder is missing and cannot be created - a
/// non-elevated run against a system AppData that grants Users only read+execute -
/// so the caller can present it before WebView2 hits the same denial opaquely. The
/// DACL grant stays best effort: if it cannot be rewritten because we hold neither
/// ownership nor WRITE_DAC, WebView2 fails as it would have without it, and there
/// is nothing better to attempt from here.
pub fn ensure_webview_profile_writable(profile_dir: &Path) -> std::io::Result<()> {
    if profile_dir.is_dir() && current_user_can_write(profile_dir) {
        return Ok(());
    }
    // create_dir_all is a no-op when the folder is present but the user lacks
    // write; when it is missing we become the owner with full control, and the
    // grant below is then redundant but harmless.
    std::fs::create_dir_all(profile_dir)?;
    grant_current_user_write(profile_dir);
    Ok(())
}

/// Whether the current process's user SID is granted write access by `dir`'s DACL,
/// evaluated for that SID directly (`GetEffectiveRightsFromAclW`) rather than for
/// this process's token, so the answer does not depend on whether the process is
/// elevated (see [`ensure_webview_profile_writable`]). On any failure resolving
/// the SID or reading the DACL, reports false so the caller re-applies the grant.
fn current_user_can_write(dir: &Path) -> bool {
    let Some(sid_buffer) = current_user_sid() else {
        return false;
    };
    // SAFETY: current_user_sid returns a buffer that starts with a TOKEN_USER
    // written by GetTokenInformation; User.Sid points to a SID within it, which
    // stays valid as long as sid_buffer does.
    let sid = unsafe { (*(sid_buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
    if sid.is_null() {
        return false;
    }

    let path = wide(&dir.to_string_lossy());

    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: path is a NUL-terminated wide string; requesting DACL info fills dacl
    // (an alias into security_descriptor, which we own and LocalFree below). The
    // owner/group/SACL out-params are unused (null).
    let get = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if get != ERROR_SUCCESS {
        return false;
    }

    // A null (not empty) DACL grants everyone full access, so there is nothing to
    // fix; an empty DACL denies everyone and falls through to rights == 0.
    let writable = if dacl.is_null() {
        true
    } else {
        let trustee = sid_trustee(sid);
        let mut rights: u32 = 0;
        // SAFETY: dacl is the DACL just read; trustee names the user SID, which
        // outlives the call via sid_buffer; rights receives the effective mask.
        let effective = unsafe { GetEffectiveRightsFromAclW(dacl, &trustee, &mut rights) };
        effective == ERROR_SUCCESS && (rights & FILE_GENERIC_WRITE) == FILE_GENERIC_WRITE
    };

    // SAFETY: security_descriptor was allocated by GetNamedSecurityInfoW and is
    // freed once.
    unsafe { LocalFree(security_descriptor) };

    writable
}

/// A `TRUSTEE_W` naming `sid`. For `TRUSTEE_IS_SID` the trustee's name field
/// carries the SID pointer itself, so `sid` (and the buffer backing it) must
/// outlive the returned trustee and any call it is passed to.
fn sid_trustee(sid: PSID) -> TRUSTEE_W {
    TRUSTEE_W {
        pMultipleTrustee: std::ptr::null_mut(),
        MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
        TrusteeForm: TRUSTEE_IS_SID,
        TrusteeType: TRUSTEE_IS_USER,
        ptstrName: sid.cast(),
    }
}

/// Merge an inheritable DACL entry granting the current user modify access into
/// `dir`'s existing DACL, so the files and subfolders WebView2 creates inside the
/// profile inherit it. Needs the current process to own the folder or hold
/// WRITE_DAC; any failure is a silent no-op (best effort, see
/// [`ensure_webview_profile_writable`]).
fn grant_current_user_write(dir: &Path) {
    // The SID lives inside this buffer (TOKEN_USER.User.Sid points into it), so it
    // must outlive every use of `sid` below.
    let Some(sid_buffer) = current_user_sid() else {
        return;
    };
    // SAFETY: current_user_sid returns a buffer that starts with a TOKEN_USER
    // written by GetTokenInformation; User.Sid points to a SID within it.
    let sid = unsafe { (*(sid_buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
    if sid.is_null() {
        return;
    }

    let path = wide(&dir.to_string_lossy());

    // Read the folder's current DACL so the grant is merged into it, not a
    // replacement that would drop the access other users rely on.
    let mut old_dacl: *mut ACL = std::ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: path is a NUL-terminated wide string; requesting DACL info fills
    // old_dacl (an alias into security_descriptor, which we own and LocalFree
    // below). The owner/group/SACL out-params are unused (null).
    let get = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut security_descriptor,
        )
    };
    if get != ERROR_SUCCESS {
        return;
    }

    // Modify access (read + write + execute + delete), inherited by both the files
    // (OBJECT_INHERIT) and subfolders (CONTAINER_INHERIT) WebView2 creates under
    // the profile. Specific file rights, not GENERIC_*, so the stored ACE carries
    // the exact mask that current_user_can_write reads back.
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_GENERIC_READ
            | FILE_GENERIC_WRITE
            | FILE_GENERIC_EXECUTE
            | DELETE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
        Trustee: sid_trustee(sid),
    };

    let mut new_dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: a single explicit-access entry describing `access`; old_dacl is the
    // DACL just read; on success new_dacl receives a LocalAlloc'd merged DACL freed
    // below. `access` (and the SID it points at) outlives the call.
    let set_entries = unsafe { SetEntriesInAclW(1, &access, old_dacl, &mut new_dacl) };
    if set_entries == ERROR_SUCCESS && !new_dacl.is_null() {
        // SAFETY: write the merged DACL back to the folder; path and new_dacl
        // outlive the call. Owner/group/SACL are left untouched (null).
        unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                new_dacl,
                std::ptr::null(),
            );
        }
        // SAFETY: new_dacl was allocated by SetEntriesInAclW and is freed once.
        unsafe { LocalFree(new_dacl.cast()) };
    }

    // SAFETY: security_descriptor was allocated by GetNamedSecurityInfoW and is
    // freed once.
    unsafe { LocalFree(security_descriptor) };
}

/// The current process user's SID, returned inside the `TOKEN_USER` buffer that
/// backs it (the SID is not a standalone allocation - it points into this
/// buffer), or `None` on any failure querying the process token.
fn current_user_sid() -> Option<Vec<u8>> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the call; on
    // success `token` receives a real handle closed on every return path below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }

    // First call sizes the buffer: a null destination is expected to fail and set
    // `needed` to the TOKEN_USER length (the SID length varies, so it cannot be a
    // fixed struct).
    let mut needed: u32 = 0;
    // SAFETY: token is the handle opened above; a null buffer of length 0 only
    // writes the required size into `needed`.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        // SAFETY: token was opened above and is closed exactly once here.
        unsafe { CloseHandle(token) };
        return None;
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: buffer holds `needed` bytes; on success it receives a TOKEN_USER
    // whose User.Sid points inside the same buffer.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    // SAFETY: token was opened above and is closed exactly once here.
    unsafe { CloseHandle(token) };

    (ok != 0).then_some(buffer)
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
    // SAFETY: AllowSetForegroundWindow just takes a process id (ASFW_ANY = any
    // process) and has no other preconditions; the BOOL result (whether a grant was
    // recorded) is advisory and unused.
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
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

/// Present a fatal startup failure as a native modal message box: used when
/// there is no webview yet to render a reply.
pub fn show_fatal(message: &str) {
    let text = wide(message);
    let caption = wide("Windhawk");
    // SAFETY: both strings are NUL-terminated; a null owner HWND is valid for a
    // standalone message box. The return value (which button) is unused.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SYSTEMMODAL,
        );
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Covers the create-and-grant path: a missing profile folder (nested, like the
    // real <appData>\UIMainData\EBWebView) is created and left writable by the
    // current user. The deny-then-fix path (an unwritable folder created by another
    // token) needs a second user or elevation and is not reproducible in a unit
    // test.
    #[test]
    fn ensure_creates_missing_profile_folder_writable() {
        let root = tempfile::tempdir().expect("temp dir");
        let profile = root.path().join("UIMainData").join("EBWebView");
        assert!(!profile.exists());

        ensure_webview_profile_writable(&profile).expect("ensure profile writable");

        assert!(profile.is_dir());
        assert!(current_user_can_write(&profile));
    }

    // An already-present, user-writable folder keeps its contents: whether or not
    // the grant runs, ensuring must not disturb what is there.
    #[test]
    fn ensure_leaves_existing_folder_contents_intact() {
        let root = tempfile::tempdir().expect("temp dir");
        let profile = root.path().join("EBWebView");
        std::fs::create_dir_all(&profile).expect("create profile");
        let marker = profile.join("keep.txt");
        std::fs::write(&marker, b"keep").expect("seed marker");

        ensure_webview_profile_writable(&profile).expect("ensure profile writable");

        assert!(current_user_can_write(&profile));
        assert_eq!(std::fs::read(&marker).expect("read marker"), b"keep");
    }

    // The crux of the fix: a folder the current user was explicitly granted write
    // reads back as writable through the DACL/effective-rights check, since the
    // granted specific rights include FILE_GENERIC_WRITE.
    #[test]
    fn grant_makes_folder_report_writable() {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("granted");
        std::fs::create_dir_all(&dir).expect("create dir");

        grant_current_user_write(&dir);

        assert!(current_user_can_write(&dir));
    }
}
