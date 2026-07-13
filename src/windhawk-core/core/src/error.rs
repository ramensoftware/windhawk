//! `CoreError`: the application-layer error - a [`CoreErrorKind`] (one variant
//! per wire code, each carrying that code's structured `details` fields) plus
//! the source location it was raised at. Dispatch serializes it into the error
//! envelope mechanically: the kind owns the code/message/details, and the
//! location rides the envelope's separate `location` field (DIAGNOSTIC, never
//! folded into the message). There is no third place where error semantics
//! live.
//!
//! Construct through the `#[track_caller]` constructors so the captured location
//! is the site that raised the error (the true origin), not this module.

use std::fmt;
use std::panic::Location;

use serde_json::{Value, json};
use windhawk_core_protocol::{ErrorCode, SourceLocation, WireError};

/// The error semantics: one variant per wire code, carrying that code's fields.
/// Equatable so tests can `matches!` a returned error against an expected kind.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreErrorKind {
    #[error("{message}")]
    InvalidRequest { message: String },
    #[error("{message}")]
    AppRootInvalid { message: String, path: String },
    #[error("mod is not installed: {mod_id}")]
    ModNotInstalled { mod_id: String },
    #[error("mod not found in the repository: {mod_id}")]
    ModNotInRepo {
        mod_id: String,
        version: Option<String>,
    },
    #[error("{message}")]
    RepoUnreachable { message: String, url: String },
    #[error("{message}")]
    CompilerFailed {
        /// The human message, built like the TS `CompilerError` (exit-code and
        /// target specific); `target`/`exit_code`/`stdout`/`stderr` ride in
        /// `details`. `target` is the clang triple the CLI labels diagnostics
        /// with (the recorded fixture carries it), so the DLL client can
        /// reconstruct the `CompilerError` faithfully.
        message: String,
        target: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    #[error("operation canceled")]
    Canceled,
    #[error("an update is already in progress")]
    UpdateInProgress,
    #[error("{message}")]
    IoFailed { message: String, path: String },
    #[error("{message}")]
    RegistryFailed { message: String, key: String },
    #[error("{message}")]
    Internal { message: String },
}

impl CoreErrorKind {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest { .. } => ErrorCode::InvalidRequest,
            Self::AppRootInvalid { .. } => ErrorCode::AppRootInvalid,
            Self::ModNotInstalled { .. } => ErrorCode::ModNotInstalled,
            Self::ModNotInRepo { .. } => ErrorCode::ModNotInRepo,
            Self::RepoUnreachable { .. } => ErrorCode::RepoUnreachable,
            Self::CompilerFailed { .. } => ErrorCode::CompilerFailed,
            Self::Canceled => ErrorCode::Canceled,
            Self::UpdateInProgress => ErrorCode::UpdateInProgress,
            Self::IoFailed { .. } => ErrorCode::IoFailed,
            Self::RegistryFailed { .. } => ErrorCode::RegistryFailed,
            Self::Internal { .. } => ErrorCode::Internal,
        }
    }

    /// The structured `details` payload per code, or `None` for codes that
    /// carry no structured data.
    fn details(&self) -> Option<Value> {
        match self {
            Self::AppRootInvalid { path, .. } => Some(json!({ "path": path })),
            Self::ModNotInstalled { mod_id } => Some(json!({ "modId": mod_id })),
            Self::ModNotInRepo { mod_id, version } => {
                Some(json!({ "modId": mod_id, "version": version }))
            }
            Self::RepoUnreachable { url, .. } => Some(json!({ "url": url })),
            Self::CompilerFailed {
                target,
                exit_code,
                stdout,
                stderr,
                ..
            } => Some(json!({
                "target": target,
                "exitCode": exit_code,
                "stdout": stdout,
                "stderr": stderr,
            })),
            Self::IoFailed { path, .. } => Some(json!({ "path": path })),
            Self::RegistryFailed { key, .. } => Some(json!({ "key": key })),
            Self::InvalidRequest { .. }
            | Self::Canceled
            | Self::UpdateInProgress
            | Self::Internal { .. } => None,
        }
    }
}

