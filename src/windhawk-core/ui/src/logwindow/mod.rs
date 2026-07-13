//! The log pane's capture backend: it tails Windhawk's live `[WH] ` debug
//! output (captured in-process from DBWIN, [`capture`]) plus the
//! compiler-output surface for failed installs/compiles, reproducing the
//! extension's out-of-band "Windhawk Compiler" output channel as a natural
//! companion in the same view.
//!
//! The pane's front-end is a read-only Monaco editor in the React app
//! (`vscode-windhawk-ui`, the Tauri build), docked as a resizable bottom split. This
//! module drives it over Tauri channels: the `wh-log` event streams live `[WH]` line
//! batches, `wh-log-show` reveals the pane, and the `wh_log_backlog` /
//! `wh_log_stop_capture` app commands ([`crate::ipc::bridge`]) serve the retained tail
//! and release capture on close.
//!
//! [`LogController`] is the seam the IPC dispatch (`showLogOutput` /
//! `showAdvancedDebugLogOutput`) and the event pump (`report_op_failure`) reach the
//! pane through, so both stay headless-testable: production is [`AppLogController`]
//! (it holds the `AppHandle`); the dispatcher tests and the integration smoke use
//! [`NoopLogController`].

mod buffer;
mod capture;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tauri::{AppHandle, Emitter};
use windhawk_core_host::{HostError, HostErrorKind};
use windhawk_core_protocol::{CompileDetails, ErrorCode};

use buffer::TailBuffer;

/// The event channels the React log pane listens on: `wh-log` carries live `[WH] `
/// line batches; `wh-log-show` reveals the pane (the showLogOutput affordance and the
/// compiler-output surface).
const LOG_EVENT: &str = "wh-log";
const LOG_SHOW_EVENT: &str = "wh-log-show";

/// The seam the IPC dispatch and the event pump reach the log pane through. Kept
/// trait-shaped so the headless paths (dispatch + pump tests, the integration smoke)
/// run with [`NoopLogController`] and no `AppHandle`.
pub trait LogController: Send + Sync {
    /// Reveal the log pane and start live `[WH]` capture if not already.
    fn show(&self);
    /// The retained tail, which the pane requests on first reveal to render the
    /// backlog (including compiler output pushed just before [`LogController::show`]
    /// revealed it).
    fn backlog(&self) -> Vec<String>;
    /// Stop live capture. The pane's Close affordance calls this, and `run`
    /// calls it on main-window close: capture is scoped to while the pane is
    /// open because it contends for the single-owner DBWIN buffer.
    fn stop_capture(&self);
    /// Surface an async op's terminal failure IF it is a local-compile failure
    /// (`installMod`/`compileInstalledMod` -> `COMPILER_FAILED`): write the compiler
    /// diagnostics to the pane and reveal it. Any other command/error is ignored (it
    /// became the command's normal failure reply). The event dispatcher calls this
    /// generically on every failed terminal; the filter lives here.
    fn report_op_failure(&self, command: &str, error: &HostError);
}

/// The no-op controller for the headless dispatcher/pump tests and the integration
/// smoke, which have no `AppHandle` and never reveal a pane.
pub struct NoopLogController;

impl LogController for NoopLogController {
    fn show(&self) {}
    fn backlog(&self) -> Vec<String> {
        Vec::new()
    }
    fn stop_capture(&self) {}
    fn report_op_failure(&self, _command: &str, _error: &HostError) {}
}

/// The production controller. Capture state lives in a shared [`LogState`], separate
/// from the `AppHandle`, so the close handler can stop capture without reaching back
/// through Tauri managed state.
pub struct AppLogController {
    app: AppHandle,
    state: Arc<LogState>,
}

#[derive(Default)]
struct LogState {
    buffer: TailBuffer,
    /// `Some` while the capture thread runs (between a `show` and the pane closing).
    capture: Mutex<Option<CaptureHandle>>,
}

struct CaptureHandle {
    shutdown: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl AppLogController {
    pub fn new(app: AppHandle) -> AppLogController {
        AppLogController {
            app,
            state: Arc::new(LogState::default()),
        }
    }
}

impl LogController for AppLogController {
    fn show(&self) {
        // The explicit log affordance: tail the live [WH] stream and reveal the pane.
        // Capture spawns its own thread and the emit is thread-safe, so this runs
        // directly on the calling wh_ipc worker - no window to create on the main
        // thread.
        ensure_capture(&self.app, &self.state);
        emit_show(&self.app);
    }

    fn backlog(&self) -> Vec<String> {
        self.state.buffer.snapshot()
    }

    fn stop_capture(&self) {
        stop_capture(&self.state);
    }

