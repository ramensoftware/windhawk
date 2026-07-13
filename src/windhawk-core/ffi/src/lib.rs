//! The exported C ABI of windhawk-core.dll and nothing else: argument
//! marshaling, `catch_unwind` at every entry, the `WhCoreFree` allocator
//! contract, callback trampolines, and the composition root that wires Win32
//! adapters into a session.
//!
//! Rules: no JSON parsing and no command knowledge here - requests, responses,
//! and the session config are opaque strings; all JSON handling happens in the
//! `windhawk-core` crate. This is one of the two crates where `unsafe` is
//! permitted.

#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case)]
// The safety contract of every export is the ABI specification, restated in the
// function docs that flow into the generated C header; per-function `# Safety`
// sections would duplicate it into the header as noise.
#![allow(clippy::missing_safety_doc)]

mod strings;

use std::ffi::{c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use windhawk_core::{CoreError, Deps, HostCallbacks, Session, core_info_json};
use windhawk_core_windows::{
    RealProcesses, SystemClock, WindowsFiles, WindowsHttp, WindowsNamedLock, WindowsStorageProvider,
};

use crate::strings::{borrow_utf8, free_owned_string, give_string};

/// ABI compatibility gate; bumped only on breaking C-surface changes.
pub const WHCORE_ABI_VERSION: i32 = 2;

/// Opaque session handle; a `Box<Session>` round-tripped through the ABI
/// (there is no global session table).
pub struct WhCoreSession {
    _private: [u8; 0],
}

/// Log callback: `level` is 0=error, 1=warn, 2=info; `message` is UTF-8,
/// borrowed for the duration of the call.
pub type WhCoreLogCallback =
    Option<unsafe extern "C" fn(ctx: *mut c_void, level: i32, message: *const c_char)>;

/// Event callback: `event_json` is one event document of the operation
/// `op_id`, borrowed for the duration of the call.
pub type WhCoreEventCallback =
    Option<unsafe extern "C" fn(ctx: *mut c_void, op_id: u64, event_json: *const c_char)>;

/// Borrow the `Session` behind an opaque handle.
///
/// # Safety
/// `session` must be null or a pointer returned by `WhCoreSessionCreate` and
/// not yet destroyed; such a pointer is a valid `Box<Session>`. Null is
/// tolerated and yields `None`. The returned reference must not outlive the
/// session.
unsafe fn session_ref<'a>(session: *mut WhCoreSession) -> Option<&'a Session> {
    // SAFETY: the caller upholds the contract above; `as_ref` then yields a
    // valid borrow, or `None` for null.
    unsafe { session.cast::<Session>().as_ref() }
}

// The ffi crate builds no JSON itself; error envelopes come from the core's
// serializer.
fn err_envelope_json(error: &CoreError) -> String {
    windhawk_core::error_envelope_json(error)
}

/// Returns the ABI version of this DLL.
#[unsafe(no_mangle)]
pub extern "C" fn WhCoreGetAbiVersion() -> i32 {
    WHCORE_ABI_VERSION
}

/// Returns static info: `{"contractVersion": "...", "coreVersion": "..."}`.
/// Free with `WhCoreFree`.
#[unsafe(no_mangle)]
pub extern "C" fn WhCoreGetInfoJson() -> *mut c_char {
    catch_unwind(|| give_string(core_info_json())).unwrap_or(std::ptr::null_mut())
}

/// Creates a session from a UTF-8 JSON config document. On success returns 0
/// and sets `*out_session`. On failure returns nonzero and, when
/// `out_error_json` is non-null, sets it to an error envelope (free with
/// `WhCoreFree`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreSessionCreate(
    config_json: *const c_char,
    log_cb: WhCoreLogCallback,
    log_ctx: *mut c_void,
    event_cb: WhCoreEventCallback,
    event_ctx: *mut c_void,
    out_session: *mut *mut WhCoreSession,
    out_error_json: *mut *mut c_char,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if out_session.is_null() {
            return Err(CoreError::invalid_request("outSession must not be null"));
        }
        // SAFETY: out_session is non-null and, per the contract, writable.
        unsafe { out_session.write(std::ptr::null_mut()) };

        // SAFETY: config_json is a NUL-terminated string per the contract;
        // borrow_utf8 rejects null and invalid UTF-8.
        let config = unsafe { borrow_utf8(config_json) }
            .ok_or_else(|| CoreError::invalid_request("config must be valid UTF-8 JSON"))?;

        let callbacks = make_host_callbacks(log_cb, log_ctx, event_cb, event_ctx);
        let deps = Deps {
            clock: Arc::new(SystemClock),
            processes: Arc::new(RealProcesses),
            storage: Arc::new(WindowsStorageProvider),
            installer_language: Arc::new(WindowsStorageProvider),
            files: Arc::new(WindowsFiles),
            named_lock: Arc::new(WindowsNamedLock),
            http: Arc::new(WindowsHttp),
        };
        let session = Session::create(config, callbacks, deps)?;
        // SAFETY: out_session checked non-null above.
        unsafe { out_session.write(Box::into_raw(Box::new(session)).cast()) };
        Ok(())
    }));

    let error = match result {
        Ok(Ok(())) => return 0,
        Ok(Err(e)) => e,
        Err(_) => CoreError::internal("panic during session creation"),
    };
    if !out_error_json.is_null() {
        // SAFETY: out_error_json is non-null and writable per the contract.
        unsafe { out_error_json.write(give_string(err_envelope_json(&error))) };
    }
    1
}

