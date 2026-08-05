//! The requesting end: the pipe LISTENER, the request multiplexer, and the sink
//! unsolicited frames are handed to.
//!
//! This end creates the pipe and waits to be connected to, which is the whole
//! security argument of the split: the privileged end never listens, so there is
//! no privileged endpoint for an arbitrary caller to reach, and the peer that
//! does connect is verified before this end acts on anything it said.
//!
//! Several callers issue requests at once, so each is assigned a monotonic id and
//! parks a slot; one reader thread routes each response to its slot and each push
//! to the sink. **That reader thread must never block on anything but its next
//! read.** Routing is a lock held for the length of a map insert and the sink is
//! required to hand off, because a reader that stops reading lets the peer's
//! writes fill the pipe buffer, at which point the peer stops reading too and a
//! large request in the other direction blocks against someone who will never
//! drain it - with every thread involved doing exactly what its own contract
//! says.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::frame::{self, FrameError};
use crate::handler::{PushSink, RequestFrames, Routed};
use crate::pipe::{Event, PipeStream};
use crate::security::{PeerPolicy, RejectReason, SelfIdentity};
use crate::version::{ChannelConfig, Handshake};

/// Why a request could not be answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// The channel is gone: the peer exited, the stream failed, or a frame
    /// arrived that this end refuses to keep reading past. Every request in
    /// flight fails this way, and no further one will be sent.
    Closed,
    /// The request was larger than the channel's cap, so it was never put on the
    /// wire and only this one request failed.
    FrameTooLarge { bytes: usize, cap: usize },
    /// The request could not be serialized at all.
    Encode(String),
    /// The request carried a deadline and outlived it. Only the requests whose
    /// whole value is being prompt carry one.
    Timeout,
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelError::Closed => write!(f, "the channel is closed"),
            ChannelError::FrameTooLarge { bytes, cap } => write!(
                f,
                "the request is {bytes} bytes, above the {cap} byte channel limit"
            ),
            ChannelError::Encode(error) => write!(f, "the request could not be encoded: {error}"),
            ChannelError::Timeout => write!(f, "the request deadline expired"),
        }
    }
}

impl std::error::Error for ChannelError {}

/// Why a peer that connected was turned away.
///
/// Reported as it happens rather than only at the end, because a peer that
/// connected and failed is a definitive answer about whoever was asked to
/// connect: waiting the remaining deadline out before saying so would add dead
/// time to every launch that has to try something else.
#[derive(Debug, Clone)]
pub enum Rejection {
    /// The peer failed the peer policy.
    Policy(RejectReason),
    /// The peer speaks a different wire version.
    Protocol { found: u32, expected: u32 },
    /// The peer is a different build of the product.
    Version { found: String, expected: String },
    /// The peer never sent a usable handshake.
    Handshake(String),
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejection::Policy(reason) => write!(f, "{reason}"),
            Rejection::Protocol { found, expected } => {
                write!(f, "peer speaks protocol {found}, expected {expected}")
            }
            Rejection::Version { found, expected } => {
                write!(f, "peer is version {found}, expected {expected}")
            }
            Rejection::Handshake(detail) => write!(f, "peer handshake failed: {detail}"),
        }
    }
}

/// Why no channel was established at all.
#[derive(Debug)]
pub enum AcceptError {
    /// The deadline passed with no peer that got through.
    Timeout,
    /// The listener itself failed.
    Io(io::Error),
}

impl std::fmt::Display for AcceptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcceptError::Timeout => write!(f, "no peer connected before the deadline"),
            AcceptError::Io(error) => write!(f, "the listener failed: {error}"),
        }
    }
}

impl std::error::Error for AcceptError {}

