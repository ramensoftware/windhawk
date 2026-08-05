//! The Windhawk frame set: what the two ends of the runtime-broker channel say
//! to each other, and the three values they have to agree on.
//!
//! It lives here rather than in the transport crate because these are Windhawk's
//! words - `invoke`, `invokeAsync`, `cancel`, `host`, `event`, `log` - and the
//! transport is generic over them so it can be tested with a toy vocabulary and
//! no command surface at all. The protocol integer, the product version, and
//! the frame cap are supplied from here for the same reason: a version read
//! inside the transport would be the TRANSPORT's, identical on both sides by
//! construction.
//!
//! **The payload-carrying fields are raw JSON values, not strings**: a
//! request carries the envelope `windhawk_core_host` already built and a response
//! carries the core's own reply, both verbatim, so the bytes the core sees are
//! the bytes the host wrote and the bytes the UI parses are the bytes the core
//! produced - structurally, not by agreement.
//!
//! That is also why the two frames are flat structs with optional fields rather
//! than the tagged enums they look like they should be. `serde_json`'s
//! `RawValue` only survives a DIRECT deserialization: an internally tagged enum
//! (like a `#[serde(tag = "t")]`) buffers the whole frame into an intermediate
//! representation first, which re-renders the payload it is supposed to carry
//! untouched and defeats the property above. So the discriminant is an ordinary
//! field, and the typed [`Response`] / [`Push`] views are recovered from the
//! frame in code.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;
use windhawk_broker::{ChannelConfig, RequestFrames, Routed};
use windhawk_core_host::{HostError, HostErrorKind};
use windhawk_core_protocol::{ErrorCode, MAX_ARCHIVE_BYTES, WireError};

/// The wire version. An integer that must match exactly - there is no
/// negotiation and no compatibility range - bumped on any change to the frames
/// below.
pub const CHANNEL_PROTOCOL: u32 = 4;

/// How much bigger than the archive it carries a frame may get. The user-data
/// archive crosses whole in both directions (inside `importUserData`'s params
/// and inside `exportUserData`'s result), and it rides inside a JSON string, so
/// every quote and backslash in it costs a second byte.
const ARCHIVE_ESCAPE_FACTOR: usize = 2;

/// What the frame may spend on everything that is not the archive: the request
/// envelope around it, the command name, the frame's own fields. Fixed, because
/// none of it scales with the payload.
const FRAME_OVERHEAD_ALLOWANCE: usize = 1024 * 1024;

/// The largest payload a frame may carry, derived from the largest one the
/// contract accepts rather than guessed. A cap below this would fail a
/// contract-legal archive on a healthy machine; the transport cannot compute it
/// itself, because it must not know Windhawk's contract.
pub const FRAME_CAP: usize =
    MAX_ARCHIVE_BYTES as usize * ARCHIVE_ESCAPE_FACTOR + FRAME_OVERHEAD_ALLOWANCE;

/// What both ends must agree on before either acts on anything.
pub fn channel_config() -> ChannelConfig {
    ChannelConfig {
        protocol: CHANNEL_PROTOCOL,
        // The PRODUCT version, which can differ across a channel even though both
        // processes run the same path on disk: replacing `windhawk-ui.exe` while
        // this one runs makes the broker started afterwards the new binary.
        version: env!("CARGO_PKG_VERSION").to_owned(),
        frame_cap: FRAME_CAP,
    }
}

/// The tag a request frame carries. One value, because the requesting end sends
/// exactly one kind of frame; the discriminant that matters is [`RequestKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestTag {
    Req,
}

/// What a request asks for. `k` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestKind {
    /// A synchronous core command, from a built request envelope.
    Invoke,
    /// An asynchronous core command's START, from a built request envelope.
    InvokeAsync,
    /// Cooperative cancellation of one core op-id.
    Cancel,
    /// One of the closed set of privileged host operations ([`HostOp`]).
    Host,
    /// The last request on the channel.
    Shutdown,
}

