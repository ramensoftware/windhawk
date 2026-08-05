//! The UI's end of the channel: a session whose commands are served by the
//! elevated broker, and the sink its unsolicited frames land in.
//!
//! [`RemoteSession`] is the same seam the in-process session implements, so no
//! handler, shaper, or follow-up can tell which one it is talking to - and the
//! payloads cross unparsed in both directions, so error codes, messages, and the
//! core's own `location` origins survive the crossing byte for byte.

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::value::RawValue;
use windhawk_broker::{ChannelError, PushSink, Requester};
use windhawk_core_host::{CancelHandle, HostError, SessionApi};
use windhawk_core_protocol::{ErrorCode, WireError};

use crate::broker::wire::{Channel, Push, Request, Response};
use crate::pump::PumpMessage;

/// How long a cancel is worth waiting for. Ordinary requests carry no deadline -
/// a slow `compileMod` is slow, not broken - but a cancel that arrives late is
/// worth nothing, and [`CancelHandle`]'s contract promises it never blocks past
/// its deadline and answers `false` rather than erroring on a dead channel.
const CANCEL_DEADLINE: Duration = Duration::from_secs(5);

/// A core session hosted by the broker process.
pub struct RemoteSession {
    requester: Arc<Requester<Channel>>,
}

impl RemoteSession {
    pub fn new(requester: Arc<Requester<Channel>>) -> RemoteSession {
        RemoteSession { requester }
    }
}

impl SessionApi for RemoteSession {
    fn invoke_raw(&self, request: &str) -> Result<String, HostError> {
        let envelope = envelope(request)?;
        match self.requester.request(Request::invoke(envelope)) {
            // The core's response envelope, exactly as the core wrote it: the
            // caller parses it with the same parse it runs in-process.
            Ok(Response::Raw(raw)) => Ok(raw.get().to_owned()),
            Ok(Response::Failed(fault)) => Err(fault.into_host()),
            Ok(_) => Err(unexpected("invoke")),
            Err(error) => Err(lost(error)),
        }
    }

    fn invoke_async_raw(&self, request: &str) -> Result<u64, HostError> {
        let envelope = envelope(request)?;
        match self.requester.request(Request::invoke_async(envelope)) {
            Ok(Response::Started(op_id)) => Ok(op_id),
            // A refused START carries the core's own typed failure, so a mod that
            // is not installed reads as that rather than as a channel problem.
            Ok(Response::Failed(fault)) => Err(fault.into_host()),
            Ok(_) => Err(unexpected("invokeAsync")),
            Err(error) => Err(lost(error)),
        }
    }

    fn cancel_token(&self, op_id: u64) -> Arc<dyn CancelHandle> {
        Arc::new(RemoteCancel {
            requester: Arc::clone(&self.requester),
            op_id,
        })
    }
}

/// A cancel bound to one op-id of the broker's session.
struct RemoteCancel {
    requester: Arc<Requester<Channel>>,
    op_id: u64,
}

impl CancelHandle for RemoteCancel {
    /// Ask the broker to signal the op. Bounded and infallible by contract: a
    /// dead channel or an expired deadline is `false`, the same answer an op that
    /// had already finished gives.
    fn cancel(&self) -> bool {
        match self
            .requester
            .request_within(Request::cancel(self.op_id), CANCEL_DEADLINE)
        {
            Ok(Response::Cancelled(cancelled)) => cancelled,
            _ => false,
        }
    }
}

/// Where the broker's unsolicited frames go.
///
/// **Every arm does exactly one thing: put a message on the pump.** The sink runs
/// on the channel's reader thread, and a reader that stops reading lets the peer's
/// writes fill the pipe buffer, after which the peer stops reading and a large
/// request in the other direction blocks against someone who will never drain it -
/// with every thread involved doing exactly what its own contract says. So even
/// printing a line happens on the pump thread: a console write is not the bounded
/// operation it looks like (a console in selection mode blocks its writers), and
/// the reader thread is the one place on the channel where blocking is a deadlock
/// rather than a delay.
///
/// The queue it hands to is unbounded, so the handoff itself cannot block either.
pub struct ChannelSink {
    pump: Sender<PumpMessage>,
    /// The generation of the session on the other end of this channel: an op-id
    /// identifies an op only within one session, and two sessions feed this pump.
    generation: u64,
    /// What to do about the channel ending, run ON the pump thread. Held as a
    /// factory rather than performed here for the reason above.
    lost: Box<dyn Fn() -> PumpMessage + Send + Sync>,
}

impl ChannelSink {
    pub fn new(
        pump: Sender<PumpMessage>,
        generation: u64,
        lost: Box<dyn Fn() -> PumpMessage + Send + Sync>,
    ) -> ChannelSink {
        ChannelSink {
            pump,
            generation,
            lost,
        }
    }
}

impl PushSink<Push> for ChannelSink {
    fn push(&self, push: Push) {
        match push {
            Push::Event { op_id, raw } => {
                // Best effort, exactly as the in-process event callback is: a
                // closed receiver means the pump (and the app) is gone.
                let _ = self.pump.send(PumpMessage::Event {
                    generation: self.generation,
                    op_id,
                    event_json: raw.get().to_owned(),
                });
            }
            // The same line the in-process log callback prints, so core
            // diagnostics read the same whichever session produced them - written
            // by the pump thread, for the reason in this type's header.
            Push::Log { level, message } => {
                let _ = self.pump.send(PumpMessage::deferred(move |_| {
                    eprintln!("[core:{level}] {message}");
                }));
            }
            // The `Global\` half of the debug-output capture, which only an
            // elevated process can open. It joins the pane's own `Local\`
            // lines in the one tail buffer, through the pump for the same reason
            // an event does: this thread hands off, it does not work.
            Push::Dbwin(lines) => {
                let _ = self.pump.send(PumpMessage::deferred(move |ctx| {
                    ctx.log.deliver_captured(&lines);
                }));
            }
            Push::Malformed(reason) => {
                let _ = self.pump.send(PumpMessage::deferred(move |_| {
                    eprintln!(
                        "windhawk-ui: the broker sent a frame that could not be routed: {reason}"
                    );
                }));
            }
        }
    }

