//! Shared HTTP plumbing for the network services (`repo`, `update`): the
//! collect-a-body-into-memory GET, the byte budget that bounds it, the 2xx
//! success test, and the user-agent the repository client sends.
//! Streaming-to-disk for the installer download lives in `update` (its sink
//! reports progress).

use std::sync::Arc;

use windhawk_core_ports::{CancelToken, Http, HttpError, HttpRequest, HttpResponse, HttpSink};

use crate::error::CoreError;
use crate::session::SessionInner;

/// Whether an HTTP status is a success (the TS `response.ok`, 200-299).
pub fn is_success(status: u16) -> bool {
    (200..=299).contains(&status)
}

/// Map an `HttpError` onto the wire model, shared by every HTTP caller (`repo`,
/// `update`, and install's precompiled download - folds the three identical
/// copies here). A transport failure is `REPO_UNREACHABLE` worded `<prefix>:
/// <cause>`; cancellation propagates untouched. The body is collected in
/// memory, so there is no chunk-time sink failure (retired the `Sink` variant).
/// The `prefix` is pre-formatted by the caller (`get_collected` passes `"Failed
/// to reach <url>"`, update passes the literal `"Failed to download update"`);
/// the typed `url` field is set separately, so both renderings stay
/// byte-identical to before.
pub fn map_http_err(error: HttpError, prefix: String, url: &str) -> CoreError {
    match error {
        HttpError::Canceled => CoreError::canceled(),
        HttpError::Transport { message, .. } => {
            CoreError::repo_unreachable(format!("{prefix}: {message}"), url.to_owned())
        }
    }
}

/// A response body accumulated in memory under a hard byte budget. Past the
/// budget it drops what it holds and latches over-limit, which the caller turns
/// into an error once the transfer returns; it cannot stop the transfer itself
/// (`HttpSink` has no failure channel), it only bounds the memory. Without a
/// budget the buffer grows to whatever the server chooses to send, and an
/// allocation failure aborts the host process rather than failing the command.
pub struct BoundedBody {
    bytes: Vec<u8>,
    limit: usize,
    over_limit: bool,
}

impl BoundedBody {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            over_limit: false,
        }
    }

    /// Reserve for an announced `Content-Length`. A length over the budget
    /// latches before a byte arrives, so the reservation is sized by the budget
    /// and never by the server's number.
    pub fn reserve(&mut self, content_length: Option<u64>) {
        let Some(len) = content_length else {
            return;
        };
        if len > self.limit as u64 {
            self.discard();
            return;
        }
        self.bytes.reserve(len as usize);
    }

    /// Append one chunk; `false` once the budget is exhausted, from which point
    /// nothing is kept.
    pub fn push(&mut self, data: &[u8]) -> bool {
        if self.over_limit || data.len() > self.limit - self.bytes.len() {
            self.discard();
            return false;
        }
        self.bytes.extend_from_slice(data);
        true
    }

    pub fn over_limit(&self) -> bool {
        self.over_limit
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn discard(&mut self) {
        self.over_limit = true;
        self.bytes = Vec::new();
    }
}

/// The byte budget for one collected repository response, both as the request's
/// `max_bytes` (which stops the transfer) and as what the sink keeps. Every
/// resource behind [`get_collected`] is small - the published catalogs are
/// under 1 MB, and the largest mod source and precompiled DLL under 2 MB - so
/// the budget sits far above any legitimate payload and only stops a server
/// that keeps sending.
pub const MAX_COLLECTED_BYTES: usize = 64 * 1024 * 1024;

/// An `HttpSink` that accumulates the whole body in memory - for the small
/// repository responses (catalog, mod source, versions.json, precompiled DLL),
/// which are read in full before use.
struct CollectSink {
    body: BoundedBody,
}

impl CollectSink {
    fn new() -> Self {
        Self {
            body: BoundedBody::new(MAX_COLLECTED_BYTES),
        }
    }
}

impl HttpSink for CollectSink {
    fn on_response(&mut self, _status: u16, content_length: Option<u64>) {
        self.body.reserve(content_length);
    }

    fn on_chunk(&mut self, data: &[u8]) {
        self.body.push(data);
    }
}

/// GET `request` collecting the whole body, shared by the repository client and
/// install's precompiled download. Returns the response head (the caller
/// interprets the status, and keeps the `ETag` if it means to revalidate) and
/// the bytes; a transport failure or a body over `MAX_COLLECTED_BYTES` is
/// `REPO_UNREACHABLE` and cancellation propagates.
pub fn get_collected(
    http: &dyn Http,
    request: &HttpRequest,
    cancel: &CancelToken,
) -> Result<(HttpResponse, Vec<u8>), CoreError> {
    let url = &request.url;
    let mut sink = CollectSink::new();
    let response = http
        .get(request, cancel, &mut sink)
        .map_err(|e| map_http_err(e, format!("Failed to reach {url}"), url))?;
    if sink.body.over_limit() {
        return Err(CoreError::repo_unreachable(
            format!("Response from {url} exceeds {MAX_COLLECTED_BYTES} bytes"),
            url.clone(),
        ));
    }
    Ok((response, sink.body.into_bytes()))
}

/// The repository `User-Agent` (`windhawkVersion` feeds the HTTP user agent).
/// The front-end-provided value wins (it owns the product identity, e.g.
/// `windhawk-cli/` vs `Windhawk/`); absent it, build the GUI-style default from
/// the session version plus the portable suffix. A server-visible string only.
pub fn repo_user_agent(session: &Arc<SessionInner>) -> Option<String> {
    if let Some(ua) = &session.config().user_agent {
        return Some(ua.clone());
    }
    let version = session
        .config()
        .windhawk_version
        .as_deref()
        .unwrap_or("unknown");
    let mut ua = format!("Windhawk/{version}");
    if session.storage().portable() {
        ua.push_str(" (portable)");
    }
    Some(ua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_body_collects_up_to_its_budget() {
        let mut body = BoundedBody::new(4);
        body.reserve(Some(4));
        assert!(body.push(b"ab"));
        assert!(body.push(b"cd"));
        assert!(!body.over_limit());
        assert_eq!(body.into_bytes(), b"abcd");
    }

    #[test]
    fn bounded_body_drops_everything_once_a_chunk_crosses_the_budget() {
        let mut body = BoundedBody::new(4);
        assert!(body.push(b"abc"));
        // The crossing chunk is refused whole, and so is every later one.
        assert!(!body.push(b"de"));
        assert!(!body.push(b"f"));
        assert!(body.over_limit());
        assert!(body.into_bytes().is_empty());
    }

    #[test]
    fn bounded_body_latches_on_an_announced_length_over_the_budget() {
        let mut body = BoundedBody::new(4);
        body.reserve(Some(u64::from(u32::MAX)));
        assert!(body.over_limit());
        assert!(!body.push(b"a"));
        assert!(body.into_bytes().is_empty());
    }
}
