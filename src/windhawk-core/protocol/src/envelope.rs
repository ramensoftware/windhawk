//! Request, response, and event envelopes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::WireError;

/// A command request: `{ "command": "...", "params": { ... } }`. Params stay
/// a raw JSON value here; each command decodes them into its own params DTO
/// (unknown commands must be rejected before any params decoding).
#[derive(Deserialize, Debug, Clone)]
pub struct RequestEnvelope {
    pub command: String,
    #[serde(default)]
    pub params: Value,
}

/// Success response envelope: `{ "ok": true, "result": ... }`.
#[derive(Serialize, Debug)]
struct OkEnvelope<'a> {
    ok: bool,
    result: &'a Value,
}

/// Failure response envelope: `{ "ok": false, "error": { ... } }`. Also the
/// shape of the `*outErrorJson` documents of `WhCoreSessionCreate` and
/// `WhCoreInvokeAsync`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ErrEnvelope {
    pub ok: bool,
    pub error: WireError,
}

fn to_json_or_internal_error<T: Serialize>(value: &T) -> String {
    // Serialization of envelope types cannot fail for tree-shaped data; if
    // it ever does, fall back to a hand-built INTERNAL envelope rather than
    // panicking across the dispatch path.
    serde_json::to_string(value).unwrap_or_else(|e| {
        format!(
            "{{\"ok\":false,\"error\":{{\"code\":\"INTERNAL\",\"message\":\"response serialization failed: {e}\"}}}}"
        )
    })
}

/// Serialize a success envelope around an already-serialized result value.
pub fn response_ok(result: &Value) -> String {
    to_json_or_internal_error(&OkEnvelope { ok: true, result })
}

/// Serialize a failure envelope.
pub fn response_err(error: &WireError) -> String {
    to_json_or_internal_error(&ErrEnvelope {
        ok: false,
        error: error.clone(),
    })
}

/// Events of an asynchronous operation: a stream of command-specific `progress`
/// payloads, plus the update flow's one-shot `installing` transition,
/// terminated by exactly one `completed` or `failed`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OperationEvent {
    Progress {
        payload: Value,
    },
    /// `startUpdate`'s transition from download to installer launch (the TS
    /// `onInstalling`); carries no payload.
    Installing,
    Completed {
        result: Value,
    },
    Failed {
        error: WireError,
    },
}

impl OperationEvent {
    pub fn to_json(&self) -> String {
        to_json_or_internal_error(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn ok_envelope_shape() {
        let s = response_ok(&serde_json::json!({"a": 1}));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v, serde_json::json!({"ok": true, "result": {"a": 1}}));
    }

    #[test]
    fn err_envelope_shape() {
        let s = response_err(&WireError::new(ErrorCode::InvalidRequest, "bad"));
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "ok": false,
                "error": {"code": "INVALID_REQUEST", "message": "bad"}
            })
        );
    }

    #[test]
    fn event_shapes() {
        let progress = OperationEvent::Progress {
            payload: serde_json::json!({"percent": 42}),
        };
        let v: Value = serde_json::from_str(&progress.to_json()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"type": "progress", "payload": {"percent": 42}})
        );

        let failed = OperationEvent::Failed {
            error: WireError::new(ErrorCode::Canceled, "canceled"),
        };
        let v: Value = serde_json::from_str(&failed.to_json()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "type": "failed",
                "error": {"code": "CANCELED", "message": "canceled"}
            })
        );
    }

    #[test]
    fn request_params_default_to_null() {
        let req: RequestEnvelope = serde_json::from_str(r#"{"command": "x"}"#).unwrap();
        assert_eq!(req.command, "x");
        assert_eq!(req.params, Value::Null);
    }
}