    /// The channel ended. This is a SIGNAL and nothing else: unwinding the work
    /// that was in flight needs the pump's seams and its single-threadedness,
    /// and none of it may happen on this thread.
    fn channel_lost(&self) {
        let _ = self.pump.send((self.lost)());
    }
}

/// The request envelope as a raw JSON value, so it crosses as the bytes
/// `windhawk_core_host` wrote rather than as a re-rendering of them.
fn envelope(request: &str) -> Result<Box<RawValue>, HostError> {
    RawValue::from_string(request.to_owned())
        .map_err(|error| internal(format!("the request envelope is not JSON: {error}")))
}

/// A channel failure, as the host failure the call sites already handle. It maps
/// onto `Transport`, which the reply shaping renders as the lost-broker error.
/// That mapping is why nothing else here raises a `Transport`: a code that tells
/// the user the elevated helper is gone has to mean a link that is actually gone.
fn lost(error: ChannelError) -> HostError {
    HostError::transport(match error {
        ChannelError::Closed => {
            "the connection to the elevated Windhawk helper was lost".to_owned()
        }
        other => format!("the connection to the elevated Windhawk helper failed: {other}"),
    })
}

/// The broker answered something no request of this kind can produce - a bug on
/// one side of a channel whose two ends ship together.
fn unexpected(kind: &str) -> HostError {
    internal(format!(
        "the elevated Windhawk helper answered a '{kind}' request with an unrelated response"
    ))
}

/// A failure of this seam itself, as the contract's `INTERNAL`. The channel is
/// answering - or was never asked - so the message describes a bug rather than a
/// connection the user could do anything about.
fn internal(message: String) -> HostError {
    HostError::wire(WireError::new(ErrorCode::Internal, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::mpsc::{Receiver, TryRecvError, channel};

    fn sink() -> (ChannelSink, Receiver<PumpMessage>) {
        let (pump, messages) = channel();
        // Nothing ever drains `messages` in these tests, which is the point: the
        // sink has to complete against a pump that is not answering.
        let sink = ChannelSink::new(pump, 7, Box::new(|| PumpMessage::deferred(|_| {})));
        (sink, messages)
    }

    /// The invariant the whole channel rests on, in the one place it is easiest to
    /// break: every arm of the sink hands its frame to the pump and returns,
    /// leaving the reader thread free to go back to its next read.
    ///
    /// Asserted as "exactly one message, for every arm" rather than as a timing
    /// bound, because that is the property with teeth: an arm that did the work
    /// itself - printed a line, took a lock, touched the log pane - would pass a
    /// stopwatch on a quiet machine and deadlock the channel on a busy one. An arm
    /// that queues nothing is the same regression seen from the other side.
    #[test]
    fn every_push_is_a_handoff_and_nothing_else() {
        let raw = || RawValue::from_string("{\"type\":\"progress\"}".to_owned()).unwrap();
        let pushes = [
            Push::Event {
                op_id: 3,
                raw: raw(),
            },
            Push::Log {
                level: 2,
                message: "a core log line".to_owned(),
            },
            Push::Dbwin(vec!["a captured line".to_owned()]),
            Push::Malformed("a frame that did not route".to_owned()),
        ];

        for push in pushes {
            let (sink, messages) = sink();
            sink.push(push);
            assert!(
                messages.try_recv().is_ok(),
                "the push queued nothing for the pump"
            );
            assert_eq!(
                messages.try_recv().err(),
                Some(TryRecvError::Empty),
                "the push queued more than the one message it hands off"
            );
        }
    }

    /// The event arm also carries the generation it was built with, since an op-id
    /// identifies an op only within one session.
    #[test]
    fn an_event_carries_the_channels_generation() {
        let (sink, messages) = sink();
        sink.push(Push::Event {
            op_id: 3,
            raw: RawValue::from_string("{}".to_owned()).unwrap(),
        });

        match messages.try_recv().expect("the event reached the pump") {
            PumpMessage::Event {
                generation,
                op_id,
                event_json,
            } => {
                assert_eq!((generation, op_id), (7, 3));
                assert_eq!(event_json, "{}");
            }
            PumpMessage::Deferred(_) => panic!("an event must reach the pump as an event"),
        }
    }

    /// Losing the channel is a signal too. What it costs - failing the ops that
    /// were in flight, putting the banner up, swapping back to the local session -
    /// needs the pump's seams, and none of it may happen here.
    #[test]
    fn a_lost_channel_is_signalled_rather_than_handled() {
        let (pump, messages) = channel();
        let signalled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let raised = Arc::clone(&signalled);
        let sink = ChannelSink::new(
            pump,
            7,
            Box::new(move || {
                raised.store(true, std::sync::atomic::Ordering::Relaxed);
                PumpMessage::deferred(|_| {})
            }),
        );

        sink.channel_lost();

        assert!(
            matches!(messages.try_recv(), Ok(PumpMessage::Deferred(_))),
            "the loss reaches the pump as work for the pump thread"
        );
        assert!(
            signalled.load(std::sync::atomic::Ordering::Relaxed),
            "the caller's own handling was asked for"
        );
    }
}
