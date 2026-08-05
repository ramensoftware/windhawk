//! The swap point: the two seams whose implementation can be replaced under a live
//! window - the core session and the privileged host operations.
//!
//! Degraded mode has to change which session the UI is talking to while the
//! front-end, the op registry, and every in-flight worker stay exactly where they
//! are. Putting the replaceable cell in the bridge context would reshape every
//! call site that reaches it; putting it BEHIND the seam means no call site
//! knows a swap is possible at all.
//!
//! Both seams live here because they have to agree: a process that reached the
//! broker for its commands and not for its editor launch, or the other way round,
//! would be of two minds about which world it is in. They are not installed at the
//! same MOMENT, though, and that is deliberate - see [`crate::broker::BrokerLink`],
//! which owns both.
//!
//! The swap point also owns WHEN an async op may start ([`Settled`]), because a
//! swap ends every op the outgoing session was carrying and only this side of the
//! seam knows a swap is coming.

use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use windhawk_core_host::{CancelHandle, HostError, SessionApi};

use crate::broker::ops::{EditorOpen, HostOpFailure, HostOps};
use crate::shell::ThemeSetting;

/// The one-way point past which the session behind the seam is the one this
/// process is going to run on: the swap to the broker's session, or degraded
/// mode, whichever comes first. Reached once per process - a Retry from degraded
/// mode swaps a settled session, and is not gated by this.
///
/// It exists because a swap ENDS every op the outgoing session was carrying
/// ([`crate::ipc::bridge::BridgeCtx::hand_over_ops`]): an op-id means nothing to
/// any other session. So an async op started in the window before the swap is
/// failed by it, for no reason the user can connect to anything they did - which
/// is why the UI's own background work waits here
/// ([`crate::broker::BrokerLink::wait_until_settled`]) and why an async op START
/// holds behind it too.
pub struct Settled {
    reached: Mutex<bool>,
    changed: Condvar,
    /// How long an op start will hold for the settle point before going ahead
    /// without it ([`Settled::hold`]).
    patience: Duration,
}

impl Settled {
    pub fn new(patience: Duration) -> Settled {
        Settled {
            reached: Mutex::new(false),
            changed: Condvar::new(),
            patience,
        }
    }

    /// The session behind the seam is now the one this process will run on.
    /// Idempotent, and one way: the point is not un-reached by a later swap.
    pub fn reach(&self) {
        *self.lock() = true;
        self.changed.notify_all();
    }

