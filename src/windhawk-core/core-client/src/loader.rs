//! `CoreLibrary`: a loaded DLL with the ABI integer gated, exposing the
//! session-free entry points (`get_info_json` raw, `invoke_stateless`) and
//! session creation.

use std::ffi::{c_char, c_void};
use std::path::Path;
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
    /// only VERSION gate; `contractVersion` is consumer policy). On a mismatch
    /// the client refuses to run.
    ///
    /// Only a fully qualified path is loaded. The OS loader resolves anything
    /// else through its search order, which reaches the current directory and
    /// PATH, so a missing core would run whichever DLL an unvetted directory
    /// offers - and it runs at load, before the ABI gate has anything to say
    /// about it. Every consumer loads through here, so the refusal sits here
    /// rather than in each of them.
    pub fn load(dll_path: &str) -> Result<CoreLibrary, ClientError> {
        if !Path::new(dll_path).is_absolute() {
            return Err(ClientError::load(format!(
                "refusing to load a path that is not absolute ({dll_path}): the OS loader \
                 would search the current directory and PATH for it"
            )));
        }
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
        let config = to_cstring(config_json, "config")?;
        // Nothing fallible may sit between here and session_create: an early
        // return would leak the ctx and the consumer's closures with it.
        let ctx = Box::into_raw(Box::new(CallbackCtx {
            log: callbacks.log,
            event: callbacks.event,
        }));
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

#[cfg(test)]
mod tests {
    use super::CoreLibrary;
    use crate::error::ClientErrorKind;

    #[test]
    fn load_refuses_every_path_the_os_loader_would_search_for() {
        // A relative path is searched too, not just a bare name: the loader
        // combines it with each directory of the standard search order. A
        // rooted path carries no drive, so it resolves against the current one.
        for dll_path in [
            "windhawk-core.dll",
            r".\windhawk-core.dll",
            "core/windhawk-core.dll",
            r"\windhawk-core.dll",
        ] {
            // CoreLibrary is not Debug, so destructure rather than unwrap_err.
            let Err(error) = CoreLibrary::load(dll_path) else {
                panic!("{dll_path} should not have been loaded");
            };
            assert!(
                matches!(error.kind(), ClientErrorKind::Load(message) if message.contains(dll_path)),
                "{dll_path} should be refused, got: {error}"
            );
        }
    }
}
