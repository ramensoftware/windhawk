//! The Node-API bridge to windhawk-core.dll: dumb plumbing only. It exposes
//! session lifecycle and invoke/invokeAsync/cancel to JS and pumps native
//! callbacks onto the JS thread via threadsafe functions. No Windhawk
//! semantics: no command names, no DTO knowledge, no error mapping - all of
//! that lives in the TypeScript client.
//!
//! The raw C ABI transport (loading the DLL, resolving exports, the ABI-integer
//! gate, string marshaling, the callback trampolines, session lifecycle) is the
//! shared `core-client` crate; this bridge is a thin napi shim over it - it
//! builds the JS-side callback delivery (threadsafe functions) and the
//! worker-pool invoke, and forwards everything else. core-client returns the
//! RAW response envelope string, which the bridge hands to its TS client
//! unparsed, and exposes `getInfoJson` raw so the TS client keeps its graceful
//! contractVersion fallback.
//!
//! This crate deliberately sits outside the core layering: it depends on no
//! core crate and reaches the DLL only through the C ABI (via core-client).

#[macro_use]
extern crate napi_derive;

use std::sync::Arc;

use napi::bindgen_prelude::{AsyncTask, FnArgs, Function};
use napi::threadsafe_function::ThreadsafeFunctionCallMode;
use napi::{Env, Task};
use windhawk_core_client::{
    CoreLibrary as ClientLibrary, CoreSession as ClientSession, SessionCallbacks,
};

fn napi_err(message: impl Into<String>) -> napi::Error {
    napi::Error::from_reason(message.into())
}

/// Load windhawk-core.dll and resolve its exports. The ABI version is checked
/// in core-client: the bridge refuses to run on a mismatch.
#[napi]
pub fn load_core(dll_path: String) -> napi::Result<CoreLibrary> {
    let inner = ClientLibrary::load(&dll_path).map_err(|e| napi_err(e.to_string()))?;
    Ok(CoreLibrary {
        inner: Arc::new(inner),
    })
}

#[napi]
pub struct CoreLibrary {
    inner: Arc<ClientLibrary>,
}

#[napi]
impl CoreLibrary {
    #[napi]
    pub fn get_abi_version(&self) -> i32 {
        self.inner.abi_version()
    }

    /// `{"contractVersion": "...", "coreVersion": "..."}`. Returned raw; the TS
    /// client checks contractVersion and falls back gracefully.
    #[napi]
    pub fn get_info_json(&self) -> napi::Result<String> {
        self.inner
            .get_info_json()
            .map_err(|e| napi_err(e.to_string()))
    }

    /// Create a session. `onLog(level, message)` and `onEvent(opId, eventJson)`
    /// are delivered on the JS thread, in order; they stop after `destroy()`
    /// returns.
    #[napi]
    pub fn create_session(
        &self,
        config_json: String,
        #[napi(ts_arg_type = "(level: number, message: string) => void")] on_log: Function<
            '_,
            FnArgs<(i32, String)>,
            (),
        >,
        // The op id crosses as an f64: ids count up from 1, and f64 is exact
        // far beyond any realistic id.
        #[napi(ts_arg_type = "(opId: number, eventJson: string) => void")] on_event: Function<
            '_,
            FnArgs<(f64, String)>,
            (),
        >,
    ) -> napi::Result<CoreSession> {
        // `callee_handled::<false>` is the infallible delivery shape: the
        // payload is the argument tuple itself rather than a Result the JS side
        // has to unwrap. `weak::<true>` keeps the callbacks from holding the
        // Node process open - a session owns them until destroy(), which would
        // otherwise turn a missed dispose() into a hang at exit.
        let log_tsfn = on_log
            .build_threadsafe_function()
            .callee_handled::<false>()
            .weak::<true>()
            .build()?;
        let event_tsfn = on_event
            .build_threadsafe_function()
            .callee_handled::<false>()
            .weak::<true>()
            .build()?;

        // core-client owns the C trampolines and the per-session ctx; the
        // bridge only supplies the JS-thread delivery as Send closures.
        let callbacks = SessionCallbacks {
            log: Box::new(move |level, message| {
                log_tsfn.call(
                    (level, message).into(),
                    ThreadsafeFunctionCallMode::NonBlocking,
                );
            }),
            event: Box::new(move |op_id, event_json| {
                event_tsfn.call(
                    (op_id as f64, event_json).into(),
                    ThreadsafeFunctionCallMode::NonBlocking,
                );
            }),
        };

        let session = self
            .inner
            .create_session(&config_json, callbacks)
            .map_err(|e| napi_err(e.to_string()))?;
        Ok(CoreSession {
            inner: Arc::new(session),
        })
    }
}

#[napi]
pub struct CoreSession {
    inner: Arc<ClientSession>,
}

/// `WhCoreInvoke` may block (file locks, network), so it runs on the libuv
/// worker pool and surfaces as a Promise.
pub struct InvokeTask {
    session: Arc<ClientSession>,
    request: String,
}

impl Task for InvokeTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        self.session
            .invoke(&self.request)
            .map_err(|e| napi_err(e.to_string()))
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

#[napi]
impl CoreSession {
    /// Synchronous core command, executed off the JS thread; resolves with the
    /// raw response envelope JSON. Fails fast (synchronously) if the session
    /// was already destroyed, so a use-after-destroy throws rather than
    /// rejecting later.
    #[napi(ts_return_type = "Promise<string>")]
    pub fn invoke(&self, request_json: String) -> napi::Result<AsyncTask<InvokeTask>> {
        if self.inner.is_destroyed() {
            return Err(napi_err("session has been destroyed"));
        }
        Ok(AsyncTask::new(InvokeTask {
            session: self.inner.clone(),
            request: request_json,
        }))
    }

    /// Start an async core command; returns the nonzero operation id. Throws
    /// the error envelope JSON as the message on start failure.
    #[napi]
    pub fn invoke_async(&self, request_json: String) -> napi::Result<i64> {
        self.inner
            .invoke_async(&request_json)
            .map(|op_id| op_id as i64)
            .map_err(|e| napi_err(e.to_string()))
    }

    /// Cooperative cancel; true if the operation was found and signaled.
    #[napi]
    pub fn cancel(&self, op_id: i64) -> napi::Result<bool> {
        Ok(self.inner.cancel(op_id as u64))
    }

    /// Destroy the session: blocks until in-flight work is drained; no
    /// callbacks are delivered afterwards. Idempotent.
    #[napi]
    pub fn destroy(&self) {
        self.inner.destroy();
    }
}
