//! Running external programs, in three forms. The capturing form runs
//! `schtasks.exe` (scheduled-task toggling in `applyAppSettings`). The detached
//! form runs the NSIS installer launch (`startUpdate`). The job-object
//! kill-on-cancel and stdin-piped-source form runs the compiler
//! (`compileInstalledMod`): the child runs in a job object so a `WhCoreCancel`
//! can terminate the whole process tree, and `stdin` carries the mod source
//! piped to `clang++`.

use std::num::NonZeroU32;

use crate::cancel::CancelToken;

/// A program to run: the executable, its argument vector (passed through
/// without shell interpretation), an optional working directory, and optional
/// bytes to pipe to the child's stdin.
#[derive(Debug, Clone, Default)]
pub struct ProcessRequest {
    pub program: String,
    pub args: Vec<String>,
    /// Working directory for the child (the compiler runs with `cwd` set to the
    /// compiler folder, matching the TS `spawn({cwd})`); `None` inherits ours.
    pub cwd: Option<String>,
    /// Bytes piped to the child's stdin then closed (the mod source for the
    /// compiler); `None` gives the child a null stdin (schtasks).
    pub stdin: Option<Vec<u8>>,
}

/// A fire-and-forget child that must outlive the session and the calling
/// process. `raw_args` is appended verbatim after the (quoted) program path
/// with no escaping - Windows `windowsVerbatimArguments` - because the NSIS
/// installer requires the `/D=` path to be the final, unquoted argument even
/// when it contains spaces.
#[derive(Debug, Clone)]
pub struct DetachedRequest {
    pub program: String,
    pub raw_args: String,
}

/// The captured result of a finished child.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A failure to start or wait on a child (distinct from the child exiting
/// nonzero, which is a successful `ProcessOutput`).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ProcessError {
    pub message: String,
    /// The raw OS error code, or `None` when not from a Win32 call (no `0`
    /// sentinel). `ProcessError` keeps its own shape rather than embedding
    /// `OsError`: it has no locus and, by default, no operation, so the
    /// mandatory-`operation` triple does not fit.
    pub os_error: Option<NonZeroU32>,
}

impl ProcessError {
    pub fn new(message: impl Into<String>, os_error: u32) -> Self {
        Self {
            message: message.into(),
            os_error: NonZeroU32::new(os_error),
        }
    }
}

pub trait Processes: Send + Sync {
    /// Run `request` to completion in a job object, capturing stdout/stderr
    /// fully. Cancellation (a hook on `cancel`) terminates the job, killing the
    /// child and its whole process tree (the compiler's `clang++` driver spawns
    /// sub-processes); the call then returns promptly with whatever exit the
    /// killed child produced - callers detect the kill by checking `cancel`
    /// rather than by the exit code (`compileInstalledMod`). Output capture is
    /// complete so callers can surface it (schtasks warnings; `COMPILER_FAILED`
    /// details). `request.stdin`, when set, is piped to the child and the pipe
    /// closed (the mod source).
    fn run_capture(
        &self,
        request: &ProcessRequest,
        cancel: &CancelToken,
    ) -> Result<ProcessOutput, ProcessError>;

    /// Spawn `request` detached and return immediately: stdio is not inherited
    /// and the child outlives the session (the NSIS installer, which restarts
    /// Windhawk and closes this process; `notifyTray` later). Failure to start
    /// is reported; the child's own exit is never awaited.
    fn spawn_detached(&self, request: &DetachedRequest) -> Result<(), ProcessError>;
}
