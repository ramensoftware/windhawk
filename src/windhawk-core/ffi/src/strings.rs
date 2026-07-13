//! The string-ownership half of the ABI: strings returned by the DLL are
//! `CString` allocations handed to the caller and reclaimed by
//! `WhCoreFree`; strings passed in are borrowed for the duration of the
//! call. Debug builds keep a live-allocation counter for the ABI suite's
//! balance check.

use std::ffi::{CStr, CString, c_char};

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicI64, Ordering};

// Debug-only instrumentation ("allocation counters balanced in debug builds");
// the release DLL has no process-global mutable state.
#[cfg(debug_assertions)]
static LIVE_STRINGS: AtomicI64 = AtomicI64::new(0);

#[cfg(debug_assertions)]
pub fn live_string_count() -> i64 {
    LIVE_STRINGS.load(Ordering::SeqCst)
}

/// NUL bytes cannot occur in the JSON the core produces (serde_json
/// escapes control characters), but log messages are arbitrary; replace
/// rather than fail.
pub fn to_cstring_lossy(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| {
        let sanitized: String = s
            .chars()
            .map(|c| if c == '\0' { '\u{fffd}' } else { c })
            .collect();
        CString::new(sanitized).unwrap_or_default()
    })
}

/// Allocate an owned, caller-freed string.
pub fn give_string(s: String) -> *mut c_char {
    #[cfg(debug_assertions)]
    LIVE_STRINGS.fetch_add(1, Ordering::SeqCst);
    to_cstring_lossy(&s).into_raw()
}

/// Reclaim a string produced by [`give_string`].
///
/// # Safety
/// `p` must be null or a pointer returned by this DLL and not yet freed.
pub unsafe fn free_owned_string(p: *mut c_char) {
    if p.is_null() {
        return;
    }
    #[cfg(debug_assertions)]
    LIVE_STRINGS.fetch_sub(1, Ordering::SeqCst);
    // SAFETY: per the caller contract, p came from CString::into_raw.
    drop(unsafe { CString::from_raw(p) });
}

/// Borrow a NUL-terminated UTF-8 string from the caller; `None` for null
/// or invalid UTF-8.
///
/// # Safety
/// `p` must be null or a valid NUL-terminated string that outlives the
/// borrow.
pub unsafe fn borrow_utf8<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    // SAFETY: per the caller contract, p is NUL-terminated and valid.
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}
