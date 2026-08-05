//! The runtime broker: an elevated second instance of this executable that owns
//! the one privileged core session, and everything the UI side does about it.
//!
//! The UI runs unelevated, so the session it issues commands against lives in
//! the broker. [`BrokerLink`] is what holds the two together: it starts the
//! elevation ladder, installs the remote session behind the seam every handler
//! already reaches through, and - when there is no broker or the channel breaks
//! - puts the local session back and says so.
//!
//! Losing the broker is not fatal and never has been designed to be. Reads
//! succeed unelevated, so a Windhawk that cannot elevate is a read-only Windhawk
//! with a banner explaining why, and every write fails with the core's own
//! access-denied error rather than with silence.
//!
//! ```text
//!   windhawk-ui.exe (medium)              windhawk-ui.exe --runtime-broker (high)
//!     window, WebView2, handlers            no window, no webview
//!     the pipe LISTENER  <---- connects out ----  the pipe CLIENT
//!     GatedCore (stateless invokes)         GatedCore + the privileged session
//! ```
//!
//! The inversion is the security argument: there is no elevated endpoint for an
//! arbitrary process to connect to, and the elevated process only ever connects to
//! an endpoint it has verified.

pub mod launch;
// The privileged host operations and the channel's vocabulary, and the UI end
// that speaks it. Public because they ARE the contract with the other process,
// and because the two-process test drives a channel it accepted itself - under
// a policy it constructs as a plain value, which is what keeps the shipping
// checks free of a test escape hatch.
pub mod ops;
pub mod remote;
mod serve;
mod swappable;
pub mod wire;

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use windhawk_broker::{Listener, Requester};
use windhawk_core_host::windhawk_ini::is_portable;
use windhawk_core_host::{Session, SessionApi};

use crate::lifecycle::window;
use crate::pump::PumpMessage;
use crate::pump::ops::FIRST_GENERATION;

use launch::{BrokerProcess, Elevated, prompt_gate};
use ops::{HostOps, RemoteHostOps};
use remote::{ChannelSink, RemoteSession};
use swappable::{Settled, SwappableHostOps, SwappableSession};
use wire::{Channel, Request};

pub use launch::PromptGate;
pub use serve::{ExitCode, run_broker};

/// Whether the ladder may raise its consent dialog yet, answered by the startup
/// path (`PromptGate`). Re-exported here because it is answered from where a
/// launch succeeds or fails rather than from anywhere that knows about rungs.
pub fn elevation_prompt_gate() -> &'static PromptGate {
    prompt_gate()
}

/// The channel name's fixed part. The random half is generated per launch, never
/// reused, and never written to disk; it is not relied on as a secret (the pipe's
/// descriptor and the peer checks are what keep strangers out) but it does stop
/// the name being squatted ahead of the process that means to create it.
const CHANNEL_PREFIX: &str = "Windhawk.Broker";

/// The event the injected banner listens on, and the state it carries.
const BROKER_EVENT: &str = "wh-broker";

/// The banner states the front-end branches on. A window that is not waiting for
/// a helper and one that has its helper are different facts, so they are
/// different names even though the banner shows nothing for either.
const LOCAL: &str = "local";
const LIVE: &str = "live";
/// A helper is being started, and it is too early for that to be worth
/// mentioning - which is every ordinary launch.
const STARTING: &str = "starting";
/// A helper is being started and the user is waiting on it.
const CONNECTING: &str = "connecting";
const DEGRADED: &str = "degraded";

/// How long a launch may spend getting its helper before the window says anything
/// about it.
///
/// The channel normally wins its race against WebView2 comfortably, so a banner
/// that appeared the moment a connect began would flash on every healthy launch
/// and mean nothing. What it is worth announcing is a connect that has taken long
/// enough for the user to have noticed the window is not working yet - a task rung
/// that produced nothing and fell through to a prompt, say. A Retry skips this
/// window entirely: there the user pressed a button and is owed an answer.
const QUIET_CONNECT: Duration = Duration::from_secs(5);

