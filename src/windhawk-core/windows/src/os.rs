//! The raw `GetLastError`/`SetLastError` wrappers: the one crate-internal home
//! for the Win32 last-error idiom. http.rs's former private `last_os_error` and
//! the ini.rs `SetLastError(0)` resets both route through here, so the bare
//! idiom lives once. Deliberately just the two raw wrappers - no formatting and
//! no `HttpError` knowledge (`http::transport` keeps that, calling `last_error`
//! internally), so this module does not grow into a Win32 grab-bag.

use windows_sys::Win32::Foundation::{GetLastError, SetLastError};

/// Read the calling thread's last-error code (`GetLastError`).
pub fn last_error() -> u32 {
    // SAFETY: GetLastError reads thread-local state and is always safe to call.
    unsafe { GetLastError() }
}

/// Reset the calling thread's last-error to 0 (`SetLastError(0)`), so a
/// subsequent call that succeeds while setting no error is distinguishable from
/// a stale prior error.
pub fn clear_last_error() {
    // SAFETY: SetLastError writes thread-local state and is always safe to call.
    unsafe { SetLastError(0) };
}
