//! The typed host over the raw `core-client` transport: DLL resolution + load,
//! the ABI integer gate (delegated to `core-client`) and the `contractVersion`
//! gate, session-config construction, the single `windhawk.ini` access point,
//! typed `invoke`/`invoke_stateless`, and the flat [`HostError`] that folds the
//! `{ok,result}` / `{ok:false,error}` envelope split and its `WireError`
//! mapping. Shared by `windhawk-cli` and `windhawk-ui` so the must-match
//! version-gating/config/error-parse code is written once.
//!
//! What deliberately stays OUT of this crate (consumer policy): the driving
//! model (the CLI's blocking `mpsc` drain vs the UI's async pump) and the
//! cancel POLICY (when to cancel). The host stops at "create a session and
//! invoke typed commands"; how you pump is the consumer's. The host owns no
//! event transport: `create_session` forwards the consumer's own log/event
//! callbacks straight to `core-client`.

#![forbid(unsafe_code)]

mod arch;
mod config;
mod error;
mod event;
mod gate;
mod loader;
mod session;
pub mod windhawk_ini;

pub use arch::arch_label;
pub use config::SessionConfig;
pub use error::{HostError, HostErrorKind};
pub use event::{EventClass, classify_event};
pub use loader::{GatedCore, resolve_dll_path};
pub use session::{CancelHandle, CancelToken, Session, SessionApi, SessionApiExt};

// Re-exported from core-client so a consumer supplies its session callbacks
// (the log/event closures) through the host; the host owns no event transport.
pub use windhawk_core_client::SessionCallbacks;

// The UI's concurrency model holds these in Tauri managed state and runs a
// composite's follow-up invoke (and the cancel) from a worker thread, so they
// must cross threads. The CLI never exercises that, so assert the guarantee at
// compile time: a future non-Send/Sync field is then a build error here, in the
// host that promises it, not a surprise in a consumer.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send_sync_unsized<T: ?Sized + Send + Sync>() {}
    assert_send_sync::<GatedCore>();
    assert_send_sync::<Session>();
    assert_send_sync::<CancelToken>();
    assert_send_sync::<HostError>();
    // The UI holds its session behind the seam rather than as a concrete type, so
    // `SessionApi` must stay object-safe (`dyn SessionApi` names a type only if it
    // is) and `Session` must keep implementing it.
    assert_send_sync_unsized::<dyn SessionApi>();
    fn assert_implements_seam<T: SessionApi>() {}
    assert_implements_seam::<Session>();
};

/// Build the `{command, params}` request envelope string the transport invokes
/// expect, serializing the typed request DTO `P` - the request-side twin of
/// [`error::parse_response`]'s decode, shared by the stateless, sync, and async
/// invokes. Serialization of the request DTOs cannot fail in practice; a
/// failure surfaces as a [`HostError::Decode`] rather than a panic.
pub(crate) fn request_envelope<P>(command: &str, params: &P) -> Result<String, HostError>
where
    P: serde::Serialize,
{
    #[derive(serde::Serialize)]
    struct Request<'a, P: serde::Serialize> {
        command: &'a str,
        params: &'a P,
    }
    serde_json::to_string(&Request { command, params })
        .map_err(|e| HostError::decode(format!("serializing request params: {e}")))
}