/// How long the broker is given to acknowledge the shutdown, and then to exit.
///
/// Both are courtesies on the way out of a process that is exiting anyway: the
/// broker's read loop ends when this process does, whatever happens here, and
/// the installer waits the broker out itself.
///
/// Short, because of where they are spent. [`BrokerLink::shutdown`] runs on the
/// event-loop thread from the window's `Destroyed` handler - the window is
/// already gone, this process still holds the single-instance state, and the
/// fatal box for a close nobody asked for waits behind it. A healthy helper
/// needs a fraction of either: the shutdown request is served off its reader
/// thread rather than queued, and its exit follows immediately.
const SHUTDOWN_ACK: Duration = Duration::from_secs(2);
const SHUTDOWN_EXIT: Duration = Duration::from_secs(2);

/// How often the shutdown looks at whether the channel has ended, when that is
/// the only thing there is to look at.
const CHANNEL_END_POLL: Duration = Duration::from_millis(20);

/// What the drained ops are failed with when a session hands over. It reaches the
/// front-end through the reply shaping's mapping for a transport failure, so an
/// operation that was in flight ends with an error rather than never ending.
/// Also what an op that was STARTING across the hand-over is failed with (the
/// bridge ends it there, since the drain that ran cannot have seen it).
pub(crate) const HANDOVER_REASON: &str =
    "the operation was cancelled because Windhawk changed which session serves it";
const LOST_REASON: &str = "the connection to the elevated Windhawk helper was lost";

/// Whether this process needs an elevated helper to do its job.
///
/// Both inputs are available with no DLL, no session, and no invoke, which is the
/// point: deciding first is what lets the elevation ladder start before the core
/// is even loaded and overlap everything after it. The core stays the
/// authority on storage resolution; this reads the one boolean the process edge
/// already reads the same way.
///
/// - A PORTABLE install keeps its data inside the install directory and writes no
///   registry, so it runs unelevated exactly as it does today.
/// - An already ELEVATED UI can do the work itself - a developer run, a user who
///   chose "Run as administrator", or a launcher path that started this process
///   elevated for reasons of its own.
///
/// It deliberately does not ask whether the current user CAN elevate. For a
/// standard user the task rung still runs and produces a broker that is not
/// elevated, which the peer policy rejects and which then exits; the ladder falls
/// through to the prompt, an administrator supplies credentials, and the broker
/// comes up on that account. Asking up front would mean predicting the
/// answer from a linked token that does not always exist.
pub fn needs_broker(app_root: &str) -> bool {
    !is_portable(app_root) && !window::is_running_as_admin()
}

/// The full pipe name of a channel.
pub(crate) fn channel_pipe_name(channel: &str) -> String {
    format!(r"\\.\pipe\{CHANNEL_PREFIX}.{channel}")
}

/// Whether `channel` is a name this build issues. Checked on the way IN, because
/// the broker takes it off a command line and builds kernel object names from it.
pub(crate) fn is_channel_token(channel: &str) -> bool {
    channel.len() == 32 && channel.bytes().all(|b| b.is_ascii_hexdigit())
}

/// An elevation ladder in flight: the listener is up, the channel name is issued,
/// and a thread is working through the ways of getting a broker onto it.
pub struct Ladder {
    thread: std::thread::JoinHandle<Result<Elevated, String>>,
}

impl Ladder {
    /// Create the listener and start the ladder against it.
    ///
    /// The listener exists before the name is handed to anyone, and it is created
    /// with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a name already in use is a
    /// squatter to be detected rather than a peer to be served.
    ///
    /// Starting it starts the SILENT rung. Its prompt rung waits on the process
    /// gate, which is what keeps a consent dialog from racing the window build
    /// (`launch::PromptGate`).
    pub fn start() -> Result<Ladder, String> {
        let (channel, listener) = listen().map_err(|error| {
            format!("the channel to the elevated Windhawk helper could not be created: {error}")
        })?;
        let thread = std::thread::Builder::new()
            .name("wh-broker-ladder".to_owned())
            .spawn(move || launch::establish(listener, &channel, prompt_gate()))
            .map_err(|error| format!("the elevation could not be started: {error}"))?;
        Ok(Ladder { thread })
    }

    fn join(self) -> Result<Elevated, String> {
        self.thread
            .join()
            .unwrap_or_else(|_| Err("the elevation ladder panicked".to_owned()))
    }
}