    fn report_op_failure(&self, command: &str, error: &HostError) {
        let Some(lines) = compiler_output_lines(command, error) else {
            return;
        };
        deliver(&self.app, &self.state, &lines);
        // Surface the compiler diagnostics in the pane WITHOUT starting live
        // DBWIN capture: a failed compile wants its error text, not to begin
        // contending for the single-owner DBWIN buffer. The lines pushed just
        // above arrive live on an open pane and are in the backlog a first
        // reveal loads.
        emit_show(&self.app);
    }
}

/// Start the live DBWIN capture thread if it is not already running, delivering
/// each `[WH]` line - and the startup status (the "Listening..." banner or the
/// capture errors) - to the tail buffer and the open pane. Started when the
/// pane is revealed (`show`); [`stop_capture`] stops it when the pane closes.
///
/// Blocks until the thread has delivered that startup status, so that on a first
/// reveal it is already in the backlog the pane loads on open rather than racing the
/// pane's live subscription (which is wired only after the backlog fetch).
fn ensure_capture(app: &AppHandle, state: &Arc<LogState>) {
    let mut guard = state.capture.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return;
    }
    let shutdown = Arc::new(AtomicBool::new(false));
    let app = app.clone();
    let thread_state = state.clone();
    let thread_shutdown = shutdown.clone();
    let (init_tx, init_rx) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("wh-log-capture".to_owned())
        .spawn(move || {
            let on_lines = |lines: &[String]| deliver(&app, &thread_state, lines);
            let init_done = || {
                let _ = init_tx.send(());
            };
            capture::run(&on_lines, &thread_shutdown, &init_done);
        })
        .expect("spawn the log capture thread");
    // Wait for the thread to deliver its startup status. A disconnect (the thread died
    // before signalling) also unblocks us; either way the handle is recorded so a
    // later reveal does not spawn a second thread.
    let _ = init_rx.recv();
    *guard = Some(CaptureHandle { shutdown, thread });
}

/// Append a batch of lines to the tail and push them live to the pane in one event. The
/// two sources - the capture thread (which coalesces a flood into batches) and the
/// compiler-output surface (which delivers its diagnostics at once) - share this one
/// sink, so lines pushed before the pane is first revealed are still in the backlog it
/// requests then, and lines pushed while it is open are delivered live. Emitting the
/// whole batch as a single `wh-log` payload keeps the IPC crossing count bounded under a
/// rapid stream instead of scaling with the line rate.
fn deliver(app: &AppHandle, state: &LogState, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    for line in lines {
        state.buffer.push(line.clone());
    }
    // Best-effort: a not-yet-built pane (the front-end is still loading) just means
    // no live listener; the lines stay in the backlog.
    let _ = app.emit(LOG_EVENT, lines);
}

/// Reveal the log pane (the showLogOutput affordance and the compiler-output
/// surface). Best-effort: the pane subscribes once the front-end has loaded.
fn emit_show(app: &AppHandle) {
    let _ = app.emit(LOG_SHOW_EVENT, ());
}

/// Signal the capture thread to stop and join it (the pane Close affordance and
/// the main-window-close handler).
fn stop_capture(state: &LogState) {
    let handle = state
        .capture
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(handle) = handle {
        handle.shutdown.store(true, Ordering::Release);
        let _ = handle.thread.join();
    }
}

/// The lines to surface for a local-compile failure, or `None` when `command`/`error`
/// is not a `COMPILER_FAILED` from an install/compile op. Decodes the structured
/// details the core attaches via the shared [`CompileDetails`] DTO.
fn compiler_output_lines(command: &str, error: &HostError) -> Option<Vec<String>> {
    if command != "installMod" && command != "compileInstalledMod" {
        return None;
    }
    let HostErrorKind::Wire(wire) = error.kind() else {
        return None;
    };
    if wire.code != ErrorCode::CompilerFailed {
        return None;
    }

    let mut lines = vec![format!("=== Compiler failed: {} ===", wire.message)];
    if let Some(details) = wire
        .details
        .clone()
        .and_then(|d| serde_json::from_value::<CompileDetails>(d).ok())
    {
        lines.extend(non_empty_lines(&details.stderr));
        lines.extend(non_empty_lines(&details.stdout));
    }
    Some(lines)
}

/// Split compiler text into non-blank lines (the diagnostics, one per row).
fn non_empty_lines(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use windhawk_core_protocol::WireError;

    fn compiler_failed() -> HostError {
        HostError::wire(WireError::with_details(
            ErrorCode::CompilerFailed,
            "clang++ exited with code 1",
            json!({
                "target": "x86_64-pc-windows-msvc",
                "exitCode": 1,
                "stdout": "",
                "stderr": "mod.cpp:10:5: error: use of undeclared identifier 'foo'\n",
            }),
        ))
    }

    #[test]
    fn compiler_failure_for_an_install_is_surfaced_with_its_diagnostics() {
        let lines = compiler_output_lines("installMod", &compiler_failed()).expect("surfaced");
        assert_eq!(
            lines[0],
            "=== Compiler failed: clang++ exited with code 1 ==="
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("undeclared identifier 'foo'"))
        );
    }

    #[test]
    fn compile_command_failure_is_surfaced_too() {
        assert!(compiler_output_lines("compileInstalledMod", &compiler_failed()).is_some());
    }

    #[test]
    fn a_non_compile_command_is_not_surfaced() {
        // A different command failing COMPILER_FAILED is not the compiler-output path.
        assert!(compiler_output_lines("installMod", &other_failure()).is_none());
        assert!(compiler_output_lines("getInstalledMods", &compiler_failed()).is_none());
    }

    fn other_failure() -> HostError {
        HostError::wire(WireError::new(ErrorCode::RepoUnreachable, "down"))
    }
}
