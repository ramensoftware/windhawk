//! The elevation ladder: how the UI gets an elevated broker onto the channel it
//! is already listening on.
//!
//! Two rungs, in the order the C++ launcher has always used for the UI itself:
//! a scheduled task, which is what keeps the everyday launch prompt-free, and
//! then a UAC prompt. Failing both is not fatal - it is degraded mode, and the
//! caller says so with a banner.
//!
//! Two threads, because the accept and the escalation are different jobs. One
//! parks in the accept; this one starts a rung, waits a while, and moves on. What
//! joins them is the terms the accept reads afresh on every attempt: the process
//! this rung started (which the accept binds the peer to) and how long the wait is
//! still worth (which jumps from "under a second" to "however long a person takes
//! to answer a dialog" the moment a prompt goes up). A rung also ends the instant a
//! peer connects and fails the policy - the everyday case there is a standard user,
//! whose task-started broker is not elevated at all, and burning the rung's whole
//! deadline on it would add seconds of dead time to their every launch.
//!
//! The prompt rung waits for one thing outside the ladder: the window. It is the
//! only rung a person answers, and until the window is up there is nothing for its
//! dialog to be owned by and no knowing whether this launch produces a window at
//! all - one that fails to build ends in a message box, and a consent dialog behind
//! that box asks to elevate something nobody is going to see. So the startup path
//! answers a [`PromptGate`] once it knows which way the launch went, and only that
//! rung waits on it: the silent one keeps the whole overlap it always had.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windhawk_broker::{AcceptError, AcceptTerms, Handshaken, Integrity, Listener, PeerPolicy};
use windows::Win32::Foundation::SCHED_E_TASK_DISABLED;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{ITaskService, TASK_RUN_AS_SELF, TaskScheduler};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VariantClear};
use windows::core::BSTR;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE, STILL_ACTIVE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::lifecycle::window;

/// The scheduled task that starts the broker elevated without a prompt. Created
/// by the installer, ACL'd for Authenticated Users, and toggled by the existing
/// "Require UAC elevation to run Windhawk" setting.
const BROKER_TASK: &str = "WindhawkRunBrokerTask";

/// How long the task rung is given. It triggers a local service and the broker it
/// starts has to load the core and create its session before it says hello, so
/// this is not sub-second - but it is short enough that a rung which produced
/// nothing does not hold up the escalation.
const TASK_RUNG_WAIT: Duration = Duration::from_secs(8);

/// How long a consent dialog may sit unanswered. Bounded by a person, so the only
/// wrong value is one short enough to give up while they are still reading it.
const PROMPT_WAIT: Duration = Duration::from_secs(300);

/// How long a broker that has definitely been STARTED is given to appear. Both
/// rungs use it once they know something is coming.
const CONNECT_WAIT: Duration = Duration::from_secs(30);

/// How long a peer that has connected gets to complete the handshake. Separate
/// from the connect deadline so a peer arriving just before that deadline is not
/// rejected for being late rather than for being wrong.
const HANDSHAKE_WINDOW: Duration = Duration::from_secs(15);

/// How often a rung that holds a process handle looks at whether the broker it
/// started is still alive. Only reached while nothing else is happening, and only
/// on a rung that could open a handle at all.
const EXIT_POLL: Duration = Duration::from_millis(250);

/// How often the wait for the window looks at whether it may prompt yet. Short,
/// because whatever it adds is added to the one rung a person is waiting through,
/// and cheap, because the wait is normally over in about the time the webview
/// takes to build.
const PERMISSION_POLL: Duration = Duration::from_millis(50);

/// The margin between the driver's own per-rung timer and the deadline the accept
/// is holding, so the accept cannot give up in the instant the driver is about to
/// escalate. The accept is the one that cannot be restarted - it consumes the
/// listener - so it must always outlive the driver's patience.
const LADDER_SLACK: Duration = Duration::from_secs(5);

/// Whether the prompt rung may raise its consent dialog.
///
/// Answered by the startup path, waited on by the ladder. Three states rather
/// than two because "not yet" and "never" are different instructions: the first
/// is what the whole of a healthy startup looks like, and the second is what ends
/// the ladder instead of leaving it parked behind a window that is not coming.
pub struct PromptGate(AtomicU8);

