//! The broker process: `windhawk-ui.exe --runtime-broker --channel <token>`.
//!
//! It has no window, no webview, and no Tauri: `main.rs` dispatches here before
//! `run()`, so nothing this path touches goes near the single-instance plugin,
//! the detect mutex, or the startup watchdog. What it owns is the one privileged
//! core session, and it serves that session over the channel it was started for.
//!
//! **The session exists before the channel does.** The DLL is loaded and
//! the session created BEFORE `hello` goes out, so a process that cannot host one
//! never becomes a channel at all: it reports the failure and exits, the UI sees
//! the absence it already knows how to handle, and the state this design has no
//! answer for - a healthy, verified channel on which every request fails - cannot
//! arise.
//!
//! Nothing here can be seen. There is no console, no window, and before the
//! handshake no channel either, so every pre-`hello` failure would otherwise be
//! invisible and the UI could only ever report "no channel". Each one is
//! therefore written through `OutputDebugStringW` with the `[WH] ` prefix the log
//! pane already captures, AND carries a distinct exit code, which the prompt rung
//! of the ladder can read off the process handle it holds.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use serde_json::value::RawValue;
use windhawk_broker::{
    BrokerHandler, Disposition, Integrity, PeerPolicy, Pusher, Responder, connect, push_queue,
};
use windhawk_core_host::{
    GatedCore, Session, SessionApi, SessionCallbacks, SessionConfig, resolve_dll_path,
};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentThreadId, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_NULL,
};

use crate::broker::ops::{HostOps, LocalHostOps};
use crate::broker::wire::{BrokerFrame, Fault, HostOp, Request, RequestKind, channel_config};
use crate::broker::{channel_pipe_name, is_channel_token};
use crate::editor::Editor;
use crate::lifecycle::session::{
    StartupInfo, discover_app_root, product_version, resolve_startup_info,
};
use crate::shell::ThemeSetting;

/// How long the broker will look for its channel before giving up. A broker that
/// cannot find the channel it was started for must not linger: it is an elevated
/// process with a loaded core session and nobody to serve.
const CONNECT_DEADLINE: Duration = Duration::from_secs(30);

/// How many requests the broker serves at once. Not load-bearing for correctness
/// (the UI's own transport caps what it can have outstanding, since each request
/// occupies one of its blocking workers), but an unbounded thread spawn in the
/// privileged process is not a property to leave to an upstream invariant. A
/// request that arrives with the pool full simply queues; the requests whose whole
/// value is being prompt bypass it entirely.
const WORKERS: usize = 8;

/// Why the broker exited. Each pre-handshake failure gets its own code: the
/// prompt rung of the ladder holds a real process handle and can read it back, so
/// on that rung the UI can say which failure it was instead of "no channel".
///
/// These numbers are read from OUTSIDE this crate - `scripts/matrix/run-matrix.ps1`
/// starts a broker with no usable channel, with a channel nobody serves, and with
/// the core hidden, and asserts the code each one exits with. Renumbering them is
/// therefore a change to something with a reader, not an internal detail.
///
/// 259 is not available to them. It is `STILL_ACTIVE`, which
/// `GetExitCodeProcess` also returns for a process that has not exited, so the
/// ladder's readout cannot tell a broker that ended with it from one that is still
/// running and reports neither. The `u8` repr is what holds a code added later to
/// that: the exit code leaves this process through `std::process::ExitCode`,
/// which takes a byte, so a variant outside that range would be truncated into a
/// code meaning something else entirely - 259 truncates to 3 - rather than
/// refused. With the repr it does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    /// The channel was served and ended normally.
    Served = 0,
    /// The command line named no usable channel.
    BadChannel = 10,
    /// Another broker already holds this channel.
    AlreadyServing = 11,
    /// No Windhawk installation was found around this executable.
    NoAppRoot = 12,
    /// `windhawk-core.dll` would not load, or failed its gate.
    CoreLoad = 13,
    /// The core session could not be created (before `hello`, deliberately).
    SessionCreate = 14,
    /// No channel was established before the deadline.
    NoChannel = 15,
    /// The peer serving the channel is not the process this broker exists for.
    PeerRejected = 16,
    /// The peer was there and the handshake did not complete.
    Handshake = 17,
}

