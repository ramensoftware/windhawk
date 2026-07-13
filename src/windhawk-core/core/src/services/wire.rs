//! The service layer's home for service<->wire conversion in BOTH directions:
//! mapping a port error onto a wire `CoreError` (`settings_err`/`file_err`, and
//! the ergonomic [`WireResultExt::wire`]) and serializing a typed result onto a
//! wire `Value` (`to_value_result`). Named `wire.rs` (not `errors.rs`) so the
//! success-direction serializer is not out of place and so it does not collide
//! with `crate::error` (the `CoreError` home).
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
/// code; the service does, by matching the per-backend `kind`.
/// `#[track_caller]` (see the module note).
#[track_caller]
pub fn settings_err(e: SettingsError) -> CoreError {
    let message = e.to_string();
    match e.kind {
        SettingsErrorKind::Registry => CoreError::registry_failed(message, e.location),
        SettingsErrorKind::Ini => CoreError::io_failed(message, e.location),
    }
}

/// Map a port `FileError` onto the wire error model: a filesystem failure is
/// `IO_FAILED` carrying the path. The not-found case is handled by callers that
/// have benign behavior for it (`MOD_NOT_INSTALLED`, an empty listing);
/// everything that reaches here is a real I/O failure. The wire `message` is
/// the BARE OS message (not the decorated `Display`), preserving the
/// pre-OsError wording. `#[track_caller]` (see the module note).
#[track_caller]
pub fn file_err(e: FileError) -> CoreError {
    CoreError::io_failed(e.os.message, e.path)
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
