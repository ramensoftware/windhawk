//! [`Session`]: a live core session and the typed invokes the consumers drive
//! it through. `Send + Sync` - the core serializes a session's internal state
//! and accepts calls from any thread - so a consumer can run a follow-up invoke
//! on a worker. The host imposes no driving model: `invoke_async` returns only
//! the core op-id and buffers nothing; the consumer drains its own events.

use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use windhawk_core_client::CoreSession;

use crate::error::{HostError, parse_response};
use crate::request_envelope;

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

    /// Synchronous command invoke over a typed request DTO `P`. Builds the
    /// request envelope, parses the raw response, and returns the success
    /// `result` value (the caller decodes it).
    pub fn invoke<P: Serialize>(&self, command: &str, params: &P) -> Result<Value, HostError> {
        let request = request_envelope(command, params)?;
        let raw = self.session.invoke(&request)?;
        parse_response(&raw)
    }

    /// Synchronous invoke that decodes its success `result` into a typed wire DTO
    /// `T`, folding [`Session::invoke`] and the `serde_json::from_value` the call
    /// sites otherwise repeat. A decode failure maps to `Decode`. `T` is first so
    /// `invoke_as::<Result>(cmd, &params)` turbofishes only the result type.
    pub fn invoke_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<T, HostError> {
        Ok(serde_json::from_value(self.invoke(command, params)?)?)
    }

    /// Start an asynchronous operation (`WhCoreInvokeAsync`); returns the core
    /// op-id. The host buffers and demultiplexes nothing - the consumer drives
    /// the operation's events (the CLI's blocking drain, the UI's pump). A failed
    /// start maps through [`HostError`] (its raw error envelope decoded to a
    /// `Wire` or the `Decode` fallback).
    pub fn invoke_async<P: Serialize>(&self, command: &str, params: &P) -> Result<u64, HostError> {
        let request = request_envelope(command, params)?;
        Ok(self.session.invoke_async(&request)?)
    }

    /// Hand out a cheap cancel handle bound to one op-id. The consumer stores it
    /// off-session (the CLI's Ctrl+C slot, the UI's `OpRegistry`) so the cancel
    /// capability travels without carrying the whole session; WHEN to cancel stays
    /// consumer-side.
    pub fn cancel_token(&self, op_id: u64) -> CancelToken {
        CancelToken {
            session: self.session.clone(),
            op_id,
        }
    }
}

/// A `Clone + Send + Sync` handle over `WhCoreCancel` bound to one op-id. It
/// carries the cancel capability without the whole `Session`, so a consumer can
/// hold it in an out-of-session slot. Cancelling a destroyed session, or an op
/// already finished, is a harmless no-op.
#[derive(Clone)]
pub struct CancelToken {
    session: Arc<CoreSession>,
    op_id: u64,
}

impl CancelToken {
    /// Signal cooperative cancellation of the bound op-id. Returns whether the op
    /// was found and signaled.
    pub fn cancel(&self) -> bool {
        self.session.cancel(self.op_id)
    }
}
