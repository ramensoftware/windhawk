//! The `Http` port adapter: a streaming GET over WinHTTP, so proxy discovery,
//! TLS, and the certificate store behave like the OS and the core ships no TLS
//! stack of its own. One WinHTTP session per request (the workload is a handful
//! of small fetches plus one installer download; no connection pool is
//! warranted).
//!
//! Cancellation is cooperative polling: the read loop checks the cancel flag
//! between chunks, so the bounded-cancel rule follows from the bounded read
//! size; the WinHTTP timeouts bound the worst case of a stalled server that
//! never sends a chunk to poll between.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_INTERNET_SCHEME_HTTPS, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};

use windhawk_core_ports::{CancelToken, Http, HttpError, HttpRequest, HttpSink};

use crate::wide::to_wide;

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

/// Debug builds only: relax the request's TLS certificate validation when the
/// caller asked for it, by setting `WINHTTP_OPTION_SECURITY_FLAGS` to ignore an
/// unknown CA, a name (CN) mismatch, an expired certificate, and wrong key
/// usage - so a developer can fetch from a server with a self-signed
/// certificate (`WINDHAWK_DEBUG_IGNORE_CERT_ERRORS`). This is the build-level
/// gate of the debug-only override: the release function below is a no-op, so a
/// shipped core compiles no path that disables certificate validation. A
/// SetOption failure is non-fatal (the send/receive surfaces any TLS error), so
/// the result is ignored.
#[cfg(debug_assertions)]
fn set_insecure_tls(request: *mut c_void, ignore_cert_errors: bool) {
    use windows_sys::Win32::Networking::WinHttp::{
        SECURITY_FLAG_IGNORE_CERT_CN_INVALID, SECURITY_FLAG_IGNORE_CERT_DATE_INVALID,
        SECURITY_FLAG_IGNORE_CERT_WRONG_USAGE, SECURITY_FLAG_IGNORE_UNKNOWN_CA,
        WINHTTP_OPTION_SECURITY_FLAGS, WinHttpSetOption,
    };

    if !ignore_cert_errors {
        return;
    }
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
fn set_insecure_tls(_request: *mut c_void, _ignore_cert_errors: bool) {}

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
    ) -> Result<u16, HttpError> {
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

        // Debug-only: honor the certificate-error override before sending, so
        // the relaxed flags apply to this request's TLS handshake.
        set_insecure_tls(req.0, request.ignore_cert_errors);

        // SAFETY: req is valid; no extra headers or request body.
        let sent = unsafe { WinHttpSendRequest(req.0, ptr::null(), 0, ptr::null(), 0, 0, 0) };
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
        sink.on_response(status, content_length);

        let mut buf = vec![0u8; READ_CHUNK];
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
            sink.on_chunk(&buf[..read as usize]);
        }

        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wide::from_wide_nul;

    fn parts(url: &str) -> UrlParts {
        crack_url(url).expect("crackable url")
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
}
