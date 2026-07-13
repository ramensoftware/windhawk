//! The stable error-code table. This enum is the single list of codes; the
//! application crate's `CoreError` variants map 1:1 onto it.

use std::fmt;
use std::panic::Location;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The source file and line an error was raised at, carried alongside (never
/// inside) the human `message` so a consumer can surface the origin in a
/// DIAGNOSTIC context without polluting the message contract. Serializable so the
/// core's true origin rides the failure envelope across the FFI to the native
/// consumers; `file` is the compiler-relative path `Location::file` reports.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
}

impl SourceLocation {
    /// Capture the caller's source location. `#[track_caller]` records the call
    /// site, so a `#[track_caller]` error constructor that calls this reports the
    /// site that built the error, not this function.
    #[track_caller]
    pub fn caller() -> SourceLocation {
        SourceLocation::from(Location::caller())
    }
}

impl From<&'static Location<'static>> for SourceLocation {
    fn from(location: &'static Location<'static>) -> SourceLocation {
        SourceLocation {
            file: location.file().to_owned(),
            line: location.line(),
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Malformed request, unknown command, bad params.
    InvalidRequest,
    /// Session create: no/invalid windhawk.ini.
    AppRootInvalid,
    /// Command targets a mod with no config/source.
    ModNotInstalled,
    /// Repository 404 for mod/version.
    ModNotInRepo,
    /// Network failure to repository/update host.
    RepoUnreachable,
    /// clang++ exited nonzero.
    CompilerFailed,
    /// Operation canceled via WhCoreCancel.
    Canceled,
    /// startUpdate while an update is already in flight.
    UpdateInProgress,
    /// Filesystem failure not covered above.
    IoFailed,
    /// Registry failure.
    RegistryFailed,
    /// Invariant violation inside the core.
    Internal,
}

/// The structured `details` payload a `COMPILER_FAILED` [`WireError`] carries
/// (produced by the core's `CompilerFailed` error): the failing clang target
/// triple, the compiler exit code, and its captured stdout/stderr.
/// [`WireError::details`] itself stays an untyped `Value` - any code may carry
/// a per-code shape - this is the typed view a consumer decodes it into to
/// render compiler diagnostics. Decode-only and lenient: the container
/// `#[serde(default)]` lets a missing/`null` field fall back rather than
/// failing the decode, so a `null`/absent `exitCode` becomes `None` (the
/// diagnostics' "unknown" case) and absent text fields stay empty.
#[derive(Deserialize, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct CompileDetails {
    pub target: String,
    pub exit_code: Option<i64>,
    pub stdout: String,
    pub stderr: String,
}

/// The error object of the failure envelope and of `failed` events:
/// `{ "code": "...", "message": "...", "details": { ... }, "location": { ... } }`.
/// `details` carries structured data per code; `location` carries the origin the
/// core raised the error at (DIAGNOSTIC, separate from `message`). Both are
/// omitted when absent, so the common envelope stays `{code,message}`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

impl WireError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            location: None,
        }
    }

    pub fn with_details(code: ErrorCode, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details),
            location: None,
        }
    }

    /// Attach the origin location (the core's `CoreError` captures it at the site
    /// the error was raised). Builder-style so the `to_wire` mapping can set it
    /// after `new`/`with_details` without a third constructor per code.
    pub fn at(mut self, location: Option<SourceLocation>) -> Self {
        self.location = location;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialize_as_screaming_snake_case() {
        let cases = [
            (ErrorCode::InvalidRequest, "INVALID_REQUEST"),
            (ErrorCode::AppRootInvalid, "APP_ROOT_INVALID"),
            (ErrorCode::ModNotInstalled, "MOD_NOT_INSTALLED"),
            (ErrorCode::ModNotInRepo, "MOD_NOT_IN_REPO"),
            (ErrorCode::RepoUnreachable, "REPO_UNREACHABLE"),
            (ErrorCode::CompilerFailed, "COMPILER_FAILED"),
            (ErrorCode::Canceled, "CANCELED"),
            (ErrorCode::UpdateInProgress, "UPDATE_IN_PROGRESS"),
            (ErrorCode::IoFailed, "IO_FAILED"),
            (ErrorCode::RegistryFailed, "REGISTRY_FAILED"),
            (ErrorCode::Internal, "INTERNAL"),
        ];
        for (code, expected) in cases {
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                Value::String(expected.into())
            );
        }
    }

    #[test]
    fn details_and_location_omitted_when_absent() {
        let s = serde_json::to_string(&WireError::new(ErrorCode::Internal, "x")).unwrap();
        assert!(!s.contains("details"));
        assert!(!s.contains("location"));
    }

    #[test]
    fn location_round_trips_as_a_separate_field() {
        let wire = WireError::new(ErrorCode::Internal, "x").at(Some(SourceLocation {
            file: "core/src/services/mods.rs".to_owned(),
            line: 42,
        }));
        let json = serde_json::to_value(&wire).unwrap();
        // Carried separately - the message stays clean.
        assert_eq!(json["message"], "x");
        assert_eq!(
            json["location"],
            serde_json::json!({ "file": "core/src/services/mods.rs", "line": 42 })
        );
        assert_eq!(serde_json::from_value::<WireError>(json).unwrap(), wire);
    }

    #[test]
    fn source_location_displays_as_file_colon_line() {
        let loc = SourceLocation {
            file: "core/src/dispatch.rs".to_owned(),
            line: 7,
        };
        assert_eq!(loc.to_string(), "core/src/dispatch.rs:7");
    }

    #[test]
    fn compile_details_decode_camel_case_and_lenient() {
        let full: CompileDetails = serde_json::from_value(serde_json::json!({
            "target": "x86_64-w64-mingw32",
            "exitCode": 1,
            "stdout": "out",
            "stderr": "err",
        }))
        .unwrap();
        assert_eq!(full.target, "x86_64-w64-mingw32");
        assert_eq!(full.exit_code, Some(1));
        assert_eq!(full.stdout, "out");
        assert_eq!(full.stderr, "err");

        // A null/absent exitCode decodes to None (the "unknown" case), and
        // absent text fields default to empty rather than failing the decode.
        let sparse: CompileDetails =
            serde_json::from_value(serde_json::json!({ "exitCode": Value::Null })).unwrap();
        assert_eq!(sparse.exit_code, None);
        assert_eq!(sparse.target, "");
        assert_eq!(sparse.stdout, "");
    }
}
