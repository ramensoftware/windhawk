//! `CoreLibrary`: a loaded DLL with the ABI integer gated, exposing the
//! session-free entry points (`get_info_json` raw, `invoke_stateless`) and
//! session creation.

use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Arc;

use crate::api::{CoreApi, to_cstring};
use crate::error::ClientError;
use crate::session::{
    CallbackCtx, CoreSession, SessionCallbacks, event_trampoline, log_trampoline,
};

/// The ABI integer this client is built against. core-client hard-gates ONLY
/// this; the `contractVersion` check is consumer policy. Kept in lockstep with
/// the DLL's `WhCoreGetAbiVersion`.
pub const EXPECTED_ABI_VERSION: i32 = 2;

/// A loaded `windhawk-core.dll`. Holds the library and its resolved exports;
/// the ABI integer was gated at load.
pub struct CoreLibrary {
    api: Arc<CoreApi>,
}

impl CoreLibrary {
    /// Load the DLL, resolve its exports, and hard-gate the ABI integer (the
    /// only hard gate). On a mismatch the client refuses to run.
    pub fn load(dll_path: &str) -> Result<CoreLibrary, ClientError> {
        let api = CoreApi::load(dll_path)?;
        // SAFETY: a resolved export of the documented signature.
        let abi = unsafe { (api.get_abi_version)() };
        if abi != EXPECTED_ABI_VERSION {
            return Err(ClientError::abi_mismatch(abi, EXPECTED_ABI_VERSION));
        }
        Ok(CoreLibrary { api: Arc::new(api) })
    }

    /// The DLL's ABI integer (already gated equal to [`EXPECTED_ABI_VERSION`]
    /// at load; exposed for consumers that surface it).
    pub fn abi_version(&self) -> i32 {
        // SAFETY: resolved export.
        unsafe { (self.api.get_abi_version)() }
    }

    /// Raw `getCoreInfo` JSON (`{"contractVersion":..,"coreVersion":..}`),
    /// returned verbatim: the `contractVersion` is the consumer's to check, so
    /// the bridge keeps its TS-side graceful fallback.
    pub fn get_info_json(&self) -> Result<String, ClientError> {
        // SAFETY: resolved export; take_string frees the returned buffer.
        let info = unsafe {
            let p = (self.api.get_info_json)();
            self.api.take_string(p)
        };
        info.ok_or_else(|| ClientError::null_result("WhCoreGetInfoJson"))
    }

    /// Stateless synchronous invoke (`WhCoreInvokeStateless`): serves the
    /// session-free pure helpers with no session. Returns the raw response
    /// envelope string.
    pub fn invoke_stateless(&self, request_json: &str) -> Result<String, ClientError> {
        let request = to_cstring(request_json, "request")?;
        // SAFETY: resolved export; take_string frees the returned buffer.
        let response = unsafe {
            let p = (self.api.invoke_stateless)(request.as_ptr());
            self.api.take_string(p)
        };
        response.ok_or_else(|| ClientError::null_result("WhCoreInvokeStateless"))
    }

    /// Create a session. The callbacks fire on core-owned threads (never the
    /// calling thread) and stop after the session is destroyed; core-client
    /// copies each borrowed callback string before invoking the closure.
    pub fn create_session(
        &self,
        config_json: &str,
        callbacks: SessionCallbacks,
    ) -> Result<CoreSession, ClientError> {
        let ctx = Box::into_raw(Box::new(CallbackCtx {
            log: callbacks.log,
            event: callbacks.event,
        }));
        let config = to_cstring(config_json, "config")?;
        let mut session: *mut c_void = ptr::null_mut();
        let mut error: *mut c_char = ptr::null_mut();
        // SAFETY: resolved export; the trampolines and ctx stay alive until
        // WhCoreSessionDestroy returns (the contract guarantees no callbacks
        // after that), and ctx is freed in destroy().
        let rc = unsafe {
            (self.api.session_create)(
                config.as_ptr(),
                Some(log_trampoline),
                ctx.cast(),
                Some(event_trampoline),
                ctx.cast(),
                &mut session,
                &mut error,
            )
        };
        if rc != 0 {
            // SAFETY: ctx was never given to a live session; no callbacks can
            // reference it.
            drop(unsafe { Box::from_raw(ctx) });
            // SAFETY: error is null or owned by us now.
            let envelope = unsafe { self.api.take_string(error) };
            return Err(ClientError::envelope(envelope.unwrap_or_else(|| {
                "WhCoreSessionCreate failed without an error document".to_owned()
            })));
        }
        Ok(CoreSession::new(self.api.clone(), session, ctx))
    }
}