/// Destroys a session: blocks until in-flight synchronous calls return and
/// async operations are canceled and joined. No callbacks fire after this
/// returns. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreSessionDestroy(session: *mut WhCoreSession) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        // SAFETY: per the contract, `session` came from WhCoreSessionCreate
        // and is destroyed at most once; reclaiming the Box is the inverse
        // of the into_raw in create.
        let session = unsafe { Box::from_raw(session.cast::<Session>()) };
        session.shutdown();
        drop(session);
    }));
}

/// Synchronous command. Returns a response envelope; never returns null
/// for valid (non-null) arguments. Free with `WhCoreFree`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreInvoke(
    session: *mut WhCoreSession,
    request_json: *const c_char,
) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let response = match (
            // SAFETY: per the ABI contract `session` is null or a live
            // session handle from WhCoreSessionCreate.
            unsafe { session_ref(session) },
            // SAFETY: request_json is NUL-terminated per the contract;
            // borrow_utf8 rejects null and invalid UTF-8.
            unsafe { borrow_utf8(request_json) },
        ) {
            (Some(session), Some(request)) => session.invoke(request),
            (None, _) => err_envelope_json(&CoreError::invalid_request("session must not be null")),
            (_, None) => err_envelope_json(&CoreError::invalid_request(
                "request must be valid UTF-8 JSON",
            )),
        };
        give_string(response)
    }));
    result.unwrap_or_else(|_| {
        give_string(err_envelope_json(&CoreError::internal(
            "panic during invoke",
        )))
    })
}

/// Stateless synchronous command: a session-free transport serving only the
/// pure helpers (`parseModSource`, `appendToModIdAndName`, `getCompileFlags`).
/// Needs no app root, so it lets a consumer parse a `.wh.cpp` with no Windhawk
/// environment. Returns a response envelope; never returns null for a non-null
/// request. A storage-bearing command is rejected with INVALID_REQUEST. Free
/// with `WhCoreFree`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreInvokeStateless(request_json: *const c_char) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let response = match
            // SAFETY: request_json is NUL-terminated per the contract;
            // borrow_utf8 rejects null and invalid UTF-8.
            unsafe { borrow_utf8(request_json) }
        {
            Some(request) => windhawk_core::invoke_stateless(request),
            None => err_envelope_json(&CoreError::invalid_request(
                "request must be valid UTF-8 JSON",
            )),
        };
        give_string(response)
    }));
    result.unwrap_or_else(|_| {
        give_string(err_envelope_json(&CoreError::internal(
            "panic during invokeStateless",
        )))
    })
}