/// Not yet: it is not known whether there will be a window to own a dialog.
const PENDING: u8 = 0;
/// A dialog may go up.
const ALLOWED: u8 = 1;
/// There will be no window, so there is nothing to prompt for.
const ABANDONED: u8 = 2;

impl PromptGate {
    const fn new() -> PromptGate {
        PromptGate(AtomicU8::new(PENDING))
    }

    /// Let the ladder raise its consent dialog.
    ///
    /// Said once the main window exists - which is what the dialog is owned by
    /// and modal to, and what proves this launch is not about to end in a message
    /// box instead. Nothing on the way to that window needs the helper, so there
    /// is no launch that has to ask sooner.
    pub fn allow(&self) {
        self.answer(ALLOWED);
    }

    /// Tell the ladder no dialog will be wanted: this launch ends with a message
    /// rather than a window.
    pub fn abandon(&self) {
        self.answer(ABANDONED);
    }

    /// The first answer wins. A launch that has already asked for a prompt cannot
    /// take the request back - by then the dialog may be on screen, and a second
    /// answer would only describe the ladder's state wrongly.
    fn answer(&self, state: u8) {
        let _ = self
            .0
            .compare_exchange(PENDING, state, Ordering::Release, Ordering::Relaxed);
    }

    fn state(&self) -> u8 {
        self.0.load(Ordering::Acquire)
    }
}

/// The gate this process's startup answers and its ladder waits on.
///
/// A `static` rather than a value threaded through the startup path, for the same
/// reason the startup watchdog's suppression flag is one: what answers it is
/// wherever a launch ends up, and every one of those places would otherwise have
/// to be handed it.
static PROMPT_GATE: PromptGate = PromptGate::new();

/// The gate for this process. A retry raised from the banner shares it, and finds
/// it already answered - the window it was pressed in is the answer.
pub fn prompt_gate() -> &'static PromptGate {
    &PROMPT_GATE
}

/// A verified channel and, when the rung that started it could report one, the
/// process on the other end of it.
pub struct Elevated {
    pub channel: Handshaken,
    pub process: Option<BrokerProcess>,
}

/// The broker process, as much of it as the rung that started it could hand back.
///
/// The prompt rung gets a real handle (`SEE_MASK_NOCLOSEPROCESS`); the task rung
/// gets a pid at best, and opening it is a medium-integrity process opening an
/// elevated one, which may simply be denied. So both halves are optional, and
/// everything they buy - the shutdown wait, the exit code behind a failed launch -
/// is a courtesy rather than a requirement.
pub struct BrokerProcess {
    pid: u32,
    /// A `SYNCHRONIZE`-capable handle, or 0.
    handle: isize,
}

impl BrokerProcess {
    fn from_handle(handle: isize, pid: u32) -> BrokerProcess {
        BrokerProcess { pid, handle }
    }

    /// A process a rung could only name, not hand over.
    ///
    /// It tries for a handle anyway, because the shutdown wait is worth having on
    /// this rung too - but it is a medium-integrity process opening an elevated
    /// one, which is commonly denied (a filtered admin token carries
    /// Administrators as deny-only, and an elevated process's default DACL grants
    /// that group rather than the user). `SYNCHRONIZE` alone, since that is the
    /// only right the wait needs and asking for more can only make the open fail;
    /// reading an exit code stays the prompt rung's, which holds a real handle.
    /// Without the open the caller observes the CHANNEL instead, which needs no
    /// handle at all.
    fn from_pid(pid: u32) -> BrokerProcess {
        // SAFETY: OpenProcess only opens a handle, and a null return is the
        // ordinary answer here rather than an error to report. The handle is
        // owned by this struct and closed once, in `Drop`.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        BrokerProcess {
            pid,
            handle: handle as isize,
        }
    }

    /// The process id, for binding the channel to the peer this ladder started.
    pub fn pid(&self) -> Option<u32> {
        (self.pid != 0).then_some(self.pid)
    }

    /// Whether there is a handle to wait on or ask about.
    fn watchable(&self) -> bool {
        self.handle != 0
    }

    /// Wait up to `patience` for the process to exit, reporting whether it did.
    ///
    /// A courtesy that avoids a momentarily orphaned elevated process, not a
    /// correctness requirement. `false` means there was nothing to wait on, or the
    /// wait ran out; the caller then watches the channel instead, since the broker
    /// closing its end is its exit either way.
    pub fn wait_for_exit(&self, patience: Duration) -> bool {
        if !self.watchable() {
            return false;
        }
        // SAFETY: `handle` is the process handle this struct owns, closed only in
        // `Drop`; the timeout is in milliseconds.
        let waited =
            unsafe { WaitForSingleObject(self.handle as HANDLE, patience.as_millis() as u32) };
        waited == WAIT_OBJECT_0
    }

    /// The exit code the broker ended with, if it has ended and this rung holds a
    /// handle to ask. That is what lets the prompt rung report WHICH pre-handshake
    /// failure happened instead of "no channel".
    pub fn exit_code(&self) -> Option<u32> {
        if !self.watchable() {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: `handle` is this struct's process handle; `code` receives the
        // exit code and is only read when the call succeeds.
        let ok = unsafe { GetExitCodeProcess(self.handle as HANDLE, &mut code) };
        (ok != 0 && code != STILL_ACTIVE as u32).then_some(code)
    }
}

