//! The CLI's driving wrapper over the shared `windhawk-core-host` session: the
//! host owns load + the ABI/contract gate, session config, and the typed invoke
//! that parses the `{ok,result}` / `{ok:false,error}` envelope into a success
//! `Value` or a [`HostError`]; this wrapper adds the one piece the host
//! deliberately leaves to the consumer - the driving model. It holds the event
//! channel the session's callback feeds, blocks draining one operation's events
//! to its terminal `completed`/`failed` in [`Core::invoke_async`], and
//! registers the running op for Ctrl+C cancellation. A `HostError` reaching a
//! command handler converts to the CLI's `CliError` through the one
//! `From<HostError>` seam (error.rs), so every `?` keeps working.

use std::sync::mpsc::{self, Receiver};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use windhawk_core_host::{
    EventClass, GatedCore, HostError, Session as HostSession, SessionApi, SessionApiExt,
    SessionCallbacks, SessionConfig, classify_event,
};
use windhawk_core_protocol::OperationEvent;

use crate::logger::Logger;

/// A live core session and the typed invoke the commands drive it through. Wraps
/// the host [`HostSession`] and owns the receiving half of the single channel the
/// session's event callback feeds: the CLI runs one operation per process, so one
/// channel suffices (the event STRATEGY is consumer policy, not in the host).
pub struct Core {
    session: HostSession,
    events: Receiver<(u64, String)>,
}

impl Core {
    /// Create a session from a resolved config, wiring the core log callback to
    /// the CLI's stderr logger and the event callback to an `mpsc` channel the
    /// main thread drains in [`Core::invoke_async`]. The callbacks fire on
    /// core-owned threads (never the caller), so the closures are `Send` and
    /// the event one only forwards `(opId, eventJson)` - it does no work on the
    /// core thread (FFI re-entrancy rule).
    pub fn create(
        core: &GatedCore,
        config: &SessionConfig,
        logger: Logger,
    ) -> Result<Core, HostError> {
        let (tx, events) = mpsc::channel::<(u64, String)>();
        let callbacks = SessionCallbacks {
            log: Box::new(move |level, message| logger.core_log(level, &message)),
            event: Box::new(move |op_id, event_json| {
                // Best-effort: a closed receiver means the operation is over.
                let _ = tx.send((op_id, event_json));
            }),
        };
        let session = core.create_session(config, callbacks)?;
        Ok(Core { session, events })
    }

    /// Synchronous command invoke over a typed request DTO `P`, returning the
    /// success `result` value (the caller decodes it). The [`HostError`]
    /// propagates so the call site's `?` converts it to a `CliError`.
    pub fn invoke<P: Serialize>(&self, command: &str, params: &P) -> Result<Value, HostError> {
        self.session.invoke(command, params)
    }

    /// Synchronous invoke that decodes its success `result` into a typed wire DTO
    /// `T`, the host's typed decode forwarded so the call sites stop hand-rolling
    /// the double `?`. `T` is first so a turbofish names only the result type.
    pub fn invoke_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<T, HostError> {
        self.session.invoke_as(command, params)
    }

    /// Asynchronous command invoke (the C ABI async path): start the operation,
    /// register it for Ctrl+C cancellation, then block draining its events on the
    /// single channel until the terminal `completed`/`failed`. `on_event` receives
    /// the intermediate `progress` / `installing` events (used by `update run`);
    /// compile-bearing commands pass a no-op since they emit none. A `failed` event
    /// becomes a [`HostError::Wire`], which preserves the wire `details` so a
    /// `COMPILER_FAILED` keeps its `[compile:<arch>]` diagnostics.
    pub fn invoke_async<P: Serialize>(
        &self,
        command: &str,
        params: &P,
        mut on_event: impl FnMut(&OperationEvent),
    ) -> Result<Value, HostError> {
        let op_id = self.session.invoke_async(command, params)?;
        crate::cancel::track(self.session.cancel_token(op_id));
        let outcome = self.drain(op_id, &mut on_event);
        crate::cancel::untrack();
        outcome
    }

    /// Asynchronous invoke that decodes its terminal `result` into a typed wire
    /// DTO `T`: the async twin of [`Core::invoke_as`], for the compile-bearing and
    /// network commands.
    pub fn invoke_async_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
        on_event: impl FnMut(&OperationEvent),
    ) -> Result<T, HostError> {
        Ok(serde_json::from_value(
            self.invoke_async(command, params, on_event)?,
        )?)
    }

    /// Drain the event channel for `op_id` until its terminal event. A stray id
    /// (impossible with one operation per process) is ignored defensively.
    fn drain(
        &self,
        op_id: u64,
        on_event: &mut impl FnMut(&OperationEvent),
    ) -> Result<Value, HostError> {
        loop {
            let (id, event_json) = self.events.recv().map_err(|_| {
                HostError::transport(
                    "core event channel closed before the operation completed".to_owned(),
                )
            })?;
            if id != op_id {
                continue;
            }
            // The host owns the failed -> WireError decode (classify_event); this
            // drain owns only the driving (block until terminal).
            match classify_event(&event_json)? {
                EventClass::Completed(result) => return Ok(result),
                EventClass::Failed(error) => return Err(HostError::wire(error)),
                EventClass::Progress(event) => on_event(&event),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use windhawk_core_host::GatedCore;
    use windhawk_core_protocol::CONTRACT_VERSION;

    /// Locate the freshly built windhawk-core cdylib next to the test deps dir.
    /// A plain `cargo test` does NOT emit the cdylib (the `cli` crate has no
    /// cargo dependency on it); a `--workspace` build does. The unit-test binary
    /// lives under `target/<profile>/deps/`, so the cdylib is two levels up.
    fn built_cdylib() -> std::path::PathBuf {
        let exe = std::env::current_exe().expect("test exe path");
        let target_dir = exe
            .parent()
            .and_then(std::path::Path::parent)
            .expect("target profile dir");
        let dll = target_dir.join("windhawk_core.dll");
        assert!(
            dll.exists(),
            "expected the cdylib at {dll:?}; is the lib target built? (cargo build --workspace)"
        );
        dll
    }

    /// End-to-end: load the built cdylib through the REAL [`GatedCore::load`],
    /// which performs the ABI gate (in core-client) and the contract-version gate.
    /// A successful load proves the DLL's reported `contractVersion` equals
    /// `CONTRACT_VERSION`; the literal assert below independently checks that
    /// constant has not drifted.
    #[test]
    fn gated_core_load_passes_the_abi_and_contract_gate() {
        GatedCore::load(&built_cdylib().to_string_lossy()).expect("load + ABI + contract gate");
        assert_eq!(CONTRACT_VERSION, "0.1.0");
    }
}
