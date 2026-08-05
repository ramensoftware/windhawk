//! The privileged host operations: the closed set of side effects that are not
//! core commands and still need rights an unelevated window does not have.
//!
//! [`HostOps`] is the seam, and it exists for the same reason `SessionApi` does.
//! Three configurations have no broker at all - a portable install, an already
//! elevated window, and degraded mode - and every operation here still has to be
//! attempted in each of them, running and failing like any other unprivileged
//! work rather than not existing. So there is one trait with two
//! implementations, held behind the same swap point as the session, and no call
//! site carries an `if there is a broker` of its own.
//!
//! **[`LocalHostOps`] is the whole implementation, and the broker runs it too.**
//! It is what the UI uses when there is no channel, and it is also what the broker
//! process calls when it serves a `host` frame - so "what the operation does" is
//! written once and the elevated path cannot drift from the unelevated one.
//! [`RemoteHostOps`] only turns a call into a frame.
//!
//! The granularity is one method per USER ACTION rather than one per file
//! operation: `editorOpen` prepares a workspace and opens the editor on it in one
//! crossing, because nothing happens between the two and a workspace index that
//! crossed back would be a path handed to the elevated process by the unelevated
//! one. The per-workspace callbacks the sweep and the locate need - `doesModExist`,
//! the `@id` parse - run against whichever core and session the implementation
//! already holds, so they cost no round trip either.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use windhawk_broker::Requester;
use windhawk_core_host::{GatedCore, HostError, SessionApi, SessionApiExt};
use windhawk_core_protocol::{ModIdParams, ParseModSourceParams, ParsedModSource};

use crate::broker::wire::{Channel, HostOp, Request, Response};
use crate::editor::Editor;
use crate::editor::workspace::WorkspaceInit;
use crate::lifecycle::mods_runtime;
use crate::logwindow::capture;
use crate::pump::PumpMessage;
use crate::shell::ThemeSetting;

/// The privileged host operations, as the UI asks for them.
///
/// Every method is fallible in principle and only the one the caller can act on
/// says so: the rest are best effort exactly as they are today (a mod-runtime seed
/// that cannot copy, a sweep that cannot enumerate, an editor theme that cannot be
/// written are all logged and dropped), and reporting them would give the front-end
/// a failure it has nothing to do with.
pub trait HostOps: Send + Sync {
    /// Copy the install tree's `ModsRuntime` into the engine's `Engine\Mods` under
    /// appData, for files not already there. Runs at startup, best effort.
    fn seed_mods_runtime(&self);

    /// Prepare the workspace for a mod and open the code editor on it.
    fn editor_open(&self, request: &EditorOpen) -> Result<(), HostOpFailure>;

    /// Garbage-collect abandoned editor workspaces. Runs at startup and after a
    /// `deleteMod`, best effort.
    fn editor_sweep(&self);

    /// Sync the shared VSCodium user settings' color-theme keys to the app's
    /// theme, so an open editor re-themes live and the next launch opens matching.
    fn editor_sync_theme(&self, theme: ThemeSetting);

    /// Start capturing the cross-session `Global\` debug output into the log pane.
    /// Paired with the pane's own `Local\` capture, which the log controller owns
    /// and which needs no privileges.
    fn dbwin_start(&self);

    /// Stop that capture, releasing the single-owner `Global\` DBWIN objects.
    fn dbwin_stop(&self);
}

/// Where a batch of captured `Global\` debug-output lines goes: the log pane in
/// the UI, a channel push in the broker.
pub type CaptureSink = Arc<dyn Fn(&[String]) + Send + Sync>;

/// What a launch entry point wants opened.
///
/// The id and the source are resolved by the HANDLER, in the UI process: the
/// template, the collision suffixes, the fork's `@id` check and the stored-source
/// read are policy with their own tests, and none of it needs privileges. What
/// crosses is the outcome of that policy, and never a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpen {
    /// The bare mod id (no `local@` scope) the workspace is marked with.
    pub mod_id: String,
    /// The `mod.wh.cpp` contents for a freshly allocated workspace.
    pub mod_source: String,
    /// Whether an existing workspace already editing this mod should be reused
    /// (`editMod`) rather than a new one allocated (`createNewMod`, `forkMod`).
    /// A reused workspace keeps its own `mod.wh.cpp`, which may hold unsaved
    /// edits, so `mod_source` is only used when nothing is found.
    pub reuse: bool,
}

