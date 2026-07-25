//! Reply shaping: mapping the core's outcome - a success `result` `Value` or a
//! [`HostError`] - into the reply `Value` the front-end expects, so success and
//! failure are two halves of ONE function. Read handlers embed their own
//! success+failure shaping inline (each swallows a core error into its
//! command-specific default, mirroring the extension's `try/catch`), so the
//! shared [`default_shaper`] here is the BACKSTOP the bridge applies to a
//! handler that PROPAGATES an `Err` (a malformed `data` it cannot shape),
//! keeping the "exactly one reply per messageWithReply" invariant total. The
//! reusable per-command `ReplyShaper` fn-pointer type lands with the async
//! terminal/progress path, where the same shaper must serve both the sync and
//! async forms of a command.

use serde_json::{Value, json};
use windhawk_core_host::{HostError, HostErrorKind};
use windhawk_core_protocol::{ErrorCode, SourceLocation};

/// The shared default shaper: forward a success `result` untouched, and represent
/// a failure as the standard error payload. Used as the bridge backstop for a
/// propagated handler `Err`.
pub fn default_shaper(result: Result<Value, HostError>) -> Value {
    match result {
        Ok(value) => value,
        Err(error) => host_error_payload(&error),
    }
}

/// The front-end's standard error payload: `{ error: { code, message, location? } }`,
/// the backstop reply for an error a handler could not shape into a command-specific
/// reply.
pub fn host_error_payload(error: &HostError) -> Value {
    json!({ "error": error_object(error) })
}

/// The standard error object `{ code, message, path?, location? }` the front-end
/// surfaces generically (the unified error notification). A [`HostErrorKind::Wire`]
/// carries its stable `code` and, for an IO/registry/network failure, the failing
/// resource as `path` (the most useful locus, far more than the source line for an
/// environmental error). The origin `location` (DIAGNOSTIC) rides a SEPARATE field,
/// present only when the error carries one, so the `message` the front-end shows
/// stays clean.
pub fn error_object(error: &HostError) -> Value {
    let (code, path) = match error.kind() {
        HostErrorKind::Wire(wire) => (
            error_code_str(wire.code),
            wire.details.as_ref().and_then(error_locus),
        ),
        _ => ("INTERNAL", None),
    };
    let mut object = error_fields(code, &error.to_string(), error.location());
    if let Some(path) = path {
        object["path"] = Value::String(path);
    }
    object
}

/// The failing resource a wire error's `details` names - the file `path`, registry
/// `key`, or repo `url` - surfaced as the human "path" the notification shows above
/// the code/origin. The single most useful locus for an IO/registry/network failure;
/// `None` for codes whose details carry none (and the compiler-output details, whose
/// surface is the log window).
fn error_locus(details: &Value) -> Option<String> {
    ["path", "key", "url"]
        .into_iter()
        .find_map(|field| details.get(field).and_then(Value::as_str))
        .map(str::to_owned)
}

/// Attach [`error_object`] to an existing reply, so a command that represents a
/// failure with a command-specific default (an empty map, `null`, or
/// `succeeded: false`) ALSO carries the machine-readable error the front-end
/// surfaces. Inserts only when the reply has no `error` field yet, so a reply that
/// already shaped its own (`startUpdate`'s `{ succeeded, error }` string) is left
/// untouched. A no-op when `reply` is not a JSON object.
pub fn attach_error(reply: &mut Value, error: &HostError) {
    attach_error_object(reply, error_object(error));
}

/// [`attach_error`] with a pre-built object, for the async terminal path that
/// captures the object before the outcome is moved into the shaper.
pub fn attach_error_object(reply: &mut Value, object: Value) {
    if let Value::Object(map) = reply {
        map.entry("error").or_insert(object);
    }
}

/// The human-facing message of a [`HostError`], for the commands whose reply
/// carries an `error` STRING rather than the standard `{ error: { code, message } }`
/// payload (`startUpdate`, whose reply is `{ succeeded, error? }`). A
/// [`HostErrorKind::Wire`] surfaces its wire `message` (the extension's
/// `e.message`); the no-wire arms surface their own message.
pub fn error_message(error: &HostError) -> String {
    match error.kind() {
        HostErrorKind::Wire(wire) => wire.message.clone(),
        _ => error.to_string(),
    }
}

/// A typed error reply payload from an explicit `code`/`message`, for the
/// development `UNSUPPORTED` stubs and the `INVALID_REQUEST` unknown command (codes
/// outside the core's [`ErrorCode`] table, so they are passed as strings). No origin
/// location (these are raised at the UI seam, not the core).
pub fn ui_error_payload(code: &str, message: &str) -> Value {
    error_payload(code, message, None)
}

/// Build `{ error: { code, message, location? } }` from explicit fields (the
/// dev-stub `UNSUPPORTED` / unknown-command `INVALID_REQUEST` payloads).
fn error_payload(code: &str, message: &str, location: Option<&SourceLocation>) -> Value {
    json!({ "error": error_fields(code, message, location) })
}

/// The inner `{ code, message, location? }` object, attaching the origin only when
/// present. One owner so the standard payload, the dev-stub/unknown-command payload,
/// and the attached-error object cannot drift in shape.
fn error_fields(code: &str, message: &str, location: Option<&SourceLocation>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(location) = location {
        error["location"] = serde_json::to_value(location).expect("SourceLocation serializes");
    }
    error
}

