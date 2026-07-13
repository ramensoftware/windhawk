//! The `NamedLock` port adapter: a named Win32 mutex for the user-profile
//! read-modify-write. Best effort - a create/wait failure or a timeout yields
//! an unheld guard so the caller still proceeds (the lock only coordinates core
//! sessions; during the migration window neither the TS backend nor the C++ app
//! take it).

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

use windhawk_core_ports::{NamedLock, NamedLockGuard};

use crate::wide::to_wide;

pub struct WindowsNamedLock;

/// Holds the mutex handle (if created) and whether ownership was acquired.
/// Drop releases ownership (if held) and closes the handle. The guard never
/// crosses threads - a Win32 mutex must be released by its owning thread - so
/// it is created and dropped within one synchronous command.
struct MutexGuard {
    handle: HANDLE,
    held: bool,
}

impl NamedLockGuard for MutexGuard {}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        if self.held {
            // SAFETY: handle is a mutex created in `acquire` on this thread,
            // owned by it (WAIT_OBJECT_0/WAIT_ABANDONED). ReleaseMutex on the
            // owning thread is the correct release.
            unsafe { ReleaseMutex(self.handle) };
        }
        // SAFETY: handle was created here and is closed exactly once.
        unsafe { CloseHandle(self.handle) };
    }
}

impl NamedLock for WindowsNamedLock {
    fn acquire(&self, name: &str, timeout_ms: u32) -> Box<dyn NamedLockGuard> {
        let name_w = to_wide(name);
        // SAFETY: name_w is NUL-terminated; null attributes and no initial
        // owner are valid arguments. Returns NULL on failure.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name_w.as_ptr()) };
        if handle.is_null() {
            return Box::new(MutexGuard {
                handle: std::ptr::null_mut(),
                held: false,
            });
        }
        // SAFETY: handle is a valid mutex; wait up to timeout_ms.
        let wait = unsafe { WaitForSingleObject(handle, timeout_ms) };
        // WAIT_ABANDONED means a previous owner died without releasing; we now
        // own the (consistent-enough) mutex, so treat it as held.
        let held = wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED;
        Box::new(MutexGuard { handle, held })
    }
}