/// Serve one channel, and return the process exit code.
pub fn run_broker(channel: &str) -> u8 {
    end_startup_feedback();
    match serve(channel) {
        Ok(()) => ExitCode::Served as u8,
        Err(failure) => {
            report(&failure.message);
            failure.code as u8
        }
    }
}

/// End the process-startup feedback: the hourglass Windows puts on the pointer
/// while a newly created process starts up.
///
/// The executable is GUI-subsystem for the window this mode does not have, so
/// creating this process arms that cursor, and the system only takes it back down
/// when the process goes input idle - documented as the first `GetMessage`,
/// whether or not a message is waiting. Nothing else in this mode touches user32:
/// there is no window and no message loop, and the main thread goes on to park in
/// the responder for the process lifetime. So the queue is created, one message is
/// put in it, and one `GetMessageW` takes it back out. Without this the pointer
/// spins out the system's own timeout, seconds after the broker is already
/// serving.
///
/// It has to happen here rather than at the launch: `STARTF_FORCEOFFFEEDBACK`
/// wants a `STARTUPINFO`, and neither rung has one to fill in - the Task Scheduler
/// owns the `CreateProcess` on the first, and `ShellExecuteExW` exposes no
/// `STARTUPINFO` on the second.
///
/// The queue this leaves behind is inert. A queue only receives what is sent to
/// it, and a process with no window and no registered class is not something the
/// system or another process broadcasts to.
fn end_startup_feedback() {
    let mut message = MSG::default();
    // SAFETY: `message` is a valid MSG slot, and a null window filter takes every
    // message posted to this thread. PeekMessageW forces this thread's message
    // queue into existence, which PostThreadMessageW needs; the post is then what
    // makes the GetMessageW below return at once rather than wait for a message
    // that would never come.
    unsafe {
        PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
        PostThreadMessageW(GetCurrentThreadId(), WM_NULL, 0, 0);
        GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
    }
}

/// What went wrong, and the code the ladder can read it back as.
struct Failure {
    code: ExitCode,
    message: String,
}

impl Failure {
    fn new(code: ExitCode, message: String) -> Failure {
        Failure { code, message }
    }
}

