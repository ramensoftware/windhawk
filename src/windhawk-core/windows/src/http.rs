//! The `Http` port adapter: a streaming GET over WinHTTP, so proxy discovery,
//! TLS, and the certificate store behave like the OS and the core ships no TLS
//! stack of its own. One WinHTTP session per request (the workload is a handful
//! of small fetches plus one installer download; no connection pool is
//! warranted).
//!
//! Cancellation is cooperative polling: the read loop checks the cancel flag
//! between chunks, so the bounded-cancel rule follows from the bounded read
//! size; the WinHTTP timeouts bound the worst case of a stalled server that
//! never sends a chunk to poll between. The request's `max_bytes` bounds the
//! opposite case - a server that keeps sending - which cancellation alone would
//! leave running until a user thinks to stop it.
//!
//! Two request options ride on the port's request struct, both set before the
//! send: transparent content decoding (`accept_compression`, which has WinHTTP
//! negotiate and inflate gzip/deflate) and the `If-None-Match` header of a
//! conditional GET. The response's `ETag` comes back with the status, so a
//! caller can revalidate the same URL later instead of re-downloading it.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_DECOMPRESSION_FLAG_DEFLATE,
    WINHTTP_DECOMPRESSION_FLAG_GZIP, WINHTTP_ENABLE_SSL_REVOCATION, WINHTTP_FLAG_SECURE,
    WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_OPTION_DECOMPRESSION, WINHTTP_OPTION_ENABLE_FEATURE,
    WINHTTP_OPTION_IGNORE_CERT_REVOCATION_OFFLINE, WINHTTP_QUERY_CONTENT_LENGTH,
    WINHTTP_QUERY_ETAG, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle,
    WinHttpConnect, WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};
use windows_sys::core::BOOL;

use windhawk_core_ports::{CancelToken, Http, HttpError, HttpRequest, HttpResponse, HttpSink};

use crate::wide::{from_wide_nul, to_wide};

/// The largest block read per `WinHttpReadData` call. Bounds both transient
/// memory and the cancellation-polling interval (one read between checks).
const READ_CHUNK: usize = 64 * 1024;

pub struct WindowsHttp;

/// Owns a WinHTTP `HINTERNET` and closes it on drop, so every early return in
/// `get` cleans up its session/connection/request handles.
struct Handle(*mut c_void);

impl Handle {
    fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a non-null handle came from a WinHttp* open call and is
            // closed exactly once here.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn transport(op: &str) -> HttpError {
    // the bare GetLastError idiom now lives once in `crate::os`.
    let os = crate::os::last_error();
    HttpError::transport(format!("WinHTTP {op} failed (os error {os})"), os)
}

/// The components a cracked URL yields for `WinHttpConnect` / `WinHttpOpenRequest`.
struct UrlParts {
    host: Vec<u16>,
    object: Vec<u16>,
    port: u16,
    secure: bool,
}

/// Crack `url` into host / object (path + query) / port / secure-flag via
/// `WinHttpCrackUrl`, which returns pointers into the wide URL buffer.
fn crack_url(url: &str) -> Result<UrlParts, HttpError> {
    let url_w = to_wide(url);
    // A URL_COMPONENTS with dwStructSize set and the four length fields set to a
    // nonzero sentinel asks WinHttpCrackUrl to fill the pointer fields with pointers
    // into url_w and the lengths with the spans.
    let mut comp = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        dwSchemeLength: u32::MAX,
        dwHostNameLength: u32::MAX,
        dwUrlPathLength: u32::MAX,
        dwExtraInfoLength: u32::MAX,
        ..Default::default()
    };

    // SAFETY: url_w is NUL-terminated (length 0 means "NUL-terminated"); comp
    // is a valid, sized URL_COMPONENTS.
    let ok = unsafe { WinHttpCrackUrl(url_w.as_ptr(), 0, 0, &mut comp) };
    if ok == 0 {
        return Err(transport("URL parse"));
    }
    if comp.lpszHostName.is_null() || comp.dwHostNameLength == 0 {
        return Err(HttpError::transport(format!("URL has no host: {url}"), 0));
    }

    // SAFETY: WinHttpCrackUrl set lpszHostName to point into url_w with
    // dwHostNameLength wide chars; copy them out and NUL-terminate.
    let host = unsafe { copy_wide(comp.lpszHostName, comp.dwHostNameLength as usize) };
    // The path and the extra info (query/fragment) are contiguous in url_w, so
    // the object name is the two spans together.
    let object = if comp.lpszUrlPath.is_null() {
        to_wide("/")
    } else {
        let len = comp.dwUrlPathLength as usize + comp.dwExtraInfoLength as usize;
        // SAFETY: lpszUrlPath points into url_w and the path+extra run for
        // `len` wide chars contiguously.
        unsafe { copy_wide(comp.lpszUrlPath, len) }
    };

    Ok(UrlParts {
        host,
        object,
        port: comp.nPort,
        secure: comp.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS,
    })
}