impl EditorOpen {
    /// Hold `mod_id` to the charset a bare id is drawn from: non-empty, and only
    /// `0-9`, `a-z`, and `-`.
    ///
    /// The process that runs this operation may be doing it for an UNELEVATED
    /// peer, with rights that peer does not have, so the id is checked where it
    /// is used rather than taken on the sender's word. Here it reaches an
    /// identity marker and the core calls the sweep makes with it, never a path
    /// component - workspaces are numbered - but it is the same id the core
    /// holds to this charset everywhere it names a file, a registry key, or a
    /// storage directory, and one carrying `\`, `/`, `:`, or `..` has no
    /// legitimate sender. Checking it here keeps that true of the elevated side
    /// on its own terms, whatever a workspace layout later does with the id.
    ///
    /// The source carries no such rule: it is content, any text is a legitimate
    /// mod source, and it lands in the workspace's `mod.wh.cpp` as written.
    fn check_mod_id(&self) -> Result<(), HostOpFailure> {
        if self.mod_id.is_empty()
            || !self
                .mod_id
                .chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='z' | '-'))
        {
            return Err(HostOpFailure::Failed(format!(
                "the mod id {:?} must be non-empty and only contain the characters 0-9, a-z, and a hyphen (-)",
                self.mod_id
            )));
        }
        Ok(())
    }
}

/// Why a privileged host operation did not happen.
#[derive(Debug)]
pub enum HostOpFailure {
    /// It ran and failed.
    Failed(String),
    /// There was no way to run it: the elevated helper is gone, or never arrived.
    /// Held apart because it is the one that says nothing about the operation:
    /// what it describes is the state the banner is already reporting, so a
    /// best-effort call passes over it in silence.
    Unavailable(HostError),
}

impl HostOpFailure {
    /// An I/O failure from an operation performed in this process.
    fn of_io(context: &str, error: &io::Error) -> HostOpFailure {
        HostOpFailure::Failed(format!("{context}: {error}"))
    }
}

impl std::fmt::Display for HostOpFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostOpFailure::Failed(message) => f.write_str(message),
            HostOpFailure::Unavailable(error) => write!(f, "{error}"),
        }
    }
}

/// The host operations performed by whichever process holds this value: the UI
/// where there is no broker, and the broker itself when it serves a `host` frame.
///
/// It reads every path it touches from its own `getCoreInfo`, and answers the
/// per-workspace questions from its own core and session, so an operation asks
/// nothing of the process that requested it.
pub struct LocalHostOps {
    core: Arc<GatedCore>,
    /// The session the sweep's `doesModExist` runs against. The LOCAL session in
    /// the UI, which is the only session there is in the configurations this
    /// implementation serves; the broker's own where the broker holds it.
    session: Arc<dyn SessionApi>,
    /// The install directory, the mod-runtime seed's copy source.
    app_root: PathBuf,
    /// The resolved Windhawk AppData directory, the parent of everything else here.
    app_data: PathBuf,
    editor: Arc<Editor>,
    /// Where captured `Global\` lines go: the log pane through the pump in the UI,
    /// a channel push in the broker.
    lines: CaptureSink,
    /// Whether a denied `Global\` capture is worth a line (see
    /// [`capture::run_global`]).
    report_denial: bool,
    /// The `Global\` capture, while it runs.
    dbwin: Mutex<Option<GlobalCapture>>,
}

impl LocalHostOps {
    /// The UI's own implementation: captured lines reach the log pane through the
    /// pump, which is the same route the broker's pushes take, and a denied
    /// `Global\` capture says nothing because an unelevated window being refused
    /// that privilege is the expected answer.
    pub fn for_ui(
        core: Arc<GatedCore>,
        session: Arc<dyn SessionApi>,
        app_root: PathBuf,
        app_data: PathBuf,
        editor: Arc<Editor>,
        pump: Sender<PumpMessage>,
    ) -> LocalHostOps {
        let lines = Arc::new(move |lines: &[String]| {
            let lines = lines.to_vec();
            // Best effort, like every other pump send: a closed receiver means the
            // app is gone.
            let _ = pump.send(PumpMessage::deferred(move |ctx| {
                ctx.log.deliver_captured(&lines);
            }));
        });
        LocalHostOps::new(core, session, app_root, app_data, editor, lines, false)
    }