/// Asynchronous command. On success returns a nonzero operation id; events
/// arrive via the session event callback. On failure returns 0 and, when
/// `out_error_json` is non-null, sets it to an error envelope (free with
/// `WhCoreFree`); no events are emitted for failed starts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreInvokeAsync(
    session: *mut WhCoreSession,
    request_json: *const c_char,
    out_error_json: *mut *mut c_char,
) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        match (
            // SAFETY: per the ABI contract `session` is null or a live
            // session handle from WhCoreSessionCreate.
            unsafe { session_ref(session) },
            // SAFETY: request_json is NUL-terminated per the contract.
            unsafe { borrow_utf8(request_json) },
        ) {
            (Some(session), Some(request)) => session.invoke_async(request),
            (None, _) => Err(CoreError::invalid_request("session must not be null")),
            (_, None) => Err(CoreError::invalid_request(
                "request must be valid UTF-8 JSON",
            )),
        }
    }));
    let error = match result {
        Ok(Ok(op_id)) => return op_id,
        Ok(Err(e)) => e,
        Err(_) => CoreError::internal("panic during invokeAsync"),
    };
    if !out_error_json.is_null() {
        // SAFETY: out_error_json is non-null and writable per the contract.
        unsafe { out_error_json.write(give_string(err_envelope_json(&error))) };
    }
    0
}

/// Cooperative cancel. Returns 0 if the operation was found and signaled;
/// nonzero if the id is unknown or already terminal (a harmless no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreCancel(session: *mut WhCoreSession, op_id: u64) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: per the ABI contract `session` is null or a live session
        // handle from WhCoreSessionCreate.
        match unsafe { session_ref(session) } {
            Some(session) if session.cancel(op_id) => 0,
            _ => 1,
        }
    }));
    result.unwrap_or(1)
}

/// Frees any `char*` returned by this DLL. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn WhCoreFree(p: *mut c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: per the contract, `p` is null or a string returned by
        // this DLL (allocated by give_string) and freed at most once.
        unsafe { free_owned_string(p) };
    }));
}

/// Debug builds only: the number of live strings handed out and not yet freed,
/// for the ABI suite's memory-ownership balance check. Not part of the stable
/// ABI.
#[cfg(debug_assertions)]
#[unsafe(no_mangle)]
pub extern "C" fn WhCoreDebugGetLiveStringCount() -> i64 {
    strings::live_string_count()
}

fn make_host_callbacks(
    log_cb: WhCoreLogCallback,
    log_ctx: *mut c_void,
    event_cb: WhCoreEventCallback,
    event_ctx: *mut c_void,
) -> HostCallbacks {
    // Contexts are opaque caller-owned pointers; the contract requires the
    // callbacks to be callable from any thread until WhCoreSessionDestroy
    // returns, which is exactly the dispatcher thread's lifetime.
    struct SendPtr(*mut c_void);
    // SAFETY: the pointer is never dereferenced by the core; it is only
    // passed back to the host callback, which the contract declares
    // thread-safe.
    unsafe impl Send for SendPtr {}
    impl SendPtr {
        // A method (not a field access), so closures capture the Send
        // wrapper rather than the raw pointer field (disjoint capture).
        fn get(&self) -> *mut c_void {
            self.0
        }
    }

    let log_ctx = SendPtr(log_ctx);
    let event_ctx = SendPtr(event_ctx);

    HostCallbacks {
        log: Box::new(move |level, message| {
            if let Some(cb) = log_cb {
                let message = strings::to_cstring_lossy(message);
                // SAFETY: cb and ctx come from WhCoreSessionCreate; the
                // string is valid for the duration of the call (borrowed,
                // per the memory ownership rules).
                unsafe { cb(log_ctx.get(), level as i32, message.as_ptr()) };
            }
        }),
        event: Box::new(move |op_id, event_json| {
            if let Some(cb) = event_cb {
                let event_json = strings::to_cstring_lossy(event_json);
                // SAFETY: as above.
                unsafe { cb(event_ctx.get(), op_id, event_json.as_ptr()) };
            }
        }),
    }
}