/// A core error: its [`CoreErrorKind`] plus the source location it was raised at.
/// Equality is by kind ONLY - the location does not participate, so a fixture
/// error compares equal to a produced one regardless of where each was built.
#[derive(Debug, Clone)]
pub struct CoreError {
    kind: CoreErrorKind,
    location: &'static Location<'static>,
}

impl CoreError {
    /// Pair a kind with an already-captured location. Plain (NOT
    /// `#[track_caller]`) so the public constructors capture `Location::caller()`
    /// in their OWN body - the site that called them - and pass it through here.
    fn with_location(kind: CoreErrorKind, location: &'static Location<'static>) -> CoreError {
        CoreError { kind, location }
    }

    #[track_caller]
    pub fn invalid_request(message: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::InvalidRequest {
                message: message.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn internal(message: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::Internal {
                message: message.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn app_root_invalid(message: impl Into<String>, path: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::AppRootInvalid {
                message: message.into(),
                path: path.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn mod_not_installed(mod_id: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::ModNotInstalled {
                mod_id: mod_id.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn mod_not_in_repo(mod_id: impl Into<String>, version: Option<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::ModNotInRepo {
                mod_id: mod_id.into(),
                version,
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn repo_unreachable(message: impl Into<String>, url: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::RepoUnreachable {
                message: message.into(),
                url: url.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn compiler_failed(
        message: impl Into<String>,
        target: impl Into<String>,
        exit_code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> CoreError {
        Self::with_location(
            CoreErrorKind::CompilerFailed {
                message: message.into(),
                target: target.into(),
                exit_code,
                stdout: stdout.into(),
                stderr: stderr.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn canceled() -> CoreError {
        Self::with_location(CoreErrorKind::Canceled, Location::caller())
    }

    #[track_caller]
    pub fn update_in_progress() -> CoreError {
        Self::with_location(CoreErrorKind::UpdateInProgress, Location::caller())
    }

    #[track_caller]
    pub fn io_failed(message: impl Into<String>, path: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::IoFailed {
                message: message.into(),
                path: path.into(),
            },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn registry_failed(message: impl Into<String>, key: impl Into<String>) -> CoreError {
        Self::with_location(
            CoreErrorKind::RegistryFailed {
                message: message.into(),
                key: key.into(),
            },
            Location::caller(),
        )
    }

    /// The error semantics, for the few call sites that classify a returned
    /// error (e.g. a test matching the expected kind, or the runtime mapping a
    /// cancellation).
    pub fn kind(&self) -> &CoreErrorKind {
        &self.kind
    }

    pub fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The wire error: stable code, human message, structured details per code,
    /// and the origin location in the envelope's separate `location` field
    /// (never folded into `message`).
    pub fn to_wire(&self) -> WireError {
        let location = Some(SourceLocation::from(self.location));
        let base = match self.kind.details() {
            Some(details) => {
                WireError::with_details(self.kind.code(), self.kind.to_string(), details)
            }
            None => WireError::new(self.kind.code(), self.kind.to_string()),
        };
        base.at(location)
    }
}

impl PartialEq for CoreError {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl Eq for CoreError {}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl std::error::Error for CoreError {}

/// Serialize the failure envelope for a `CoreError`. Exists so the FFI crate
/// (which must not touch JSON) can produce its error envelopes through the
/// core.
pub fn error_envelope_json(error: &CoreError) -> String {
    windhawk_core_protocol::response_err(&error.to_wire())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_mapping_carries_code_details_and_origin_location() {
        let e = CoreError::mod_not_installed("test-mod");
        let w = e.to_wire();
        assert_eq!(w.code, ErrorCode::ModNotInstalled);
        assert_eq!(w.details, Some(json!({ "modId": "test-mod" })));
        // The origin is captured separately - this test's own file/line.
        let location = w.location.expect("location captured");
        assert!(location.file.contains("error.rs"), "{}", location.file);
    }

    #[test]
    fn canceled_has_no_details_but_keeps_a_location() {
        let w = CoreError::canceled().to_wire();
        assert_eq!(w.code, ErrorCode::Canceled);
        assert_eq!(w.details, None);
        assert!(w.location.is_some());
    }

    #[test]
    fn equality_is_by_kind_ignoring_location() {
        // Built at two different lines; equal because the kind matches.
        let a = CoreError::canceled();
        let b = CoreError::canceled();
        assert_eq!(a, b);
    }
}