    /// The broker's implementation. `lines` pushes them down the channel, and a
    /// denial IS reported: this process exists to hold `SeCreateGlobalPrivilege`,
    /// so being refused it is a failure of its job rather than a fact of life.
    pub fn for_broker(
        core: Arc<GatedCore>,
        session: Arc<dyn SessionApi>,
        app_root: PathBuf,
        app_data: PathBuf,
        editor: Arc<Editor>,
        lines: CaptureSink,
    ) -> LocalHostOps {
        LocalHostOps::new(core, session, app_root, app_data, editor, lines, true)
    }

    fn new(
        core: Arc<GatedCore>,
        session: Arc<dyn SessionApi>,
        app_root: PathBuf,
        app_data: PathBuf,
        editor: Arc<Editor>,
        lines: CaptureSink,
        report_denial: bool,
    ) -> LocalHostOps {
        LocalHostOps {
            core,
            session,
            app_root,
            app_data,
            editor,
            lines,
            report_denial,
            dbwin: Mutex::new(None),
        }
    }
}

impl HostOps for LocalHostOps {
    fn seed_mods_runtime(&self) {
        mods_runtime::copy_mods_runtime_libs(&self.app_root, &self.app_data);
    }

    fn editor_open(&self, request: &EditorOpen) -> Result<(), HostOpFailure> {
        request.check_mod_id()?;

        let workspaces = self.editor.workspaces();

        // Edit-reuse: the workspace already editing this mod, found by its identity
        // marker with an `@id`-parse fallback. The parse is a stateless core call,
        // so it runs here per candidate rather than crossing back.
        let located = if request.reuse {
            workspaces
                .locate(&request.mod_id, |source| parse_bare_id(&self.core, source))
                .map_err(|error| {
                    HostOpFailure::of_io("the editor workspaces could not be read", &error)
                })?
        } else {
            None
        };

        let workspace = match located {
            Some(workspace) => {
                // Keep the existing `mod.wh.cpp` (it may hold unsaved edits) but
                // re-seed the editor-mode settings: a workspace found via the `@id`
                // fallback may have had `editedModId` cleared by a prior exit, and
                // without rewriting it the extension would enter browse mode.
                workspaces
                    .reseed_editor_mode(workspace.path(), &request.mod_id)
                    .map_err(|error| {
                        HostOpFailure::of_io("the editor workspace could not be re-seeded", &error)
                    })?;
                workspace
            }
            None => {
                let flags = compile_flags(&self.core)
                    .map_err(|error| HostOpFailure::Failed(error.to_string()))?;
                workspaces
                    .allocate_and_initialize(&WorkspaceInit {
                        mod_source: &request.mod_source,
                        mod_id: &request.mod_id,
                        compile_flags: &flags,
                    })
                    .map_err(|error| {
                        HostOpFailure::of_io("the editor workspace could not be prepared", &error)
                    })?
            }
        };

        self.editor
            .launcher()
            .open_workspace(workspace.path())
            .map_err(|error| HostOpFailure::Failed(error.to_string()))
    }

    fn editor_sweep(&self) {
        // A core failure on the keep/reclaim question defaults to KEEP, so a
        // transient error never reclaims a real mod's workspace.
        let result = self.editor.workspaces().sweep(
            |storage_id| does_mod_exist(self.session.as_ref(), storage_id).unwrap_or(true),
            |source| parse_bare_id(&self.core, source),
        );
        match result {
            Ok(report) => {
                if !report.reclaimed.is_empty() || !report.in_use.is_empty() {
                    eprintln!(
                        "windhawk-ui: editor workspace sweep - kept {:?}, reclaimed {:?}, in use {:?}",
                        report.kept, report.reclaimed, report.in_use
                    );
                }
            }
            Err(error) => eprintln!("windhawk-ui: editor workspace sweep failed: {error}"),
        }
    }

    fn editor_sync_theme(&self, theme: ThemeSetting) {
        // Best effort: a write failure must not fail the settings change, which has
        // already applied.
        if let Err(error) = self.editor.launcher().sync_theme(theme) {
            eprintln!("windhawk-ui: could not sync the editor theme to VSCodium: {error}");
        }
    }