/// The closed set of privileged host operations. Each names a fixed effect: no
/// operation takes an executable path, a command line, or a path outside the
/// app-data tree from the requesting end, and the broker derives every path it
/// touches from its own `getCoreInfo`.
///
/// One member per USER ACTION rather than per file operation, which is what keeps
/// the local and the remote implementation of [`crate::broker::ops::HostOps`] both
/// natural: a finer set would make the local one a pointless indirection and this
/// one chatty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOp {
    /// Copy the install tree's `ModsRuntime` into the engine's `Engine\Mods`.
    SeedModsRuntime,
    /// Prepare the workspace for a mod and open the code editor on it. Its
    /// arguments are [`crate::broker::ops::EditorOpen`].
    EditorOpen,
    /// Garbage-collect abandoned editor workspaces.
    EditorSweep,
    /// Sync the shared VSCodium user settings' color-theme keys. Its argument is
    /// `{"theme":"dark"|"light"|"auto"}`, the setting as the core stores it.
    EditorSyncTheme,
    /// Start the cross-session `Global\` debug-output capture, whose lines come
    /// back as [`BrokerTag::Dbwin`] pushes.
    DbwinStart,
    /// Stop it, releasing the single-owner `Global\` DBWIN objects.
    DbwinStop,
}

impl HostOp {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            HostOp::SeedModsRuntime => "seedModsRuntime",
            HostOp::EditorOpen => "editorOpen",
            HostOp::EditorSweep => "editorSweep",
            HostOp::EditorSyncTheme => "editorSyncTheme",
            HostOp::DbwinStart => "dbwinStart",
            HostOp::DbwinStop => "dbwinStop",
        }
    }

    /// The operation a peer named, or `None` for one this build does not serve.
    /// An unknown operation is answered with a typed failure and never falls
    /// through to anything general-purpose.
    pub fn parse(op: &str) -> Option<HostOp> {
        match op {
            "seedModsRuntime" => Some(HostOp::SeedModsRuntime),
            "editorOpen" => Some(HostOp::EditorOpen),
            "editorSweep" => Some(HostOp::EditorSweep),
            "editorSyncTheme" => Some(HostOp::EditorSyncTheme),
            "dbwinStart" => Some(HostOp::DbwinStart),
            "dbwinStop" => Some(HostOp::DbwinStop),
            _ => None,
        }
    }
}

/// A frame from the UI to the broker.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub t: RequestTag,
    /// The correlation id the transport stamps in; the response echoes it.
    pub id: u64,
    pub k: RequestKind,
    /// The core request envelope, carried verbatim for [`RequestKind::Invoke`]
    /// and [`RequestKind::InvokeAsync`]. NOT `command` + `params` as separate
    /// fields: the seam it comes from is string-shaped, so splitting them
    /// would mean parsing the envelope back apart and re-serializing it - a full
    /// copy of every payload up to [`FRAME_CAP`], and a re-rendering of the
    /// request rather than the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Box<RawValue>>,
    /// The core op-id a [`RequestKind::Cancel`] targets.
    #[serde(default, rename = "opId", skip_serializing_if = "Option::is_none")]
    pub op_id: Option<u64>,
    /// The [`HostOp`] a [`RequestKind::Host`] names, as its wire spelling: a
    /// string rather than a typed field so an operation this build does not know
    /// is answered rather than treated as an undecodable frame, which would take
    /// the channel down with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    /// What that operation was asked for, where it takes anything. An ordinary
    /// `Value` rather than a [`RawValue`]: these are small, host-authored
    /// arguments with no core payload in them, so nothing here has to survive
    /// byte for byte the way an envelope does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

impl Request {
    fn new(k: RequestKind) -> Request {
        Request {
            t: RequestTag::Req,
            // Stamped by the transport when the request is dispatched.
            id: 0,
            k,
            envelope: None,
            op_id: None,
            op: None,
            args: None,
        }
    }