/// What a peer connecting RIGHT NOW has to satisfy, and how long the listener is
/// still willing to wait for one.
///
/// Read afresh for every connection attempt rather than fixed when the accept
/// starts, because a caller that works through several ways of starting a peer
/// learns things as it goes and cannot re-enter the accept to apply them: the
/// process id it managed to obtain differs per attempt (and is unavailable on
/// some of them), and the time it is willing to wait jumps from "this should be
/// under a second" to "this is bounded by how long a human takes to answer a
/// dialog" the moment one of those attempts puts a prompt on screen.
#[derive(Debug, Clone)]
pub struct AcceptTerms {
    /// The policy to apply to whoever connects next.
    pub policy: PeerPolicy,
    /// How long the listener will go on waiting for a peer to connect. Honoured
    /// while a wait is already in progress, so extending it releases nothing and
    /// leaves no gap for a peer to arrive into unseen.
    pub connect_deadline: Instant,
    /// How long a peer that HAS connected gets to complete the handshake,
    /// measured from the moment it connects.
    ///
    /// Separate from the connect deadline so a peer arriving just before that
    /// deadline does not inherit a handshake window of nothing and get rejected
    /// for being late rather than for being wrong - which would burn the very
    /// peer the caller was waiting for.
    pub handshake: Duration,
}

/// The single listening instance of a channel, created before its name is handed
/// to anyone.
pub struct Listener {
    pipe: PipeStream,
    name: String,
    config: ChannelConfig,
    me: SelfIdentity,
}

/// A peer that connected, was verified, and was acked.
pub struct Handshaken {
    pipe: PipeStream,
    config: ChannelConfig,
    /// The peer's process id, as it reported it.
    pub peer_pid: u32,
    /// The product version the peer reported. Equal to this end's by the time it
    /// is here, and carried so the caller can log it.
    pub peer_version: String,
    /// The peer's token could not be read, so its integrity was taken on trust.
    /// This end degrades rather than rejects on that, since its own check is
    /// anti-spoofing rather than the privilege boundary.
    pub integrity_unverified: bool,
}

impl Listener {
    /// Create the listener on a fresh, single-use name under `prefix`.
    pub fn create(prefix: &str, config: ChannelConfig) -> io::Result<Listener> {
        Listener::with_name(&crate::pipe::channel_name(prefix)?, config)
    }

    /// Create the listener on an exact name. Creation fails if the name already
    /// exists, so a squatter is detected rather than served.
    pub fn with_name(name: &str, config: ChannelConfig) -> io::Result<Listener> {
        let security = crate::security::PipeSecurity::for_current_user()?;
        Ok(Listener {
            pipe: PipeStream::create_listener(name, &security)?,
            name: name.to_owned(),
            config,
            me: SelfIdentity::resolve()?,
        })
    }

    /// The name to hand to whoever is being asked to connect.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The event that abandons an accept in progress.
    ///
    /// It belongs to the pipe, not to the accept, so the channel that comes out
    /// of a successful accept inherits it. Signalling it after a peer has already
    /// been accepted therefore kills that channel rather than cancelling the wait
    /// it was aimed at.
    pub fn shutdown_signal(&self) -> Arc<Event> {
        self.pipe.shutdown_signal()
    }

    /// Wait for a peer that passes the current [`AcceptTerms`] and the handshake.
    ///
    /// `terms` is read afresh for each connection attempt and while a wait is in
    /// progress, so a caller working through several ways of starting a peer can
    /// bind each attempt to the process it started and extend the deadline when
    /// one of them puts a prompt on screen.
    ///
    /// A peer that fails is disconnected and the listener keeps waiting, so one
    /// rogue connect cannot consume the single instance and deny the real peer
    /// its channel - but `on_reject` is called the moment it happens, so a caller
    /// running an escalating sequence of ways to start a peer learns that this
    /// one is spent without waiting the deadline out.
    pub fn accept(
        self,
        terms: &dyn Fn() -> AcceptTerms,
        on_reject: &dyn Fn(Rejection),
    ) -> Result<Handshaken, AcceptError> {
        let Listener {
            pipe, config, me, ..
        } = self;
        loop {
            if Instant::now() >= terms().connect_deadline {
                return Err(AcceptError::Timeout);
            }
            match pipe.accept_until(&|| terms().connect_deadline) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    return Err(AcceptError::Timeout);
                }
                Err(error) => return Err(AcceptError::Io(error)),
            }

