//! The transport error type. It carries enough to render a message; the
//! consumer maps it to its own error model (`napi::Error` in the bridge, the
//! CLI error enum in the CLI). It holds no Windhawk semantics.
//!
//! Each error also carries the source location it was raised at, captured by the
//! `#[track_caller]` constructors. The location is kept ALONGSIDE the message
//! (never folded into `Display`), so a consumer can surface the origin in a
//! diagnostic context while the message text stays clean.

use std::fmt;
use std::panic::Location;

/// The transport failure semantics, separate from the captured location.
#[derive(Debug)]
pub enum ClientErrorKind {
    /// The DLL could not be loaded, or a required export was missing.
    Load(String),
    /// The DLL's ABI integer does not match the one this client was built
    /// against - the one hard gate core-client enforces.
    AbiMismatch { dll: i32, expected: i32 },
    /// An input string contained an interior NUL byte (the named argument).
    NulByte(&'static str),
    /// A DLL call that must return a string returned null (the named export).
    NullResult(&'static str),
    /// A DLL error envelope (raw JSON) from a failed session-create or async
    /// start; forwarded to the consumer verbatim.
    Envelope(String),
    /// The session has already been destroyed.
    Destroyed,
}

/// A transport failure plus the source location it was raised at. Build through
/// the `#[track_caller]` constructors so the location is the call site.
#[derive(Debug)]
pub struct ClientError {
    kind: ClientErrorKind,
    location: &'static Location<'static>,
}

impl ClientError {
    /// Pair a kind with an already-captured location. Plain (NOT
    /// `#[track_caller]`) so each public constructor captures `Location::caller()`
    /// in its own body - the site that called it - and passes it through here.
    fn with_location(kind: ClientErrorKind, location: &'static Location<'static>) -> ClientError {
        ClientError { kind, location }
    }

    #[track_caller]
    pub fn load(message: String) -> ClientError {
        Self::with_location(ClientErrorKind::Load(message), Location::caller())
    }

    #[track_caller]
    pub fn abi_mismatch(dll: i32, expected: i32) -> ClientError {
        Self::with_location(
            ClientErrorKind::AbiMismatch { dll, expected },
            Location::caller(),
        )
    }

    #[track_caller]
    pub fn nul_byte(what: &'static str) -> ClientError {
        Self::with_location(ClientErrorKind::NulByte(what), Location::caller())
    }

    #[track_caller]
    pub fn null_result(what: &'static str) -> ClientError {
        Self::with_location(ClientErrorKind::NullResult(what), Location::caller())
    }

    #[track_caller]
    pub fn envelope(raw: String) -> ClientError {
        Self::with_location(ClientErrorKind::Envelope(raw), Location::caller())
    }

    #[track_caller]
    pub fn destroyed() -> ClientError {
        Self::with_location(ClientErrorKind::Destroyed, Location::caller())
    }

    /// The failure semantics, for a consumer that classifies the error (the
    /// host's flat-sum mapping).
    pub fn kind(&self) -> &ClientErrorKind {
        &self.kind
    }

    /// The source location the error was raised at (DIAGNOSTIC).
    pub fn location(&self) -> &'static Location<'static> {
        self.location
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ClientErrorKind::Load(message) => write!(f, "{message}"),
            ClientErrorKind::AbiMismatch { dll, expected } => write!(
                f,
                "windhawk-core ABI version mismatch: DLL has {dll}, client expects {expected}"
            ),
            ClientErrorKind::NulByte(what) => write!(f, "{what} must not contain NUL bytes"),
            ClientErrorKind::NullResult(what) => write!(f, "{what} returned null"),
            ClientErrorKind::Envelope(envelope) => write!(f, "{envelope}"),
            ClientErrorKind::Destroyed => write!(f, "session has been destroyed"),
        }
    }
}

impl std::error::Error for ClientError {}