impl Drop for BrokerProcess {
    fn drop(&mut self) {
        if self.handle != 0 {
            // SAFETY: the handle came from ShellExecuteExW's
            // SEE_MASK_NOCLOSEPROCESS and is closed exactly once.
            unsafe { CloseHandle(self.handle as HANDLE) };
        }
    }
}

/// Run the ladder against a listener that is already up, and return the channel it
/// produced.
///
/// The listener is created by the caller and BEFORE the channel name is disclosed
/// to anyone, which is what makes the name unsquattable rather than secret.
pub fn establish(listener: Listener, channel: &str, gate: &PromptGate) -> Result<Elevated, String> {
    let terms = Arc::new(Mutex::new(AcceptTerms {
        policy: policy(None),
        connect_deadline: Instant::now() + TASK_RUNG_WAIT + LADDER_SLACK,
        handshake: HANDSHAKE_WINDOW,
    }));
    let (notes, acceptor) = spawn_acceptor(listener, Arc::clone(&terms));

    let mut refused = Vec::new();
    // The task rung's broker, kept for as long as the ladder might still end on
    // its channel: one that connects late - while the rung below waits for the
    // window - is still this process, and the handle is what the shutdown wait and
    // the exit-code readout need.
    let mut task_process = None;

    // Rung 1: the scheduled task, which elevates without a prompt. A task that is
    // missing or disabled fails immediately and costs nothing.
    match run_task(channel) {
        Ok(pid) => {
            let process = pid.map(BrokerProcess::from_pid);
            bind(&terms, pid);
            match await_peer(&notes, TASK_RUNG_WAIT, process.as_ref()) {
                Peer::Channel(channel) => return Ok(Elevated { channel, process }),
                Peer::Gone(reason) => return Err(reason),
                Peer::Retired(reason) => refused.push(format!("the scheduled task {reason}")),
                Peer::Exited(code) => refused.push(format!(
                    "the broker the scheduled task started exited with code {code}"
                )),
                Peer::Elapsed => {
                    refused.push("the scheduled task started no broker".to_owned());
                }
            }
            task_process = process;
        }
        Err(reason) => refused.push(format!("the scheduled task {reason}")),
    }

    // Rung 2: the prompt, which waits for a window to own it.
    match await_permission(&notes, &terms, gate) {
        Permitted::Prompt => {}
        Permitted::Channel(channel) => {
            return Ok(Elevated {
                channel,
                process: task_process,
            });
        }
        Permitted::Gone(reason) => return Err(reason),
        Permitted::Abandoned(reason) => {
            refused.push(reason);
            return finish(&terms, notes, acceptor, refused).map(|channel| Elevated {
                channel,
                process: task_process,
            });
        }
    }

    // The accept's deadline is extended BEFORE the dialog goes up, not after it is
    // answered: this thread is inside `ShellExecuteExW` for the whole of it and
    // cannot extend anything from there.
    extend(&terms, PROMPT_WAIT + CONNECT_WAIT);
    let process = match run_prompt(channel) {
        Ok(process) => process,
        Err(reason) => {
            refused.push(reason);
            return finish(&terms, notes, acceptor, refused).map(|channel| Elevated {
                channel,
                process: task_process,
            });
        }
    };
    bind(&terms, process.pid());
    extend(&terms, CONNECT_WAIT);
    let waited = match await_peer(&notes, CONNECT_WAIT, Some(&process)) {
        Peer::Channel(channel) => {
            return Ok(Elevated {
                channel,
                process: Some(process),
            });
        }
        Peer::Gone(reason) => return Err(reason),
        Peer::Retired(reason) => format!("the elevated helper {reason}"),
        // The prompt rung holds a real process handle, so a broker that failed
        // before it could say anything still says WHICH failure it was - and says
        // it when it happens rather than when the wait runs out.
        Peer::Exited(code) => format!("the elevated helper exited with code {code}"),
        Peer::Elapsed => "the elevated helper never connected".to_owned(),
    };
    refused.push(waited);
    finish(&terms, notes, acceptor, refused).map(|channel| Elevated {
        channel,
        process: Some(process),
    })
}