/// The stable SCREAMING_SNAKE string of an [`ErrorCode`] (its serde wire form).
fn error_code_str(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidRequest => "INVALID_REQUEST",
        ErrorCode::AppRootInvalid => "APP_ROOT_INVALID",
        ErrorCode::ModNotInstalled => "MOD_NOT_INSTALLED",
        ErrorCode::ModNotInRepo => "MOD_NOT_IN_REPO",
        ErrorCode::RepoUnreachable => "REPO_UNREACHABLE",
        ErrorCode::CompilerFailed => "COMPILER_FAILED",
        ErrorCode::DevToolsMissing => "DEV_TOOLS_MISSING",
        ErrorCode::RestartRequired => "RESTART_REQUIRED",
        ErrorCode::Canceled => "CANCELED",
        ErrorCode::UpdateInProgress => "UPDATE_IN_PROGRESS",
        ErrorCode::IoFailed => "IO_FAILED",
        ErrorCode::RegistryFailed => "REGISTRY_FAILED",
        ErrorCode::Internal => "INTERNAL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windhawk_core_protocol::WireError;

    #[test]
    fn default_shaper_forwards_success() {
        let value = json!({ "appSettings": { "language": "en" } });
        assert_eq!(default_shaper(Ok(value.clone())), value);
    }

    #[test]
    fn default_shaper_maps_wire_error_to_its_code() {
        // A wire error without a location yields the clean `{ code, message }`.
        let err = HostError::wire(WireError::new(ErrorCode::ModNotInstalled, "nope"));
        assert_eq!(
            default_shaper(Err(err)),
            json!({ "error": { "code": "MOD_NOT_INSTALLED", "message": "nope" } })
        );
    }

    #[test]
    fn wire_error_payload_carries_the_origin_as_a_separate_field() {
        let wire = WireError::new(ErrorCode::Internal, "boom").at(Some(SourceLocation {
            file: "core/src/services/mods.rs".to_owned(),
            line: 12,
        }));
        let payload = host_error_payload(&HostError::wire(wire));
        assert_eq!(payload["error"]["message"], "boom");
        assert_eq!(
            payload["error"]["location"],
            json!({ "file": "core/src/services/mods.rs", "line": 12 })
        );
    }

    #[test]
    fn default_shaper_collapses_no_wire_arms_to_internal() {
        let err = HostError::decode("bad data".to_owned());
        let payload = default_shaper(Err(err));
        assert_eq!(payload["error"]["code"], "INTERNAL");
        assert_eq!(payload["error"]["message"], "bad data");
        // A no-wire arm captures its #[track_caller] origin, carried separately.
        assert!(payload["error"]["location"].is_object());
    }

    #[test]
    fn attach_error_adds_the_error_object_to_a_failure_reply() {
        let mut reply = json!({ "modId": "m", "succeeded": false });
        attach_error(
            &mut reply,
            &HostError::wire(WireError::new(ErrorCode::RegistryFailed, "denied")),
        );
        assert_eq!(reply["modId"], json!("m"));
        assert_eq!(reply["succeeded"], json!(false));
        assert_eq!(
            reply["error"],
            json!({ "code": "REGISTRY_FAILED", "message": "denied" })
        );
    }

    #[test]
    fn attach_error_does_not_clobber_an_existing_error_field() {
        // startUpdate shapes its own `error` STRING; attaching must leave it.
        let mut reply = json!({ "succeeded": false, "error": "already updating" });
        attach_error(
            &mut reply,
            &HostError::wire(WireError::new(ErrorCode::UpdateInProgress, "x")),
        );
        assert_eq!(reply["error"], json!("already updating"));
    }

    #[test]
    fn attach_error_is_a_noop_on_a_non_object_reply() {
        let mut reply = json!(null);
        attach_error(&mut reply, &HostError::decode("x".to_owned()));
        assert_eq!(reply, json!(null));
    }

    #[test]
    fn error_object_surfaces_the_failing_path_from_details() {
        let wire = WireError::with_details(
            ErrorCode::IoFailed,
            "Access is denied. (os error 5)",
            json!({ "path": "C:\\mods\\x.wh.cpp" }),
        );
        let object = error_object(&HostError::wire(wire));
        assert_eq!(object["code"], json!("IO_FAILED"));
        assert_eq!(object["path"], json!("C:\\mods\\x.wh.cpp"));
    }

    #[test]
    fn error_object_uses_the_registry_key_then_url_as_the_locus() {
        let registry = WireError::with_details(
            ErrorCode::RegistryFailed,
            "denied",
            json!({ "key": "Engine\\Mods\\x" }),
        );
        assert_eq!(
            error_object(&HostError::wire(registry))["path"],
            json!("Engine\\Mods\\x")
        );

        let repo = WireError::with_details(
            ErrorCode::RepoUnreachable,
            "down",
            json!({ "url": "https://mods.windhawk.net" }),
        );
        assert_eq!(
            error_object(&HostError::wire(repo))["path"],
            json!("https://mods.windhawk.net")
        );
    }

    #[test]
    fn error_object_has_no_path_when_details_carry_no_locus() {
        let object = error_object(&HostError::wire(WireError::new(ErrorCode::Canceled, "x")));
        assert_eq!(object.get("path"), None);
    }

    #[test]
    fn error_code_str_matches_the_serde_wire_form() {
        // Guard the hand-written table against the serde derive drifting.
        for code in [
            ErrorCode::InvalidRequest,
            ErrorCode::AppRootInvalid,
            ErrorCode::ModNotInstalled,
            ErrorCode::ModNotInRepo,
            ErrorCode::RepoUnreachable,
            ErrorCode::CompilerFailed,
            ErrorCode::Canceled,
            ErrorCode::UpdateInProgress,
            ErrorCode::IoFailed,
            ErrorCode::RegistryFailed,
            ErrorCode::Internal,
        ] {
            assert_eq!(
                Value::String(error_code_str(code).to_owned()),
                serde_json::to_value(code).unwrap()
            );
        }
    }
}
