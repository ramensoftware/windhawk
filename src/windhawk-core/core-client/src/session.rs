//! `CoreSession`: a live session handle (sync invoke, async start, cancel,
//! destroy) and the callback trampolines that copy the borrowed callback
//! strings and forward them to consumer-supplied closures.

use std::ffi::{CStr, c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::api::{CoreApi, to_cstring};
use crate::error::ClientError;

/// Host callbacks for a session. Both fire on core-owned threads (the contract
/// guarantees serial delivery on a single dispatcher thread, never on the
/// calling thread), so they must be `Send`. core-client owns the C trampolines
/// and hands each closure an OWNED copy of the borrowed callback string. The
/// consumer chooses the delivery strategy - the bridge forwards to a
/// threadsafe-function, the CLI sends over an mpsc channel - so core-client
/// stays out of that choice.
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
    // SAFETY: ctx is the CallbackCtx of a live session (no callbacks fire after
    // destroy returns); the message is borrowed only for this call and copied
    // here, per the ABI's memory rules.
    let ctx = unsafe { &*ctx.cast::<CallbackCtx>() };
    let message = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned();
    (ctx.log)(level, message);
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
    let event_json = unsafe { CStr::from_ptr(event_json) }
        .to_string_lossy()
        .into_owned();
    (ctx.event)(op_id, event_json);
}

/// A live session handle. The raw response envelope is returned UNPARSED (the
/// `{ok,result}` / `{ok:false,error}` split is consumer policy). `Send + Sync`
/// so a consumer can run a blocking `invoke` off-thread (the bridge's libuv
/// worker pool); the underlying session is thread-safe per the ABI.
pub struct CoreSession {
    api: Arc<CoreApi>,
    session: AtomicPtr<c_void>,
    ctx: AtomicPtr<CallbackCtx>,
}

impl CoreSession {
    pub(crate) fn new(
        api: Arc<CoreApi>,
        session: *mut c_void,
        ctx: *mut CallbackCtx,
    ) -> CoreSession {
        CoreSession {
            api,
            session: AtomicPtr::new(session),
            ctx: AtomicPtr::new(ctx),
        }
    }

    fn live(&self) -> Result<*mut c_void, ClientError> {
        let p = self.session.load(Ordering::SeqCst);
        if p.is_null() {
            Err(ClientError::destroyed())
        } else {
            Ok(p)
        }
    }

    /// True once the session has been destroyed; a consumer can fail fast
    /// before scheduling a blocking `invoke` off-thread.
    pub fn is_destroyed(&self) -> bool {
        self.session.load(Ordering::SeqCst).is_null()
    }

    /// Synchronous invoke (`WhCoreInvoke`). May block (file locks, network), so
    /// the caller decides whether to run it off-thread. Returns the raw
    /// response envelope string.
    pub fn invoke(&self, request_json: &str) -> Result<String, ClientError> {
        let session = self.live()?;
        let request = to_cstring(request_json, "request")?;
        // SAFETY: resolved export; the session is live (the contract forbids
        // invoke after destroy); take_string frees the returned buffer.
        let response = unsafe {
            let p = (self.api.invoke)(session, request.as_ptr());
            self.api.take_string(p)
        };
        response.ok_or_else(|| ClientError::null_result("WhCoreInvoke"))
    }

    /// Start an async operation (`WhCoreInvokeAsync`); returns the nonzero
    /// operation id. A failed start yields `ClientError::Envelope` carrying the
    /// raw error envelope; no events are emitted for it.
    pub fn invoke_async(&self, request_json: &str) -> Result<u64, ClientError> {
        let session = self.live()?;
        let request = to_cstring(request_json, "request")?;
        let mut error: *mut c_char = std::ptr::null_mut();
        // SAFETY: resolved export; error is null or owned by us after the call.
        let op_id = unsafe { (self.api.invoke_async)(session, request.as_ptr(), &mut error) };
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
        let Ok(session) = self.live() else {
            return false;
        };
        // SAFETY: resolved export.
        unsafe { (self.api.cancel)(session, op_id) == 0 }
    }

    /// Destroy the session (`WhCoreSessionDestroy`): blocks until in-flight
    /// work is drained; no callbacks fire afterwards. Idempotent.
    pub fn destroy(&self) {
        let session = self.session.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !session.is_null() {
            // SAFETY: resolved export; this pointer is destroyed exactly once
            // (the swap above claimed it).
            unsafe { (self.api.session_destroy)(session) };
        }
        let ctx = self.ctx.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !ctx.is_null() {
            // SAFETY: after WhCoreSessionDestroy returns, no callback can
            // reference ctx; the swap claimed the only owning pointer.
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