/// What a peer has to satisfy, which is what the whole ladder is trying to
/// produce: an ELEVATED helper, in this logon session, and - when the rung could
/// say which process it started - that process.
///
/// There is no user check and its absence is the design: a standard user
/// elevates by supplying an administrator's credentials, which puts the broker on
/// a different account. The image is not checked either, for a duller reason: this
/// side is the pipe server and has no handle to the peer's process to ask with.
fn policy(expected_pid: Option<u32>) -> PeerPolicy {
    PeerPolicy {
        integrity: Integrity::HIGH,
        same_session: true,
        same_image: false,
        expected_pid,
    }
}

/// What the acceptor thread reports back. `Rejected` arrives as it happens, so a
/// rung that has been definitively answered does not have to wait its deadline out.
enum Note {
    Rejected(String),
    Done(Result<Handshaken, AcceptError>),
}

/// What the driver found while waiting for this rung's peer.
enum Peer {
    Channel(Handshaken),
    /// A peer connected and was turned away; this rung is spent.
    Retired(String),
    /// The broker this rung started is gone without ever connecting. Its exit code
    /// says which pre-handshake failure it was, which is the whole reason the
    /// broker has a distinct code per failure.
    Exited(u32),
    /// The rung's own window elapsed with nothing connecting.
    Elapsed,
    /// The accept itself is over. It consumed the listener, so there is no next
    /// rung to try.
    Gone(String),
}

/// What the wait for the window ended with.
enum Permitted {
    /// A dialog may go up.
    Prompt,
    /// A broker connected while waiting, so there is nothing left to prompt for.
    Channel(Handshaken),
    /// There will be no window to own a dialog, and so no prompt rung.
    Abandoned(String),
    /// The accept itself is over.
    Gone(String),
}

/// Wait until the window says a consent dialog may go up.
///
/// Two things make this a poll rather than a park. The accept is re-extended on
/// every pass, because this wait is bounded by the window build rather than by
/// anything this thread holds, and the accept - which cannot be restarted - must
/// always outlive the driver's patience. And the notes are watched alongside the
/// gate, because a rung that produced nothing inside its own deadline can still
/// produce a broker while this waits, and taking that channel is better than
/// putting a dialog up for a second one.
fn await_permission(
    notes: &Receiver<Note>,
    terms: &Mutex<AcceptTerms>,
    gate: &PromptGate,
) -> Permitted {
    loop {
        match gate.state() {
            ALLOWED => return Permitted::Prompt,
            ABANDONED => {
                return Permitted::Abandoned(
                    "no elevated helper was asked for: the window never came up".to_owned(),
                );
            }
            _ => {}
        }
        extend(terms, PERMISSION_POLL + LADDER_SLACK);
        match notes.recv_timeout(PERMISSION_POLL) {
            Ok(Note::Done(Ok(channel))) => return Permitted::Channel(channel),
            Ok(Note::Done(Err(error))) => return Permitted::Gone(error.to_string()),
            // No rung is in flight for a refusal to retire, so a peer turned away
            // during the wait changes nothing about what happens after it.
            Ok(Note::Rejected(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Permitted::Gone("the channel listener stopped".to_owned());
            }
        }
    }
}

fn spawn_acceptor(
    listener: Listener,
    terms: Arc<Mutex<AcceptTerms>>,
) -> (Receiver<Note>, JoinHandle<()>) {
    let (sender, notes) = channel();
    let rejections: Sender<Note> = sender.clone();
    let thread = std::thread::Builder::new()
        .name("wh-broker-accept".to_owned())
        .spawn(move || {
            let outcome = listener.accept(
                &|| {
                    terms
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone()
                },
                &|rejection| {
                    let _ = rejections.send(Note::Rejected(rejection.to_string()));
                },
            );
            let _ = sender.send(Note::Done(outcome));
        })
        .expect("spawn the broker accept thread");
    (notes, thread)
}

/// Wait `patience` for this rung to produce the peer.
fn await_peer(notes: &Receiver<Note>, patience: Duration, watch: Option<&BrokerProcess>) -> Peer {
    // Only slice the wait when there is a handle to check: a rung that could not
    // open one has nothing to learn from waking up.
    let watch = watch.filter(|process| process.watchable());
    let deadline = Instant::now() + patience;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Peer::Elapsed;
        }
        let slice = match watch {
            Some(_) => remaining.min(EXIT_POLL),
            None => remaining,
        };
        match notes.recv_timeout(slice) {
            Ok(Note::Done(Ok(channel))) => return Peer::Channel(channel),
            Ok(Note::Done(Err(error))) => return Peer::Gone(error.to_string()),
            Ok(Note::Rejected(reason)) => return Peer::Retired(format!("was refused: {reason}")),
            Err(RecvTimeoutError::Timeout) => {
                if let Some(code) = watch.and_then(BrokerProcess::exit_code) {
                    return Peer::Exited(code);
                }
            }
            // The acceptor thread is gone without a word, which it cannot be.
            Err(RecvTimeoutError::Disconnected) => {
                return Peer::Gone("the channel listener stopped".to_owned());
            }
        }
    }
}

