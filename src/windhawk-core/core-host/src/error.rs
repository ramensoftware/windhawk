//! [`HostError`]: the host's failure type - a flat [`HostErrorKind`] sum both
//! consumers destructure individually plus the source location it was raised at.
//! `core-client` returns raw envelope strings and a transport [`ClientError`];
//! the `{ok,result}` / `{ok:false,error}` split and the `WireError` mapping are
//! consumer policy and live here, so neither the CLI nor the UI re-implements
//! them.
//!
//! The location is carried ALONGSIDE the message (never folded into `Display`):
//! a [`HostErrorKind::Wire`] adopts the core's origin from the `WireError`'s own
//! `location`; the no-wire arms capture the host/consumer site via the
//! `#[track_caller]` constructors. A consumer renders it only in a diagnostic
//! context (the UI fatal box, the CLI human stderr), so the message contract
//! stays clean.

use std::fmt;
use std::panic::Location;

use serde_json::Value;
use windhawk_core_client::{ClientError, ClientErrorKind};
use windhawk_core_protocol::{SourceLocation, WireError};

/// A host-layer failure's semantics. A FLAT sum both consumers destructure
/// individually: [`HostErrorKind::Wire`] for a structured `{ok:false,error}` /
/// `failed`-event failure, and four no-wire arms each carrying their message.
/// The no-wire arms are NOT nested under a single `Host(..)` arm: the UI splits
/// a FATAL startup failure (`Load`/`Gate`) from a recoverable per-`reply`
/// `Decode`, so the nesting would only add an unwrap. The CLI collapses the
/// four no-wire arms to its `GENERIC` exit class with one `From` arm, so the
/// typing costs it nothing.
///
/// The host owns the WORDING of every no-wire arm (phrased consumer-neutrally),
/// so the diagnostic text is defined once and cannot accidentally drift between
/// consumers - the duplication this extraction exists to remove.
#[derive(Debug)]
pub enum HostErrorKind {
    /// A structured wire error from a `{ok:false,error}` envelope or a `failed`
    /// event. Carries the typed [`WireError`] (`code`, `message`, `details`) so a
    /// consumer can canonicalize the code or read its per-code `details`. Boxed:
    /// it is the by-far-largest variant, and boxing it keeps `HostError` small
    /// enough that `Result<_, HostError>` does not bloat every host call's return
    /// (clippy `result_large_err`).
    Wire(Box<WireError>),
    /// The DLL could not be loaded, or a required export was missing.
    Load(String),
    /// The ABI integer or the `contractVersion` gate rejected the DLL.
    Gate(String),
    /// A transport failure with no wire error: a destroyed session, an interior
    /// NUL byte, or a null result from an export.
    Transport(String),
    /// A typed-result decode failure, or the not-JSON / missing-`error` envelope
    /// fallback - a response that did not parse into the expected shape.
    Decode(String),
}

/// A host failure: its [`HostErrorKind`] plus the source location it was raised
/// at. Build through the constructors so the location is captured (the core's
/// origin for a `Wire`, the `#[track_caller]` call site for the no-wire arms).
#[derive(Debug)]
pub struct HostError {
    kind: HostErrorKind,
    location: Option<SourceLocation>,
}

impl HostError {
    fn with_location(kind: HostErrorKind, location: Option<SourceLocation>) -> HostError {
        HostError { kind, location }
    }

    /// A structured wire error. Its origin is the core's (carried in the
    /// `WireError`'s `location` field), so adopt it rather than capturing a host
    /// site - the consumer wants the line the core raised it at.
    pub fn wire(error: WireError) -> HostError {
        let location = error.location.clone();
        HostError {
            kind: HostErrorKind::Wire(Box::new(error)),
            location,
        }
    }

    #[track_caller]
    pub fn load(message: String) -> HostError {
        Self::with_location(
            HostErrorKind::Load(message),
            Some(SourceLocation::from(Location::caller())),
        )
    }

    #[track_caller]
    pub fn gate(message: String) -> HostError {
        Self::with_location(
            HostErrorKind::Gate(message),
            Some(SourceLocation::from(Location::caller())),
        )
    }

    #[track_caller]
    pub fn transport(message: String) -> HostError {
        Self::with_location(
            HostErrorKind::Transport(message),
            Some(SourceLocation::from(Location::caller())),
        )
    }