            // Read once, here: the peer is connected, so these are the terms it
            // is judged by, and a deadline that moves underneath a handshake in
            // progress would only make the outcome harder to reason about.
            let current = terms();
            let handshake_deadline = Instant::now() + current.handshake;
            match handshake(&pipe, &config, &me, &current.policy, handshake_deadline) {
                Ok(verified) => {
                    // The pipe moves out on the one path that succeeds; every
                    // rejection above leaves it here, because the listener goes on
                    // listening on the same instance.
                    return Ok(Handshaken {
                        pipe,
                        config,
                        peer_pid: verified.peer_pid,
                        peer_version: verified.peer_version,
                        integrity_unverified: verified.integrity_unverified,
                    });
                }
                Err(rejection) => {
                    on_reject(rejection);
                    pipe.disconnect();
                }
            }
        }
    }
}

struct Verified {
    peer_pid: u32,
    peer_version: String,
    integrity_unverified: bool,
}

/// Read the peer's `hello`, verify the peer, and ack.
///
/// The order is the point. A pipe server cannot obtain its client's token until
/// the client has written, so verifying the instant the connection completes
/// would be a check that works on some machines and not others - the worst
/// possible shape for a security check. Reading `hello` first proves the peer's
/// data has arrived; the ack is withheld until the policy passes, so this end
/// still acts on nothing from an unverified peer, and what it is exposed to
/// before verifying is one length-capped JSON frame from a peer that already
/// passed the pipe's descriptor.
fn handshake(
    pipe: &PipeStream,
    config: &ChannelConfig,
    me: &SelfIdentity,
    policy: &PeerPolicy,
    deadline: Instant,
) -> Result<Verified, Rejection> {
    let cap = config.frame_cap;
    let hello: Handshake =
        frame::read_frame(&mut pipe.reader(Some(deadline)), cap).map_err(|error| {
            Rejection::Handshake(match error {
                FrameError::Eof => "the peer connected and left without a hello".to_owned(),
                other => other.to_string(),
            })
        })?;
    let (protocol, version, pid) = match hello {
        Handshake::Hello {
            protocol,
            version,
            pid,
        } => (protocol, version, pid),
        Handshake::HelloAck { .. } => {
            return Err(Rejection::Handshake(
                "the peer opened with an ack instead of a hello".to_owned(),
            ));
        }
    };

    let peer = crate::security::identify_client(pipe)
        .map_err(|error| Rejection::Handshake(format!("peer identification failed: {error}")))?;
    let accepted = policy
        .evaluate_client(&peer, pid, me)
        .map_err(Rejection::Policy)?;

    if protocol != config.protocol {
        return Err(Rejection::Protocol {
            found: protocol,
            expected: config.protocol,
        });
    }
    if version != config.version {
        return Err(Rejection::Version {
            found: version,
            expected: config.version.clone(),
        });
    }

    let ack = Handshake::HelloAck {
        protocol: config.protocol,
    };
    let bytes = frame::encode(&ack, cap)
        .map_err(|error| Rejection::Handshake(format!("the ack could not be built: {error}")))?;
    pipe.write_all(&bytes, Some(deadline))
        .map_err(|error| Rejection::Handshake(format!("the ack could not be sent: {error}")))?;

    Ok(Verified {
        peer_pid: peer.pid,
        peer_version: version,
        integrity_unverified: accepted.integrity_unverified,
    })
}