/// Bind the accept to the process this rung started, when it could report one.
/// Complementary evidence rather than a substitute for the integrity check: it
/// says nothing about privilege, but it answers a sharper question than a token
/// can - not "some elevated process" but "the process I asked for".
fn bind(terms: &Mutex<AcceptTerms>, pid: Option<u32>) {
    terms
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .policy = policy(pid);
}

/// Give the accept `patience` more. It reads the deadline afresh while parked, so
/// this releases nothing and leaves no gap for a peer to arrive into unseen.
fn extend(terms: &Mutex<AcceptTerms>, patience: Duration) {
    terms
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .connect_deadline = Instant::now() + patience;
}

/// End the ladder: stop the accept, and report either the channel that arrived
/// after all or why there is none.
///
/// The accept is stopped by collapsing its deadline rather than by signalling the
/// pipe's shutdown, and the difference matters: the shutdown event belongs to the
/// PIPE, so signalling it would kill a channel the accept may have completed in
/// the instant this thread gave up. Collapsing the deadline lets the accept finish
/// what it is doing and hand it over, which is why the last thing this does is
/// look for a channel that beat the decision to stop waiting for one.
///
/// The price is that the join is bounded by the accept's work rather than by the
/// deadline: a peer already INSIDE the handshake holds its own [`HANDSHAKE_WINDOW`]
/// (15 s), which the collapsed connect deadline does not reach, so the teardown
/// can take that long. It is worth paying - it is the failure path, and letting
/// the handshake finish is what turns a peer that arrived at the last moment into
/// the channel this returns rather than into a rejection.
fn finish(
    terms: &Mutex<AcceptTerms>,
    notes: Receiver<Note>,
    acceptor: JoinHandle<()>,
    refused: Vec<String>,
) -> Result<Handshaken, String> {
    terms
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .connect_deadline = Instant::now();
    let _ = acceptor.join();
    for note in notes.try_iter() {
        if let Note::Done(Ok(channel)) = note {
            return Ok(channel);
        }
    }
    Err(refused.join("; "))
}

