//! The four webview envelope types. One JSON shape carries all four; the `type`
//! discriminator and the optional `messageId` distinguish them. The front-end
//! correlates a `reply` to its `messageWithReply` by `(command, messageId)`, so
//! an outbound `reply` MUST echo the request's `command` and `messageId`
//! verbatim.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The envelope `type` discriminator. Serialized camelCase to match the
/// front-end's literal union (`message` | `messageWithReply` | `reply` | `event`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvelopeType {
    /// Front-end -> backend, fire-and-forget (no reply).
    Message,
    /// Front-end -> backend, expects exactly one `reply` with the same
    /// `(command, messageId)`.
    MessageWithReply,
    /// Backend -> front-end, the response to a `messageWithReply`.
    Reply,
    /// Backend -> front-end, an unsolicited push.
    Event,
}

/// One webview IPC envelope. `message_id` is present on `messageWithReply` and its
/// `reply`, absent on `message` and `event` (skipped on serialize so an outbound
/// `event` carries no `messageId`, matching the front-end shape).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    #[serde(rename = "type")]
    pub kind: EnvelopeType,
    pub command: String,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<i64>,
}

impl Envelope {
    /// Build a `reply` envelope for a `messageWithReply`, echoing its `command`
    /// and `messageId` so the front-end's `(command, messageId)` correlation
    /// resolves the right promise.
    pub fn reply(command: impl Into<String>, message_id: i64, data: Value) -> Envelope {
        Envelope {
            kind: EnvelopeType::Reply,
            command: command.into(),
            data,
            message_id: Some(message_id),
        }
    }

    /// Build an unsolicited `event` envelope (no `messageId`).
    pub fn event(command: impl Into<String>, data: Value) -> Envelope {
        Envelope {
            kind: EnvelopeType::Event,
            command: command.into(),
            data,
            message_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_with_reply_round_trips() {
        let raw = json!({
            "type": "messageWithReply",
            "command": "getModConfig",
            "data": { "modId": "m" },
            "messageId": 7,
        });
        let env: Envelope = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(env.kind, EnvelopeType::MessageWithReply);
        assert_eq!(env.command, "getModConfig");
        assert_eq!(env.message_id, Some(7));
        assert_eq!(serde_json::to_value(&env).unwrap(), raw);
    }

    #[test]
    fn message_has_no_message_id() {
        let raw = json!({ "type": "message", "command": "showLogOutput", "data": {} });
        let env: Envelope = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(env.kind, EnvelopeType::Message);
        assert_eq!(env.message_id, None);
        // Absent messageId is skipped on serialize, not emitted as null.
        assert_eq!(serde_json::to_value(&env).unwrap(), raw);
    }

    #[test]
    fn reply_and_event_builders_shape_the_envelope() {
        let reply = Envelope::reply("getAppSettings", 3, json!({ "appSettings": {} }));
        assert_eq!(
            serde_json::to_value(&reply).unwrap(),
            json!({
                "type": "reply",
                "command": "getAppSettings",
                "data": { "appSettings": {} },
                "messageId": 3,
            })
        );

        let event = Envelope::event("setNewModConfig", json!({ "modId": "m", "config": {} }));
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "event",
                "command": "setNewModConfig",
                "data": { "modId": "m", "config": {} },
            })
        );
    }
}
