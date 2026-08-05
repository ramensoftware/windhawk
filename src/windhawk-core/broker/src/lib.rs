//! The runtime-broker transport: a length-prefixed JSON channel over a duplex
//! named pipe, carrying requests one way and responses plus unsolicited pushes
//! the other.
//!
//! The two role pairs are deliberately opposite and are named separately
//! throughout, because that inversion is the security argument: the
//! **requester** is the pipe **server** (it creates and listens) and the
//! **responder** is the pipe **client** (it only ever connects out). There is
//! therefore no listening endpoint in the elevated process for an arbitrary
//! caller to reach.
//!
//! Nothing here knows a Windhawk command. The channel is generic over the
//! request, response, and push types ([`RequestFrames`], [`BrokerHandler`]),
//! and the three values a Windhawk channel has to agree on - the protocol
//! integer, the product version, and the frame size cap - are supplied by the
//! caller in a [`ChannelConfig`] rather than defined here. A constant in this
//! crate would compare the TRANSPORT's version, which is identical on both
//! sides by construction, and a frame cap here could not see the contract
//! constant it has to admit. Both would look right in review.
//!
//! Everything the crate does with a deadline - the connect, the handshake, and
//! a clean shutdown - needs an I/O operation that can be waited on with a
//! timeout and then abandoned, which blocking-mode pipe calls cannot provide.
//! So the pipe is created with `FILE_FLAG_OVERLAPPED` and every read, write,
//! and connect goes through an `OVERLAPPED` with a per-handle event, waited on
//! together with a shutdown event.

// The pipe and its security descriptor are raw Win32, so this crate cannot
// `forbid(unsafe_code)`. It follows the convention the `windows/` adapter and
// the `ui` crate took: deny unsafe operations outside an `unsafe` block, keep
// unsafe confined to `pipe.rs` and `security.rs`, and carry a `// SAFETY:` note
// on every block. The rest of the crate is safe.
#![deny(unsafe_op_in_unsafe_fn)]

mod frame;
mod handler;
mod pipe;
mod requester;
mod responder;
mod security;
mod version;

pub use frame::{FRAME_HEADER_BYTES, FrameError, decode, encode, read_frame, write_frame};
pub use handler::{BrokerHandler, Disposition, PushSink, RequestFrames, Routed};
pub use pipe::{
    Event, PipeReader, PipeStream, channel_name, connect_flags, listener_open_mode,
    listener_pipe_mode,
};
pub use requester::{
    AcceptError, AcceptTerms, ChannelError, Handshaken, Listener, Rejection, Requester,
};
pub use responder::{ConnectError, Connection, PushQueue, Pusher, Responder, connect, push_queue};
pub use security::{
    Accepted, ClientPeer, Integrity, PeerPolicy, PipeSecurity, RejectReason, SelfIdentity,
    ServerPeer, identify_client, identify_server,
};
pub use version::{ChannelConfig, Handshake};
