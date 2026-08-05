//! The session seam and its in-process implementation. [`SessionApi`] is the
//! object-safe surface a consumer holds a session behind - three methods over
//! the already-built request envelope - and [`SessionApiExt`] is the typed sugar
//! (`invoke` / `invoke_as` / `invoke_async`) blanket-implemented on top of it, so
//! the envelope is built once and the response parsed once for every
//! implementation. [`Session`] is the in-process implementation over a live core
//! session: `Send + Sync` (the core serializes a session's internal state and
//! accepts calls from any thread), so a consumer can run a follow-up invoke on a
//! worker. The host imposes no driving model: `invoke_async` returns only the
//! core op-id and buffers nothing; the consumer drains its own events.

use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use windhawk_core_client::CoreSession;

use crate::error::{HostError, parse_response};
use crate::request_envelope;

/// The object-safe session surface: everything a consumer needs from a session,
/// reduced to what can cross a `dyn` boundary.
///
/// The invokes take the request envelope STRING and hand back the core's raw
/// response string, rather than `(command, &Value)` and a parsed `Value`. That
/// keeps the payload to exactly one serialization (`request_envelope` writes it,
/// nobody re-writes it) and one parse ([`parse_response`], at the single call
/// site in [`SessionApiExt`]) however the implementation reaches the core - which
/// matters for the payloads measured in tens of megabytes, and makes "the bytes
/// the core sees are the bytes the caller built" a structural property rather
/// than a `Value` round-tripping one.
pub trait SessionApi: Send + Sync {
    /// Invoke a command synchronously from a built request envelope, returning the
    /// core's raw response envelope.
    fn invoke_raw(&self, request: &str) -> Result<String, HostError>;

    /// Start an asynchronous operation from a built request envelope, returning the
    /// core op-id.
    fn invoke_async_raw(&self, request: &str) -> Result<u64, HostError>;

    /// Hand out a cancel handle bound to one op-id.
    fn cancel_token(&self, op_id: u64) -> Arc<dyn CancelHandle>;
}

/// A cancel capability bound to one op-id, held off-session (the CLI's Ctrl+C
/// slot, the UI's `OpRegistry`) so the capability travels without carrying the
/// whole session; WHEN to cancel stays consumer-side.
///
/// The contract every implementation owes its callers: `cancel` returns `false`
/// rather than an error when the session it named is gone, and it never blocks
/// past the request deadline.
pub trait CancelHandle: Send + Sync {
    /// Signal cooperative cancellation of the bound op-id. Returns whether the op
    /// was found and signaled.
    fn cancel(&self) -> bool;
}

/// The typed invokes the call sites use, over any [`SessionApi`]. Blanket
/// implemented, including for `dyn SessionApi`, so a consumer holding
/// `Arc<dyn SessionApi>` reaches the same three methods a concrete [`Session`]
/// offers. Import this trait to call them.
pub trait SessionApiExt: SessionApi {
    /// Synchronous command invoke over a typed request DTO `P`. Builds the request
    /// envelope, parses the raw response, and returns the success `result` value
    /// (the caller decodes it).
    fn invoke<P: Serialize>(&self, command: &str, params: &P) -> Result<Value, HostError>;

    /// Synchronous invoke that decodes its success `result` into a typed wire DTO
    /// `T`, folding [`SessionApiExt::invoke`] and the `serde_json::from_value` the
    /// call sites otherwise repeat. A decode failure maps to `Decode`. `T` is first
    /// so `invoke_as::<Result>(cmd, &params)` turbofishes only the result type.
    fn invoke_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<T, HostError>;

    /// Start an asynchronous operation; returns the core op-id. The host buffers
    /// and demultiplexes nothing - the consumer drives the operation's events (the
    /// CLI's blocking drain, the UI's pump). A failed start maps through
    /// [`HostError`] (its raw error envelope decoded to a `Wire` or the `Decode`
    /// fallback).
    fn invoke_async<P: Serialize>(&self, command: &str, params: &P) -> Result<u64, HostError>;
}

impl<S: SessionApi + ?Sized> SessionApiExt for S {
    fn invoke<P: Serialize>(&self, command: &str, params: &P) -> Result<Value, HostError> {
        let request = request_envelope(command, params)?;
        let raw = self.invoke_raw(&request)?;
        parse_response(&raw)
    }

    fn invoke_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<T, HostError> {
        Ok(serde_json::from_value(self.invoke(command, params)?)?)
    }

    fn invoke_async<P: Serialize>(&self, command: &str, params: &P) -> Result<u64, HostError> {
        let request = request_envelope(command, params)?;
        self.invoke_async_raw(&request)
    }
}

/// A live session handle. Holds an `Arc<CoreSession>` (the session owns an `Arc`
/// of the loaded library internally, so the DLL stays loaded until the session is
/// destroyed on drop).
pub struct Session {
    session: Arc<CoreSession>,
}

impl Session {
    pub(crate) fn new(session: CoreSession) -> Session {
        Session {
            session: Arc::new(session),
        }
    }
}

impl SessionApi for Session {
    fn invoke_raw(&self, request: &str) -> Result<String, HostError> {
        Ok(self.session.invoke(request)?)
    }

    fn invoke_async_raw(&self, request: &str) -> Result<u64, HostError> {
        Ok(self.session.invoke_async(request)?)
    }

    fn cancel_token(&self, op_id: u64) -> Arc<dyn CancelHandle> {
        Arc::new(CancelToken {
            session: self.session.clone(),
            op_id,
        })
    }
}

/// A `Clone + Send + Sync` [`CancelHandle`] over `WhCoreCancel` bound to one
/// op-id, carrying the cancel capability without the whole [`Session`].
/// Cancelling a destroyed session, or an op already finished, is a harmless
/// no-op.
#[derive(Clone)]
pub struct CancelToken {
    session: Arc<CoreSession>,
    op_id: u64,
}

impl CancelHandle for CancelToken {
    fn cancel(&self) -> bool {
        self.session.cancel(self.op_id)
    }
}
