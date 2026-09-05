//! `CoreSession`: a live session handle (sync invoke, async start, cancel,
//! destroy) and the callback trampolines that copy the borrowed callback
//! strings and forward them to consumer-supplied closures.

use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

use crate::api::{CoreApi, to_cstring};
use crate::error::ClientError;

/// Host callbacks for a session. Both fire on core-owned threads (the contract
/// guarantees serial delivery on a single dispatcher thread, never on the
/// calling thread), so they must be `Send`. core-client owns the C trampolines
/// and hands each closure an OWNED copy of the borrowed callback string. The
/// consumer chooses the delivery strategy - the bridge forwards to a
/// threadsafe-function, the CLI sends over an mpsc channel - so core-client
/// stays out of that choice. A closure that panics loses that one callback and
/// nothing else: the trampolines contain it rather than let it abort the host.
pub struct SessionCallbacks {
    pub log: Box<dyn Fn(i32, String) + Send>,
    pub event: Box<dyn Fn(u64, String) + Send>,
}

/// The per-session context handed to the DLL as the callback `ctx` pointer;
/// owned by `CoreSession` and freed after destroy.
pub(crate) struct CallbackCtx {
    pub(crate) log: Box<dyn Fn(i32, String) + Send>,
    pub(crate) event: Box<dyn Fn(u64, String) + Send>,
}

pub(crate) unsafe extern "C" fn log_trampoline(
    ctx: *mut c_void,
    level: i32,
    message: *const c_char,
) {
    if ctx.is_null() || message.is_null() {
        return;
    }
    // SAFETY: ctx is the CallbackCtx of a live session; no callbacks fire after
    // destroy returns.
    let ctx = unsafe { &*ctx.cast::<CallbackCtx>() };
    // SAFETY: per the ABI's memory rules the DLL passes a NUL-terminated string
    // that stays valid for this call; it is copied here and not retained.
    let message = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    // A panic escaping this extern "C" frame aborts the whole host process, so
    // an unwinding consumer closure drops its callback instead. AssertUnwindSafe:
    // the closure is the consumer's own state to keep consistent, and the DLL
    // keeps calling here either way.
    let _ = catch_unwind(AssertUnwindSafe(|| (ctx.log)(level, message)));
}

pub(crate) unsafe extern "C" fn event_trampoline(
    ctx: *mut c_void,
    op_id: u64,
    event_json: *const c_char,
) {
    if ctx.is_null() || event_json.is_null() {
        return;
    }
    // SAFETY: as in log_trampoline.
    let ctx = unsafe { &*ctx.cast::<CallbackCtx>() };
    // SAFETY: as in log_trampoline.
    let event_json = unsafe { CStr::from_ptr(event_json) }
        .to_string_lossy()
        .into_owned();
    // Contained as in log_trampoline.
    let _ = catch_unwind(AssertUnwindSafe(|| (ctx.event)(op_id, event_json)));
}

/// A live session handle. The raw response envelope is returned UNPARSED (the
/// `{ok,result}` / `{ok:false,error}` split is consumer policy). `Send + Sync`
/// so a consumer can run a blocking `invoke` off-thread (the bridge's libuv
/// worker pool); the underlying session is thread-safe per the ABI.
pub struct CoreSession {
    api: Arc<CoreApi>,
    /// Shared by every call into the DLL, exclusive for `destroy`: the ABI
    /// makes a call that starts after destroy has begun undefined behavior, and
    /// a safe `Sync` API cannot leave that to caller discipline, so the handle
    /// stays claimed for as long as a caller is inside the DLL. The gate guards
    /// no data, so a poisoned lock carries no broken invariant and both sides
    /// take the guard out of the poison.
    gate: RwLock<()>,
    session: AtomicPtr<c_void>,
    ctx: AtomicPtr<CallbackCtx>,
}

/// A session pointer with the shared side of the teardown gate attached: the
/// handle cannot be freed until this is dropped.
struct LiveSession<'a> {
    session: *mut c_void,
    _gate: RwLockReadGuard<'a, ()>,
}

impl CoreSession {
    pub(crate) fn new(
        api: Arc<CoreApi>,
        session: *mut c_void,
        ctx: *mut CallbackCtx,
    ) -> CoreSession {
        CoreSession {
            api,
            gate: RwLock::new(()),
            session: AtomicPtr::new(session),
            ctx: AtomicPtr::new(ctx),
        }
    }

