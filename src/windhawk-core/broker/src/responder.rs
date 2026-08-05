//! The responding end: the pipe CLIENT, the serve loop, and the writer thread
//! that owns unsolicited pushes.
//!
//! This end never listens. It connects out to a name it was handed, identifies
//! the peer serving that name before it says anything, and exits if the peer is
//! not the one it was started for.
//!
//! Three rules shape the threading here, and they only work together:
//!
//! - **A push is written by a dedicated thread, never by whoever produced it.**
//!   Pushes are typically produced on threads that are not allowed to block; one
//!   that wrote straight to the pipe would stall its producer for as long as the
//!   peer stopped reading.
//! - **Requests are served on a small fixed pool**, because a slow request must
//!   not block a fast one, and because an unbounded thread spawn in the
//!   privileged process is not a property to leave to an upstream invariant.
//! - **The prompt requests bypass the pool**, served inline on the reader thread.
//!   A pool thread is held for the whole of a request, so a saturated pool is
//!   reachable in normal use, and a request whose only value is being prompt
//!   would queue behind exactly the work it exists to interrupt. Those requests
//!   are a lookup and a signal on this side, so serving them off the reader
//!   thread costs nothing - except that it puts a WRITE on the reader thread,
//!   which is safe only while the peer keeps draining. That is why the
//!   requesting end's reader is forbidden to block: these two rules ship
//!   together or not at all.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::frame::{self, FrameError};
use crate::handler::{BrokerHandler, Disposition};
use crate::pipe::PipeStream;
use crate::security::{PeerPolicy, RejectReason, SelfIdentity};
use crate::version::{ChannelConfig, Handshake};

/// How long the writer thread parks before re-checking whether the channel has
/// ended. Only reached when there is nothing to write.
const WRITER_IDLE_SLICE: Duration = Duration::from_millis(100);

/// Why this end could not establish a channel.
#[derive(Debug)]
pub enum ConnectError {
    /// The deadline passed with no channel. A responder that cannot find its
    /// channel must not linger.
    Timeout,
    /// The peer failed the peer policy. This direction never degrades: a peer
    /// that cannot be identified is a peer that is not served.
    Rejected(RejectReason),
    /// The peer speaks a different wire version.
    Protocol {
        found: u32,
        expected: u32,
    },
    /// The handshake did not complete.
    Handshake(String),
    Io(io::Error),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Timeout => write!(f, "no channel was established before the deadline"),
            ConnectError::Rejected(reason) => write!(f, "the peer was refused: {reason}"),
            ConnectError::Protocol { found, expected } => {
                write!(f, "peer speaks protocol {found}, expected {expected}")
            }
            ConnectError::Handshake(detail) => write!(f, "the handshake failed: {detail}"),
            ConnectError::Io(error) => write!(f, "the connect failed: {error}"),
        }
    }
}

impl std::error::Error for ConnectError {}

/// A verified, acked channel, ready to be served.
pub struct Connection {
    pipe: PipeStream,
    config: ChannelConfig,
    /// The peer's process id, from the pipe rather than from anything it said.
    pub peer_pid: u32,
}

/// Connect to `name`, verify whoever is serving it, and complete the handshake.
///
/// `hello` goes out only after the peer has been identified, and it is a claim
/// that this end can already serve: whatever the caller needs in order to answer
/// requests is built BEFORE this is called, so a process that cannot serve never
/// becomes a channel at all.
pub fn connect(
    name: &str,
    config: &ChannelConfig,
    policy: &PeerPolicy,
    deadline: Instant,
) -> Result<Connection, ConnectError> {
    let me = SelfIdentity::resolve().map_err(ConnectError::Io)?;
    let pipe = match PipeStream::connect(name, deadline) {
        Ok(pipe) => pipe,
        Err(error) if error.kind() == io::ErrorKind::TimedOut => return Err(ConnectError::Timeout),
        Err(error) => return Err(ConnectError::Io(error)),
    };

    // Identified the moment the connection succeeds, before a byte is written:
    // this end has the instruments to do it (a more privileged process opening a
    // less privileged one always works), and this is the direction that decides
    // whether privileged work happens on someone's behalf.
    let peer = crate::security::identify_server(&pipe)
        .map_err(|error| ConnectError::Handshake(format!("peer identification failed: {error}")))?;
    policy
        .evaluate_server(&peer, &me)
        .map_err(ConnectError::Rejected)?;

    let hello = Handshake::Hello {
        protocol: config.protocol,
        version: config.version.clone(),
        pid: me.pid,
    };
    let bytes = frame::encode(&hello, config.frame_cap).map_err(|error| {
        ConnectError::Handshake(format!("the hello could not be built: {error}"))
    })?;
    pipe.write_all(&bytes, Some(deadline)).map_err(|error| {
        ConnectError::Handshake(format!("the hello could not be sent: {error}"))
    })?;

    let ack: Handshake = frame::read_frame(&mut pipe.reader(Some(deadline)), config.frame_cap)
        .map_err(|error| {
            ConnectError::Handshake(match error {
                FrameError::Eof => "the peer closed the channel instead of acking".to_owned(),
                other => other.to_string(),
            })
        })?;
    match ack {
        Handshake::HelloAck { protocol } if protocol == config.protocol => {}
        Handshake::HelloAck { protocol } => {
            return Err(ConnectError::Protocol {
                found: protocol,
                expected: config.protocol,
            });
        }
        Handshake::Hello { .. } => {
            return Err(ConnectError::Handshake(
                "the peer answered a hello with a hello".to_owned(),
            ));
        }
    }

    Ok(Connection {
        pipe,
        config: config.clone(),
        peer_pid: peer.pid,
    })
}