fn serve(channel: &str) -> Result<(), Failure> {
    if !is_channel_token(channel) {
        return Err(Failure::new(
            ExitCode::BadChannel,
            format!("the channel '{channel}' is not a channel name this build issues"),
        ));
    }

    // Belt and braces over `nMaxInstances = 1`, which already means a second
    // broker's connect fails: a double task trigger cannot produce two brokers for
    // one channel. `Local\`, not `Global\`: the channel belongs to one UI in one
    // logon session, so a cross-session name would be wider than the thing it
    // guards, and it should not reach for a privilege to do its job.
    let _guard = match SingleInstance::hold(&format!(r"Local\Windhawk.Broker.{channel}")) {
        Some(guard) => guard,
        None => {
            return Err(Failure::new(
                ExitCode::AlreadyServing,
                "another broker is already serving this channel".to_owned(),
            ));
        }
    };

    let app_root = discover_app_root().ok_or_else(|| {
        Failure::new(
            ExitCode::NoAppRoot,
            "no windhawk.ini was found walking up from windhawk-ui.exe".to_owned(),
        )
    })?;

    // The push queue is built BEFORE the session, so the session's callbacks have
    // somewhere to put what they produce from the moment they exist - during the
    // session create, during the connect, and during the handshake. Everything
    // queued in that window goes out once the writer thread starts, which is the
    // same queue it would have gone through anyway.
    let (pusher, pushes) = push_queue::<BrokerFrame>();

    let core = Arc::new(
        GatedCore::load(&core_path()?)
            .map_err(|error| Failure::new(ExitCode::CoreLoad, error.to_string()))?,
    );
    let config = SessionConfig::resolve(app_root, "windhawk-ui", product_version(), None);
    let session = Arc::new(
        core.create_session(&config, callbacks(pusher.clone()))
            .map_err(|error| {
                Failure::new(
                    ExitCode::SessionCreate,
                    format!("the core session could not be created: {error}"),
                )
            })?,
    );

    // The paths every host operation works from are the broker's OWN, read from
    // its own session: no path, command line, or executable ever crosses the
    // channel.
    let StartupInfo {
        app_root_path,
        app_data_path,
        ui_path,
        compiler_path,
        ..
    } = resolve_startup_info(session.as_ref()).map_err(|error| {
        Failure::new(
            ExitCode::SessionCreate,
            format!(
                "the Windhawk core info could not be read: {}",
                error.message
            ),
        )
    })?;

    // The host operations are the SAME implementation the UI runs when it has no
    // broker (`ops::LocalHostOps`): what an operation does is written once, and
    // this process differs only in being allowed to do it. Its captured lines go
    // out as pushes rather than to a log pane it does not have.
    let capture_pusher = pusher.clone();
    let host: Arc<dyn HostOps> = Arc::new(LocalHostOps::for_broker(
        Arc::clone(&core),
        Arc::clone(&session) as Arc<dyn SessionApi>,
        app_root_path,
        app_data_path.clone(),
        Arc::new(Editor::new(&app_data_path, ui_path, compiler_path)),
        Arc::new(move |lines: &[String]| {
            capture_pusher.push(BrokerFrame::dbwin(lines.to_vec()));
        }),
    ));

    let policy = PeerPolicy {
        // A CEILING on this side: an already elevated peer is not the unelevated
        // process this broker exists to serve.
        integrity: Integrity::MEDIUM,
        same_session: true,
        same_image: true,
        // The UI is the peer that created the channel; there is no pid this side
        // was told to expect.
        expected_pid: None,
    };
    let connection = connect(
        &channel_pipe_name(channel),
        &channel_config(),
        &policy,
        Instant::now() + CONNECT_DEADLINE,
    )
    .map_err(|error| {
        use windhawk_broker::ConnectError;
        let code = match &error {
            ConnectError::Timeout | ConnectError::Io(_) => ExitCode::NoChannel,
            ConnectError::Rejected(_) => ExitCode::PeerRejected,
            ConnectError::Protocol { .. } | ConnectError::Handshake(_) => ExitCode::Handshake,
        };
        Failure::new(code, error.to_string())
    })?;

    // The channel is the only thing keeping this process alive, so EOF is
    // sufficient - but an orphaned elevated process holding a core session is
    // exactly what an attacker would want to find, and the C++ side now waits on
    // this process during an upgrade, so a broker that failed to notice a vanished
    // UI would turn an upgrade into a failed install. Watching the peer's handle
    // costs one thread.
    watch_peer(connection.peer_pid);

    let service = Arc::new(Service { session, host });
    Responder::start(connection, service, WORKERS, pushes).join();
    Ok(())
}

/// The core this broker will load, as a path that names ONE file.
///
/// The resolver falls back to a bare `windhawk-core.dll` when it finds nothing
/// beside the executable, and a bare name is resolved by the loader's search -
/// which includes the current directory. This process runs elevated and inherits
/// its working directory from the unelevated one that started it, so on an
/// install whose core is missing, that search is a directory the caller chooses.
/// The install directory not being writable by non-administrators is already an
/// assumption of the threat model; letting a search out of it would put the
/// same weight on a directory nobody vetted.
///
/// So the broker takes the path only if it is absolute and there, and treats
/// anything else as the load failure it is about to be anyway.
fn core_path() -> Result<String, Failure> {
    let path = resolve_dll_path();
    let resolved = Path::new(&path);
    if resolved.is_absolute() && resolved.is_file() {
        return Ok(path);
    }
    Err(Failure::new(
        ExitCode::CoreLoad,
        format!("no windhawk-core.dll beside this executable ({path})"),
    ))
}

/// The session callbacks, which the core fires on its own threads under a
/// no-blocking rule. They do exactly one thing: hand the output to the writer
/// thread's queue. A callback that wrote straight down the pipe would stall a core
/// operation thread for as long as the UI stopped reading - a hang, not a
/// slowdown.
fn callbacks(pusher: Pusher<BrokerFrame>) -> SessionCallbacks {
    let events = pusher.clone();
    SessionCallbacks {
        log: Box::new(move |level, message| {
            pusher.push(BrokerFrame::log(level, message));
        }),
        event: Box::new(
            move |op_id, event_json| match RawValue::from_string(event_json) {
                Ok(raw) => {
                    events.push(BrokerFrame::event(op_id, raw));
                }
                Err(error) => report(&format!(
                    "the core produced an event for op {op_id} that is not JSON ({error}); it was dropped"
                )),
            },
        ),
    }
}

