//! The service layer's home for service<->wire conversion in BOTH directions:
//! mapping a port error onto a wire `CoreError` (`settings_err`/`file_err`, and
//! the ergonomic [`WireResultExt::wire`]) and serializing a typed result onto a
//! wire `Value` (`to_value_result`). Named `wire.rs` (not `errors.rs`) so the
//! success-direction serializer is not out of place and so it does not collide
//! with `crate::error` (the `CoreError` home).
//!
//! Both error converters render the message the same way
//! (`OsError::render_operation`): the operation and the cause, with the locus
//! and the raw OS code left to the typed `details` fields. So the two ports read
//! alike on the wire, and neither says in prose what `details` already carries.
//!
//! The error converters are `#[track_caller]` so the captured origin is the
//! SERVICE call site that performed the failing operation, not this module. That
//! only holds when the `#[track_caller]` chain stays intact: reach them through a
//! DIRECT call or `.wire()`'s `match`, NOT `.map_err(file_err)` - coercing a
//! `#[track_caller]` fn to a function pointer (what `map_err` does) drops the
//! attribute and pins every IO/settings origin back onto `wire.rs`.

use serde_json::Value;
use windhawk_core_ports::{FileError, SettingsError, SettingsErrorKind};

use crate::error::CoreError;

/// Map a port `SettingsError` onto the wire error model: registry failures are
/// `REGISTRY_FAILED`, file failures `IO_FAILED`. The adapter never chooses the
/// code; the service does, by matching the per-backend `kind`. The locus and the
/// raw OS code ride along into `details` so a front-end can classify the failure
/// without parsing the message. `#[track_caller]` (see the module note).
#[track_caller]
pub fn settings_err(e: SettingsError) -> CoreError {
    let message = e.os.render_operation();
    let os_error = e.os.os_error;
    match e.kind {
        SettingsErrorKind::Registry => CoreError::registry_failed(message, e.location, os_error),
        SettingsErrorKind::Ini => CoreError::io_failed(message, e.location, os_error),
    }
}

/// Map a port `FileError` onto the wire error model: a filesystem failure is
/// `IO_FAILED` carrying the path. The not-found case is handled by callers that
/// have benign behavior for it (`MOD_NOT_INSTALLED`, an empty listing);
/// everything that reaches here is a real I/O failure. `#[track_caller]` (see
/// the module note).
#[track_caller]
pub fn file_err(e: FileError) -> CoreError {
    let message = e.os.render_operation();
    let os_error = e.os.os_error;
    CoreError::io_failed(message, e.path, os_error)
}

/// `.wire()`: map a port-error `Result` onto a wire `CoreError`, the ergonomic
/// replacement for `.map_err(file_err)` / `.map_err(settings_err)` that keeps the
/// `#[track_caller]` origin (the service call site). The `match` calls the
/// converter DIRECTLY - a `.map_err(closure)` or fn-pointer coercion would reset
/// the captured location to this module.
pub trait WireResultExt<T> {
    fn wire(self) -> Result<T, CoreError>;
}

impl<T> WireResultExt<T> for Result<T, FileError> {
    #[track_caller]
    fn wire(self) -> Result<T, CoreError> {
        match self {
            Ok(value) => Ok(value),
            Err(error) => Err(file_err(error)),
        }
    }
}

impl<T> WireResultExt<T> for Result<T, SettingsError> {
    #[track_caller]
    fn wire(self) -> Result<T, CoreError> {
        match self {
            Ok(value) => Ok(value),
            Err(error) => Err(settings_err(error)),
        }
    }
}

pub fn to_value_result<T: serde::Serialize>(command: &str, value: &T) -> Result<Value, CoreError> {
    serde_json::to_value(value)
        .map_err(|e| CoreError::internal(format!("{command} result serialization: {e}")))
}

#[cfg(test)]
mod tests {
    use windhawk_core_ports::FileErrorKind;

    use super::*;

    /// The locus a port error carries in its typed field, and which the wire
    /// therefore keeps in `details` rather than in the message.
    const LOCUS: &str = "C:\\fixture\\AppData\\settings.ini";

    #[test]
    fn both_ports_render_the_operation_and_the_cause_alike() {
        // The two converters are the same rule over different locus fields, so
        // a message that named the locus (or the raw code) would repeat what
        // `details` already carries - and would differ per port for no reason.
        let file = file_err(FileError::new(
            "read",
            LOCUS,
            FileErrorKind::Other,
            32,
            "sharing violation",
        ));
        let ini = settings_err(SettingsError::ini("set", LOCUS, 32, "sharing violation"));
        let registry = settings_err(SettingsError::registry(
            "open",
            "Settings",
            5,
            "RegOpenKeyEx",
        ));

        assert_eq!(file.to_string(), "read failed: sharing violation");
        assert_eq!(ini.to_string(), "set failed: sharing violation");
        assert_eq!(registry.to_string(), "open failed: RegOpenKeyEx");

        // The locus and the code reach the caller through `details` instead.
        for (error, locus_field, locus, code) in [
            (file, "path", LOCUS, 32),
            (ini, "path", LOCUS, 32),
            (registry, "key", "Settings", 5),
        ] {
            let message = error.to_string();
            assert!(!message.contains(locus), "locus repeated in {message:?}");
            assert!(
                !message.contains("os error"),
                "code repeated in {message:?}"
            );
            let details = error.to_wire().details.expect("details");
            assert_eq!(details[locus_field], serde_json::json!(locus));
            assert_eq!(details["osError"], serde_json::json!(code));
        }
    }
}
