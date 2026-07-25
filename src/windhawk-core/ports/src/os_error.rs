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
///
/// The code lives in `os_error` and NOWHERE else: `message` is the cause alone,
/// so a renderer decides once whether to spell the code out. The constructor
/// enforces it, because the two kinds of message the adapters produce disagree -
/// a `std::io::Error` string ends in the code, a raw Win32 call name does not.
#[derive(Debug, Clone)]
pub struct OsError {
    /// The attempted operation (`read`, `open`, `set_string`, ...), for logs.
    pub operation: &'static str,
    /// The raw Win32/errno code, or `None` when the failure was not from an OS
    /// call (a can't-happen guard, an exhaustion path, or a non-OS `std::io`
    /// error carrying no raw code).
    pub os_error: Option<NonZeroU32>,
    /// The cause alone, never carrying the raw code (see the type note).
    pub message: String,
}

impl OsError {
    /// Build from a raw `u32` code: `0` maps to `None` (no OS call), any
    /// nonzero code to `Some`. So the OS-code change stays invisible at the
    /// outer types' positional constructors.
    ///
    /// A `message` ending in `std`'s own ` (os error N)` for this code is
    /// trimmed of it, which is what keeps the code in one field: an adapter
    /// that formats a `std::io::Error` hands over a message that carries the
    /// code inline, and a renderer appending the suffix would show it twice.
    pub fn new(operation: &'static str, os_error: u32, message: impl Into<String>) -> Self {
        let os_error = NonZeroU32::new(os_error);
        Self {
            operation,
            os_error,
            message: trim_os_error_suffix(message.into(), os_error),
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

    /// The `{operation} failed: {message}` form - what was attempted and the
    /// cause, carrying NEITHER the locus nor the raw code. The partial form, for
    /// a rendering that keeps those two as its own structured fields and would
    /// otherwise state them twice; the outer error types' `Display` is the
    /// standalone one that folds all of it into a single string.
    pub fn render_operation(&self) -> String {
        format!("{} failed: {}", self.operation, self.message)
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

/// Drop `std`'s ` (os error N)` tail from a message that already carries it, so
/// the code rides in `os_error` alone. `std` spells the code as the `i32` it was
/// built from, so a code that came back negative is matched in that spelling too.
fn trim_os_error_suffix(message: String, os_error: Option<NonZeroU32>) -> String {
    let Some(code) = os_error else {
        return message;
    };
    let code = code.get();
    for spelling in [
        format!(" (os error {code})"),
        format!(" (os error {})", code as i32),
    ] {
        if let Some(trimmed) = message.strip_suffix(&spelling) {
            return trimmed.to_owned();
        }
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_std_io_message_does_not_carry_the_code_twice() {
        // The shape the file/INI adapters produce: the message IS a
        // `std::io::Error` rendering, which ends in the code the triple also
        // stores. Built from `std` rather than an English literal, so the test
        // holds on a localized Windows.
        let rendered = std::io::Error::from_raw_os_error(32).to_string();
        let os = OsError::new("read", 32, rendered.clone());

        assert_eq!(os.message, rendered.replace(" (os error 32)", ""));
        assert!(!os.message.contains("os error"), "{:?}", os.message);
        // The standalone form spells the code out exactly once.
        assert_eq!(
            render("settings.ini", &os),
            format!("read failed for settings.ini: {rendered}")
        );
    }

    #[test]
    fn a_bare_message_is_left_alone_and_still_gets_the_suffix() {
        // The shape the registry adapter produces: the failing call name, with
        // no code of its own to trim.
        let os = OsError::new("open", 5, "RegOpenKeyEx");
        assert_eq!(os.message, "RegOpenKeyEx");
        assert_eq!(
            render("Settings", &os),
            "open failed for Settings: RegOpenKeyEx (os error 5)"
        );
    }

    #[test]
    fn a_non_os_failure_keeps_its_message_and_renders_no_suffix() {
        // `0` means the failure did not come from an OS call, so there is no
        // code to trim and none to append.
        let os = OsError::new(
            "create_temp_dir",
            0,
            "could not create a unique temp directory",
        );
        assert_eq!(os.os_error, None);
        assert_eq!(
            render("C:\\tmp", &os),
            "create_temp_dir failed for C:\\tmp: could not create a unique temp directory"
        );
    }
}