/// The privileged session and host operations, and the requests served against
/// them.
struct Service {
    session: Arc<Session>,
    host: Arc<dyn HostOps>,
}

impl BrokerHandler for Service {
    type Request = Request;
    type Response = BrokerFrame;
    type Push = BrokerFrame;

    fn request_id(&self, request: &Request) -> u64 {
        request.id
    }

    fn disposition(&self, request: &Request) -> Disposition {
        match request.k {
            // A cancel that queued behind the operations it is meant to interrupt
            // would be useless, and a shutdown must queue behind nothing. Both are
            // a lookup and a signal on this side, so serving them off the reader
            // thread costs nothing.
            RequestKind::Cancel => Disposition::Immediate,
            RequestKind::Shutdown => Disposition::Final,
            RequestKind::Invoke | RequestKind::InvokeAsync | RequestKind::Host => {
                Disposition::Pooled
            }
        }
    }

    fn handle(&self, request: Request) -> BrokerFrame {
        let id = request.id;
        match request.k {
            RequestKind::Invoke => match envelope(&request) {
                Ok(envelope) => self.invoke(id, envelope),
                Err(fault) => BrokerFrame::failed(id, fault),
            },
            RequestKind::InvokeAsync => match envelope(&request) {
                Ok(envelope) => self.invoke_async(id, envelope),
                Err(fault) => BrokerFrame::failed(id, fault),
            },
            RequestKind::Cancel => match request.op_id {
                Some(op_id) => {
                    BrokerFrame::cancelled(id, self.session.cancel_token(op_id).cancel())
                }
                None => BrokerFrame::failed(id, Fault::broker("a cancel names no op".to_owned())),
            },
            RequestKind::Host => self.host_op(id, request.op.as_deref(), request.args),
            // The response goes out and the channel closes behind it; the session
            // is dropped as this process exits.
            RequestKind::Shutdown => BrokerFrame::done(id),
        }
    }

    /// A reply too large for the wire fails that one request legibly rather than
    /// taking the channel down. It can only be an `exportUserData` of an archive
    /// the import cap would reject anyway.
    fn oversized(&self, id: u64, bytes: usize, cap: usize) -> BrokerFrame {
        BrokerFrame::failed(
            id,
            Fault::broker(format!(
                "the reply is {bytes} bytes, above the {cap} byte channel limit"
            )),
        )
    }

    fn push_dropped(&self, bytes: usize, cap: usize) {
        report(&format!(
            "an unsolicited frame of {bytes} bytes was dropped, above the {cap} byte channel limit"
        ));
    }
}

impl Service {
    /// A synchronous command. The core's response envelope goes back verbatim, so
    /// error codes, messages, and the core's own origins cross unchanged.
    fn invoke(&self, id: u64, envelope: &str) -> BrokerFrame {
        match self.session.invoke_raw(envelope) {
            Ok(response) => match RawValue::from_string(response) {
                Ok(raw) => BrokerFrame::raw(id, raw),
                // A response that is not JSON cannot ride as a raw JSON value.
                // Saying so here is what the raw-value spelling costs: in-process
                // that response would have reached the caller's parse and produced
                // its decode error there.
                Err(error) => BrokerFrame::failed(
                    id,
                    Fault::broker(format!(
                        "the core produced a response that is not JSON: {error}"
                    )),
                ),
            },
            Err(error) => BrokerFrame::failed(id, Fault::of(&error)),
        }
    }

    /// An asynchronous command's START. Its events arrive later as pushes, off the
    /// session callbacks.
    fn invoke_async(&self, id: u64, envelope: &str) -> BrokerFrame {
        match self.session.invoke_async_raw(envelope) {
            Ok(op_id) => BrokerFrame::started(id, op_id),
            Err(error) => BrokerFrame::failed(id, Fault::of(&error)),
        }
    }