/// A handle for emitting unsolicited frames. Cloneable, and never blocking: it
/// hands the push to the writer thread and returns.
pub struct Pusher<Push> {
    queue: Sender<Push>,
}

impl<Push> Clone for Pusher<Push> {
    fn clone(&self) -> Self {
        Pusher {
            queue: self.queue.clone(),
        }
    }
}

impl<Push> Pusher<Push> {
    /// Queue a push. Returns false once the channel has ended, at which point
    /// there is nowhere for it to go.
    pub fn push(&self, push: Push) -> bool {
        self.queue.send(push).is_ok()
    }
}

/// The receiving half of the push queue, handed to the responder it feeds.
pub struct PushQueue<Push> {
    queue: Receiver<Push>,
}

/// Build the push queue.
///
/// Separate from [`Responder::start`] because the things that produce pushes
/// generally exist BEFORE the channel does: whatever this end has to be able to
/// serve is built first, precisely so that a process which cannot serve never
/// becomes a channel at all, and it can start emitting the moment it exists -
/// during its own construction, during the connect, and during the handshake.
/// Everything queued in that window waits here and goes out once the writer
/// thread starts, which is the same queue it would have gone through anyway.
pub fn push_queue<Push>() -> (Pusher<Push>, PushQueue<Push>) {
    let (sender, receiver) = channel();
    (Pusher { queue: sender }, PushQueue { queue: receiver })
}

/// The serve loop over an established channel.
pub struct Responder<H: BrokerHandler> {
    shared: Arc<Shared<H>>,
    threads: Vec<JoinHandle<()>>,
}

struct Shared<H: BrokerHandler> {
    pipe: PipeStream,
    cap: usize,
    handler: Arc<H>,
    stopped: AtomicBool,
}

impl<H: BrokerHandler> Responder<H> {
    /// Start serving. `workers` bounds how many requests are in flight at once;
    /// one that arrives with the pool full simply queues. `pushes` is the queue
    /// from [`push_queue`], whatever has already accumulated in it included.
    pub fn start(
        connection: Connection,
        handler: Arc<H>,
        workers: usize,
        pushes: PushQueue<H::Push>,
    ) -> Responder<H> {
        let shared = Arc::new(Shared {
            pipe: connection.pipe,
            cap: connection.config.frame_cap,
            handler,
            stopped: AtomicBool::new(false),
        });

        let (work_tx, work_rx) = channel::<H::Request>();
        let work_rx = Arc::new(Mutex::new(work_rx));
        let push_rx = pushes.queue;

        let mut threads = Vec::with_capacity(workers + 2);
        for index in 0..workers.max(1) {
            let shared = Arc::clone(&shared);
            let work_rx = Arc::clone(&work_rx);
            threads.push(
                std::thread::Builder::new()
                    .name(format!("windhawk-broker-worker-{index}"))
                    .spawn(move || work_loop(shared, work_rx))
                    .expect("a responder worker thread must start"),
            );
        }

        let writing = Arc::clone(&shared);
        threads.push(
            std::thread::Builder::new()
                .name("windhawk-broker-writer".to_owned())
                .spawn(move || write_loop(writing, push_rx))
                .expect("the responder writer thread must start"),
        );

        let reading = Arc::clone(&shared);
        threads.push(
            std::thread::Builder::new()
                .name("windhawk-broker-responder".to_owned())
                .spawn(move || read_loop(reading, work_tx))
                .expect("the responder reader thread must start"),
        );

        Responder { shared, threads }
    }

