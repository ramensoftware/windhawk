//! [`classify_event`]: the one piece of event interpretation both drivers
//! share. The host imposes no driving model - the CLI blocks draining an `mpsc`
//! channel, the UI pumps a bounded channel to `emit` - but the `failed ->
//! WireError` terminal decode is identical for both, so it lives ONCE here,
//! alongside the sync envelope decode, rather than being re-spelled in each
//! driver.

use serde_json::Value;
use windhawk_core_protocol::{OperationEvent, WireError};

use crate::error::HostError;

/// A classified operation event: the terminal `completed`/`failed`, or a
/// non-terminal `progress`/`installing` the consumer routes as it pleases. A
/// terminal class yields exactly one outcome (a result value or a wire error);
/// the consumer owns what to do with each.
pub enum EventClass {
    /// A non-terminal `progress` or `installing` event, carried verbatim so the
    /// consumer's progress mapper reads its payload.
    Progress(OperationEvent),
    /// The operation completed with this result value.
    Completed(Value),
    /// The operation failed with this structured wire error.
    Failed(WireError),
}

/// Decode a raw operation-event JSON and classify it. A malformed event JSON is a
/// [`HostError::Decode`] (the same wording a result-decode failure carries).
pub fn classify_event(event_json: &str) -> Result<EventClass, HostError> {
    match serde_json::from_str::<OperationEvent>(event_json)? {
        OperationEvent::Completed { result } => Ok(EventClass::Completed(result)),
        OperationEvent::Failed { error } => Ok(EventClass::Failed(error)),
        event @ (OperationEvent::Progress { .. } | OperationEvent::Installing) => {
            Ok(EventClass::Progress(event))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HostErrorKind;
    use serde_json::json;
    use windhawk_core_protocol::ErrorCode;

    #[test]
    fn completed_carries_the_result() {
        let event = json!({ "type": "completed", "result": { "ok": 1 } }).to_string();
        let EventClass::Completed(result) = classify_event(&event).unwrap() else {
            panic!("expected Completed");
        };
        assert_eq!(result, json!({ "ok": 1 }));
    }

    #[test]
    fn failed_carries_the_decoded_wire_error() {
        let event = json!({ "type": "failed", "error": { "code": "CANCELED", "message": "stop" } })
            .to_string();
        let EventClass::Failed(wire) = classify_event(&event).unwrap() else {
            panic!("expected Failed");
        };
        assert_eq!(wire.code, ErrorCode::Canceled);
        assert_eq!(wire.message, "stop");
    }

    #[test]
    fn progress_and_installing_are_forwarded() {
        let progress = json!({ "type": "progress", "payload": { "progress": 42 } }).to_string();
        assert!(matches!(
            classify_event(&progress).unwrap(),
            EventClass::Progress(OperationEvent::Progress { .. })
        ));
        let installing = json!({ "type": "installing" }).to_string();
        assert!(matches!(
            classify_event(&installing).unwrap(),
            EventClass::Progress(OperationEvent::Installing)
        ));
    }

    #[test]
    fn malformed_event_is_a_decode_error() {
        let Err(error) = classify_event("not json") else {
            panic!("expected a decode error");
        };
        assert!(matches!(error.kind(), HostErrorKind::Decode(_)));
    }
}
