//! The seams the two ends are generic over: the requester's frame vocabulary,
//! the responder's handler, and the sink unsolicited pushes are handed to.
//!
//! Everything specific to what the channel carries lives in the caller's
//! implementations of these traits, including the shape of the frames on the
//! wire. The transport contributes exactly one thing to a request - the
//! correlation id it assigned - and reads exactly one thing back out of an
//! arriving frame: which parked request, if any, that frame answers.
//!
//! The frame types are serialized and deserialized whole, from and into the
//! caller's own types, never through an intermediate `Value` and never through
//! a flattened wrapper. That is what lets a caller carry an already-serialized
//! payload across verbatim - the bytes one side wrote are the bytes the other
//! side reads - which a buffering wrapper would quietly turn into a re-rendering
//! of the same value.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// What an arriving frame turned out to be.
pub enum Routed<Response, Push> {
    /// The answer to the request that was assigned this id.
    Response(u64, Response),
    /// An unsolicited frame, which parks under no id.
    Push(Push),
}

/// The frame vocabulary the requesting end speaks.
pub trait RequestFrames: Send + Sync + 'static {
    /// What this end sends. The transport stamps the correlation id into it and
    /// serializes it; where that id sits on the wire is the caller's choice.
    type Request: Serialize + Send + 'static;
    /// What a parked request resolves to.
    type Response: Send + 'static;
    /// What arrives unsolicited.
    type Push: Send + 'static;
    /// The one type the reader deserializes into: a response and a push arrive
    /// on the same stream, so they are told apart after parsing, not by trying
    /// two parses.
    type Incoming: DeserializeOwned + Send + 'static;

    /// Record the correlation id the transport assigned to this request.
    fn stamp(request: &mut Self::Request, id: u64);

    /// Classify an arriving frame.
    fn route(incoming: Self::Incoming) -> Routed<Self::Response, Self::Push>;
}

/// Where a request is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// On the worker pool. Everything that can take real time.
    Pooled,
    /// Inline on the reader thread, ahead of anything queued. For requests whose
    /// whole value is being prompt and which cannot block: one that queued behind
    /// the operations it is meant to interrupt would be useless. The handler must
    /// keep these non-blocking, or the reader stops reading, which is the one
    /// state this channel cannot recover from.
    Immediate,
    /// Inline on the reader thread, and the last request served: the response
    /// goes out and the channel closes behind it.
    Final,
}

/// The responding end: how a request is answered, and where.
pub trait BrokerHandler: Send + Sync + 'static {
    /// What this end receives.
    type Request: DeserializeOwned + Send + 'static;
    /// What it answers with.
    type Response: Serialize + Send + 'static;
    /// What it emits unsolicited, through a [`Pusher`](crate::Pusher).
    type Push: Serialize + Send + 'static;

    /// The correlation id carried by this request, needed only to attribute a
    /// response this end could not write.
    fn request_id(&self, request: &Self::Request) -> u64;

    /// Where to serve this request.
    fn disposition(&self, request: &Self::Request) -> Disposition;

    /// Serve it.
    fn handle(&self, request: Self::Request) -> Self::Response;

    /// Build the response that reports a reply too large to put on the wire, so
    /// the requester gets a legible failure for that one request instead of a
    /// dead channel.
    fn oversized(&self, id: u64, bytes: usize, cap: usize) -> Self::Response;

    /// A push too large to put on the wire was dropped. A push answers no
    /// request, so there is nothing to fail; the default is to say nothing.
    fn push_dropped(&self, bytes: usize, cap: usize) {
        let _ = (bytes, cap);
    }
}

/// Where the requesting end delivers unsolicited frames.
///
/// Implementations must HAND OFF and return - a queue send, a lock held for the
/// length of an insert - and never do the work inline. The sink runs on the
/// reader thread, and a reader that stops reading is what turns a peer's full
/// pipe buffer into a deadlock in which neither side is locally at fault.
pub trait PushSink<Push>: Send + Sync + 'static {
    fn push(&self, push: Push);

    /// The channel ended: no further push will arrive and every request in
    /// flight has been failed. Called exactly once.
    ///
    /// The same hand-off rule applies, and this is the harder place to keep it:
    /// whatever a caller wants to do about a lost channel - unwind the work that
    /// was in flight, put a notice in front of the user, fall back to some other
    /// way of working - is real work with its own thread affinity, and none of it
    /// belongs on the reader thread. SIGNAL it and return.
    fn channel_lost(&self) {}
}