    pub fn invoke(envelope: Box<RawValue>) -> Request {
        Request {
            envelope: Some(envelope),
            ..Request::new(RequestKind::Invoke)
        }
    }

    pub fn invoke_async(envelope: Box<RawValue>) -> Request {
        Request {
            envelope: Some(envelope),
            ..Request::new(RequestKind::InvokeAsync)
        }
    }

    pub fn cancel(op_id: u64) -> Request {
        Request {
            op_id: Some(op_id),
            ..Request::new(RequestKind::Cancel)
        }
    }

    pub fn host(op: HostOp, args: Option<Value>) -> Request {
        Request {
            op: Some(op.as_str().to_owned()),
            args,
            ..Request::new(RequestKind::Host)
        }
    }

    pub fn shutdown() -> Request {
        Request::new(RequestKind::Shutdown)
    }
}

/// The tag a frame from the broker carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerTag {
    /// The answer to one request.
    Res,
    /// A core operation event, unsolicited.
    Event,
    /// A core log line, unsolicited.
    Log,
    /// A batch of captured cross-session debug-output lines, unsolicited.
    Dbwin,
}

/// A frame from the broker to the UI: a response or an unsolicited push.
///
/// There is no `ok` field. It would say only what `error`'s presence already
/// says, and a core-level failure does not set it either way - that failure
/// rides inside `raw`, in the core's own envelope, which is the whole point of
/// carrying the envelope. A field nobody reads is one the next reader has
/// to research before they dare remove it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerFrame {
    pub t: BrokerTag,
    /// The request this answers ([`BrokerTag::Res`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// The core's response envelope for a sync invoke, or the operation event
    /// JSON for an [`BrokerTag::Event`] - either way exactly the bytes the core
    /// produced, unparsed by the broker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Box<RawValue>>,
    /// The core op-id an async start returned, or the op an event belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_id: Option<u64>,
    /// Whether a cancel found its op and signalled it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    /// A failure raised by the BROKER: the session is gone, a host operation
    /// failed, the operation is unknown. A core-level failure is never reported
    /// this way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Fault>,
    /// A [`BrokerTag::Log`] line's core log level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// A [`BrokerTag::Log`] line's text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// A [`BrokerTag::Dbwin`] batch: formatted `Global\` capture lines, already
    /// coalesced by the capture loop's own batching, so a flood costs a bounded
    /// number of channel crossings rather than one per line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
}

impl BrokerFrame {
    fn res(id: u64) -> BrokerFrame {
        BrokerFrame {
            t: BrokerTag::Res,
            id: Some(id),
            raw: None,
            op_id: None,
            cancelled: None,
            error: None,
            level: None,
            message: None,
            lines: None,
        }
    }

    /// The core's response envelope, verbatim.
    pub fn raw(id: u64, raw: Box<RawValue>) -> BrokerFrame {
        BrokerFrame {
            raw: Some(raw),
            ..BrokerFrame::res(id)
        }
    }

    /// The op-id an async command started under.
    pub fn started(id: u64, op_id: u64) -> BrokerFrame {
        BrokerFrame {
            op_id: Some(op_id),
            ..BrokerFrame::res(id)
        }
    }

    pub fn cancelled(id: u64, cancelled: bool) -> BrokerFrame {
        BrokerFrame {
            cancelled: Some(cancelled),
            ..BrokerFrame::res(id)
        }
    }

    /// The request is served and had nothing to return (a host operation, the
    /// shutdown).
    pub fn done(id: u64) -> BrokerFrame {
        BrokerFrame::res(id)
    }

    pub fn failed(id: u64, error: Fault) -> BrokerFrame {
        BrokerFrame {
            error: Some(error),
            ..BrokerFrame::res(id)
        }
    }