/// Create the listener on a fresh, single-use channel name, and return the name
/// to hand to whoever is being asked to connect.
///
/// The listener exists before the name does anything, which is what makes the
/// name unsquattable rather than secret.
pub fn listen() -> io::Result<(String, Listener)> {
    let name = windhawk_broker::channel_name(CHANNEL_PREFIX)?;
    let channel = name
        .rsplit('.')
        .next()
        .expect("a channel name ends in its random half")
        .to_owned();
    let listener = Listener::with_name(&name, wire::channel_config())?;
    Ok((channel, listener))
}

/// The UI's link to its privileged helper: the session seam, the channel behind
/// it, and the state the banner reports.
pub struct BrokerLink {
    /// What every handler invokes through. Which session is behind it is this
    /// type's business and nobody else's.
    session: Arc<SwappableSession>,
    /// What every privileged side effect goes through, on the same terms.
    host: Arc<SwappableHostOps>,
    /// The local session, kept for the process lifetime. It is what serves
    /// the startup reads before the channel arrives, and what a lost channel falls
    /// back to - so degraded mode is one state reached two ways rather than two
    /// states with different capabilities.
    local: Arc<Session>,
    /// The host operations performed in THIS process, kept for the same reason and
    /// put back in the same places.
    local_host: Arc<dyn HostOps>,
    pump: Sender<PumpMessage>,
    /// Whether a broker is wanted at all. False for a portable install and for an
    /// already elevated UI, where there is nothing to degrade FROM.
    wanted: bool,
    /// The generation of the next session. One per session, for that session's
    /// whole life: the local session keeps [`FIRST_GENERATION`] forever, including
    /// across a swap away and back.
    generations: AtomicU64,
    /// The session behind the seam is the one this process is going to run on.
    /// Held apart from [`State`] because it is what the SEAM waits on (an async
    /// op start holds behind it), and because it is one way: every other field
    /// here changes back and forth with the channel.
    settled: Arc<Settled>,
    state: Mutex<State>,
    /// The window, once it exists. The banner cannot be told anything before then,
    /// so the state is also readable on demand (`wh_broker_state`).
    app: OnceLock<AppHandle>,
}

#[derive(Default)]
struct State {
    /// The channel, while there is one.
    live: Option<Live>,
    /// Why there is no channel.
    degraded: Option<String>,
    /// When the ladder run in flight started, if one is.
    connecting_since: Option<Instant>,
    /// That run was ASKED for (the banner's Retry) rather than being part of
    /// starting up, so the user is watching it and it is announced at once.
    retrying: bool,
}

/// A live channel and everything that has to outlive a single request on it.
struct Live {
    generation: u64,
    requester: Arc<Requester<Channel>>,
    /// The broker process, when the rung that started it could report one.
    process: Option<BrokerProcess>,
}

impl BrokerLink {
    /// Take over a ladder (or the absence of one) and start watching it.
    ///
    /// `ladder` is `None` when no broker is wanted, and `Some(Err(..))` when one
    /// is wanted and could not even be started - which is degraded mode, reached
    /// before the ladder rather than by it.
    pub fn new(
        local: Arc<Session>,
        local_host: Arc<dyn HostOps>,
        pump: Sender<PumpMessage>,
        ladder: Option<Result<Ladder, String>>,
    ) -> Arc<BrokerLink> {
        // An op start holds for the settle point for as long as the launch is
        // still expected to be an ordinary one - the same window the banner stays
        // quiet for, and for the same reason: past it, the helper is not moments
        // away and the UI stops arranging itself around one that is.
        let settled = Arc::new(Settled::new(QUIET_CONNECT));
        let link = Arc::new(BrokerLink {
            session: Arc::new(SwappableSession::new(local.clone(), Arc::clone(&settled))),
            host: Arc::new(SwappableHostOps::new(Arc::clone(&local_host))),
            local,
            local_host,
            pump,
            wanted: ladder.is_some(),
            // The local session holds FIRST_GENERATION, so every remote one takes
            // a generation above it.
            generations: AtomicU64::new(FIRST_GENERATION + 1),
            settled,
            state: Mutex::new(State::default()),
            app: OnceLock::new(),
        });

        match ladder {
            // Nothing to wait for: this process does its own privileged work.
            None => link.settled.reach(),
            Some(Err(reason)) => link.degraded(reason),
            Some(Ok(ladder)) => {
                link.lock().connecting_since = Some(Instant::now());
                link.announce_if_slow();
                let watching = Arc::clone(&link);
                let watcher = std::thread::Builder::new()
                    .name("wh-broker-link".to_owned())
                    .spawn(move || watching.adopt(ladder.join()));
                // Nothing else will ever settle the link, so a thread that could
                // not start has to say so rather than leave every waiter parked.
                if let Err(error) = watcher {
                    link.degraded(format!("the elevation could not be watched: {error}"));
                }
            }
        }
        link
    }

