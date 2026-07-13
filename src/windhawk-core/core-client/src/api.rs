//! The loaded DLL and its resolved exports, plus string marshaling. Internal
//! to the crate; `CoreLibrary` and `CoreSession` are the public face.

use std::ffi::{CStr, CString, c_char, c_void};

use crate::error::ClientError;

pub(crate) type LogCb = unsafe extern "C" fn(*mut c_void, i32, *const c_char);
pub(crate) type EventCb = unsafe extern "C" fn(*mut c_void, u64, *const c_char);

type GetAbiVersionFn = unsafe extern "C" fn() -> i32;
type GetInfoJsonFn = unsafe extern "C" fn() -> *mut c_char;
type SessionCreateFn = unsafe extern "C" fn(
    *const c_char,
    Option<LogCb>,
    *mut c_void,
    Option<EventCb>,
    *mut c_void,
    *mut *mut c_void,
    *mut *mut c_char,
) -> i32;
type SessionDestroyFn = unsafe extern "C" fn(*mut c_void);
type InvokeFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type InvokeStatelessFn = unsafe extern "C" fn(*const c_char) -> *mut c_char;
type InvokeAsyncFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> u64;
type CancelFn = unsafe extern "C" fn(*mut c_void, u64) -> i32;
type FreeFn = unsafe extern "C" fn(*mut c_char);

/// The resolved exports of one loaded windhawk-core.dll. The library handle is
/// held for as long as any function pointer may be called.
pub(crate) struct CoreApi {
    _lib: libloading::Library,
    pub(crate) get_abi_version: GetAbiVersionFn,
    pub(crate) get_info_json: GetInfoJsonFn,
    pub(crate) session_create: SessionCreateFn,
    pub(crate) session_destroy: SessionDestroyFn,
    pub(crate) invoke: InvokeFn,
    pub(crate) invoke_stateless: InvokeStatelessFn,
    pub(crate) invoke_async: InvokeAsyncFn,
    pub(crate) cancel: CancelFn,
    pub(crate) free: FreeFn,
}

impl CoreApi {
    /// Load the DLL and resolve every export by its undecorated ABI name. Does
    /// NOT gate the ABI integer; the caller (`CoreLibrary::load`) does.
    pub(crate) fn load(dll_path: &str) -> Result<CoreApi, ClientError> {
        // SAFETY: loading a DLL runs its initialization; the path is chosen by
        // the trusted host, not by remote input.
        let lib = unsafe { libloading::Library::new(dll_path) }.map_err(|e| {
            ClientError::load(format!("failed to load {dll_path}: {}", error_chain(&e)))
        })?;

        macro_rules! resolve {
            ($name:literal, $ty:ty) => {{
                // SAFETY: the export is resolved by its undecorated ABI name
                // and transmuted to its documented signature.
                let symbol: libloading::Symbol<'_, $ty> =
                    unsafe { lib.get($name) }.map_err(|e| {
                        ClientError::load(format!(
                            "missing export in {dll_path}: {}",
                            error_chain(&e)
                        ))
                    })?;
                *symbol
            }};
        }

        Ok(CoreApi {
            get_abi_version: resolve!(b"WhCoreGetAbiVersion\0", GetAbiVersionFn),
            get_info_json: resolve!(b"WhCoreGetInfoJson\0", GetInfoJsonFn),
            session_create: resolve!(b"WhCoreSessionCreate\0", SessionCreateFn),
            session_destroy: resolve!(b"WhCoreSessionDestroy\0", SessionDestroyFn),
            invoke: resolve!(b"WhCoreInvoke\0", InvokeFn),
            invoke_stateless: resolve!(b"WhCoreInvokeStateless\0", InvokeStatelessFn),
            invoke_async: resolve!(b"WhCoreInvokeAsync\0", InvokeAsyncFn),
            cancel: resolve!(b"WhCoreCancel\0", CancelFn),
            free: resolve!(b"WhCoreFree\0", FreeFn),
            _lib: lib,
        })
    }

    /// Copy and free a string returned by the DLL.
    ///
    /// # Safety
    /// `p` must be null or a live string returned by this DLL.
    pub(crate) unsafe fn take_string(&self, p: *mut c_char) -> Option<String> {
        if p.is_null() {
            return None;
        }
        // SAFETY: the DLL returns NUL-terminated UTF-8; the copy happens
        // before the free.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        // SAFETY: p came from the DLL and is freed exactly once.
        unsafe { (self.free)(p) };
        Some(s)
    }
}

/// A C string for an argument borrowed by the DLL for the call. `what` names
/// the argument for the error message.
pub(crate) fn to_cstring(s: &str, what: &'static str) -> Result<CString, ClientError> {
    CString::new(s).map_err(|_| ClientError::nul_byte(what))
}

/// Format an error together with its full `source()` chain, joined by `": "`.
/// `libloading`'s `Display` names only the failed Win32 call ("LoadLibraryExW
/// failed", "GetProcAddress failed") and keeps the OS error in its source, so
/// walking the chain is what surfaces the code and text the loader actually
/// returned (e.g. "The specified module could not be found. (os error 126)").
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(inner) = source {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::{CoreApi, error_chain};
    use std::fmt;

    /// An error whose `Display` hides its cause in `source()`, mirroring how
    /// `libloading` keeps the OS error off its own `Display`.
    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl fmt::Display for Outer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "LoadLibraryExW failed")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn error_chain_appends_the_source_so_the_os_code_survives() {
        let err = Outer(std::io::Error::from_raw_os_error(126));
        let rendered = error_chain(&err);
        assert!(rendered.starts_with("LoadLibraryExW failed: "));
        assert!(rendered.contains("os error 126"));
    }

    #[test]
    fn error_chain_of_a_sourceless_error_is_just_its_display() {
        let err = std::io::Error::other("boom");
        assert_eq!(error_chain(&err), "boom");
    }

    #[test]
    fn load_failure_names_the_path_and_os_error_with_a_clean_message() {
        // A path that resolves nowhere fails in LoadLibraryExW: the message names
        // the path and carries the OS error chain (code + text) but NOT the
        // location (that is structural). (CoreApi is not Debug, so destructure
        // rather than unwrap_err.)
        let Err(error) = CoreApi::load("windhawk-core-does-not-exist.dll") else {
            panic!("loading a nonexistent DLL must fail");
        };
        let message = error.to_string();
        assert!(
            message.contains("windhawk-core-does-not-exist.dll"),
            "{message}"
        );
        assert!(message.contains("os error"), "{message}");
        assert!(
            !message.contains("(at "),
            "message must stay clean: {message}"
        );
    }

    #[test]
    fn load_failure_captures_the_api_rs_origin_via_track_caller() {
        let Err(error) = CoreApi::load("windhawk-core-does-not-exist.dll") else {
            panic!("loading a nonexistent DLL must fail");
        };
        // ClientError::load's #[track_caller] records the api.rs call site, not
        // error.rs where the constructor is defined.
        assert!(
            error.location().file().contains("api.rs"),
            "{}",
            error.location().file()
        );
    }
}