    /// Stop serving. Every thread is released; anything mid-write is abandoned.
    pub fn shutdown(&self) {
        self.shared.stop();
    }

    /// Wait until the channel ends, then stop everything it started.
    pub fn join(mut self) {
        // The reader is last in the list, and it is the one whose end means the
        // channel is over; joining it first keeps the wait honest rather than
        // parking on a worker that is idle by construction.
        let threads = std::mem::take(&mut self.threads);
        let mut threads = threads.into_iter().rev();
        if let Some(reader) = threads.next() {
            let _ = reader.join();
        }
        // The caller keeps the sending half of the push queue, so the writer is
        // released by the stop flag rather than by the queue closing.
        self.shared.stop();
        for thread in threads {
            let _ = thread.join();
        }
    }
}

impl<H: BrokerHandler> Drop for Responder<H> {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

impl<H: BrokerHandler> Shared<H> {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.pipe.signal_shutdown();
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Write a response, or - when it is too large for the wire - a response that
    /// says so, so the peer gets a legible failure for that one request rather
    /// than a dead channel.
    fn respond(&self, id: u64, response: H::Response) {
        // Once the channel is over, a request still in a worker's hands has
        // nowhere to send its answer. Checked rather than left to the write to
        // fail, because a small write can complete without ever reaching the
        // wait that would notice.
        if self.is_stopped() {
            return;
        }
        let bytes = match frame::encode(&response, self.cap) {
            Ok(bytes) => bytes,
            Err(FrameError::TooLarge { bytes, cap }) => {
                match frame::encode(&self.handler.oversized(id, bytes, cap), cap) {
                    Ok(bytes) => bytes,
                    Err(_) => return,
                }
            }
            Err(_) => return,
        };
        if self.pipe.write_all(&bytes, None).is_err() {
            self.stop();
        }
    }
}

fn read_loop<H: BrokerHandler>(shared: Arc<Shared<H>>, work: Sender<H::Request>) {
    loop {
        // Any failure ends the channel, an over-cap frame included: the peer's
        // writer enforces the same cap, so one arriving is a bug or a corrupted
        // stream, and there is no request id outside the payload to attribute it
        // to anyway.
        let Ok(request) =
            frame::read_frame::<_, H::Request>(&mut shared.pipe.reader(None), shared.cap)
        else {
            break;
        };
        let id = shared.handler.request_id(&request);
        match shared.handler.disposition(&request) {
            Disposition::Pooled => {
                if work.send(request).is_err() {
                    break;
                }
            }
            Disposition::Immediate => {
                let response = shared.handler.handle(request);
                shared.respond(id, response);
            }
            Disposition::Final => {
                let response = shared.handler.handle(request);
                shared.respond(id, response);
                break;
            }
        }
        if shared.is_stopped() {
            break;
        }
    }
    // Dropping `work` here is what lets the pool threads finish.
    drop(work);
    shared.stop();
}

/// One pool thread: take the next request off the shared queue, serve it, answer.
///
/// The queue is an mpsc receiver behind a mutex, so it is the HAND-OFF that is
/// serialized - one thread parked in `recv`, the rest waiting their turn to be
/// the one - and not the serving. The lock is released before the handler runs,
/// which is where the whole cost of a request is, so the pool is as concurrent as
/// its size whatever the handler does.
fn work_loop<H: BrokerHandler>(shared: Arc<Shared<H>>, work: Arc<Mutex<Receiver<H::Request>>>) {
    loop {
        let request = {
            let queue = work
                .lock()
                .expect("the responder work queue lock is poisoned");
            match queue.recv() {
                Ok(request) => request,
                Err(_) => break,
            }
        };
        let id = shared.handler.request_id(&request);
        let response = shared.handler.handle(request);
        shared.respond(id, response);
    }
}

fn write_loop<H: BrokerHandler>(shared: Arc<Shared<H>>, pushes: Receiver<H::Push>) {
    loop {
        if shared.is_stopped() {
            break;
        }
        match pushes.recv_timeout(WRITER_IDLE_SLICE) {
            Ok(push) => match frame::encode(&push, shared.cap) {
                Ok(bytes) => {
                    if shared.pipe.write_all(&bytes, None).is_err() {
                        shared.stop();
                        break;
                    }
                }
                // A push answers no request, so there is nothing to fail with it.
                Err(FrameError::TooLarge { bytes, cap }) => shared.handler.push_dropped(bytes, cap),
                Err(_) => {}
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if shared.is_stopped() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