    #[track_caller]
    pub fn decode(message: String) -> HostError {
        Self::with_location(
            HostErrorKind::Decode(message),
            Some(SourceLocation::from(Location::caller())),
        )
    }

    /// The failure semantics, for a consumer that destructures the host's flat
    /// sum (the CLI's exit-class mapping, the UI's wire-code/message readout).
    pub fn kind(&self) -> &HostErrorKind {
        &self.kind
    }

    /// The source location the error was raised at (DIAGNOSTIC), or `None` for a
    /// wire error the core sent without one.
    pub fn location(&self) -> Option<&SourceLocation> {
        self.location.as_ref()
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            HostErrorKind::Wire(error) => write!(f, "{}", error.message),
            HostErrorKind::Load(message)
            | HostErrorKind::Gate(message)
            | HostErrorKind::Transport(message)
            | HostErrorKind::Decode(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostError {}

/// A typed-result decode failure (`serde_json::from_value`/`from_str` into a wire
/// DTO) is a `Decode`, with the wording the CLI used verbatim so the parity
/// self-diff stays empty. Reached through `?`, so the captured location is this
/// conversion (the host decode path), not the `?` site `?` cannot forward.
impl From<serde_json::Error> for HostError {
    fn from(error: serde_json::Error) -> HostError {
        HostError::decode(format!("decoding command result: {error}"))
    }
}

/// Map a transport-level [`ClientError`] to the host's flat sum, preserving the
/// `ClientError`'s captured origin (the DLL-load site in `core-client`'s
/// `api.rs`, the ABI gate in `loader.rs`, ...). A `ClientError::Envelope` carries
/// a raw error envelope (a failed session-create or async start); decode it so a
/// structured `WireError` becomes a `Wire` (adopting the core's origin) and an
/// undecodable body the fallback `Decode` (tagged with the envelope's origin).
/// The other transport failures classify by kind: load/missing-export -> `Load`,
/// the ABI mismatch -> `Gate`, a NUL byte / null result / destroyed session ->
/// `Transport`.
impl From<ClientError> for HostError {
    fn from(error: ClientError) -> HostError {
        let location = SourceLocation::from(error.location());
        match error.kind() {
            ClientErrorKind::Envelope(raw) => host_error_from_envelope(raw, location),
            ClientErrorKind::Load(message) => {
                HostError::with_location(HostErrorKind::Load(message.clone()), Some(location))
            }
            ClientErrorKind::AbiMismatch { .. } => {
                HostError::with_location(HostErrorKind::Gate(error.to_string()), Some(location))
            }
            ClientErrorKind::NulByte(_)
            | ClientErrorKind::NullResult(_)
            | ClientErrorKind::Destroyed => HostError::with_location(
                HostErrorKind::Transport(error.to_string()),
                Some(location),
            ),
        }
    }
}

/// Parse a raw response envelope: return the `result` value on success, map the
/// `error` object to a [`HostError`] on failure. The failure branch reuses
/// [`wire_error_or_host`] - the single owner of the failure-envelope fallback -
/// with the `error` value it already extracted, so no second parse and no
/// duplicated fallback text. `#[track_caller]` captures the invoke site as the
/// origin of a not-JSON / undecodable response (the wire branch keeps the core's
/// own origin instead).
#[track_caller]
pub(crate) fn parse_response(raw: &str) -> Result<Value, HostError> {
    let here = SourceLocation::from(Location::caller());
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        HostError::with_location(
            HostErrorKind::Decode(format!("invalid response envelope: {e}")),
            Some(here.clone()),
        )
    })?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        _ => {
            let error = value.get("error").cloned().unwrap_or(Value::Null);
            Err(wire_error_or_host(error, raw, here))
        }
    }
}

/// Decode a raw failure envelope (`{"ok":false,"error":{...}}`) into a
/// [`HostError`]. Extracts the `error` object and delegates to
/// [`wire_error_or_host`]; a non-JSON body or a missing `error` funnels through
/// the same fallback via an absent (`Null`) error value, so the not-JSON case
/// reuses the one fallback text rather than spelling a second copy. `fallback` is
/// the origin to tag the `Decode` fallback with (the envelope's `ClientError`
/// site); the wire branch keeps the core's own origin instead.
fn host_error_from_envelope(raw: &str, fallback: SourceLocation) -> HostError {
    let error = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.get("error").cloned())
        .unwrap_or(Value::Null);
    wire_error_or_host(error, raw, fallback)
}

