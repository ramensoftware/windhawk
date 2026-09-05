//! In-memory `Http` port fake (core-internals.md section 3.3, testkit). A
//! behavioral fake: it records the requests it was asked to make and replays a
//! canned response per URL (status, body, content length) through the sink,
//! splitting the body into chunks so progress events fire. Supports a transport
//! fault (the `REPO_UNREACHABLE` paths) and two cancellation modes - block
//! until canceled, and cancel once the body is delivered - so both download
//! cancellation windows are deterministic without a real socket.
//!
//! A response given an `ETag` also revalidates like a server does: a request
//! whose `If-None-Match` matches it is answered `304` with no body, so the
//! caller's conditional-fetch path is exercised end to end.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windhawk_core_ports::{CancelToken, Http, HttpError, HttpRequest, HttpResponse, HttpSink};

/// A canned response for one URL.
#[derive(Clone)]
pub struct FakeResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// What `on_response` reports as the content length (`None` = no
    /// Content-Length header, the chunked-transfer case).
    pub content_length: Option<u64>,
    /// How many chunks to split the body into for `on_chunk` (>= 1), so
    /// progress-percentage tests see intermediate values.
    pub chunks: usize,
    /// When set, `get` returns this transport error instead of replaying a
    /// response (the fetch-rejection paths).
    pub fault: Option<HttpError>,
    /// When set, deliver the first chunk then block until canceled, returning
    /// `HttpError::Canceled` - the deterministic download-cancel path.
    pub block_until_canceled: bool,
    /// When set, signal the caller's cancel token after the last chunk and
    /// still report success - a transfer that finished before the cancel
    /// reached it, so the cancel lands in the caller's post-download window.
    pub cancel_after_body: bool,
    /// The entity validator this response reports, and revalidates against: a
    /// request carrying it as `If-None-Match` is answered `304` with no body.
    pub etag: Option<String>,
}

impl FakeResponse {
    /// A 200 response carrying `body`, delivered in one chunk with a matching
    /// Content-Length.
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        let body = body.into();
        Self {
            status: 200,
            content_length: Some(body.len() as u64),
            body,
            chunks: 1,
            fault: None,
            block_until_canceled: false,
            cancel_after_body: false,
            etag: None,
        }
    }

    /// A response with the given status code and an empty body (e.g. a 404).
    pub fn status(status: u16) -> Self {
        Self {
            status,
            body: Vec::new(),
            content_length: Some(0),
            chunks: 1,
            fault: None,
            block_until_canceled: false,
            cancel_after_body: false,
            etag: None,
        }
    }

    /// A transport failure (DNS/connect/TLS/disconnect), as if the fetch
    /// rejected.
    pub fn transport_fault() -> Self {
        Self {
            status: 0,
            body: Vec::new(),
            content_length: None,
            chunks: 1,
            fault: Some(HttpError::transport("simulated transport failure", 0)),
            block_until_canceled: false,
            cancel_after_body: false,
            etag: None,
        }
    }

    pub fn with_chunks(mut self, chunks: usize) -> Self {
        self.chunks = chunks.max(1);
        self
    }

    pub fn with_content_length(mut self, content_length: Option<u64>) -> Self {
        self.content_length = content_length;
        self
    }

    pub fn blocking(mut self) -> Self {
        self.block_until_canceled = true;
        self
    }

    pub fn canceled_after_body(mut self) -> Self {
        self.cancel_after_body = true;
        self
    }

    /// Publish an entity validator, which also arms the `304` answer to a
    /// request that sends it back.
    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }
}

#[derive(Clone, Default)]
pub struct FakeHttp {
    responses: Arc<Mutex<HashMap<String, FakeResponse>>>,
    default: Arc<Mutex<Option<FakeResponse>>>,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeHttp {
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure the response for an exact URL.
    pub fn on(&self, url: impl Into<String>, response: FakeResponse) {
        self.responses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url.into(), response);
    }

    /// Configure the response for any URL without a specific one (the catalog
    /// fallback path hits two URLs, for instance).
    pub fn set_default(&self, response: FakeResponse) {
        *self.default.lock().unwrap_or_else(|e| e.into_inner()) = Some(response);
    }

    /// The requests passed to `get`, in order.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn response_for(&self, url: &str) -> Option<FakeResponse> {
        if let Some(r) = self
            .responses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(url)
        {
            return Some(r.clone());
        }
        self.default
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Http for FakeHttp {
    fn get(
        &self,
        request: &HttpRequest,
        cancel: &CancelToken,
        sink: &mut dyn HttpSink,
    ) -> Result<HttpResponse, HttpError> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.clone());

        let response = self.response_for(&request.url).unwrap_or_else(|| {
            // An unconfigured URL is a test bug; surface it as a transport
            // error so the service maps it visibly rather than hanging.
            FakeResponse {
                status: 0,
                body: Vec::new(),
                content_length: None,
                chunks: 1,
                fault: Some(HttpError::transport(
                    format!("no FakeHttp response configured for {}", request.url),
                    0,
                )),
                block_until_canceled: false,
                cancel_after_body: false,
                etag: None,
            }
        });

        if let Some(fault) = response.fault {
            return Err(fault);
        }

        // A conditional request whose validator still matches gets what a
        // server gives it: 304 and no body, the entity coming from the caller's
        // own cache.
        if let Some(etag) = &response.etag
            && request.if_none_match.as_deref() == Some(etag.as_str())
        {
            sink.on_response(304, Some(0));
            return Ok(HttpResponse {
                status: 304,
                etag: response.etag,
            });
        }

        sink.on_response(response.status, response.content_length);

        let chunks = split_chunks(&response.body, response.chunks);
        for (i, chunk) in chunks.iter().enumerate() {
            if cancel.is_canceled() {
                return Err(HttpError::Canceled);
            }
            sink.on_chunk(chunk);
            if response.block_until_canceled && i == 0 {
                // Block until canceled (or a generous timeout, so a misuse
                // does not hang the suite forever).
                if cancel.wait(Duration::from_secs(10)) {
                    return Err(HttpError::Canceled);
                }
            }
        }
        if response.cancel_after_body {
            cancel.cancel();
        }
        Ok(HttpResponse {
            status: response.status,
            etag: response.etag,
        })
    }
}

/// Split `body` into `n` contiguous, roughly-equal chunks (always at least one,
/// even for an empty body, so `on_chunk` is exercised).
fn split_chunks(body: &[u8], n: usize) -> Vec<Vec<u8>> {
    let n = n.max(1);
    if body.is_empty() {
        return vec![Vec::new()];
    }
    let chunk_size = body.len().div_ceil(n);
    body.chunks(chunk_size).map(<[u8]>::to_vec).collect()
}