    /// Park until the point is reached.
    pub fn wait(&self) {
        let mut reached = self.lock();
        while !*reached {
            reached = self
                .changed
                .wait(reached)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    /// Park until the point is reached, or until the patience runs out.
    ///
    /// Bounded because the two costs are not symmetric. Holding an op back
    /// through the ordinary launch is invisible - the settle point is a moment
    /// away and the front-end is showing a spinner it would have shown anyway.
    /// Holding it through a consent dialog nobody has answered is not: the ladder
    /// is bounded by a person, and a page that renders its content is worth more
    /// than avoiding a swap that is now minutes rather than milliseconds away -
    /// and is correspondingly unlikely to land inside this one op.
    fn hold(&self) {
        let reached = self.lock();
        if *reached {
            return;
        }
        let _ = self
            .changed
            .wait_timeout_while(reached, self.patience, |reached| !*reached);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        self.reached
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

/// A session that can be replaced. Every call reads the current implementation
/// and forwards to it.
pub struct SwappableSession {
    inner: RwLock<Arc<dyn SessionApi>>,
    settled: Arc<Settled>,
}

impl SwappableSession {
    pub fn new(inner: Arc<dyn SessionApi>, settled: Arc<Settled>) -> SwappableSession {
        SwappableSession {
            inner: RwLock::new(inner),
            settled,
        }
    }

    /// Put `inner` behind the seam. Calls already forwarded to the outgoing
    /// implementation run to completion against it; calls made after this see the
    /// incoming one.
    pub fn install(&self, inner: Arc<dyn SessionApi>) {
        *self
            .inner
            .write()
            .unwrap_or_else(|error| error.into_inner()) = inner;
    }

    /// The current implementation, CLONED OUT so the read guard is released
    /// before the call goes through it.
    ///
    /// Forwarding while still holding the guard would make an install - a write
    /// lock - wait behind whatever calls are in flight, which is precisely the
    /// state the install exists to escape: a channel that has just died is the one
    /// whose calls are still parked. The one-liner that keeps the guard is the
    /// version that compiles.
    fn current(&self) -> Arc<dyn SessionApi> {
        Arc::clone(&self.inner.read().unwrap_or_else(|error| error.into_inner()))
    }
}

impl SessionApi for SwappableSession {
    fn invoke_raw(&self, request: &str) -> Result<String, HostError> {
        self.current().invoke_raw(request)
    }

    /// Held behind the settle point, unlike the synchronous invoke beside it.
    ///
    /// A sync call is answered by whichever session serves it and is over; an
    /// async one leaves an op registered against that session, and the swap ends
    /// every op the outgoing session was carrying. So the first moments of a
    /// launch - where the front-end is live and the broker is not yet - are
    /// exactly where an op start must not land, and this is the one place that
    /// knows it. Gating the sync side too would be a different thing entirely:
    /// the startup reads that build the window run through this seam, so it would
    /// put every launch behind the ladder.
    fn invoke_async_raw(&self, request: &str) -> Result<u64, HostError> {
        self.settled.hold();
        self.current().invoke_async_raw(request)
    }

    /// The handle is bound to the session that issued the op-id, not to the seam:
    /// an op-id means nothing to any other session, so a cancel that arrived after
    /// a swap must reach the session that started the op (where it finds it gone
    /// and answers `false`) rather than the one that happens to be installed now.
    fn cancel_token(&self, op_id: u64) -> Arc<dyn CancelHandle> {
        self.current().cancel_token(op_id)
    }
}

/// Host operations that can be replaced, on the same terms as the session above:
/// every call reads the current implementation, clones it out, and forwards to it
/// with no lock held.
pub struct SwappableHostOps {
    inner: RwLock<Arc<dyn HostOps>>,
    /// Whether the debug-output capture has been asked for and not released.
    ///
    /// Every other operation is a one-shot, so a swap simply changes who serves
    /// the next one. The capture is not: it is a request to keep running, held by
    /// the implementation that was installed when the pane was opened, and a swap
    /// would otherwise leave the log pane quietly missing half its stream until
    /// someone closed and reopened it.
    ///
    /// A lock rather than a flag, because the flag and the call that acts on it
    /// have to move together. Held across the whole of a start, a stop, and an
    /// install's hand-over, so a start that races an install cannot leave the
    /// capture running on the implementation that is on its way out - or running
    /// twice on the one that arrived. It guards nothing else: the ordinary
    /// one-shot operations forward with no lock held, and an install still takes
    /// the write lock for no longer than the swap itself.
    capturing: Mutex<bool>,
}

impl SwappableHostOps {
    pub fn new(inner: Arc<dyn HostOps>) -> SwappableHostOps {
        SwappableHostOps {
            inner: RwLock::new(inner),
            capturing: Mutex::new(false),
        }
    }

    /// Put `inner` behind the seam, and hand the running capture over with it.
    pub fn install(&self, inner: Arc<dyn HostOps>) {
        let capturing = self.capture_lock();
        let outgoing = std::mem::replace(
            &mut *self
                .inner
                .write()
                .unwrap_or_else(|error| error.into_inner()),
            Arc::clone(&inner),
        );
        if Arc::ptr_eq(&outgoing, &inner) {
            return;
        }
        if *capturing {
            outgoing.dbwin_stop();
            inner.dbwin_start();
        }
    }

    /// The capture lock. Taken before the seam's own lock wherever both are held,
    /// which is [`SwappableHostOps::install`] alone.
    fn capture_lock(&self) -> std::sync::MutexGuard<'_, bool> {
        self.capturing
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn current(&self) -> Arc<dyn HostOps> {
        Arc::clone(&self.inner.read().unwrap_or_else(|error| error.into_inner()))
    }
}

impl HostOps for SwappableHostOps {
    fn seed_mods_runtime(&self) {
        self.current().seed_mods_runtime();
    }

    fn editor_open(&self, request: &EditorOpen) -> Result<(), HostOpFailure> {
        self.current().editor_open(request)
    }

    fn editor_sweep(&self) {
        self.current().editor_sweep();
    }

    fn editor_sync_theme(&self, theme: ThemeSetting) {
        self.current().editor_sync_theme(theme);
    }

    fn dbwin_start(&self) {
        let mut capturing = self.capture_lock();
        *capturing = true;
        self.current().dbwin_start();
    }

    fn dbwin_stop(&self) {
        let mut capturing = self.capture_lock();
        *capturing = false;
        self.current().dbwin_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A session that records what it was asked and answers with its own name.
    /// An async op start is reported by name through `started`, which is what the
    /// settle-point tests read: the question there is not how many starts there
    /// were but WHICH session served them, and when.
    struct Named {
        name: &'static str,
        calls: AtomicUsize,
        started: Option<std::sync::mpsc::Sender<&'static str>>,
    }

    impl Named {
        fn new(name: &'static str) -> Arc<Named> {
            Arc::new(Named {
                name,
                calls: AtomicUsize::new(0),
                started: None,
            })
        }

        fn reporting(
            name: &'static str,
            started: std::sync::mpsc::Sender<&'static str>,
        ) -> Arc<Named> {
            Arc::new(Named {
                name,
                calls: AtomicUsize::new(0),
                started: Some(started),
            })
        }
    }

    impl SessionApi for Named {
        fn invoke_raw(&self, _request: &str) -> Result<String, HostError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.name.to_owned())
        }

        fn invoke_async_raw(&self, _request: &str) -> Result<u64, HostError> {
            if let Some(started) = &self.started {
                let _ = started.send(self.name);
            }
            Ok(1)
        }

        fn cancel_token(&self, _op_id: u64) -> Arc<dyn CancelHandle> {
            struct Never;
            impl CancelHandle for Never {
                fn cancel(&self) -> bool {
                    false
                }
            }
            Arc::new(Never)
        }
    }

    /// A settle point that has already been reached, for the tests that are
    /// about which session serves a call rather than about when it may.
    fn settled() -> Arc<Settled> {
        let settled = Arc::new(Settled::new(PATIENCE));
        settled.reach();
        settled
    }

    /// Long enough that a test which reaches the settle point is measuring the
    /// wake-up rather than the timeout.
    const PATIENCE: Duration = Duration::from_secs(30);

    #[test]
    fn calls_reach_whichever_session_is_installed() {
        let local = Named::new("local");
        let remote = Named::new("remote");
        let session = SwappableSession::new(local.clone(), settled());

        assert_eq!(session.invoke_raw("{}").unwrap(), "local");
        session.install(remote.clone());
        assert_eq!(session.invoke_raw("{}").unwrap(), "remote");
        // Swapping back is ordinary: the local session is kept for the process
        // lifetime precisely so a lost channel has somewhere to land.
        session.install(local.clone());
        assert_eq!(session.invoke_raw("{}").unwrap(), "local");

        assert_eq!(local.calls.load(Ordering::Relaxed), 2);
        assert_eq!(remote.calls.load(Ordering::Relaxed), 1);
    }

    /// An install must not wait for the calls in flight through the seam - the
    /// channel it is replacing is typically the one whose calls are parked.
    #[test]
    fn an_install_does_not_wait_for_a_call_in_flight() {
        use std::sync::mpsc::{Receiver, Sender, channel};

        /// A session whose invoke parks until it is released.
        struct Parked {
            entered: Sender<()>,
            release: Mutex<Receiver<()>>,
        }
        use std::sync::Mutex;

        impl SessionApi for Parked {
            fn invoke_raw(&self, _request: &str) -> Result<String, HostError> {
                let _ = self.entered.send(());
                let _ = self.release.lock().expect("the release lock").recv();
                Ok("parked".to_owned())
            }

            fn invoke_async_raw(&self, _request: &str) -> Result<u64, HostError> {
                Ok(0)
            }

            fn cancel_token(&self, _op_id: u64) -> Arc<dyn CancelHandle> {
                unreachable!("this session is only ever invoked")
            }
        }

        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        let parked = Arc::new(Parked {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        let session = Arc::new(SwappableSession::new(parked, settled()));

        let calling = Arc::clone(&session);
        let call = std::thread::spawn(move || calling.invoke_raw("{}"));
        entered_rx.recv().expect("the call reached the session");

        // The install completes while that call is still inside the outgoing
        // session; if it waited for the guard, this would deadlock.
        session.install(Named::new("live"));
        assert_eq!(session.invoke_raw("{}").unwrap(), "live");

        let _ = release_tx.send(());
        assert_eq!(call.join().expect("the call thread").unwrap(), "parked");
    }

    /// The launch race this exists for: the front-end is live before the broker
    /// is, and an op it starts in that window would be ended by the swap that
    /// follows - a failure with nothing behind it the user can see. So the start
    /// waits, and lands on the session that is going to serve it.
    #[test]
    fn an_async_op_start_waits_for_the_settle_point() {
        let (started_tx, started) = std::sync::mpsc::channel();
        let settled = Arc::new(Settled::new(PATIENCE));
        let local = Named::reporting("local", started_tx.clone());
        let session = Arc::new(SwappableSession::new(local, Arc::clone(&settled)));

        let starting = Arc::clone(&session);
        let start = std::thread::spawn(move || starting.invoke_async_raw("{}"));
        assert!(
            started.recv_timeout(Duration::from_millis(100)).is_err(),
            "the op started against the session that is about to be replaced"
        );

        // The order the swap runs in: the incoming session is behind the seam,
        // and the hand-over that ends the outgoing session's ops is done, BEFORE
        // anything held here is let through.
        session.install(Named::reporting("remote", started_tx));
        settled.reach();

        assert_eq!(
            started.recv_timeout(PATIENCE).expect("the op started"),
            "remote"
        );
        assert_eq!(start.join().expect("the start thread").unwrap(), 1);
    }

    /// A settle point that is minutes away (a consent dialog nobody has answered)
    /// is not worth holding a page's content for, so the wait is bounded.
    #[test]
    fn an_async_op_start_gives_up_on_a_settle_point_that_does_not_come() {
        let (started_tx, started) = std::sync::mpsc::channel();
        let session = SwappableSession::new(
            Named::reporting("local", started_tx),
            Arc::new(Settled::new(Duration::from_millis(50))),
        );

        assert_eq!(session.invoke_async_raw("{}").unwrap(), 1);
        assert_eq!(started.try_recv().expect("the op started"), "local");
    }

    /// The startup reads that build the window run through this seam, so gating
    /// them on the settle point would put every launch behind the ladder. They
    /// are also the calls a swap cannot hurt: a sync call is answered and over.
    #[test]
    fn a_sync_invoke_does_not_wait_for_the_settle_point() {
        let session = SwappableSession::new(Named::new("local"), Arc::new(Settled::new(PATIENCE)));

        let started = std::time::Instant::now();
        assert_eq!(session.invoke_raw("{}").unwrap(), "local");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the read waited for a settle point that never came"
        );
    }

    /// Host operations that record what they were asked, and answer nothing.
    #[derive(Default)]
    struct Recording {
        started: AtomicUsize,
        stopped: AtomicUsize,
    }

    impl HostOps for Recording {
        fn seed_mods_runtime(&self) {}
        fn editor_open(&self, _request: &EditorOpen) -> Result<(), HostOpFailure> {
            Ok(())
        }
        fn editor_sweep(&self) {}
        fn editor_sync_theme(&self, _theme: ThemeSetting) {}
        fn dbwin_start(&self) {
            self.started.fetch_add(1, Ordering::Relaxed);
        }
        fn dbwin_stop(&self) {
            self.stopped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The capture is a standing request, not a one-shot, so it has to follow the
    /// swap: an open log pane whose helper arrives (or goes) must not quietly lose
    /// half its stream until someone closes and reopens it.
    #[test]
    fn a_running_capture_moves_to_the_incoming_implementation() {
        let local = Arc::new(Recording::default());
        let remote = Arc::new(Recording::default());
        let host = SwappableHostOps::new(local.clone());

        host.dbwin_start();
        assert_eq!(local.started.load(Ordering::Relaxed), 1);

        host.install(remote.clone());
        assert_eq!(local.stopped.load(Ordering::Relaxed), 1);
        assert_eq!(remote.started.load(Ordering::Relaxed), 1);

        // And a capture nobody asked for is not started by a swap.
        host.dbwin_stop();
        host.install(local.clone());
        assert_eq!(local.started.load(Ordering::Relaxed), 1);
    }

    /// Host operations that record each capture call AS IT COMPLETES, and can be
    /// made to park inside a start. Recording on the way out is what makes the
    /// order meaningful: a hand-over that ran through the middle of a start shows
    /// up as a start completing after the stop that was supposed to follow it.
    struct Ordered {
        name: &'static str,
        events: Arc<Mutex<Vec<String>>>,
        entered: Option<std::sync::mpsc::Sender<()>>,
        park: Option<Mutex<std::sync::mpsc::Receiver<()>>>,
    }

    impl Ordered {
        fn record(&self, what: &str) {
            self.events
                .lock()
                .expect("the event log")
                .push(format!("{}:{what}", self.name));
        }
    }

    impl HostOps for Ordered {
        fn seed_mods_runtime(&self) {}
        fn editor_open(&self, _request: &EditorOpen) -> Result<(), HostOpFailure> {
            Ok(())
        }
        fn editor_sweep(&self) {}
        fn editor_sync_theme(&self, _theme: ThemeSetting) {}
        fn dbwin_start(&self) {
            if let Some(entered) = &self.entered {
                let _ = entered.send(());
            }
            if let Some(park) = &self.park {
                let _ = park.lock().expect("the release lock").recv();
            }
            self.record("start");
        }
        fn dbwin_stop(&self) {
            self.record("stop");
        }
    }

    /// A swap is atomic from a caller's view, and the capture is part of what it
    /// swaps: a start that is still in flight when the helper arrives must finish
    /// against the implementation it was made on, and be handed over after -
    /// never left running on the one the swap retired.
    #[test]
    fn an_install_does_not_interleave_with_a_capture_start() {
        use std::sync::mpsc::channel;

        let events = Arc::new(Mutex::new(Vec::new()));
        let (entered_tx, entered) = channel();
        let (release_tx, release) = channel();
        let local = Arc::new(Ordered {
            name: "local",
            events: Arc::clone(&events),
            entered: Some(entered_tx),
            park: Some(Mutex::new(release)),
        });
        let remote = Arc::new(Ordered {
            name: "remote",
            events: Arc::clone(&events),
            entered: None,
            park: None,
        });
        let host = Arc::new(SwappableHostOps::new(local));

        let starting = Arc::clone(&host);
        let start = std::thread::spawn(move || starting.dbwin_start());
        entered
            .recv()
            .expect("the start reached the implementation");

        let installing = Arc::clone(&host);
        let (installed_tx, installed) = channel();
        let install = std::thread::spawn(move || {
            installing.install(remote);
            let _ = installed_tx.send(());
        });
        assert!(
            installed.recv_timeout(Duration::from_millis(100)).is_err(),
            "the swap ran while the start it has to follow was still in flight"
        );

        let _ = release_tx.send(());
        start.join().expect("the start thread");
        install.join().expect("the install thread");

        assert_eq!(
            *events.lock().expect("the event log"),
            ["local:start", "local:stop", "remote:start"],
        );
    }

    /// Reinstalling what is already there is ordinary - degraded mode reaches it
    /// from a failed Retry - and must not churn the capture underneath an open
    /// pane.
    #[test]
    fn reinstalling_the_same_implementation_leaves_the_capture_alone() {
        let local = Arc::new(Recording::default());
        let host = SwappableHostOps::new(local.clone());

        host.dbwin_start();
        host.install(local.clone());

        assert_eq!(local.started.load(Ordering::Relaxed), 1);
        assert_eq!(local.stopped.load(Ordering::Relaxed), 0);
    }
}