    /// One core operation event, its JSON carried unparsed.
    pub fn event(op_id: u64, raw: Box<RawValue>) -> BrokerFrame {
        BrokerFrame {
            t: BrokerTag::Event,
            id: None,
            raw: Some(raw),
            op_id: Some(op_id),
            cancelled: None,
            error: None,
            level: None,
            message: None,
            lines: None,
        }
    }

    /// One core log line.
    pub fn log(level: i32, message: String) -> BrokerFrame {
        BrokerFrame {
            t: BrokerTag::Log,
            id: None,
            raw: None,
            op_id: None,
            cancelled: None,
            error: None,
            level: Some(level),
            message: Some(message),
            lines: None,
        }
    }

    /// One batch of captured cross-session debug-output lines.
    pub fn dbwin(lines: Vec<String>) -> BrokerFrame {
        BrokerFrame {
            t: BrokerTag::Dbwin,
            id: None,
            raw: None,
            op_id: None,
            cancelled: None,
            error: None,
            level: None,
            message: None,
            lines: Some(lines),
        }
    }
}

/// A failure the BROKER raised, projected onto the host's own failure sum so it
/// arrives on the other side as the same kind of error an in-process session
/// would have produced.
///
/// A [`Fault::Wire`] carries the typed [`WireError`] whole - its stable code, its
/// per-code `details`, and the origin the CORE raised it at - so a command
/// refused by the core reads identically whichever session served it. That is
/// what keeps a rejected `invokeAsync` (a mod that is not installed, a malformed
/// install request) from being downgraded to a transport-flavoured message.
///
/// [`Fault::Broker`] is the arm for a failure the broker itself raised, and it is
/// deliberately NOT [`Fault::Transport`]: transport means the link is gone, which
/// is the one failure the UI puts a banner and a Retry behind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fault {
    Wire { error: Box<WireError> },
    Load { message: String },
    Gate { message: String },
    Transport { message: String },
    Decode { message: String },
    Broker { message: String },
}

impl Fault {
    /// Project a host failure onto the wire.
    ///
    /// The no-wire arms carry their message but not the source location they
    /// were raised at: the location field is the CORE's origin, and it is a
    /// [`Fault::Wire`] that has one. A broker-internal failure re-acquires the UI
    /// site when it is rebuilt, which names the seam it crossed rather than
    /// nothing at all.
    pub fn of(error: &HostError) -> Fault {
        match error.kind() {
            HostErrorKind::Wire(wire) => Fault::Wire {
                error: wire.clone(),
            },
            HostErrorKind::Load(message) => Fault::Load {
                message: message.clone(),
            },
            HostErrorKind::Gate(message) => Fault::Gate {
                message: message.clone(),
            },
            HostErrorKind::Transport(message) => Fault::Transport {
                message: message.clone(),
            },
            HostErrorKind::Decode(message) => Fault::Decode {
                message: message.clone(),
            },
        }
    }

    /// A broker-side failure with no core error behind it: a request the broker
    /// would not serve, a host operation that ran and failed, a reply too large
    /// for the wire. The channel carrying it is healthy - answering is what it
    /// just did - so it must not be spelled as a transport failure, which is the
    /// UI's word for a link that is gone.
    pub fn broker(message: String) -> Fault {
        Fault::Broker { message }
    }

    /// Rebuild the host failure.
    pub fn into_host(self) -> HostError {
        match self {
            Fault::Wire { error } => HostError::wire(*error),
            Fault::Load { message } => HostError::load(message),
            Fault::Gate { message } => HostError::gate(message),
            Fault::Transport { message } => HostError::transport(message),
            Fault::Decode { message } => HostError::decode(message),
            // The broker's own failure is neither the core's error nor a lost
            // link, so it rebuilds as the contract's `INTERNAL` carrying the
            // broker's message: the code the front-end already shows for a
            // failure with no better one. It is a [`WireError`] the BROKER
            // raised, so it carries no core origin - the message says which
            // limit or which operation, which is the locus that helps here.
            Fault::Broker { message } => {
                HostError::wire(WireError::new(ErrorCode::Internal, message))
            }
        }
    }
}