/// The multiplexing requester over an established channel.
pub struct Requester<F: RequestFrames> {
    inner: Arc<Inner<F>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

struct Inner<F: RequestFrames> {
    pipe: PipeStream,
    cap: usize,
    next_id: AtomicU64,
    table: Mutex<Slots<F::Response>>,
    settled: Condvar,
}

struct Slots<R> {
    parked: HashMap<u64, Option<Result<R, ChannelError>>>,
    closed: bool,
}

impl<F: RequestFrames> Requester<F> {
    /// Take over a verified channel and start routing.
    pub fn start(handshaken: Handshaken, sink: Arc<dyn PushSink<F::Push>>) -> Requester<F> {
        let inner = Arc::new(Inner {
            pipe: handshaken.pipe,
            cap: handshaken.config.frame_cap,
            next_id: AtomicU64::new(1),
            table: Mutex::new(Slots {
                parked: HashMap::new(),
                closed: false,
            }),
            settled: Condvar::new(),
        });

        let reading = Arc::clone(&inner);
        let reader = std::thread::Builder::new()
            .name("windhawk-broker-requester".to_owned())
            .spawn(move || read_loop::<F>(reading, sink))
            .expect("the requester reader thread must start");

        Requester {
            inner,
            reader: Mutex::new(Some(reader)),
        }
    }

    /// Send a request and wait for its answer for as long as it takes.
    ///
    /// Ordinary requests carry no deadline on purpose: a request that never comes
    /// back hangs exactly as the in-process call it replaced would, and a timeout
    /// here would report slow but healthy work as a transport failure.
    pub fn request(&self, request: F::Request) -> Result<F::Response, ChannelError> {
        self.dispatch(request, None)
    }

    /// Send a request that is only worth anything promptly, and give up on it
    /// after `within`.
    pub fn request_within(
        &self,
        request: F::Request,
        within: Duration,
    ) -> Result<F::Response, ChannelError> {
        self.dispatch(request, Some(within))
    }

    fn dispatch(
        &self,
        mut request: F::Request,
        within: Option<Duration>,
    ) -> Result<F::Response, ChannelError> {
        // One deadline for the whole call, not one per step: a request whose
        // value is being prompt is late whether it spent the time in the write or
        // in the wait, and two `within`s in series would let it take twice as long
        // as it asked for.
        let deadline = within.map(|within| Instant::now() + within);
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        F::stamp(&mut request, id);

        // Encoded before a slot is parked: an over-cap request never reaches the
        // wire, so it fails alone and the channel is untouched.
        let bytes = match frame::encode(&request, self.inner.cap) {
            Ok(bytes) => bytes,
            Err(FrameError::TooLarge { bytes, cap }) => {
                return Err(ChannelError::FrameTooLarge { bytes, cap });
            }
            Err(error) => return Err(ChannelError::Encode(error.to_string())),
        };

        {
            let mut table = self.inner.lock_table();
            if table.closed {
                return Err(ChannelError::Closed);
            }
            table.parked.insert(id, None);
        }

        // The deadline covers the write too. A peer that has stopped draining
        // fills the pipe buffer, and a write that parks against it would burn the
        // caller's thread before the deadline was ever consulted - which is the
        // one thing the deadline-bearing requests exist to avoid.
        if self.inner.pipe.write_all(&bytes, deadline).is_err() {
            self.inner.lock_table().parked.remove(&id);
            // A failed write is a dead channel, not a failed request: release the
            // reader so it settles everything else the same way. That covers the
            // write that ran out of deadline as well - it may have put part of a
            // frame on the wire, and a stream with half a frame in it is not one
            // to keep using.
            self.inner.pipe.signal_shutdown();
            return Err(ChannelError::Closed);
        }

        self.inner.wait_for(id, deadline)
    }

    /// Whether the channel is still usable.
    pub fn is_open(&self) -> bool {
        !self.inner.lock_table().closed
    }