    /// The seam to hand the bridge. Every handler reaches the core through this
    /// and cannot tell which session is behind it.
    pub fn session(&self) -> Arc<dyn SessionApi> {
        Arc::clone(&self.session) as Arc<dyn SessionApi>
    }

    /// The host-operation seam to hand the bridge, on the same terms.
    pub fn host(&self) -> Arc<dyn HostOps> {
        Arc::clone(&self.host) as Arc<dyn HostOps>
    }

    /// Give the link the window, so state changes reach the banner as they happen.
    /// Whatever happened before this is not lost: the front-end asks for the
    /// current state when it loads.
    pub fn attach(&self, app: AppHandle) {
        let _ = self.app.set(app);
        self.announce();
    }

    /// Wait until the session behind the seam is the one this process is going to
    /// run on - the swap, or degraded mode, whichever comes first.
    ///
    /// Everything the UI starts for itself waits here, and waits UNBOUNDED,
    /// unlike the seam's own hold on an async op start: nobody is looking at this
    /// work, so there is nothing for it to be late for. Not a nicety: the startup
    /// catalog refresh ends in a profile WRITE, so left where it was it would be
    /// issued against the local session on every launch, in the window before the
    /// broker arrives, and would either fail unelevated or be drained by the swap -
    /// a guaranteed per-launch failure on the one path that is supposed to be
    /// indistinguishable from today.
    pub fn wait_until_settled(&self) {
        self.settled.wait();
    }

    /// Run the whole ladder again, from a fresh channel name (the Retry the banner
    /// offers). The window and the front-end state survive it: what changes is
    /// which session is behind the seam.
    ///
    /// The settle point is not un-reached for it, so the swap this may end in is
    /// not one an op start holds for: a Retry is a button the user pressed on a
    /// window they have been using, where an op in flight is work they can point
    /// at and being told it was interrupted is an answer. The hold exists for the
    /// launch, where an op in flight is the page loading itself and the same
    /// message means nothing.
    pub fn retry(self: &Arc<Self>) {
        {
            let mut state = self.lock();
            if !self.wanted || state.connecting_since.is_some() || state.live.is_some() {
                return;
            }
            state.connecting_since = Some(Instant::now());
            // Announced at once rather than after the quiet window: this run is
            // the answer to a button the user just pressed, and a button that
            // appears to do nothing is worse than a slow one.
            state.retrying = true;
            state.degraded = None;
        }
        self.announce();

        let link = Arc::clone(self);
        let running = std::thread::Builder::new()
            .name("wh-broker-retry".to_owned())
            .spawn(move || match Ladder::start() {
                Ok(ladder) => link.adopt(ladder.join()),
                Err(reason) => link.degraded(reason),
            });
        if let Err(error) = running {
            self.degraded(format!("the elevation could not be retried: {error}"));
        }
    }

    /// Tell the broker to stop, and give it a moment to. The channel's EOF would
    /// end it anyway when this process exits; asking first is what keeps an
    /// elevated process from being momentarily orphaned - which also matters to the
    /// installer, whose terminate-wait now enumerates the broker too.
    pub fn shutdown(&self) {
        let live = self.lock().live.take();
        let Some(live) = live else {
            return;
        };
        let _ = live
            .requester
            .request_within(Request::shutdown(), SHUTDOWN_ACK);

        // Wait on the process where there is a handle to wait on. The rung that
        // raised the prompt always has one; the one that triggered the task may
        // not, since opening an elevated process from here is commonly denied.
        let exited = live
            .process
            .as_ref()
            .is_some_and(|process| process.wait_for_exit(SHUTDOWN_EXIT));
        if !exited {
            // The channel says the same thing without a handle: the broker closing
            // its end IS its exit. Only reached when the wait above could not
            // happen or ran out, so the budget is not spent twice.
            wait_for_channel_end(&live.requester, SHUTDOWN_EXIT);
        }
        live.requester.close();
    }