/// Trigger the scheduled task, handing it the channel as its parameter. Returns
/// the pid it reports, which it may not have (the task may not have started yet
/// when it is asked).
fn run_task(channel: &str) -> Result<Option<u32>, String> {
    let _com = ComThread::enter()?;

    // SAFETY: every call below is a COM call on an interface obtained from the
    // one before it, each checked before it is used. `params` owns its BSTR until
    // it is cleared, after the call that reads it.
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_ALL)
            .map_err(|error| format!("could not be reached: {error}"))?;
        service
            .Connect(
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
                &VARIANT::default(),
            )
            .map_err(|error| format!("could not be reached: {error}"))?;
        let root = service
            .GetFolder(&BSTR::from("\\"))
            .map_err(|error| format!("could not be read: {error}"))?;
        let task = root
            .GetTask(&BSTR::from(BROKER_TASK))
            .map_err(|error| format!("is not installed: {error}"))?;

        let mut params = bstr_variant(channel);
        let started = task.RunEx(&params, TASK_RUN_AS_SELF.0, 0, &BSTR::new());
        let _ = VariantClear(&mut params);

        match started {
            Ok(running) => Ok(running.EnginePID().ok().filter(|pid| *pid != 0)),
            // What "Require UAC elevation to run Windhawk" produces. Not a
            // failure of the ladder: it is the configuration asking for the
            // prompt the next rung puts up.
            Err(error) if error.code() == SCHED_E_TASK_DISABLED => Err("is disabled".to_owned()),
            Err(error) => Err(format!("could not be started: {error}")),
        }
    }
}

/// Put the UAC prompt up and start the broker behind it.
fn run_prompt(channel: &str) -> Result<BrokerProcess, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("windhawk-ui.exe could not be located: {error}"))?;
    let file = wide_path(&exe);
    let verb = wide("runas");
    let parameters = wide(&format!("--runtime-broker --channel {channel}"));
    // The install directory, not this process's. With no directory the elevated
    // child inherits the UNELEVATED caller's current directory, which the caller
    // chooses and which the loader's search for a bare file name reaches into.
    let directory = wide_path(exe.parent().unwrap_or(&exe));

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // The handle is what makes the shutdown wait and the exit-code readout
    // possible; without it a failed broker is indistinguishable from one that
    // never started.
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.lpDirectory = directory.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // Owned by the main window, so the consent dialog is modal to Windhawk and
    // cannot be lost behind it. That window is what the prompt gate waits for, so
    // by the time this runs there is one; a null owner is the unowned dialog a
    // window that has since gone would leave, and is answerable all the same.
    info.hwnd = window::main_window_handle();

    let (started, error) = {
        // The dialog is on screen for exactly the length of this call, and this
        // is the only thing in the ladder a person can hold up: the startup
        // watchdog and any second instance both stand down while it is raised.
        let _prompt = window::hold_elevation_prompt();
        // SAFETY: `info` is a fully initialized SHELLEXECUTEINFOW whose string
        // fields outlive the call; on success it receives a process handle owned
        // by the caller.
        let started = unsafe { ShellExecuteExW(&mut info) };
        // SAFETY: reads this thread's last error, set by the call above.
        let error = unsafe { GetLastError() };
        (started, error)
    };

    if started == 0 {
        return Err(if error == ERROR_CANCELLED {
            "elevation was declined".to_owned()
        } else {
            format!("the elevated helper could not be started (error {error})")
        });
    }

    let handle = info.hProcess as isize;
    let pid = if handle == 0 {
        0
    } else {
        // SAFETY: `handle` is the process handle ShellExecuteExW just returned.
        unsafe { windows_sys::Win32::System::Threading::GetProcessId(handle as HANDLE) }
    };
    Ok(BrokerProcess::from_handle(handle, pid))
}

/// A COM apartment for the calling thread, left when the guard drops.
///
/// Multithreaded, not apartment-threaded: this thread has no message loop, and an
/// STA that does not pump messages cannot make an out-of-process COM call - which
/// is exactly what the Task Scheduler is.
struct ComThread;

impl ComThread {
    fn enter() -> Result<ComThread, String> {
        // SAFETY: initializes COM for this thread; the guard leaves the apartment
        // on drop, exactly once.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_err() {
            return Err(format!(
                "could not be reached: COM refused this thread ({hr})"
            ));
        }
        Ok(ComThread)
    }
}

