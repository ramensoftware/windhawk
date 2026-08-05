//! The handshake frames and the values the two ends compare before either acts
//! on anything.
//!
//! The responder sends `hello` the moment it has verified the peer it connected
//! to; the requester replies `helloAck` only after the peer has passed its own
//! verification. Neither the protocol integer nor the product version is defined
//! here: both are handed in by the caller. A version read from this crate's own
//! `CARGO_PKG_VERSION` would be the TRANSPORT's version, identical on both sides
//! by construction, so the check would pass on precisely the mismatched pair it
//! exists to catch.

use serde::{Deserialize, Serialize};

/// The frames the transport itself exchanges. Everything after `helloAck`
/// belongs to the caller's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Handshake {
    /// Sent by the responder, first frame on the channel. It is a claim that
    /// this side can already serve: the caller does whatever it needs to become
    /// able to answer requests BEFORE it connects, so a peer that cannot serve
    /// never becomes a channel at all.
    Hello {
        protocol: u32,
        version: String,
        pid: u32,
    },
    /// Sent by the requester once `hello` matches and the peer has passed the
    /// peer policy. No request and no push may cross before it.
    HelloAck { protocol: u32 },
}

/// What the two ends must agree on, supplied by whoever builds the channel.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// An integer that must match exactly. Bumped by the caller on any wire
    /// change; there is no negotiation and no compatibility range.
    pub protocol: u32,
    /// The caller's product version. It can differ across a channel even though
    /// both processes run the same path on disk, because the file can be
    /// replaced while a process that started from the old one keeps running.
    pub version: String,
    /// The largest payload a frame may carry, derived by the caller from the
    /// largest payload its own contract accepts.
    pub frame_cap: usize,
}