    fn dbwin_start(&self) {
        let mut running = self.dbwin.lock().unwrap_or_else(|error| error.into_inner());
        if running.is_some() {
            return;
        }
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let lines = Arc::clone(&self.lines);
        let report_denial = self.report_denial;
        let thread = std::thread::Builder::new()
            .name("wh-log-capture-global".to_owned())
            .spawn(move || {
                let on_lines = |batch: &[String]| lines(batch);
                capture::run_global(&on_lines, &thread_shutdown, report_denial);
            });
        match thread {
            Ok(thread) => *running = Some(GlobalCapture { shutdown, thread }),
            Err(error) => {
                eprintln!("windhawk-ui: the global debug-output capture could not start: {error}");
            }
        }
    }

    fn dbwin_stop(&self) {
        let running = self
            .dbwin
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(capture) = running {
            capture
                .shutdown
                .store(true, std::sync::atomic::Ordering::Release);
            let _ = capture.thread.join();
        }
    }
}

/// The running `Global\` capture: the flag its loop watches and the thread it runs
/// on.
struct GlobalCapture {
    shutdown: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

/// How long the debug-output capture toggles wait for the helper to answer.
///
/// They are the two host operations reached from a thread that cannot be held:
/// the log pane's Close, and the window teardown, which runs on the event loop
/// and holds the single-instance state open for as long as it lasts. Every other
/// operation here runs on a worker with nobody waiting on it and so waits as long
/// as the in-process call it replaced would.
///
/// The helper serves `host` frames from its thread pool, which the mod compiles
/// share, so one of these can queue behind however much work is in flight -
/// which is what makes a deadline necessary. What makes it affordable is that
/// giving up on the ANSWER does not call the operation off: the frame is on the
/// wire and the helper still serves it, so a toggle that outlasts this still
/// takes effect, just with nobody parked on it.
const CAPTURE_DEADLINE: Duration = Duration::from_secs(2);

/// The host operations performed by the broker, over the channel.
pub struct RemoteHostOps {
    requester: Arc<Requester<Channel>>,
    /// The broker's process id, where the ladder that started it could report one.
    /// Only the editor launch needs it (see [`RemoteHostOps::editor_open`]).
    broker_pid: Option<u32>,
}

impl RemoteHostOps {
    pub fn new(requester: Arc<Requester<Channel>>, broker_pid: Option<u32>) -> RemoteHostOps {
        RemoteHostOps {
            requester,
            broker_pid,
        }
    }

    /// Ask for one operation and reduce the answer to "it was done" or why not.
    ///
    /// `within` bounds the wait for the answer, for the callers that have a
    /// thread to hand back. `None` waits as long as it takes, which is what the
    /// in-process implementation does and what a caller doing the work on a
    /// worker of its own wants.
    fn ask(&self, request: Request, within: Option<Duration>) -> Result<(), HostOpFailure> {
        let answered = match within {
            None => self.requester.request(request),
            Some(within) => self.requester.request_within(request, within),
        };
        match answered {
            Ok(Response::Done) => Ok(()),
            Ok(Response::Failed(fault)) => {
                Err(HostOpFailure::Failed(fault.into_host().to_string()))
            }
            Ok(_) => Err(HostOpFailure::Unavailable(HostError::transport(
                "the elevated Windhawk helper answered a host operation with an unrelated response"
                    .to_owned(),
            ))),
            Err(error) => Err(HostOpFailure::Unavailable(HostError::transport(format!(
                "the elevated Windhawk helper could not be reached: {error}"
            )))),
        }
    }

    /// Run a best-effort operation, reporting a failure the way the in-process
    /// implementation reports its own: to the log, not to the caller.
    ///
    /// A helper that is simply not there says nothing. That is not a failure of the
    /// operation, it is the state the banner already reports, and every best-effort
    /// call made while it lasts would otherwise print the same line again.
    fn ask_quietly(&self, what: &str, request: Request, within: Option<Duration>) {
        match self.ask(request, within) {
            Ok(()) | Err(HostOpFailure::Unavailable(_)) => {}
            Err(failure) => {
                eprintln!("windhawk-ui: {what} failed in the elevated Windhawk helper: {failure}");
            }
        }
    }
}

impl HostOps for RemoteHostOps {
    fn seed_mods_runtime(&self) {
        self.ask_quietly(
            "seeding the mod runtime libraries",
            Request::host(HostOp::SeedModsRuntime, None),
            None,
        );
    }