impl Drop for ComThread {
    fn drop(&mut self) {
        // SAFETY: balances the CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// A `VARIANT` holding `text`, which is how the Task Scheduler takes the argument
/// its `$(Arg0)` is substituted from. The caller clears it once the call that
/// reads it has returned.
fn bstr_variant(text: &str) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: a zeroed VARIANT is VT_EMPTY, so writing the discriminant and the
    // matching union arm together is the documented way to build one. The BSTR is
    // owned by the variant from here on, and freed by the VariantClear the caller
    // runs.
    unsafe {
        let inner = &mut variant.Anonymous.Anonymous;
        inner.vt = VT_BSTR;
        inner.Anonymous.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(text));
    }
    variant
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A path as the wide string Windows stored it as.
///
/// `encode_wide` is the exact inverse of how a path arrives from the system, so
/// it round-trips whatever the filesystem holds. Going through `to_string_lossy`
/// would substitute replacement characters for anything that is not well-formed
/// Unicode, and hand `ShellExecuteExW` a path to a file that does not exist.
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terms whose deadline has already run out, so anything the wait does to
    /// keep the accept alive is visible rather than hidden under the slack the
    /// ladder starts with.
    fn spent_terms() -> Mutex<AcceptTerms> {
        Mutex::new(AcceptTerms {
            policy: policy(None),
            connect_deadline: Instant::now(),
            handshake: HANDSHAKE_WINDOW,
        })
    }

    fn deadline(terms: &Mutex<AcceptTerms>) -> Instant {
        terms.lock().expect("the terms").connect_deadline
    }

    /// The everyday shape: the window came up, so the rung goes ahead.
    #[test]
    fn a_window_releases_the_prompt() {
        let gate = PromptGate::new();
        gate.allow();
        let (_sender, notes) = channel();

        assert!(matches!(
            await_permission(&notes, &spent_terms(), &gate),
            Permitted::Prompt
        ));
    }

    /// The reason the gate exists: a launch that ends in a message box must not
    /// leave a consent dialog behind it asking to elevate a window nobody is
    /// going to see.
    #[test]
    fn a_launch_that_ends_takes_the_prompt_with_it() {
        let gate = PromptGate::new();
        gate.abandon();
        let (_sender, notes) = channel();

        assert!(matches!(
            await_permission(&notes, &spent_terms(), &gate),
            Permitted::Abandoned(_)
        ));
    }

    /// The first answer wins. A window that came up has already been prompted
    /// for, and a failure after it - a window destroyed behind Tauri's back, say -
    /// cannot retract a dialog that may be on screen.
    #[test]
    fn the_first_answer_wins() {
        let gate = PromptGate::new();
        gate.allow();
        gate.abandon();
        let (_sender, notes) = channel();

        assert!(matches!(
            await_permission(&notes, &spent_terms(), &gate),
            Permitted::Prompt
        ));
    }

    /// The wait is bounded by the window build, not by this thread, so it has to
    /// hold the accept open for as long as it runs: the accept consumed the
    /// listener and there is no second one to open.
    #[test]
    fn waiting_for_the_window_keeps_the_accept_alive() {
        let gate = Arc::new(PromptGate::new());
        let terms = spent_terms();
        let started = deadline(&terms);
        let opening = Arc::clone(&gate);
        std::thread::spawn(move || {
            std::thread::sleep(PERMISSION_POLL * 4);
            opening.allow();
        });
        let (_sender, notes) = channel();

        assert!(matches!(
            await_permission(&notes, &terms, &gate),
            Permitted::Prompt
        ));
        assert!(deadline(&terms) > started + LADDER_SLACK);
    }

    /// Nothing is in flight while the wait runs, so a peer that connects and
    /// fails the policy is not this rung being retired - it is a stranger, and
    /// the window is still the thing being waited for.
    #[test]
    fn a_refusal_while_waiting_is_not_the_end_of_the_wait() {
        let gate = Arc::new(PromptGate::new());
        let (sender, notes) = channel();
        sender
            .send(Note::Rejected("not elevated".to_owned()))
            .expect("the note");
        let opening = Arc::clone(&gate);
        std::thread::spawn(move || {
            std::thread::sleep(PERMISSION_POLL * 4);
            opening.allow();
        });

        assert!(matches!(
            await_permission(&notes, &spent_terms(), &gate),
            Permitted::Prompt
        ));
    }

    /// An accept that is over ends the wait rather than parking in it: it took
    /// the listener with it, so there is nothing a prompt could connect to.
    #[test]
    fn a_stopped_accept_ends_the_wait() {
        let gate = PromptGate::new();
        let (sender, notes) = channel::<Note>();
        drop(sender);

        assert!(matches!(
            await_permission(&notes, &spent_terms(), &gate),
            Permitted::Gone(_)
        ));
    }
}
