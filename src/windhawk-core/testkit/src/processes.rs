//! In-memory `Processes` fake: records the requests it was asked to run and
//! returns a canned `ProcessOutput`, so tests can assert on what was spawned
//! (e.g. the `schtasks.exe /change /tn ... /enable` lines, or the NSIS
//! installer launch) without touching the OS. Wired to a `FakeFiles` it also
//! leaves behind the artifact a zero-exit run names with `-o`, so a caller that
//! checks its compiler produced something sees what a real one would.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use windhawk_core_ports::{
    CancelToken, DetachedRequest, ProcessError, ProcessOutput, ProcessRequest, Processes,
};

use crate::files::FakeFiles;

/// The path a request names with the separate `-o <path>` - the spelling the
/// compile and PCH arg builders use.
fn output_path(args: &[String]) -> Option<&String> {
    let flag = args.iter().position(|arg| arg == "-o")?;
    args.get(flag + 1)
}

#[derive(Clone)]
pub struct FakeProcesses {
    calls: Arc<Mutex<Vec<ProcessRequest>>>,
    detached: Arc<Mutex<Vec<DetachedRequest>>>,
    /// The canned result every call returns; a nonzero exit code lets tests
    /// drive the schtasks warning path (and the compiler `COMPILER_FAILED`
    /// path).
    result: Arc<Mutex<Result<ProcessOutput, ProcessError>>>,
    /// When set, `spawn_detached` fails with this error (the installer
    /// launch-failure path).
    detached_fault: Arc<Mutex<Option<ProcessError>>>,
    /// When true, `run_capture` blocks on the cancel token until cancellation
    /// (or a generous timeout), modeling the real adapter's behavior of
    /// returning a killed child only once `WhCoreCancel` fires - so the
    /// compiler cancel path (kill + unlink pending) is exercisable.
    block_until_canceled: Arc<Mutex<bool>>,
    /// Where a zero-exit run writes the file it was told to produce. Set, the
    /// fake models the half of a compiler its caller checks for: that the
    /// artifact named by `-o` is on disk afterwards.
    output_files: Arc<Mutex<Option<FakeFiles>>>,
}

impl FakeProcesses {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            detached: Arc::new(Mutex::new(Vec::new())),
            result: Arc::new(Mutex::new(Ok(ProcessOutput {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            }))),
            detached_fault: Arc::new(Mutex::new(None)),
            block_until_canceled: Arc::new(Mutex::new(false)),
            output_files: Arc::new(Mutex::new(None)),
        }
    }

    /// Make a zero-exit `run_capture` write the file its arguments name with
    /// `-o` into `files`, the way clang leaves a DLL (or a `.pch`) behind. A
    /// compile that succeeds without producing its output is a real failure the
    /// caller rejects, so a fixture that never writes cannot reach the success
    /// path it means to test. A run with no `-o` writes nothing.
    pub fn set_output_files(&self, files: FakeFiles) {
        *self.output_files.lock().unwrap_or_else(|e| e.into_inner()) = Some(files);
    }

    /// Take the wiring back off, so a zero-exit run leaves nothing behind: a
    /// compiler that reports success without producing its output, which is
    /// what `-fsyntax-only`, an antivirus, or an output redirect the flag
    /// filter missed all look like from the caller's side.
    pub fn clear_output_files(&self) {
        *self.output_files.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn set_result(&self, result: Result<ProcessOutput, ProcessError>) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = result;
    }

    /// Make `run_capture` block until the cancel token is signaled (then return
    /// the canned result), so tests can drive the kill-on-cancel path.
    pub fn set_block_until_canceled(&self, block: bool) {
        *self
            .block_until_canceled
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = block;
    }

    /// Make `spawn_detached` fail (the installer launch-failure path).
    pub fn set_detached_fault(&self, error: ProcessError) {
        *self
            .detached_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    /// The requests passed to `run_capture`, in order.
    pub fn calls(&self) -> Vec<ProcessRequest> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The requests passed to `spawn_detached`, in order.
    pub fn detached_calls(&self) -> Vec<DetachedRequest> {
        self.detached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for FakeProcesses {
    fn default() -> Self {
        Self::new()
    }
}

impl Processes for FakeProcesses {
    fn run_capture(
        &self,
        request: &ProcessRequest,
        cancel: &CancelToken,
    ) -> Result<ProcessOutput, ProcessError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.clone());
        if *self
            .block_until_canceled
            .lock()
            .unwrap_or_else(|e| e.into_inner())
        {
            // Mirror the real adapter: return only once the kill (cancel)
            // arrives. The bounded wait keeps a mis-driven test from hanging.
            cancel.wait(Duration::from_secs(10));
        }
        let result = self
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Ok(output) = &result
            && output.exit_code == 0
            && let Some(files) = self
                .output_files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            && let Some(path) = output_path(&request.args)
        {
            files.seed(path, b"fake compiler output".to_vec());
        }
        result
    }

    fn spawn_detached(&self, request: &DetachedRequest) -> Result<(), ProcessError> {
        self.detached
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request.clone());
        match self
            .detached_fault
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