    /// One privileged host operation, performed by the same implementation the UI
    /// would run in this process's place. An operation this build does not serve is
    /// a typed failure, never a fallthrough to anything general-purpose.
    fn host_op(&self, id: u64, op: Option<&str>, args: Option<Value>) -> BrokerFrame {
        let Some(parsed) = op.and_then(HostOp::parse) else {
            return BrokerFrame::failed(
                id,
                Fault::broker(format!(
                    "'{}' is not a host operation this build serves",
                    op.unwrap_or("(none)")
                )),
            );
        };

        let outcome = match parsed {
            HostOp::SeedModsRuntime => {
                self.host.seed_mods_runtime();
                Ok(())
            }
            HostOp::EditorOpen => match decode(args) {
                Ok(request) => self.host.editor_open(&request),
                Err(fault) => return BrokerFrame::failed(id, fault),
            },
            HostOp::EditorSweep => {
                self.host.editor_sweep();
                Ok(())
            }
            HostOp::EditorSyncTheme => match decode::<ThemeArgs>(args) {
                Ok(args) => {
                    self.host
                        .editor_sync_theme(ThemeSetting::parse(&args.theme));
                    Ok(())
                }
                Err(fault) => return BrokerFrame::failed(id, fault),
            },
            HostOp::DbwinStart => {
                self.host.dbwin_start();
                Ok(())
            }
            HostOp::DbwinStop => {
                self.host.dbwin_stop();
                Ok(())
            }
        };

        match outcome {
            Ok(()) => BrokerFrame::done(id),
            Err(failure) => BrokerFrame::failed(id, Fault::broker(failure.to_string())),
        }
    }
}

/// The `editorSyncTheme` argument: the theme setting as the core stores it.
#[derive(serde::Deserialize)]
struct ThemeArgs {
    theme: String,
}

/// Read a host operation's arguments, or the typed failure to answer with. A frame
/// whose arguments do not decode is answered rather than dropped, for the same
/// reason an unknown operation is: the alternative takes the channel down.
fn decode<T: serde::de::DeserializeOwned>(args: Option<Value>) -> Result<T, Fault> {
    let args =
        args.ok_or_else(|| Fault::broker("the host operation carries no arguments".to_owned()))?;
    serde_json::from_value(args).map_err(|error| {
        Fault::broker(format!(
            "the host operation's arguments could not be read: {error}"
        ))
    })
}

/// The request envelope a command frame carries.
fn envelope(request: &Request) -> Result<&str, Fault> {
    match &request.envelope {
        Some(envelope) => Ok(envelope.get()),
        None => Err(Fault::broker("the request carries no envelope".to_owned())),
    }
}

/// Exit when the UI does, whether or not the channel notices first.
fn watch_peer(pid: u32) {
    // SAFETY: OpenProcess only opens a handle; a null return means the process is
    // gone or cannot be opened, both of which this treats as "nothing to watch".
    // An elevated process opening a medium-integrity one always works, so a null
    // here means the UI has already exited.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return;
    }
    let handle = handle as isize;
    let _ = std::thread::Builder::new()
        .name("windhawk-broker-peer-watch".to_owned())
        .spawn(move || {
            // SAFETY: `handle` is the process handle opened above, owned by this
            // thread and closed exactly once below.
            unsafe {
                WaitForSingleObject(handle as HANDLE, INFINITE);
                CloseHandle(handle as HANDLE);
            }
            report("the Windhawk UI exited; the broker is following it");
            std::process::exit(ExitCode::Served as i32);
        });
}

/// The single-instance guard, held for the process lifetime.
struct SingleInstance(HANDLE);

impl SingleInstance {
    /// Take `name`, or `None` if someone else already holds it.
    fn hold(name: &str) -> Option<SingleInstance> {
        let name = wide(name);
        // SAFETY: a null descriptor takes the default (this user, this session);
        // `name` is a NUL-terminated wide string the call copies. The handle is
        // owned by the returned guard.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: reads this thread's last error, set by the call above.
        let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let guard = SingleInstance(handle);
        (!existed).then_some(guard)
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW above and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Report something about the runtime broker where it can be seen: the log pane
/// captures `[WH] ` debug output, so what became of the broker is diagnosable
/// with the tools that ship rather than not at all.
pub fn report(message: &str) {
    let line = wide(&format!("[WH] windhawk-ui broker: {message}\n"));
    // SAFETY: `line` is a NUL-terminated wide string that outlives the call, which
    // only copies it to whatever debugger or capture is listening.
    unsafe { OutputDebugStringW(line.as_ptr()) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
