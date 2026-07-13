//! The `Processes` adapter. Provides `run_capture`, for `schtasks.exe`;
//! `spawn_detached`, for the NSIS installer launch; and the job-object
//! kill-on-cancel and the stdin-piped-source form the compiler needs: the child
//! runs in a job object, a cancel hook terminates the job (and the whole
//! `clang++` process tree), and `request.stdin` is piped to the child.

use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};

use windhawk_core_ports::{
    CancelToken, DetachedRequest, ProcessError, ProcessOutput, ProcessRequest, Processes,
};

pub struct RealProcesses;

/// Owns a job-object handle, closed exactly once on drop. Shared (behind a
/// mutex, in an `Option`) between `run_capture` and its cancel hook so the
/// kill (`TerminateJobObject`) and the close (`CloseHandle`) can never race
/// into a use-after-close: each takes the mutex, and the close `take`s the
/// `Option`, after which the hook sees `None`.
struct JobHandle(HANDLE);

// SAFETY: a Win32 job-object handle is a kernel handle valid from any thread;
// TerminateJobObject and CloseHandle are thread-safe.
unsafe impl Send for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed exactly
        // once (the owning Option is taken exactly once, under the mutex).
        unsafe { CloseHandle(self.0) };
    }
}

impl Processes for RealProcesses {
    fn run_capture(
        &self,
        request: &ProcessRequest,
        cancel: &CancelToken,
    ) -> Result<ProcessOutput, ProcessError> {
        let mut command = Command::new(&request.program);
        command.args(&request.args);
        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }
        command
            .stdin(if request.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);

        let mut child = command.spawn().map_err(|e| {
            ProcessError::new(
                format!("failed to run {}: {e}", request.program),
                e.raw_os_error().unwrap_or(0) as u32,
            )
        })?;

        // Put the child in a job object so a cancel can kill the whole tree
        // (the clang++ driver spawns the real compiler). Best effort: if the
        // job cannot be created or assigned, the child still runs to completion
        // and only loses kill-on-cancel.
        // SAFETY: null attributes and a null (unnamed) name are valid; returns
        // null on failure.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let job_slot: Arc<Mutex<Option<JobHandle>>> = if job.is_null() {
            Arc::new(Mutex::new(None))
        } else {
            // SAFETY: the child is alive (not yet waited); its raw handle is a
            // valid process handle with the access AssignProcessToJobObject
            // needs, and the job was just created.
            unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };
            Arc::new(Mutex::new(Some(JobHandle(job))))
        };

        // Kill hook: terminate the job (and thus the process tree) on cancel.
        {
            let job_slot = job_slot.clone();
            cancel.on_cancel(Box::new(move || {
                let guard = job_slot.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(job) = guard.as_ref() {
                    // SAFETY: the handle is still open - the close below takes
                    // the same mutex, so while we hold it the Option is Some
                    // and the handle valid.
                    unsafe { TerminateJobObject(job.0, 1) };
                }
            }));
        }

        // Drain stdout/stderr on their own threads so a chatty compiler cannot
        // deadlock against a full pipe while we are writing stdin.
        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let out_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(s) = stdout.as_mut() {
                let _ = s.read_to_end(&mut buf);
            }
            buf
        });
        let err_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(s) = stderr.as_mut() {
                let _ = s.read_to_end(&mut buf);
            }
            buf
        });

        if let Some(data) = &request.stdin
            && let Some(si) = stdin.as_mut()
        {
            // A broken pipe (the child exited or was killed) is not our error.
            let _ = si.write_all(data);
        }
        drop(stdin); // close stdin so the compiler sees EOF

        let status = child.wait().map_err(|e| {
            ProcessError::new(
                format!("failed waiting for {}: {e}", request.program),
                e.raw_os_error().unwrap_or(0) as u32,
            )
        })?;

        let stdout = out_reader.join().unwrap_or_default();
        let stderr = err_reader.join().unwrap_or_default();

        // Close the job handle now the child has exited, under the mutex so a
        // racing cancel hook never terminates a closed handle.
        job_slot.lock().unwrap_or_else(|e| e.into_inner()).take();

        Ok(ProcessOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    fn spawn_detached(&self, request: &DetachedRequest) -> Result<(), ProcessError> {
        // `raw_arg` appends the tail verbatim after the (auto-quoted) program
        // path - the Windows `windowsVerbatimArguments` the NSIS `/D=` path
        // needs (last, unquoted, spaces and all). Detach from our console and
        // ignore stdio so the child outlives this process; dropping the Child
        // handle does not wait or kill, so the installer keeps running.
        let mut command = Command::new(&request.program);
        if !request.raw_args.is_empty() {
            command.raw_arg(&request.raw_args);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
            .map(|_child| ())
            .map_err(|e| {
                ProcessError::new(
                    format!("failed to start {}: {e}", request.program),
                    e.raw_os_error().unwrap_or(0) as u32,
                )
            })
    }
}