/// Decode an ALREADY-extracted `error` object: a typed [`WireError`] becomes a
/// `Wire` (adopting the core's origin from its `location`); anything that will
/// not decode - an undecodable error object, or `Null` for a missing/non-JSON
/// envelope - falls back to the single `Decode` text tagged with `fallback`. The
/// ONE owner of the failure-envelope fallback, shared by both decode entry points
/// ([`parse_response`] and [`host_error_from_envelope`]).
fn wire_error_or_host(error: Value, raw: &str, fallback: SourceLocation) -> HostError {
    match serde_json::from_value::<WireError>(error) {
        Ok(wire) => HostError::wire(wire),
        Err(_) => HostError::with_location(
            HostErrorKind::Decode(format!("windhawk-core error: {raw}")),
            Some(fallback),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use windhawk_core_protocol::ErrorCode;

    #[test]
    fn parse_response_returns_success_result() {
        let raw = r#"{"ok":true,"result":{"value":42}}"#;
        let result = parse_response(raw).unwrap();
        assert_eq!(result, json!({ "value": 42 }));
    }

    #[test]
    fn parse_response_maps_error_envelope_to_wire() {
        let raw = r#"{"ok":false,"error":{"code":"MOD_NOT_INSTALLED","message":"no"}}"#;
        let err = parse_response(raw).unwrap_err();
        let HostErrorKind::Wire(wire) = err.kind() else {
            panic!("expected Wire");
        };
        assert_eq!(wire.code, ErrorCode::ModNotInstalled);
        assert_eq!(wire.message, "no");
    }

    #[test]
    fn wire_error_adopts_the_cores_origin_location() {
        // A wire error carrying a location surfaces it as the host error's origin.
        let raw = r#"{"ok":false,"error":{"code":"INTERNAL","message":"boom","location":{"file":"core/src/x.rs","line":9}}}"#;
        let err = parse_response(raw).unwrap_err();
        let location = err.location().expect("wire origin adopted");
        assert_eq!(location.file, "core/src/x.rs");
        assert_eq!(location.line, 9);
    }

    #[test]
    fn from_client_envelope_decodes_the_wire_error() {
        let raw = r#"{"ok":false,"error":{"code":"APP_ROOT_INVALID","message":"no ini"}}"#;
        let err = HostError::from(ClientError::envelope(raw.to_owned()));
        let HostErrorKind::Wire(wire) = err.kind() else {
            panic!("expected Wire");
        };
        assert_eq!(wire.code, ErrorCode::AppRootInvalid);
        assert_eq!(wire.message, "no ini");
    }

    #[test]
    fn envelope_decode_falls_back_to_one_text() {
        // An undecodable error object and a non-JSON body both yield the single
        // `windhawk-core error: <raw>` fallback as a `Decode`.
        let undecodable = r#"{"ok":false,"error":{"nope":true}}"#;
        let err = HostError::from(ClientError::envelope(undecodable.to_owned()));
        assert!(
            matches!(err.kind(), HostErrorKind::Decode(m) if *m == format!("windhawk-core error: {undecodable}"))
        );

        let not_json = "this is not json";
        let err = HostError::from(ClientError::envelope(not_json.to_owned()));
        assert!(
            matches!(err.kind(), HostErrorKind::Decode(m) if *m == format!("windhawk-core error: {not_json}"))
        );
    }

    #[test]
    fn transport_failures_classify_by_kind_and_keep_the_client_origin() {
        let gate = HostError::from(ClientError::abi_mismatch(1, 2));
        assert!(matches!(gate.kind(), HostErrorKind::Gate(_)));
        // The ABI gate's origin is core-client's loader.rs, carried through.
        assert!(gate.location().is_some());

        assert!(matches!(
            HostError::from(ClientError::destroyed()).kind(),
            HostErrorKind::Transport(_)
        ));
        assert!(matches!(
            HostError::from(ClientError::load("boom".to_owned())).kind(),
            HostErrorKind::Load(m) if m == "boom"
        ));
    }
}
