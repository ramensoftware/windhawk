//! The genuine OS-call error triple shared (by COMPOSITION, not inheritance)
//! across the port error types. `FileError` and `SettingsError` each EMBED an
//! `OsError` and keep their own typed locus field (`path` / `location`); the
//! triple lives here once instead of being re-rolled per port.

use std::num::NonZeroU32;

/// The OS-call triple embedded by `FileError` and `SettingsError`: the
/// attempted operation, the raw OS code when there was one, and the OS message.
///
/// `os_error` is `Option<NonZeroU32>` so that "no OS code" is type-distinct
/// from "the OS returned 0". The old design passed `os_error: 0` to mean both,
/// which rendered a misleading `(os error 0)` suffix on a non-OS failure; with
/// `None` the suffix is simply omitted (the one intended, fixture-invisible
/// rendering delta).
#[derive(Debug, Clone)]
pub struct OsError {
    /// The attempted operation (`read`, `open`, `set_string`, ...), for logs.
    pub operation: &'static str,
    /// The raw Win32/errno code, or `None` when the failure was not from an OS
    /// call (a can't-happen guard, an exhaustion path, or a non-OS `std::io`
    /// error carrying no raw code).
    pub os_error: Option<NonZeroU32>,
    pub message: String,
}

impl OsError {
    /// Build from a raw `u32` code: `0` maps to `None` (no OS call), any
    /// nonzero code to `Some`. So the OS-code change stays invisible at the
    /// outer types' positional constructors.
    pub fn new(operation: &'static str, os_error: u32, message: impl Into<String>) -> Self {
        Self {
            operation,
            os_error: NonZeroU32::new(os_error),
            message: message.into(),
        }
    }

    /// `" (os error N)"` when an OS code is present, else `""`. The conditional
    /// a thiserror `#[error]` derive cannot express over an `Option`; the outer
    /// error types' hand-written `Display` append it.
    pub fn os_error_suffix(&self) -> String {
        match self.os_error {
            Some(code) => format!(" (os error {code})"),
            None => String::new(),
        }
    }
}

/// The `{operation} failed for {locus}: {message}` + conditional os-error
/// suffix shared by `FileError` and `SettingsError`'s hand-written `Display`
/// (only the typed locus field name differs). Folded into one helper so the
/// composition is not copied per type.
pub(crate) fn render(locus: &str, os: &OsError) -> String {
    format!(
        "{} failed for {}: {}{}",
        os.operation,
        locus,
        os.message,
        os.os_error_suffix()
    )
}
