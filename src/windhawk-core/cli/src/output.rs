//! The compute-then-render seam: command bodies return a typed value
//! implementing [`CommandResult`]; this module renders it to text OR the
//! `--json` envelope. One renderer emits both forms, so the two modes cannot
//! drift.

use std::io::{self, Write};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::CliError;

/// `--json` envelope schema version, locked to 1; a backward-incompatible
/// change bumps it.
const SCHEMA_VERSION: u32 = 1;

/// A command's result: the JSON `data` payload, and the human-readable text
/// form. Implemented by a per-command struct so the data shape and the text
/// shape are produced from the same typed value.
pub trait CommandResult {
    fn json_data(&self) -> Value;
    fn write_text(&self, out: &mut dyn Write) -> io::Result<()>;

    /// The process exit code for a SUCCESSFULLY produced result. Almost every
    /// command returns 0 (a failure is an `Err(CliError)` that carries its own
    /// exit class); `data import` overrides this so a partial import - the
    /// operation completed, but at least one mod failed - still emits its full
    /// summary AND exits nonzero (the summary is the contract, so it is not an
    /// error-envelope case).
    fn exit_code(&self) -> i32 {
        0
    }
}

/// Emit a successful result to stdout: the `--json` envelope, or the text form.
pub fn emit_result(json_mode: bool, result: &dyn CommandResult) -> io::Result<()> {
    let mut out = io::stdout().lock();
    if json_mode {
        let envelope = json!({
            "schemaVersion": SCHEMA_VERSION,
            "success": true,
            "data": result.json_data(),
        });
        writeln!(out, "{}", to_string(&envelope))
    } else {
        result.write_text(&mut out)
    }
}

/// Emit an error: the `--json` failure envelope on stdout, or `error: <msg>` on
/// stderr (text mode), followed by an `os error <n>:` line spelling out a raw OS
/// code the message names and a `hint:` line when the error has an actionable
/// follow-up. Returns the process exit code from the error's category.
pub fn emit_error(json_mode: bool, err: &CliError) -> i32 {
    // A compile failure carries the real compiler diagnostics; stream them to
    // stderr in BOTH modes (so stdout stays clean for the single result object)
    // before the summary error - mirroring the TS output.error /
    // writeCompilerDiagnostics.
    if let Some(diagnostics) = err.compiler_diagnostics() {
        eprintln!("{diagnostics}");
    }
    if json_mode {
        // Best-effort: a broken stdout still must not mask the exit code. The
        // origin location is DIAGNOSTIC and stays OUT of the machine envelope,
        // as do the `os error`/`hint:` lines - a machine consumer reads
        // `error.details`.
        let _ = writeln!(io::stdout().lock(), "{}", to_string(&error_envelope(err)));
    } else {
        match err.location() {
            Some(location) => eprintln!("error: {} (at {location})", err.message()),
            None => eprintln!("error: {}", err.message()),
        }
        // What the raw OS code in the message means, then what to do about it.
        if let Some((code, message)) = err.os_error_message() {
            eprintln!("os error {code}: {message}");
        }
        if let Some(hint) = err.hint() {
            eprintln!("hint: {hint}");
        }
    }
    err.exit_code()
}

/// Build the `--json` failure envelope: `{ schemaVersion, success: false,
/// error: { code, message } }`, plus the structured wire `details` inside
/// `error` when the error carries them - a `COMPILER_FAILED` surfaces its
/// target / exitCode / output here so a machine consumer need not scrape the
/// `[compile:<arch>]` stderr text. One owner for the shape, so `emit_error` and
/// its test cannot drift.
fn error_envelope(err: &CliError) -> Value {
    let mut error = json!({ "code": err.code(), "message": err.message() });
    if let Some(details) = err.details() {
        // The `json!` macro always builds an object here, so the insert cannot
        // fail; `expect` names the invariant rather than silently dropping.
        error
            .as_object_mut()
            .expect("error envelope is a JSON object")
            .insert("details".to_owned(), details.clone());
    }
    json!({
        "schemaVersion": SCHEMA_VERSION,
        "success": false,
        "error": error,
    })
}

/// Serialize a struct to a JSON `Value` for output. The CLI's result and config
/// structs cannot fail to serialize (no non-string map keys, no non-finite
/// floats), so a failure is a bug, not a runtime condition: panic with a clear
/// message rather than emit a silently wrong (empty or partial) render via a
/// `Null` fallback.
pub(crate) fn to_value(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("serialization is infallible")
}

/// Serialize the `--json` envelope to its wire string. The envelope is a plain
/// JSON object, so this cannot fail; panic rather than emit a `{}` that drops
/// the entire success/error payload a `--json` consumer reads.
fn to_string(value: &Value) -> String {
    serde_json::to_string(value).expect("serialization is infallible")
}

/// Render a `CommandResult`'s text form to a `String` (the golden-test helper):
/// the same `write_text` the text-mode `emit_result` drives, captured into a
/// buffer so a command module's render tests can assert the exact text without
/// a live session. Used by the per-command `render_tests` modules (snapshot
/// tests of the compute-then-render seam).
#[cfg(test)]
pub(crate) fn render_text(result: &dyn CommandResult) -> String {
    let mut buf = Vec::new();
    result.write_text(&mut buf).expect("render text");
    String::from_utf8(buf).expect("utf8 text")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl CommandResult for Fixed {
        fn json_data(&self) -> Value {
            json!({ "a": 1 })
        }
        fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
            writeln!(out, "a=1")
        }
    }

    #[test]
    fn json_envelope_field_order_and_shape() {
        let envelope = json!({
            "schemaVersion": SCHEMA_VERSION,
            "success": true,
            "data": Fixed.json_data(),
        });
        // preserve_order keeps schemaVersion, success, data in that order.
        assert_eq!(
            to_string(&envelope),
            r#"{"schemaVersion":1,"success":true,"data":{"a":1}}"#
        );
    }

    #[test]
    fn to_value_serializes_without_a_silent_fallback() {
        // The helper exists to panic on the impossible serialization failure
        // rather than mask it as `Null`; on a real value it just serializes.
        assert_eq!(to_value(&json!({ "a": 1 })), json!({ "a": 1 }));
    }

    #[test]
    fn error_envelope_omits_details_when_absent() {
        let err = CliError::mod_not_installed("m");
        assert_eq!(
            to_string(&error_envelope(&err)),
            r#"{"schemaVersion":1,"success":false,"error":{"code":"MOD_NOT_INSTALLED","message":"Mod not installed: m"}}"#
        );
    }

    #[test]
    fn error_envelope_carries_structured_details_when_present() {
        // A COMPILER_FAILED carries its wire `details`; the --json envelope
        // surfaces them VERBATIM under error.details, after code/message.
        use windhawk_core_protocol::{ErrorCode, WireError};
        let err = CliError::from_wire(WireError::with_details(
            ErrorCode::CompilerFailed,
            "Compilation failed",
            json!({ "target": "x86_64-w64-mingw32", "exitCode": 1 }),
        ));
        assert_eq!(
            to_string(&error_envelope(&err)),
            r#"{"schemaVersion":1,"success":false,"error":{"code":"COMPILE_FAILED","message":"Compilation failed","details":{"target":"x86_64-w64-mingw32","exitCode":1}}}"#
        );
    }
}