    /// Close the channel and wait for the reader to settle every parked request.
    ///
    /// **Not callable from the reader thread**, which is to say not from a
    /// [`PushSink`] method - this joins that thread, so a sink that called it
    /// would wait for itself and hang for good. It is the sharp edge behind the
    /// hand-off rule those methods are documented under, and the one place the
    /// rule is a deadlock rather than a delay, so it is asserted here in debug
    /// builds as well as written down there.
    pub fn close(&self) {
        self.inner.pipe.signal_shutdown();
        let reader = self
            .reader
            .lock()
            .expect("the reader handle lock is poisoned")
            .take();
        if let Some(reader) = reader {
            debug_assert_ne!(
                std::thread::current().id(),
                reader.thread().id(),
                "close() joins the reader thread, so calling it from a PushSink \
                 would join that thread to itself"
            );
            let _ = reader.join();
        }
    }
}

impl<F: RequestFrames> Drop for Requester<F> {
    fn drop(&mut self) {
        self.close();
    }
}

impl<F: RequestFrames> Inner<F> {
    fn lock_table(&self) -> std::sync::MutexGuard<'_, Slots<F::Response>> {
        self.table
            .lock()
            .expect("the request slot lock is poisoned")
    }

    fn wait_for(&self, id: u64, deadline: Option<Instant>) -> Result<F::Response, ChannelError> {
        let mut table = self.lock_table();
        loop {
            match table.parked.get(&id) {
                None => return Err(ChannelError::Closed),
                Some(Some(_)) => {
                    return table
                        .parked
                        .remove(&id)
                        .expect("the slot was just observed")
                        .expect("the slot was just observed to be settled");
                }
                Some(None) => {}
            }
            match deadline {
                None => {
                    table = self
                        .settled
                        .wait(table)
                        .expect("the request slot lock is poisoned");
                }
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        table.parked.remove(&id);
                        return Err(ChannelError::Timeout);
                    }
                    let (guard, _) = self
                        .settled
                        .wait_timeout(table, remaining)
                        .expect("the request slot lock is poisoned");
                    table = guard;
                }
            }
        }
    }

    /// Hand a response to the request that is waiting for it. A response for an
    /// id nobody is waiting on is dropped: the requester gave up on it, or the
    /// peer answered something it was never asked.
    ///
    /// The FIRST answer to an id is the answer. A peer that answers the same id
    /// twice is answering something it was never asked the second time, and
    /// letting it land would rewrite an outcome the waiter has been told about but
    /// has not yet woken to collect.
    fn settle(&self, id: u64, outcome: Result<F::Response, ChannelError>) {
        let mut table = self.lock_table();
        if let Some(slot @ None) = table.parked.get_mut(&id) {
            *slot = Some(outcome);
            drop(table);
            self.settled.notify_all();
        }
    }

    /// Fail everything in flight, and refuse everything after.
    fn close_all(&self) {
        let mut table = self.lock_table();
        table.closed = true;
        for slot in table.parked.values_mut() {
            if slot.is_none() {
                *slot = Some(Err(ChannelError::Closed));
            }
        }
        drop(table);
        self.settled.notify_all();
    }
}

fn read_loop<F: RequestFrames>(inner: Arc<Inner<F>>, sink: Arc<dyn PushSink<F::Push>>) {
    loop {
        // Every failure here ends the channel, including an over-cap frame: the
        // request id is inside the payload that would have to be skipped, so
        // there is nothing to attribute it to, and a length above a cap the peer's
        // writer also enforces is a bug or a corrupted stream rather than a
        // payload.
        let Ok(incoming) =
            frame::read_frame::<_, F::Incoming>(&mut inner.pipe.reader(None), inner.cap)
        else {
            break;
        };
        match F::route(incoming) {
            Routed::Response(id, response) => inner.settle(id, Ok(response)),
            // A handoff, never work: see this module's header.
            Routed::Push(push) => sink.push(push),
        }
    }
    inner.close_all();
    sink.channel_lost();
}
