//! The HTTP port: a single streaming GET with chunk-granularity progress and
//! cooperative cancellation - what the repository client's body collection and
//! the update download's progress events and `WhCoreCancel` hang off. The
//! production adapter is WinHTTP (`windhawk-core-windows`), so proxy discovery,
//! TLS, and the certificate store behave like the OS; the core ships no TLS
//! stack of its own.
//!
//! The body is delivered to a caller-supplied `HttpSink`: `on_response` once
//! with the final status (after redirects) and content length, then `on_chunk`
//! per body block in order. The sink decides what to do with the bytes (collect
//! to a string, stream to a file with progress); the adapter never collects.

use crate::cancel::CancelToken;

/// A GET request. The user agent, when set, is sent as the `User-Agent`
/// header (the repository client identifies itself; the update download sends
/// none, matching the TS).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub user_agent: Option<String>,
    /// Debug-only TLS escape hatch: when set, the WinHTTP adapter ignores
    /// certificate-validation errors (unknown CA, name mismatch, expiry, wrong
    /// usage) for this request, for testing against a server with a self-signed
    /// certificate. The adapter honors it only in debug builds (release builds
    /// compile no cert-bypass path), and the core only ever sets it from the
    /// debug-only `debugOverrides.ignoreCertErrors`
    /// (`WINDHAWK_DEBUG_IGNORE_CERT_ERRORS`); a release core never disables
    /// certificate validation.
    pub ignore_cert_errors: bool,
}

/// A streaming-GET failure. Distinct variants so services map them without
/// inspecting message strings: a transport failure becomes `REPO_UNREACHABLE`,
/// and cancellation propagates untouched. There is no sink-failure variant: the
/// sinks accumulate in memory and the disk write happens after the transfer
/// (mapped via `file_err`), so `on_chunk` cannot fail.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HttpError {
    /// DNS, connect, TLS, send/receive, or a mid-body disconnect.
    #[error("{message}")]
    Transport { message: String, os_error: u32 },
    /// The transfer was canceled via the `CancelToken`.
    #[error("transfer canceled")]
    Canceled,
}

impl HttpError {
    pub fn transport(message: impl Into<String>, os_error: u32) -> Self {
        Self::Transport {
            message: message.into(),
            os_error,
        }
    }
}

/// The body destination of an `Http::get`. The adapter calls `on_response`
/// exactly once before any chunk, then `on_chunk` per body block in order.
pub trait HttpSink {
    /// The final response status (after redirects) and the `Content-Length`
    /// when the server sent one. Called before any chunk.
    fn on_response(&mut self, status: u16, content_length: Option<u64>);

    /// One body block, in order. Infallible: the sinks accumulate in memory
    /// (any disk write happens after the transfer, mapped via `file_err`), so
    /// there is no chunk-time failure to surface.
    fn on_chunk(&mut self, data: &[u8]);
}

pub trait Http: Send + Sync {
    /// Perform a streaming GET, delivering the response to `sink`. Returns the
    /// final HTTP status code (after redirects); a non-2xx status is not an
    /// error here - only the service has the context to map a status to a wire
    /// code. Cancellation is checked between chunks (bounded by chunk size) and
    /// surfaces as `HttpError::Canceled`.
    fn get(
        &self,
        request: &HttpRequest,
        cancel: &CancelToken,
        sink: &mut dyn HttpSink,
    ) -> Result<u16, HttpError>;
}
