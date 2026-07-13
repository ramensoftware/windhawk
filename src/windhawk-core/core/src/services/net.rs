//! Shared HTTP plumbing for the network services (`repo`, `update`): a sink
//! that collects a small response body into memory, the 2xx success test, and
//! the user-agent the repository client sends. Streaming-to-disk for the
//! installer download lives in `update` (its sink reports progress).

use std::sync::Arc;

use windhawk_core_ports::{HttpError, HttpSink};

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
/// The `prefix` is pre-formatted by the caller (repo/install pass `"Failed to
/// reach <url>"`, update passes the literal `"Failed to download update"`); the
/// typed `url` field is set separately, so both renderings stay byte-identical
/// to before.
pub fn map_http_err(error: HttpError, prefix: String, url: &str) -> CoreError {
    match error {
        HttpError::Canceled => CoreError::canceled(),
        HttpError::Transport { message, .. } => {
            CoreError::repo_unreachable(format!("{prefix}: {message}"), url.to_owned())
        }
    }
}

/// An `HttpSink` that accumulates the whole body in memory - for the small
/// repository responses (catalog, mod source, versions.json), which are read
/// in full before use.
#[derive(Default)]
pub struct CollectSink {
    bytes: Vec<u8>,
}

impl CollectSink {
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl HttpSink for CollectSink {
    fn on_response(&mut self, _status: u16, content_length: Option<u64>) {
        if let Some(len) = content_length {
            self.bytes.reserve(len.min(64 * 1024 * 1024) as usize);
        }
    }

    fn on_chunk(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
    }
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