    fn live(&self) -> Result<LiveSession<'_>, ClientError> {
        let gate = self.gate.read().unwrap_or_else(PoisonError::into_inner);
        let session = self.session.load(Ordering::SeqCst);
        if session.is_null() {
            Err(ClientError::destroyed())
        } else {
            Ok(LiveSession {
                session,
                _gate: gate,
            })
        }
    }

    /// True once the session has been destroyed; a consumer can fail fast
    /// before scheduling a blocking `invoke` off-thread. Advisory and lock-free:
    /// a destroy that lands after the check makes the call itself fail instead.
    pub fn is_destroyed(&self) -> bool {
        self.session.load(Ordering::SeqCst).is_null()
    }

    /// Synchronous invoke (`WhCoreInvoke`). May block (file locks, network), so
    /// the caller decides whether to run it off-thread. Returns the raw
    /// response envelope string.
    pub fn invoke(&self, request_json: &str) -> Result<String, ClientError> {
        let live = self.live()?;
        let request = to_cstring(request_json, "request")?;
        // SAFETY: resolved export; the gate guard holds the handle live across
        // the call; take_string frees the returned buffer.
        let response = unsafe {
            let p = (self.api.invoke)(live.session, request.as_ptr());
            self.api.take_string(p)
        };
        response.ok_or_else(|| ClientError::null_result("WhCoreInvoke"))
    }

    /// Start an async operation (`WhCoreInvokeAsync`); returns the nonzero
    /// operation id. A failed start yields `ClientError::Envelope` carrying the
    /// raw error envelope; no events are emitted for it.
    pub fn invoke_async(&self, request_json: &str) -> Result<u64, ClientError> {
        let live = self.live()?;
        let request = to_cstring(request_json, "request")?;
        let mut error: *mut c_char = std::ptr::null_mut();
        // SAFETY: resolved export; the gate guard holds the handle live across
        // the call; error is null or owned by us after it.
        let op_id = unsafe { (self.api.invoke_async)(live.session, request.as_ptr(), &mut error) };
        if op_id == 0 {
            // SAFETY: error is null or a string the DLL handed to us.
            let envelope = unsafe { self.api.take_string(error) };
            return Err(ClientError::envelope(envelope.unwrap_or_else(|| {
                "WhCoreInvokeAsync failed without an error document".to_owned()
            })));
        }
        Ok(op_id)
    }

    /// Cooperative cancel (`WhCoreCancel`); true if the operation was found and
    /// signaled. A destroyed session is a harmless `false`.
    pub fn cancel(&self, op_id: u64) -> bool {
        let Ok(live) = self.live() else {
            return false;
        };
        // SAFETY: resolved export; the gate guard holds the handle live across
        // the call.
        unsafe { (self.api.cancel)(live.session, op_id) == 0 }
    }

    /// Destroy the session (`WhCoreSessionDestroy`): blocks until in-flight
    /// work is drained; no callbacks fire afterwards. Idempotent. A call
    /// another thread is already making is waited out, not cut short.
    pub fn destroy(&self) {
        let _gate = self.gate.write().unwrap_or_else(PoisonError::into_inner);
        let session = self.session.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !session.is_null() {
            // SAFETY: resolved export; this pointer is destroyed exactly once
            // (the swap above claimed it) and no call holds it, since the
            // exclusive gate outlasts every one of them.
            unsafe { (self.api.session_destroy)(session) };
        }
        let ctx = self.ctx.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !ctx.is_null() {
            // SAFETY: after WhCoreSessionDestroy returns, no callback can
            // reference ctx; the swap claimed the only owning pointer, and the
            // exclusive gate keeps a second destroy out of the window between
            // the two swaps.
            drop(unsafe { Box::from_raw(ctx) });
        }
    }
}

impl Drop for CoreSession {
    fn drop(&mut self) {
        // A leaked/last-Arc-dropped session still tears down safely; consumers
        // call destroy() explicitly for deterministic teardown.
        self.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::{CallbackCtx, event_trampoline, log_trampoline};
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Without the guard this does not fail, it ABORTS the test process: the
    /// panic reaches the trampoline's extern "C" frame.
    #[test]
    fn a_panicking_consumer_closure_stays_inside_the_trampoline() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (log_calls, event_calls) = (Arc::clone(&calls), Arc::clone(&calls));
        let mut ctx = CallbackCtx {
            log: Box::new(move |_, _| {
                log_calls.fetch_add(1, Ordering::SeqCst);
                panic!("log callback");
            }),
            event: Box::new(move |_, _| {
                event_calls.fetch_add(1, Ordering::SeqCst);
                panic!("event callback");
            }),
        };
        let ctx_ptr = std::ptr::from_mut(&mut ctx).cast::<c_void>();

        // SAFETY: ctx_ptr is a live CallbackCtx untouched for the duration of
        // the calls, and each string is NUL-terminated and borrowed only by the
        // call it is passed to, as the ABI requires.
        unsafe {
            log_trampoline(ctx_ptr, 0, c"message".as_ptr());
            event_trampoline(ctx_ptr, 7, c"{}".as_ptr());
        }

        assert_eq!(calls.load(Ordering::SeqCst), 2, "both closures ran");
    }
}