/// What a request resolved to.
pub enum Response {
    /// A sync invoke's core response envelope, verbatim: the caller parses it
    /// exactly as it parses an in-process session's.
    Raw(Box<RawValue>),
    /// An async command started, under this core op-id.
    Started(u64),
    /// A cancel was served; whether it found its op.
    Cancelled(bool),
    /// The request is served and returns nothing.
    Done,
    /// The broker could not serve the request.
    Failed(Fault),
}

/// A frame that answers no request.
pub enum Push {
    /// A core operation event, its JSON unparsed, for the op the broker's
    /// session issued the id to.
    Event { op_id: u64, raw: Box<RawValue> },
    /// A core log line.
    Log { level: i32, message: String },
    /// A batch of captured cross-session debug-output lines, for the log pane.
    Dbwin(Vec<String>),
    /// A frame that is none of the above. It cannot be attributed to a request -
    /// that is what makes it a push - so it is delivered to the sink to be
    /// reported rather than dropped where nobody would ever see it.
    Malformed(String),
}

/// The Windhawk channel: the vocabulary the transport is instantiated with.
pub struct Channel;

impl RequestFrames for Channel {
    type Request = Request;
    type Response = Response;
    type Push = Push;
    type Incoming = BrokerFrame;

    fn stamp(request: &mut Request, id: u64) {
        request.id = id;
    }

    fn route(incoming: BrokerFrame) -> Routed<Response, Push> {
        match incoming.t {
            BrokerTag::Res => match incoming.id {
                Some(id) => Routed::Response(id, response(incoming)),
                None => Routed::Push(Push::Malformed(
                    "a response frame carries no request id".to_owned(),
                )),
            },
            BrokerTag::Event => match (incoming.op_id, incoming.raw) {
                (Some(op_id), Some(raw)) => Routed::Push(Push::Event { op_id, raw }),
                _ => Routed::Push(Push::Malformed(
                    "an event frame carries no op-id or no event".to_owned(),
                )),
            },
            BrokerTag::Log => match (incoming.level, incoming.message) {
                (Some(level), Some(message)) => Routed::Push(Push::Log { level, message }),
                _ => Routed::Push(Push::Malformed(
                    "a log frame carries no level or no message".to_owned(),
                )),
            },
            BrokerTag::Dbwin => match incoming.lines {
                Some(lines) => Routed::Push(Push::Dbwin(lines)),
                None => Routed::Push(Push::Malformed(
                    "a capture frame carries no lines".to_owned(),
                )),
            },
        }
    }
}