    /// What the banner shows.
    pub fn state_payload(&self) -> Value {
        let state = self.lock();
        let name = banner_state(&state, self.wanted, Instant::now());
        match name {
            DEGRADED => json!({
                "state": DEGRADED,
                "reason": state.degraded.clone().unwrap_or_default(),
            }),
            name => json!({ "state": name }),
        }
    }

    /// Say so if the helper is still not there once the quiet window has passed.
    ///
    /// The state is what decides whether that is worth mentioning
    /// ([`banner_state`]); this only makes sure someone ASKS at the moment the
    /// answer changes, since nothing else happens then - a connect that is still
    /// running produces no event of its own.
    fn announce_if_slow(self: &Arc<Self>) {
        let link = Arc::downgrade(self);
        let _ = std::thread::Builder::new()
            .name("wh-broker-slow".to_owned())
            .spawn(move || {
                std::thread::sleep(QUIET_CONNECT);
                if let Some(link) = link.upgrade()
                    && link.lock().connecting_since.is_some()
                {
                    link.announce();
                }
            });
    }

    /// Take the ladder's answer: start routing on the channel it produced, or
    /// enter degraded mode with the reason it gives.
    fn adopt(self: Arc<BrokerLink>, outcome: Result<Elevated, String>) {
        let elevated = match outcome {
            Ok(elevated) => elevated,
            Err(reason) => return self.degraded(reason),
        };

        serve::report(&adopted_line(
            elevated.channel.peer_pid,
            &elevated.channel.peer_version,
            elevated.channel.integrity_unverified,
        ));

        let generation = self.generations.fetch_add(1, Ordering::Relaxed);
        // The sink signals; it never works. What a lost channel costs - unwinding
        // the operations that were in flight, putting the notice up, falling back
        // to the local session - all needs the pump thread.
        let losing = Arc::downgrade(&self);
        let sink = ChannelSink::new(
            self.pump.clone(),
            generation,
            Box::new(move || {
                let losing = losing.clone();
                PumpMessage::deferred(move |ctx| {
                    if let Some(link) = losing.upgrade() {
                        link.lost(ctx, generation);
                    }
                })
            }),
        );
        let requester = Arc::new(Requester::start(elevated.channel, Arc::new(sink)));
        let session = Arc::new(RemoteSession::new(Arc::clone(&requester)));
        let broker_pid = elevated.process.as_ref().and_then(BrokerProcess::pid);

        // The host operations go over here, without waiting for the pump. They
        // carry no per-session state - no op-ids, no registry, nothing a drain has
        // to unwind - so there is nothing for the pump to make atomic. The SESSION
        // cannot go over here for exactly the reason this one can: its swap is also
        // an op-registry hand-over.
        self.host.install(Arc::new(RemoteHostOps::new(
            Arc::clone(&requester),
            broker_pid,
        )));

        {
            let mut state = self.lock();
            state.live = Some(Live {
                generation,
                requester,
                process: elevated.process,
            });
            state.degraded = None;
            state.connecting_since = None;
            state.retrying = false;
        }

        // The swap itself belongs to the pump thread, which makes it a
        // single-threaded critical section rather than three threads agreeing
        // about ordering.
        let installing = Arc::clone(&self);
        let _ = self.pump.send(PumpMessage::deferred(move |ctx| {
            installing.install(ctx, session, generation);
        }));
    }

    /// Put the broker's session behind the seam (the pump thread).
    ///
    /// The session goes in BEFORE the registry is handed over, and the order is
    /// deliberate: an op started in the window between the two is stamped with the
    /// outgoing generation and is therefore drained and FAILED, which is what
    /// happens to every in-flight op at a swap anyway. The other order would stamp
    /// it with the incoming generation while it ran on the outgoing session, and
    /// its events - carrying the outgoing generation - would be dropped, so it
    /// would never end at all.
    fn install(
        &self,
        ctx: &crate::ipc::bridge::BridgeCtx,
        session: Arc<dyn SessionApi>,
        generation: u64,
    ) {
        // A retry that superseded this one, or a channel that was lost before the
        // pump got here: the session this was going to install is already history.
        if self.lock().live.as_ref().map(|live| live.generation) != Some(generation) {
            return;
        }
        self.session.install(session);
        ctx.hand_over_ops(generation, HANDOVER_REASON);
        // Last of the three, and that is the whole point of the hold: an op start
        // let through here finds the incoming session behind the seam and a
        // hand-over that is already done, so it cannot be ended by the swap that
        // released it.
        self.settled.reach();
        self.announce();
    }