/// Copy `len` wide chars from `ptr` and append a NUL terminator.
///
/// # Safety
/// `ptr` must point to at least `len` readable `u16`s.
unsafe fn copy_wide(ptr: *const u16, len: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(len + 1);
    // SAFETY: the caller guarantees `len` readable u16s at `ptr`.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    out.extend_from_slice(slice);
    out.push(0);
    out
}

/// Turn on revocation checking for this request's TLS handshake. WinHTTP does
/// not consult CRL/OCSP unless asked, so without this a certificate for the
/// catalog or installer host that was wrongly issued or stolen, and has since
/// been revoked, still builds a trusted chain and passes validation.
///
/// The check is deliberately soft-fail:
/// `WINHTTP_OPTION_IGNORE_CERT_REVOCATION_OFFLINE` keeps a responder the machine
/// cannot reach - captive portal, filtering proxy, no route at all - from
/// turning a working catalog fetch or update check into a transport error.
/// Revocation is enabled only where that option is accepted (Windows 10 version
/// 2004 and later; an older build answers `ERROR_WINHTTP_INVALID_OPTION`), so an
/// older Windows keeps the OS default rather than gaining a hard failure mode.
/// Both options are request-handle-only and take effect only if set before the
/// send.
///
/// Returns whether revocation checking ended up enabled.
fn enable_ssl_revocation(request: *mut c_void) -> bool {
    let tolerate_offline: BOOL = 1;
    // SAFETY: request is a valid HINTERNET request handle; tolerate_offline is
    // a BOOL read for the passed length.
    let ok = unsafe {
        WinHttpSetOption(
            request,
            WINHTTP_OPTION_IGNORE_CERT_REVOCATION_OFFLINE,
            (&tolerate_offline as *const BOOL).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    };
    if ok == 0 {
        return false;
    }

    let feature = WINHTTP_ENABLE_SSL_REVOCATION;
    // SAFETY: request is a valid HINTERNET request handle; feature is a u32
    // read for the passed length.
    let ok = unsafe {
        WinHttpSetOption(
            request,
            WINHTTP_OPTION_ENABLE_FEATURE,
            (&feature as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    ok != 0
}

/// Debug builds only: relax this request's TLS certificate validation, by
/// setting `WINHTTP_OPTION_SECURITY_FLAGS` to ignore an unknown CA, a name (CN)
/// mismatch, an expired certificate, and wrong key usage - so a developer can
/// fetch from a server with a self-signed certificate
/// (`WINDHAWK_DEBUG_IGNORE_CERT_ERRORS`). This is the build-level gate of the
/// debug-only override: the release function below is a no-op, so a shipped core
/// compiles no path that disables certificate validation. A SetOption failure is
/// non-fatal (the send/receive surfaces any TLS error), so the result is
/// ignored.
#[cfg(debug_assertions)]
fn set_insecure_tls(request: *mut c_void) {
    use windows_sys::Win32::Networking::WinHttp::{
        SECURITY_FLAG_IGNORE_CERT_CN_INVALID, SECURITY_FLAG_IGNORE_CERT_DATE_INVALID,
        SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE, SECURITY_FLAG_IGNORE_UNKNOWN_CA,
        WINHTTP_OPTION_SECURITY_FLAGS,
    };

    let flags: u32 = SECURITY_FLAG_IGNORE_UNKNOWN_CA
        | SECURITY_FLAG_IGNORE_CERT_CN_INVALID
        | SECURITY_FLAG_IGNORE_CERT_DATE_INVALID
        | SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE;
    // SAFETY: request is a valid HINTERNET request handle; flags is a u32 read
    // for the passed length.
    unsafe {
        WinHttpSetOption(
            request,
            WINHTTP_OPTION_SECURITY_FLAGS,
            (&flags as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
}

/// Release builds: the debug-only certificate-error override does not exist, so
/// this is a no-op and certificate validation can never be disabled.
#[cfg(not(debug_assertions))]
fn set_insecure_tls(_request: *mut c_void) {}

/// Turn on transparent content decoding for this request: WinHTTP sends
/// `Accept-Encoding: gzip, deflate` and inflates the response itself, so the
/// sink reads the same bytes it would have read uncompressed while the wire
/// carries a fraction of them. Request-handle-only and effective only before
/// the send.
///
/// Advisory, like revocation checking: Windows 8.1 and later accept the option
/// and an older build answers `ERROR_WINHTTP_INVALID_OPTION`, where the fetch
/// simply proceeds uncompressed rather than failing.
///
/// Returns whether decoding ended up enabled.
fn enable_decompression(request: *mut c_void) -> bool {
    let flags: u32 = WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE;
    // SAFETY: request is a valid HINTERNET request handle; flags is a u32 read
    // for the passed length.
    let ok = unsafe {
        WinHttpSetOption(
            request,
            WINHTTP_OPTION_DECOMPRESSION,
            (&flags as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    ok != 0
}

/// The extra request-header block for a conditional GET (`If-None-Match`), or
/// `None` when the request carries no validator and the send passes no headers.
///
/// A validator is an `ETag` the server itself chose, echoed back into a request
/// that same server parses, so a CR or LF inside it would end the header line
/// and let the remainder be read as headers of our own. No conforming server
/// produces one; drop such a value rather than send it, which costs at most one
/// unconditional fetch.
fn conditional_header(if_none_match: Option<&str>) -> Option<Vec<u16>> {
    let etag = if_none_match?;
    if etag.is_empty() || etag.contains(['\r', '\n', '\0']) {
        return None;
    }
    Some(to_wide(&format!("If-None-Match: {etag}\r\n")))
}

/// Read one string response header (the `ETag`) via `WinHttpQueryHeaders`.
/// `None` when the header is absent, which is the ordinary case for a server
/// that publishes no validator.
fn query_string(request: *mut c_void, info_level: u32) -> Option<String> {
    let mut bytes: u32 = 0;
    // The sizing call: it is expected to fail with ERROR_INSUFFICIENT_BUFFER
    // and report the byte count including the terminator. Any other failure is
    // the header not being there (ERROR_WINHTTP_HEADER_NOT_FOUND).
    // SAFETY: request is a valid handle past WinHttpReceiveResponse; a null
    // buffer with a length out-param is the documented sizing call.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            info_level,
            ptr::null(),
            ptr::null_mut(),
            &mut bytes,
            ptr::null_mut(),
        )
    };
    if ok != 0 || crate::os::last_error() != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }

    // The reported size is in bytes and covers the terminator; one spare wide
    // char keeps the buffer NUL-terminated whatever rounding the count implies.
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
    let mut capacity = (buf.len() * std::mem::size_of::<u16>()) as u32;
    // SAFETY: request is valid; buf holds `capacity` writable bytes; name/index
    // are the documented null sentinels.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            info_level,
            ptr::null(),
            buf.as_mut_ptr().cast(),
            &mut capacity,
            ptr::null_mut(),
        )
    };
    (ok != 0).then(|| from_wide_nul(&buf))
}

/// Read one numeric response header (status code, content length) via
/// `WinHttpQueryHeaders` with the NUMBER flag. `None` when the header is
/// absent (e.g. no Content-Length on a chunked response).
fn query_number(request: *mut c_void, info_level: u32) -> Option<u32> {
    let mut value: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: request is a valid handle past WinHttpReceiveResponse; value is a
    // u32 buffer of `len` bytes; name/index are the documented null sentinels.
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            info_level | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&mut value as *mut u32).cast(),
            &mut len,
            ptr::null_mut(),
        )
    };
    (ok != 0).then_some(value)
}

impl Http for WindowsHttp {
    fn get(
        &self,
        request: &HttpRequest,
        cancel: &CancelToken,
        sink: &mut dyn HttpSink,
    ) -> Result<HttpResponse, HttpError> {
        if cancel.is_canceled() {
            return Err(HttpError::Canceled);
        }

        let parts = crack_url(&request.url)?;

        let agent_w = request.user_agent.as_deref().map(to_wide);
        let agent_ptr = agent_w.as_ref().map_or(ptr::null(), |w| w.as_ptr());

        // SAFETY: agent_ptr is null or a NUL-terminated wide string living in
        // agent_w; the default-proxy access type with null proxy/bypass is the
        // documented direct/system-proxy configuration.
        let session = Handle(unsafe {
            WinHttpOpen(
                agent_ptr,
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        });
        if session.is_null() {
            return Err(transport("session open"));
        }
        // Bound a stalled transfer (the cancel-poll's worst case): resolve and
        // connect 60s, send and receive 30s.
        const RESOLVE_TIMEOUT_MS: i32 = 60_000;
        const CONNECT_TIMEOUT_MS: i32 = 60_000;
        const SEND_TIMEOUT_MS: i32 = 30_000;
        const RECEIVE_TIMEOUT_MS: i32 = 30_000;
        // SAFETY: session is a valid HINTERNET.
        unsafe {
            WinHttpSetTimeouts(
                session.0,
                RESOLVE_TIMEOUT_MS,
                CONNECT_TIMEOUT_MS,
                SEND_TIMEOUT_MS,
                RECEIVE_TIMEOUT_MS,
            )
        };

        // SAFETY: session is valid; host is NUL-terminated.
        let connect =
            Handle(unsafe { WinHttpConnect(session.0, parts.host.as_ptr(), parts.port, 0) });
        if connect.is_null() {
            return Err(transport("connect"));
        }

        let verb = to_wide("GET");
        let flags = if parts.secure { WINHTTP_FLAG_SECURE } else { 0 };
        // SAFETY: connect is valid; verb and object are NUL-terminated; the
        // null version/referrer/accept-types are the documented defaults.
        let req = Handle(unsafe {
            WinHttpOpenRequest(
                connect.0,
                verb.as_ptr(),
                parts.object.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                flags,
            )
        });
        if req.is_null() {
            return Err(transport("open request"));
        }

        // This request's TLS policy, set before the send so it applies to the
        // handshake. The debug-only certificate-error override stands in for
        // revocation checking rather than joining it: a caller that asked to
        // accept an invalid certificate is not served by a stricter check its
        // server cannot satisfy.
        if request.ignore_cert_errors {
            set_insecure_tls(req.0);
        } else {
            enable_ssl_revocation(req.0);
        }

        // Content decoding, when the caller asked for it - also set before the
        // send, since it is what puts `Accept-Encoding` on the request.
        if request.accept_compression {
            enable_decompression(req.0);
        }

        // The header block a conditional GET adds. The `-1` length sentinel
        // tells WinHTTP the block is NUL-terminated; a request without a
        // validator passes the documented null/zero pair instead.
        let headers_w = conditional_header(request.if_none_match.as_deref());
        let (headers_ptr, headers_len) = match &headers_w {
            Some(headers) => (headers.as_ptr(), u32::MAX),
            None => (ptr::null(), 0),
        };
        // SAFETY: req is valid; headers_ptr is null or a NUL-terminated wide
        // string living in headers_w for this call; no request body.
        let sent =
            unsafe { WinHttpSendRequest(req.0, headers_ptr, headers_len, ptr::null(), 0, 0, 0) };
        if sent == 0 {
            return Err(transport("send request"));
        }
        // SAFETY: req is valid and a request was sent.
        let received = unsafe { WinHttpReceiveResponse(req.0, ptr::null_mut()) };
        if received == 0 {
            return Err(transport("receive response"));
        }

        let status = query_number(req.0, WINHTTP_QUERY_STATUS_CODE)
            .ok_or_else(|| transport("query status"))? as u16;
        let content_length = query_number(req.0, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
        // An empty ETag is no validator; keeping it would only produce a
        // conditional request no server can answer.
        let etag = query_string(req.0, WINHTTP_QUERY_ETAG).filter(|etag| !etag.is_empty());
        sink.on_response(status, content_length);

        let mut buf = vec![0u8; READ_CHUNK];
        let mut total: u64 = 0;
        loop {
            if cancel.is_canceled() {
                return Err(HttpError::Canceled);
            }
            let mut available: u32 = 0;
            // SAFETY: req is valid; available is a u32 out-param. Blocks until
            // data is buffered or the response completes.
            let ok = unsafe { WinHttpQueryDataAvailable(req.0, &mut available) };
            if ok == 0 {
                return Err(transport("query data available"));
            }
            if available == 0 {
                break;
            }
            let to_read = (available as usize).min(buf.len());
            let mut read: u32 = 0;
            // SAFETY: req is valid; buf has `to_read` writable bytes; read is a
            // u32 out-param.
            let ok = unsafe {
                WinHttpReadData(req.0, buf.as_mut_ptr().cast(), to_read as u32, &mut read)
            };
            if ok == 0 {
                return Err(transport("read data"));
            }
            if read == 0 {
                break;
            }
            total += u64::from(read);
            if total > request.max_bytes {
                return Err(HttpError::transport(
                    format!("response body exceeds {} bytes", request.max_bytes),
                    0,
                ));
            }
            sink.on_chunk(&buf[..read as usize]);
        }

        Ok(HttpResponse { status, etag })
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Networking::WinHttp::ERROR_WINHTTP_INVALID_OPTION;

    use super::*;

    fn parts(url: &str) -> UrlParts {
        crack_url(url).expect("crackable url")
    }

    /// A session / connection / request triple for `url` that touches no
    /// network: `WinHttpConnect` only records the target and the request is
    /// never sent. All three are returned so the session and connection outlive
    /// the request handle.
    fn unsent_request(url: &str) -> (Handle, Handle, Handle) {
        let parts = parts(url);
        // SAFETY: the null agent and the default-proxy access type with null
        // proxy/bypass are the documented direct/system-proxy configuration.
        let session = Handle(unsafe {
            WinHttpOpen(
                ptr::null(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        });
        assert!(!session.is_null(), "session open");
        // SAFETY: session is valid; host is NUL-terminated.
        let connect =
            Handle(unsafe { WinHttpConnect(session.0, parts.host.as_ptr(), parts.port, 0) });
        assert!(!connect.is_null(), "connect");
        let verb = to_wide("GET");
        // SAFETY: connect is valid; verb and object are NUL-terminated; the
        // null version/referrer/accept-types are the documented defaults.
        let req = Handle(unsafe {
            WinHttpOpenRequest(
                connect.0,
                verb.as_ptr(),
                parts.object.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        });
        assert!(!req.is_null(), "open request");
        (session, connect, req)
    }

    #[test]
    fn https_url_selects_the_secure_flag_and_default_port() {
        // The secure-scheme path the real repository/installer endpoints take:
        // WINHTTP_FLAG_SECURE is set and the port defaults to 443.
        let p = parts("https://mods.windhawk.net/catalog.json");
        assert!(p.secure);
        assert_eq!(p.port, 443);
        assert_eq!(from_wide_nul(&p.host), "mods.windhawk.net");
        assert_eq!(from_wide_nul(&p.object), "/catalog.json");
    }

    #[test]
    fn http_url_is_not_secure_and_defaults_to_port_80() {
        let p = parts("http://example.com/x");
        assert!(!p.secure);
        assert_eq!(p.port, 80);
        assert_eq!(from_wide_nul(&p.host), "example.com");
    }

    #[test]
    fn explicit_port_and_query_are_preserved_in_the_object() {
        let p = parts("http://127.0.0.1:8080/mods/test-mod/1.0.wh.cpp?ref=x");
        assert!(!p.secure);
        assert_eq!(p.port, 8080);
        assert_eq!(from_wide_nul(&p.host), "127.0.0.1");
        assert_eq!(from_wide_nul(&p.object), "/mods/test-mod/1.0.wh.cpp?ref=x");
    }

    #[test]
    fn an_unparsable_url_is_a_transport_error() {
        assert!(matches!(
            crack_url("not a url"),
            Err(HttpError::Transport { .. })
        ));
    }

    #[test]
    fn a_conditional_header_is_built_only_for_a_usable_validator() {
        let header = conditional_header(Some("W/\"abc\"")).expect("a validator builds a header");
        assert_eq!(from_wide_nul(&header), "If-None-Match: W/\"abc\"\r\n");

        // No validator, and the degenerate empty one, send no header at all.
        assert!(conditional_header(None).is_none());
        assert!(conditional_header(Some("")).is_none());
    }

    #[test]
    fn a_validator_carrying_a_line_break_is_dropped_rather_than_sent() {
        // Header injection through an ETag we echo back: a CR or LF would end
        // the line and let the rest be read as headers of our own.
        assert!(conditional_header(Some("\"a\"\r\nX-Injected: 1")).is_none());
        assert!(conditional_header(Some("\"a\"\nX-Injected: 1")).is_none());
        assert!(conditional_header(Some("\"a\"\0")).is_none());
    }

    #[test]
    fn decompression_is_accepted_on_an_unsent_request_handle() {
        let (_session, _connect, req) = unsent_request("https://mods.windhawk.net/catalog.json");
        // Like revocation checking, the only tolerated failure is the
        // INVALID_OPTION an older Windows answers; a wrong option value, handle
        // type, or buffer size fails with something else.
        let enabled = enable_decompression(req.0);
        let os = crate::os::last_error();
        assert!(
            enabled || os == ERROR_WINHTTP_INVALID_OPTION,
            "enabling decompression failed with os error {os}"
        );
    }

    #[test]
    fn revocation_checking_is_accepted_on_an_unsent_request_handle() {
        let (_session, _connect, req) = unsent_request("https://mods.windhawk.net/catalog.json");
        // The only tolerated failure is the INVALID_OPTION a Windows older than
        // 10 version 2004 answers for the offline-tolerance option; a wrong
        // option value, handle type, or buffer size fails with something else.
        let enabled = enable_ssl_revocation(req.0);
        let os = crate::os::last_error();
        assert!(
            enabled || os == ERROR_WINHTTP_INVALID_OPTION,
            "enabling SSL revocation failed with os error {os}"
        );
    }
}