/// Read the answer out of a response frame. The fields are mutually exclusive by
/// construction (one constructor each), and a frame carrying none of them is the
/// [`Response::Done`] a host operation or the shutdown produces.
fn response(frame: BrokerFrame) -> Response {
    if let Some(error) = frame.error {
        return Response::Failed(error);
    }
    if let Some(raw) = frame.raw {
        return Response::Raw(raw);
    }
    if let Some(op_id) = frame.op_id {
        return Response::Started(op_id);
    }
    if let Some(cancelled) = frame.cancelled {
        return Response::Cancelled(cancelled);
    }
    Response::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The cap has to admit the largest archive the contract allows, inside the
    /// envelope that carries it and with every character of it escaped. Measured
    /// rather than asserted about, because "headroom" that nobody computes decays
    /// back into the guess it replaced: the expansion is per character, so it is
    /// read off a sample and scaled to the real payload.
    #[test]
    fn the_frame_cap_admits_the_largest_archive_the_contract_accepts() {
        let sample = "\"".repeat(64 * 1024);
        let envelope = serde_json::to_string(&json!({
            "command": "importUserData",
            "params": { "archive": sample },
        }))
        .expect("the envelope serializes");
        let request = serde_json::to_string(&Request::invoke(
            RawValue::from_string(envelope.clone()).expect("the envelope is JSON"),
        ))
        .expect("the request serializes");

        // Everything the frame spends beyond the escaped archive itself, and how
        // much each archive byte costs once escaped.
        let fixed = request.len() - envelope.len();
        let per_byte = envelope.len() as f64 / sample.len() as f64;
        let worst_case = (MAX_ARCHIVE_BYTES as f64 * per_byte) as usize + fixed;

        assert!(
            worst_case <= FRAME_CAP,
            "a {MAX_ARCHIVE_BYTES} byte archive needs {worst_case} bytes, above the {FRAME_CAP} byte cap"
        );
    }

    /// The envelope crosses as the bytes the host built, not as a re-rendering of
    /// them: the field ordering, the spacing, and the escaping of what goes in is
    /// what comes out.
    #[test]
    fn a_request_carries_its_envelope_verbatim() {
        let envelope = r#"{"command":"enableMod","params":{"modId":"a","enabled":true}}"#;
        let request = Request::invoke(RawValue::from_string(envelope.to_owned()).expect("JSON"));
        let encoded = serde_json::to_string(&request).expect("the request serializes");

        assert!(encoded.contains(envelope), "{encoded}");

        let decoded: Request = serde_json::from_str(&encoded).expect("the request decodes");
        assert_eq!(
            decoded.envelope.expect("the envelope survives").get(),
            envelope
        );
    }

    /// A core failure crosses as the typed wire error, so its code, its per-code
    /// details, and the origin the core raised it at all survive the crossing.
    #[test]
    fn a_wire_failure_crosses_with_its_code_details_and_origin() {
        let wire = WireError::with_details(
            windhawk_core_protocol::ErrorCode::CompilerFailed,
            "clang++ exited with code 1",
            json!({ "exitCode": 1 }),
        );
        let fault = Fault::of(&HostError::wire(wire.clone()));
        let encoded = serde_json::to_string(&fault).expect("the fault serializes");
        let decoded: Fault = serde_json::from_str(&encoded).expect("the fault decodes");

        match decoded.into_host().kind() {
            HostErrorKind::Wire(found) => assert_eq!(**found, wire),
            other => panic!("expected a wire failure, got {other:?}"),
        }
    }

    /// A failure the broker RAISED crosses as its own kind, and reaches the
    /// front-end as `INTERNAL` carrying what actually went wrong.
    ///
    /// Asserted all the way through the reply shaping rather than on the host kind
    /// alone, because the two ends are the whole point: a broker fault spelled as
    /// a transport failure is indistinguishable from a dead channel by the time it
    /// gets here, and the user is told the elevated helper is gone and invited to
    /// retry a connection that just answered them.
    #[test]
    fn a_broker_side_failure_does_not_read_as_a_lost_channel() {
        let message = "the reply is 9 bytes, above the 8 byte channel limit";
        let fault = Fault::broker(message.to_owned());
        let encoded = serde_json::to_string(&fault).expect("the fault serializes");
        let decoded: Fault = serde_json::from_str(&encoded).expect("the fault decodes");

        let error = decoded.into_host();
        assert!(
            !matches!(error.kind(), HostErrorKind::Transport(_)),
            "the broker answered; the channel is not what failed"
        );

        let object = crate::ipc::reply::error_object(&error);
        assert_eq!(object["code"], "INTERNAL");
        assert_eq!(object["message"], message);
    }

    /// A response frame that answers no id cannot be routed to a parked request,
    /// and is reported rather than dropped silently.
    #[test]
    fn an_unattributable_response_is_reported_as_malformed() {
        let frame = BrokerFrame {
            id: None,
            ..BrokerFrame::done(7)
        };
        match Channel::route(frame) {
            Routed::Push(Push::Malformed(_)) => {}
            _ => panic!("a response with no id has no request to answer"),
        }
    }
}
