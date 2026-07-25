//! The session-free synchronous transport (`WhCoreInvokeStateless`): dispatches
//! ONLY the stateless subset of the command table - the pure helpers whose
//! handler is `Handler::Stateless`, which read no session state and need no
//! resolved storage. A storage-bearing command (any non-stateless handler) is
//! rejected with INVALID_REQUEST, so a caller cannot reach storage without a
//! session; the session-owns-storage invariant stays intact. The same commands
//! stay reachable through `Session::invoke` (`WhCoreInvoke`) on a real session.

use serde_json::Value;
use windhawk_core_protocol::{RequestEnvelope, response_err, response_ok};

use crate::dispatch::{CommandKind, Handler, find_command};
use crate::error::CoreError;

/// Serve a stateless command with no session. Always returns a response
/// envelope (the ffi `catch_unwind` is the outer net; no stateless handler
/// panics on valid input).
pub fn invoke_stateless(request_json: &str) -> String {
    match dispatch_stateless(request_json) {
        Ok(value) => response_ok(&value),
        Err(e) => response_err(&e.to_wire()),
    }
}

fn dispatch_stateless(request_json: &str) -> Result<Value, CoreError> {
    let request: RequestEnvelope = serde_json::from_str(request_json)
        .map_err(|e| CoreError::invalid_request(format!("invalid request JSON: {e}")))?;
    let spec = find_command(&request.command).ok_or_else(|| {
        CoreError::invalid_request(format!("unknown command: {}", request.command))
    })?;
    match (&spec.kind, &spec.handler) {
        (CommandKind::Sync, Handler::Stateless(run)) => run(request.params),
        // Anything that is not a stateless handler needs a session; reject it
        // here rather than silently spinning up one.
        _ => Err(CoreError::invalid_request(format!(
            "command requires a session and must be invoked via WhCoreInvoke: {}",
            request.command
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn invoke(request: Value) -> Value {
        serde_json::from_str(&invoke_stateless(&request.to_string())).expect("response envelope")
    }

    #[test]
    fn serves_parse_mod_source_without_a_session() {
        let source = "// ==WindhawkMod==\n// @id stateless-test\n// ==/WindhawkMod==\n";
        let resp = invoke(json!({
            "command": "parseModSource",
            "params": {"source": source, "language": "en"},
        }));
        assert_eq!(resp["ok"], json!(true));
        assert_eq!(resp["result"]["metadata"]["id"], json!("stateless-test"));
    }

    #[test]
    fn serves_the_other_pure_helpers() {
        // getCompileFlags takes no params.
        let flags = invoke(json!({"command": "getCompileFlags", "params": {}}));
        assert_eq!(flags["ok"], json!(true));
        assert!(flags["result"].is_array());

        // appendToModIdAndName is a pure source transform.
        let appended = invoke(json!({
            "command": "appendToModIdAndName",
            "params": {"source": "// @id foo\n", "appendToId": "x", "appendToName": null},
        }));
        assert_eq!(appended["ok"], json!(true));
    }

    #[test]
    fn serves_inspect_user_data_without_a_session() {
        // inspectUserData is pure over the archive string, so the session-free
        // transport serves it (letting `data inspect` validate a file with no
        // app root).
        let archive = "{\"format\": \"windhawk-user-data-v1\", \"mods\": []}";
        let resp = invoke(json!({
            "command": "inspectUserData",
            "params": { "archive": archive },
        }));
        assert_eq!(resp["ok"], json!(true), "{resp}");
        assert_eq!(resp["result"]["manifest"]["hasAppSettings"], json!(false));

        // A malformed archive is rejected as INVALID_REQUEST, not a panic.
        let bad = invoke(json!({
            "command": "inspectUserData",
            "params": { "archive": "not an archive" },
        }));
        assert_eq!(bad["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn rejects_a_storage_bearing_command_with_invalid_request() {
        let resp = invoke(json!({"command": "getAppSettings"}));
        assert_eq!(resp["ok"], json!(false));
        assert_eq!(resp["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn rejects_export_user_data_which_needs_a_session() {
        // exportUserData reads storage, so the session-free transport must
        // reject it (it is not a stateless handler).
        let resp = invoke(json!({"command": "exportUserData"}));
        assert_eq!(resp["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn rejects_an_async_command_with_invalid_request() {
        let resp = invoke(json!({"command": "fetchCatalog"}));
        assert_eq!(resp["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn rejects_an_unknown_command() {
        let resp = invoke(json!({"command": "noSuchCommand"}));
        assert_eq!(resp["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn rejects_malformed_request_json() {
        let resp: Value = serde_json::from_str(&invoke_stateless("not json")).unwrap();
        assert_eq!(resp["error"]["code"], json!("INVALID_REQUEST"));
    }
}