    /// The editor is spawned by the broker, so the broker's child is the one that
    /// has to come up in front - and foreground rights belong to the process the
    /// user last interacted with, which is this one. The broker has never received
    /// input and owns no window, so without this grant VSCodium opens BEHIND the
    /// Windhawk window and the symptom reads as a VSCodium problem.
    fn editor_open(&self, request: &EditorOpen) -> Result<(), HostOpFailure> {
        crate::lifecycle::window::allow_foreground_for(self.broker_pid);
        self.ask(
            Request::host(HostOp::EditorOpen, Some(json!(request))),
            None,
        )
    }

    fn editor_sweep(&self) {
        self.ask_quietly(
            "the editor workspace sweep",
            Request::host(HostOp::EditorSweep, None),
            None,
        );
    }

    fn editor_sync_theme(&self, theme: ThemeSetting) {
        self.ask_quietly(
            "syncing the editor theme",
            Request::host(
                HostOp::EditorSyncTheme,
                Some(json!({ "theme": theme.as_str() })),
            ),
            None,
        );
    }

    fn dbwin_start(&self) {
        self.ask_quietly(
            "starting the cross-session debug-output capture",
            Request::host(HostOp::DbwinStart, None),
            Some(CAPTURE_DEADLINE),
        );
    }

    fn dbwin_stop(&self) {
        self.ask_quietly(
            "stopping the cross-session debug-output capture",
            Request::host(HostOp::DbwinStop, None),
            Some(CAPTURE_DEADLINE),
        );
    }
}

/// The bare `@id` from a mod source (`parseModSource`, stateless), or `None` when
/// the source carries no parseable metadata id. The language is irrelevant to the
/// id, so it is fixed to `en`.
///
/// One of the three core reads that both the launch handlers (which resolve what to
/// open) and the host operations (which prepare and sweep the workspaces) need.
/// They live here rather than beside either caller because whichever process runs
/// the operation answers them from its OWN core and session, which is what keeps
/// the workspace callbacks off the channel.
pub(crate) fn parse_bare_id(core: &GatedCore, source: &str) -> Option<String> {
    core.invoke_stateless_as::<ParsedModSource, _>(
        "parseModSource",
        &ParseModSourceParams {
            source: source.to_owned(),
            language: "en".to_owned(),
        },
    )
    .ok()
    .and_then(|parsed| parsed.metadata)
    .and_then(|metadata| metadata.id)
}

/// The clangd flag set for `compile_flags.txt` (`getCompileFlags`, stateless). See
/// [`parse_bare_id`] for why it lives here.
pub(crate) fn compile_flags(core: &GatedCore) -> Result<Vec<String>, HostError> {
    core.invoke_stateless_as::<Vec<String>, _>("getCompileFlags", &json!({}))
}

/// Whether a storage id is occupied (`doesModExist`, with the `local@` scope
/// already applied by the caller). See [`parse_bare_id`] for why it lives here.
pub(crate) fn does_mod_exist(
    session: &dyn SessionApi,
    storage_id: &str,
) -> Result<bool, HostError> {
    session.invoke_as::<bool, _>(
        "doesModExist",
        &ModIdParams {
            mod_id: storage_id.to_owned(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(mod_id: &str) -> EditorOpen {
        EditorOpen {
            mod_id: mod_id.to_owned(),
            mod_source: String::new(),
            reuse: false,
        }
    }

    #[test]
    fn a_bare_id_passes_the_charset_check() {
        for id in ["a", "taskbar-clock-customization", "mod-2", "0"] {
            assert!(open(id).check_mod_id().is_ok(), "refused {id:?}");
        }
    }

    #[test]
    fn an_id_outside_the_charset_is_refused() {
        // Path syntax, the scoped storage form (the field is the BARE id), and
        // everything else the core's `@id` rule leaves out.
        for id in [
            "",
            "..",
            "..\\..\\evil",
            "a/b",
            "c:evil",
            "local@demo",
            "Demo",
            "demo mod",
            "demo\n",
        ] {
            assert!(open(id).check_mod_id().is_err(), "accepted {id:?}");
        }
    }
}