    /// The channel ended (the pump thread). Put the retained local session back,
    /// end everything the broker's session was carrying, and say what happened.
    fn lost(&self, ctx: &crate::ipc::bridge::BridgeCtx, generation: u64) {
        {
            let mut state = self.lock();
            if state.live.as_ref().map(|live| live.generation) != Some(generation) {
                // A channel that was already replaced or shut down.
                return;
            }
            state.live = None;
            state.degraded = Some(LOST_REASON.to_owned());
        }
        self.host.install(Arc::clone(&self.local_host));
        self.session.install(self.local.clone());
        ctx.hand_over_ops(FIRST_GENERATION, LOST_REASON);
        self.settled.reach();
        self.announce();
    }

    /// There is no channel, and this is why.
    fn degraded(&self, reason: String) {
        // Reached from a Retry that failed as well as from the first ladder, so the
        // seam may be holding a remote implementation with nothing behind it.
        self.host.install(Arc::clone(&self.local_host));
        {
            let mut state = self.lock();
            state.live = None;
            state.degraded = Some(reason);
            state.connecting_since = None;
            state.retrying = false;
        }
        self.settled.reach();
        self.announce();
    }

    /// Push the current state to the banner. Best effort: before the window exists
    /// there is nobody to tell, and the front-end asks on load anyway.
    fn announce(&self) {
        if let Some(app) = self.app.get() {
            let _ = app.emit(BROKER_EVENT, self.state_payload());
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// Wait up to `patience` for the peer to close its end of the channel, which is
/// what its exit looks like from here.
///
/// The alternative to a process handle, and the only instrument available when
/// the rung that started the broker could not open one. Polled rather than
/// waited on: the reader thread settles the channel when it reads EOF, and there
/// is nothing to hand this thread a signal about it that would not outlive the
/// use.
fn wait_for_channel_end(requester: &Requester<Channel>, patience: Duration) {
    let deadline = Instant::now() + patience;
    while requester.is_open() && Instant::now() < deadline {
        std::thread::sleep(CHANNEL_END_POLL);
    }
}

/// What the debug stream is told about a channel that was taken: who is on the
/// other end of it, and what could not be established about them.
///
/// The integrity clause is the reason there is a line at all. A peer whose token
/// could not be read is accepted rather than refused - the pipe is reachable by
/// this user alone, so the check is anti-spoofing rather than the privilege
/// boundary - and saying so is the only trace that it did not happen.
fn adopted_line(peer_pid: u32, peer_version: &str, integrity_unverified: bool) -> String {
    let unverified = match integrity_unverified {
        true => "; its token could not be read, so its integrity is unverified",
        false => "",
    };
    format!("connected to the elevated helper (pid {peer_pid}, version {peer_version}){unverified}")
}

/// Which of the banner states the link is in.
///
/// The policy lives here rather than in the page for the same reason the reply
/// shaping lives in Rust: the injected script is a placeholder for a front-end
/// component that will be written elsewhere, and every decision left in it is one
/// that has to be reimplemented when it is. It renders what it is told.
///
/// `now` is a parameter so the quiet window is testable without waiting it out.
fn banner_state(state: &State, wanted: bool, now: Instant) -> &'static str {
    if !wanted {
        return LOCAL;
    }
    if state.live.is_some() {
        return LIVE;
    }
    match state.connecting_since {
        Some(since) if state.retrying || now.saturating_duration_since(since) >= QUIET_CONNECT => {
            CONNECTING
        }
        Some(_) => STARTING,
        // No channel and nothing on its way to one.
        None => DEGRADED,
    }
}

/// What the banner should be showing. Asked for when the front-end loads, since
/// the state can have changed - or settled for good - before the page existed. An
/// app command, not ACL-gated, like the log pane's.
#[tauri::command]
pub fn wh_broker_state(link: tauri::State<'_, Arc<BrokerLink>>) -> Value {
    link.state_payload()
}

/// Run the elevation ladder again (the banner's Retry). Returns immediately: the
/// ladder can involve a consent dialog, and this runs on the thread that serves
/// the webview.
#[tauri::command]
pub fn wh_broker_retry(link: tauri::State<'_, Arc<BrokerLink>>) {
    link.inner().retry();
}

/// The banner the front-end shows while the UI is without its privileged helper -
/// permanently ([`DEGRADED`]) or for now ([`CONNECTING`]) - injected the way the
/// log pane's own front-end pieces are.
///
/// It lives here rather than in the shared React app because that app ships from
/// another repository on another release cycle, and a broker-less run has to be
/// legible in the build that introduces the broker - the whole promise of
/// degrading rather than failing is that the user is told what is going on. The
/// proper home is still the front-end, and this is the placeholder that does not
/// block on it. Everything it would have to decide is decided in
/// [`banner_state`], so the component that replaces it inherits the policy rather
/// than reimplementing it.
pub fn banner_init_script() -> &'static str {
    include_str!("banner.js")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window that wants a helper and is waiting for one, as of `waited`.
    fn connecting(waited: Duration, retrying: bool) -> (State, Instant) {
        let since = Instant::now();
        let state = State {
            connecting_since: Some(since),
            retrying,
            ..State::default()
        };
        (state, since + waited)
    }

    /// The ordinary launch: the connect is under way and the window says nothing
    /// about it, because it is about to finish and a banner on every healthy
    /// launch is a banner nobody reads.
    #[test]
    fn a_connect_that_has_just_started_is_not_announced() {
        let (state, now) = connecting(Duration::from_secs(1), false);

        assert_eq!(banner_state(&state, true, now), STARTING);
    }

    /// Long enough that the user has seen a window which cannot save anything.
    #[test]
    fn a_connect_that_outlasts_the_quiet_window_is_announced() {
        let (state, now) = connecting(QUIET_CONNECT, false);

        assert_eq!(banner_state(&state, true, now), CONNECTING);
    }

    /// A Retry is announced immediately: the user pressed a button, and one that
    /// appears to do nothing for five seconds reads as broken.
    #[test]
    fn a_retry_is_announced_at_once() {
        let (state, now) = connecting(Duration::ZERO, true);

        assert_eq!(banner_state(&state, true, now), CONNECTING);
    }

    /// Nothing in flight and no channel is the state the banner exists for.
    #[test]
    fn no_channel_and_nothing_coming_is_degraded() {
        let state = State {
            degraded: Some("the scheduled task is disabled".to_owned()),
            ..State::default()
        };

        assert_eq!(banner_state(&state, true, Instant::now()), DEGRADED);
    }

    /// A portable install and an already elevated window do their own privileged
    /// work, so a slow connect they are not having is not a state they can be in.
    #[test]
    fn a_window_that_wants_no_helper_says_nothing() {
        let (state, now) = connecting(QUIET_CONNECT, true);

        assert_eq!(banner_state(&state, false, now), LOCAL);
    }

    /// The peer the launch ended up with is named, so a session's helper is
    /// identifiable from the debug stream alone.
    #[test]
    fn an_adopted_channel_names_its_peer() {
        let line = adopted_line(4321, "1.6.0", false);

        assert!(line.contains("4321"), "{line}");
        assert!(line.contains("1.6.0"), "{line}");
    }

    /// The one case the accept degrades on rather than refusing. It is not an
    /// escalation - the pipe is this user's - but it is the anti-spoofing check
    /// not happening, and a silent adoption is what would leave no trace of it.
    #[test]
    fn a_peer_whose_token_could_not_be_read_says_so() {
        let line = adopted_line(4321, "1.6.0", true);

        assert!(line.contains("integrity is unverified"), "{line}");
    }

    #[test]
    fn a_channel_name_and_its_token_agree() {
        let (channel, listener) = listen().expect("a listener on a fresh name");
        assert!(
            is_channel_token(&channel),
            "'{channel}' is not a channel name this build issues"
        );
        assert_eq!(listener.name(), channel_pipe_name(&channel));
    }

    /// The token is used to build kernel object names, so what a peer can put on
    /// the command line is checked rather than trusted.
    #[test]
    fn a_channel_name_that_is_not_ours_is_refused() {
        assert!(!is_channel_token(""));
        assert!(!is_channel_token("../../evil"));
        assert!(!is_channel_token(&"a".repeat(31)));
        assert!(!is_channel_token(&"g".repeat(32)));
        assert!(is_channel_token(&"0123456789abcdef".repeat(2)));
    }
}
