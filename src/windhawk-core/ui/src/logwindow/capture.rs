//! In-process DBWIN capture: read Windhawk's live `[WH] `-prefixed
//! `OutputDebugString` stream directly from the system DBWIN shared buffer, so
//! the log window needs no bundled DebugView child. The classic Win32 monitor
//! handshake: hold the `DBWIN_BUFFER` file mapping plus the
//! `DBWIN_BUFFER_READY`/`DBWIN_DATA_READY` auto-reset events; a writer in any
//! process waits for BUFFER_READY, writes a leading `DWORD` pid then a
//! NUL-terminated ANSI message into the mapping, signals DATA_READY, then waits
//! again; the monitor wakes on DATA_READY, copies the message, and re-signals
//! BUFFER_READY.
//!
//! This mirrors the bundled `DbgViewMini.exe` (the extension's helper) so the log
//! window shows the same thing the extension did:
//! - **Both namespaces.** It captures the per-session `Local\` objects AND the
//!   cross-session `Global\` ones. The two run as separate loops on separate
//!   threads, because they now live in separate processes: the unelevated UI owns
//!   [`run_local`], and the elevated broker owns [`run_global`], which needs
//!   `SeCreateGlobalPrivilege`. Both feed the same tail buffer, so a reader sees
//!   one merged stream ordered by arrival, as before. Where there is no broker
//!   the UI runs both loops itself, and the `Global\` half is simply denied when
//!   it is not elevated.
//!   Only the local loop signals the pane's reveal: waiting for the broker would
//!   make the log pane un-openable in degraded mode, which is exactly when someone
//!   wants to read it.
//! - **Any integrity level.** The objects carry a permissive descriptor built from an
//!   SDDL whose mandatory label is Low, so a writer at any IL (a sandboxed/Low-IL or
//!   AppContainer process, or - crucially - an ordinary Medium-IL process while the UI
//!   runs elevated) can open and write to them.
//! - **Correct text.** `OutputDebugStringW` down-converts to the system code page
//!   before filling the CHAR buffer, so the bytes are CP_ACP, decoded here to UTF-16
//!   (not assumed UTF-8). Internal newlines collapse to spaces (one line per call).
//! - **Context.** Each line is prefixed `HH:MM:SS.mmm <pid> <process>` and keeps the
//!   `[WH] ` marker, matching DbgViewMini's output.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, SYSTEMTIME, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE,
    UnmapViewOfFile,
};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};

/// The DBWIN shared buffer is a fixed 4 KiB page: a leading `DWORD` pid then the
/// NUL-terminated ANSI message.
const DBWIN_BUFFER_SIZE: u32 = 4096;
const MESSAGE_OFFSET: usize = 4;
/// Wake from the wait in slices so the shutdown flag is observed promptly when
/// the window closes (capture only while open), and so a slow trickle of lines
/// is flushed even when neither the batch cap nor the flush interval is
/// reached.
const POLL_MS: u32 = 200;
/// Coalesce captured lines into batches: under a flood, hand them downstream in bulk
/// rather than one emit per line, so a runaway logger costs a bounded number of IPC
/// crossings and DOM writes instead of one per message.
///
/// `FLUSH_INTERVAL` bounds the added latency: a pending batch is flushed at least this
/// often while any DBWIN traffic keeps the loop awake (the check runs every iteration,
/// not only on a `[WH]` line), and once traffic stops the trailing batch is flushed
/// within `POLL_MS`. `BATCH_MAX` bounds the size of a single flush so a burst drains in
/// fixed-size chunks and no unbounded `Vec` builds up.
const FLUSH_INTERVAL: Duration = Duration::from_millis(50);
const BATCH_MAX: usize = 1000;
/// Windhawk tags its debug output with this prefix (the extension filters on it via
/// `DbgViewMini --pattern '[WH] *'`).
const WH_PREFIX: &str = "[WH] ";
/// The security descriptor for the DBWIN objects, matching DbgViewMini: a permissive
/// DACL (Everyone, System, Admins, Anonymous, Restricted, ALL APPLICATION PACKAGES)
/// plus a Low-integrity mandatory label so a writer at ANY integrity level can write.
/// Without the Low label, a higher-IL UI (the `mustRunAsAdmin` path) would capture
/// nothing from ordinary Medium-IL processes.
const DBWIN_SDDL: &str = "D:(A;;GRGWGX;;;WD)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGWGX;;;AN)(A;;GRGWGX;;;RC)(A;;GRGWGX;;;S-1-15-2-1)S:(ML;;NW;;;LW)";

/// Capture the per-session `Local\` objects - the ones Windhawk's injected mods
/// write to - until `shutdown` is set, delivering formatted `[WH] ` lines to
/// `on_lines` in batches.
///
/// This is the loop the log pane's reveal follows: it emits the startup status (the
/// "Listening..." banner on success, the error plus the DebugView hint on failure)
/// and then calls `init_done`, whether or not capture started, so the caller can
/// reveal the pane knowing the status is already in the tail buffer. All the raw
/// handles/views live and die on this one thread, so they never cross a boundary.
pub fn run_local(on_lines: &dyn Fn(&[String]), shutdown: &AtomicBool, init_done: &dyn Fn()) {
    let security = SecurityDescriptor::from_sddl(DBWIN_SDDL);
    let sa = security.as_ref().map(SecurityDescriptor::attributes);

    // The startup status lines are low-volume, so each goes out as its own one-line
    // batch through the same sink the live stream uses.
    let status = |line: String| on_lines(&[line]);

    let monitor = match Dbwin::create(Namespace::Local, sa.as_ref()) {
        Ok(monitor) => Some(monitor),
        Err(error) => {
            status(format!("Local capture error: {error}."));
            status(
                "Another debug monitor (such as DebugView) might already be running.".to_owned(),
            );
            None
        }
    };

    let Some(monitor) = monitor else {
        // Nothing could be monitored; the error is already in the tail buffer.
        // Unblock the reveal, then stop - reopening the pane retries from scratch.
        init_done();
        return;
    };

    status("Listening for debug messages...".to_owned());
    init_done();

    pump(&[monitor], on_lines, shutdown);
    // `security` (and its LocalFree) drops here, after every Create* that referenced
    // it has returned and the kernel objects hold their own security copy.
}

/// Capture the cross-session `Global\` objects - the output of processes in other
/// sessions and of service-hosted ones - until `shutdown` is set, delivering
/// formatted `[WH] ` lines to `on_lines` in batches.
///
/// Creating them needs `SeCreateGlobalPrivilege`, so this is the half the elevated
/// broker runs. `report_denial` says whether a denial is worth a line: it is in the
/// broker, which exists to hold that privilege and has failed at its job without
/// it, and it is not in an unelevated UI running without a broker, where the denial
/// is the expected answer and the banner already explains the situation.
///
/// It emits no "Listening..." banner and signals no reveal: those belong to
/// [`run_local`], the half that is always present.
pub fn run_global(on_lines: &dyn Fn(&[String]), shutdown: &AtomicBool, report_denial: bool) {
    let security = SecurityDescriptor::from_sddl(DBWIN_SDDL);
    let sa = security.as_ref().map(SecurityDescriptor::attributes);

    let monitor = match Dbwin::create(Namespace::Global, sa.as_ref()) {
        Ok(monitor) => monitor,
        Err(DbwinError::AccessDenied) if !report_denial => return,
        Err(error) => {
            on_lines(&[format!("Global capture error: {error}.")]);
            return;
        }
    };

    pump(&[monitor], on_lines, shutdown);
}

/// Drive the monitors on this thread: signal every buffer free, then wait on all the
/// DATA_READY events at once, reading whichever fires.
///
/// Lines are coalesced into batches and handed to `on_lines` in bulk - at most
/// `BATCH_MAX` at a time, and at least every `FLUSH_INTERVAL` while any DBWIN traffic
/// keeps the loop awake - so a flood costs a bounded number of downstream emits (and DOM
/// writes) rather than one per line. The DBWIN handshake supplies the backpressure: this
/// thread
/// only re-signals a writer's buffer after copying its message out, so a runaway logger
/// is throttled to the rate this loop drains, never dropping this thread's memory into
/// an unbounded backlog.
fn pump(monitors: &[Dbwin], on_lines: &dyn Fn(&[String]), shutdown: &AtomicBool) {
    let data_ready: Vec<HANDLE> = monitors.iter().map(|m| m.data_ready).collect();
    for monitor in monitors {
        // SAFETY: buffer_ready is a valid auto-reset event handle for this thread.
        unsafe { SetEvent(monitor.buffer_ready) };
    }

    let mut names = ProcessNames::new();
    let mut batch: Vec<String> = Vec::new();
    let mut last_flush = Instant::now();
    while !shutdown.load(Ordering::Acquire) {
        // SAFETY: every handle in `data_ready` is a valid auto-reset event handle
        // that outlives this call (the `Dbwin`s are borrowed for the whole pump).
        let wait = unsafe {
            WaitForMultipleObjects(data_ready.len() as u32, data_ready.as_ptr(), 0, POLL_MS)
        };
        if wait == WAIT_TIMEOUT {
            // No message this slice: flush whatever accumulated so a slow trickle is
            // not held back, then loop to re-check shutdown.
            flush(&mut batch, on_lines, &mut last_flush);
            continue;
        }
        let index = wait.wrapping_sub(WAIT_OBJECT_0) as usize;
        if index >= monitors.len() {
            break; // WAIT_FAILED or an unexpected wake; stop capturing.
        }
        let monitor = &monitors[index];
        let (pid, message) = monitor.read_record();
        // Release this monitor's buffer so its blocked writer proceeds with the next
        // message; the message was already copied out, so an immediate overwrite is
        // safe.
        // SAFETY: buffer_ready is a valid auto-reset event handle.
        unsafe { SetEvent(monitor.buffer_ready) };
        if message.starts_with(WH_PREFIX) {
            batch.push(format!(
                "{} {} {}  {}",
                local_timestamp(),
                pid,
                names.get(pid),
                message
            ));
            // Cap the batch so a burst drains in fixed-size chunks and no unbounded
            // `Vec` builds up; deliver the moment it is full.
            if batch.len() >= BATCH_MAX {
                flush(&mut batch, on_lines, &mut last_flush);
            }
        }
        // Bound the coalesce latency in wall-clock time, checked every iteration - even
        // after a non-`[WH]` message. DBWIN carries all system-wide OutputDebugString
        // traffic, so gating this on a `[WH]` match would let unrelated debug output
        // keep the wait from timing out and hold a pending `[WH]` batch indefinitely.
        if last_flush.elapsed() >= FLUSH_INTERVAL {
            flush(&mut batch, on_lines, &mut last_flush);
        }
    }
    flush(&mut batch, on_lines, &mut last_flush);
}

/// Hand the accumulated batch to `on_lines` and reset it. A no-op on an empty batch, so
/// the idle-timeout and shutdown paths can call it unconditionally.
fn flush(batch: &mut Vec<String>, on_lines: &dyn Fn(&[String]), last_flush: &mut Instant) {
    if !batch.is_empty() {
        on_lines(batch);
        batch.clear();
    }
    *last_flush = Instant::now();
}

/// One namespace's held DBWIN monitor objects. `Drop` releases them.
struct Dbwin {
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    buffer_ready: HANDLE,
    data_ready: HANDLE,
}

impl Dbwin {
    fn create(
        namespace: Namespace,
        attributes: Option<&SECURITY_ATTRIBUTES>,
    ) -> Result<Dbwin, DbwinError> {
        let sa = attributes.map_or(std::ptr::null(), |a| a as *const SECURITY_ATTRIBUTES);
        // Each object is created with FAILS-IF-EXISTS semantics: a non-NULL handle
        // plus ERROR_ALREADY_EXISTS means another monitor owns DBWIN, so we bail.
        let buffer_ready = create_event(&namespace.object("DBWIN_BUFFER_READY"), sa)?;
        let data_ready = match create_event(&namespace.object("DBWIN_DATA_READY"), sa) {
            Ok(handle) => handle,
            Err(error) => {
                close(buffer_ready);
                return Err(error);
            }
        };
        let mapping = match create_mapping(&namespace.object("DBWIN_BUFFER"), sa) {
            Ok(handle) => handle,
            Err(error) => {
                close(data_ready);
                close(buffer_ready);
                return Err(error);
            }
        };
        // SAFETY: a valid mapping handle; FILE_MAP_READ maps the whole 4 KiB page.
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0) };
        if view.Value.is_null() {
            let error = DbwinError::from_last_error();
            close(mapping);
            close(data_ready);
            close(buffer_ready);
            return Err(error);
        }

        Ok(Dbwin {
            mapping,
            view,
            buffer_ready,
            data_ready,
        })
    }

    /// Copy the pid (leading `DWORD`) and the message (CP_ACP-decoded, newline-collapsed)
    /// out of the mapping.
    fn read_record(&self) -> (u32, String) {
        let base = self.view.Value as *const u8;
        // SAFETY: the mapped view spans DBWIN_BUFFER_SIZE bytes; the pid is the leading
        // DWORD. read_unaligned is sound regardless of the view's alignment (it is in
        // fact page-aligned). The event handshake guarantees the writer is blocked on
        // BUFFER_READY (not mid-write) until pump() re-signals it.
        let pid = unsafe { base.cast::<u32>().read_unaligned() };
        // SAFETY: the `max` bytes after the pid DWORD are within the 4 KiB view.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                base.add(MESSAGE_OFFSET),
                DBWIN_BUFFER_SIZE as usize - MESSAGE_OFFSET,
            )
        };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        (pid, collapse_newlines(&decode_acp(&bytes[..end])))
    }
}

impl Drop for Dbwin {
    fn drop(&mut self) {
        // SAFETY: each handle/view was produced by a Create*/MapViewOfFile in `create`
        // and is released exactly once here.
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.mapping);
            CloseHandle(self.data_ready);
            CloseHandle(self.buffer_ready);
        }
    }
}

/// Which DBWIN namespace to monitor.
#[derive(Clone, Copy)]
enum Namespace {
    /// `Local\` - the per-session objects ordinary user processes (Windhawk's injected
    /// mods) write to.
    Local,
    /// `Global\` - the cross-session objects; capturing them needs SeCreateGlobalPrivilege.
    Global,
}

impl Namespace {
    fn object(self, base: &str) -> String {
        let prefix = match self {
            Namespace::Local => "Local\\",
            Namespace::Global => "Global\\",
        };
        format!("{prefix}{base}")
    }
}

/// A failure creating a DBWIN object. `AlreadyExists` (another monitor) and
/// `AccessDenied` (global without privilege) are distinguished so the caller can keep
/// going on the latter.
enum DbwinError {
    AlreadyExists,
    AccessDenied,
    Failed(u32),
}

impl DbwinError {
    fn from_last_error() -> DbwinError {
        // SAFETY: GetLastError reads the calling thread's last-error value.
        match unsafe { GetLastError() } {
            ERROR_ACCESS_DENIED => DbwinError::AccessDenied,
            code => DbwinError::Failed(code),
        }
    }
}

impl std::fmt::Display for DbwinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbwinError::AlreadyExists => {
                f.write_str("another monitor already owns the DBWIN objects")
            }
            DbwinError::AccessDenied => f.write_str("access denied"),
            DbwinError::Failed(code) => write!(f, "Win32 error {code}"),
        }
    }
}

/// The security descriptor for the DBWIN objects, owned and `LocalFree`d on drop.
struct SecurityDescriptor {
    descriptor: *mut core::ffi::c_void,
}

impl SecurityDescriptor {
    fn from_sddl(sddl: &str) -> Option<SecurityDescriptor> {
        let sddl_w = wide(sddl);
        let mut descriptor: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: sddl_w is NUL-terminated; on success `descriptor` receives a
        // LocalAlloc'd security descriptor (freed in Drop). A null size out-pointer is
        // allowed.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_w.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || descriptor.is_null() {
            None
        } else {
            Some(SecurityDescriptor { descriptor })
        }
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: `descriptor` was allocated by
        // ConvertStringSecurityDescriptorToSecurityDescriptorW and is freed once here.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// Full re-snapshot cadence: refresh the pid -> name table at least this often so a
/// long-lived pid still resolves after the process table has churned.
const NAMES_FULL_REFRESH: Duration = Duration::from_secs(60);
/// Miss-driven re-snapshot floor: a line from a pid not in the table triggers a refresh
/// to pick up a newly started process, but no more than once per this interval. Without
/// the floor, a flood from a pid that is absent (e.g. one that already exited, so it is
/// never found) would run a full system process enumeration on every single line.
const NAMES_MISS_REFRESH: Duration = Duration::from_secs(1);

/// Process-id -> exe-name cache for labelling lines, mirroring DbgViewMini.
struct ProcessNames {
    by_pid: HashMap<u32, String>,
    refreshed: Option<Instant>,
}

impl ProcessNames {
    fn new() -> ProcessNames {
        ProcessNames {
            by_pid: HashMap::new(),
            refreshed: None,
        }
    }

    fn get(&mut self, pid: u32) -> String {
        let age = self.refreshed.map(|at| at.elapsed());
        let miss = !self.by_pid.contains_key(&pid);
        if should_refresh(age, miss) {
            self.by_pid = snapshot_process_names();
            self.refreshed = Some(Instant::now());
        }
        self.by_pid
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| "<unknown>".to_owned())
    }
}

/// Whether to re-snapshot the pid -> name table, given the time since the last snapshot
/// (`age`, `None` if never taken) and whether the wanted pid is currently missing from
/// it. Refreshes on the periodic full cadence, or - for a miss - to pick up a
/// just-started process, but no more than once per `NAMES_MISS_REFRESH`. Rate-limiting
/// the miss path is what stops a flood of lines from an absent pid (e.g. one that has
/// already exited, so it is never found) enumerating the whole process table per line.
fn should_refresh(age: Option<Duration>, miss: bool) -> bool {
    let periodic = age.is_none_or(|d| d >= NAMES_FULL_REFRESH);
    let miss_due = miss && age.is_none_or(|d| d >= NAMES_MISS_REFRESH);
    periodic || miss_due
}

/// Create a `Global\`/`Local\` auto-reset event (non-signaled). See [`classify`].
fn create_event(name: &str, sa: *const SECURITY_ATTRIBUTES) -> Result<HANDLE, DbwinError> {
    let name_w = wide(name);
    // SAFETY: auto-reset (0) + non-signaled (0); NUL-terminated name; `sa` is a valid
    // SECURITY_ATTRIBUTES pointer or null. Returns NULL on failure.
    let handle = unsafe { CreateEventW(sa, 0, 0, name_w.as_ptr()) };
    classify(handle)
}

/// Create the pagefile-backed `DBWIN_BUFFER` mapping. See [`classify`].
fn create_mapping(name: &str, sa: *const SECURITY_ATTRIBUTES) -> Result<HANDLE, DbwinError> {
    let name_w = wide(name);
    // SAFETY: INVALID_HANDLE_VALUE + a nonzero size requests a pagefile-backed mapping;
    // NUL-terminated name; `sa` is a valid pointer or null. Returns NULL on failure.
    let handle = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            sa,
            PAGE_READWRITE,
            0,
            DBWIN_BUFFER_SIZE,
            name_w.as_ptr(),
        )
    };
    classify(handle)
}

/// Validate a freshly created named kernel object: NULL -> the create's error;
/// non-NULL but `ERROR_ALREADY_EXISTS` -> another monitor owns it (close + bail).
fn classify(handle: HANDLE) -> Result<HANDLE, DbwinError> {
    if handle.is_null() {
        return Err(DbwinError::from_last_error());
    }
    // SAFETY: reads the create's status before any other Win32 call can clobber it.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        close(handle);
        return Err(DbwinError::AlreadyExists);
    }
    Ok(handle)
}

fn close(handle: HANDLE) {
    // SAFETY: handle was returned by a Create* call in this module and is closed once.
    unsafe { CloseHandle(handle) };
}

/// Local wall-clock `HH:MM:SS.mmm`, matching DbgViewMini's timestamp.
fn local_timestamp() -> String {
    let mut st = SYSTEMTIME::default();
    // SAFETY: GetLocalTime fills the SYSTEMTIME out-param.
    unsafe { GetLocalTime(&mut st) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

/// Snapshot every running process's id -> exe name (the base file name).
fn snapshot_process_names() -> HashMap<u32, String> {
    let mut names = HashMap::new();
    // SAFETY: a process snapshot of the whole system; INVALID_HANDLE_VALUE on failure.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return names;
    }
    // PROCESSENTRY32W is a C POD; dwSize must be set before the first enumeration call.
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot is valid; entry is sized.
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        names.insert(entry.th32ProcessID, wide_buf_to_string(&entry.szExeFile));
        // SAFETY: same valid snapshot and entry.
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    // SAFETY: the snapshot handle is closed exactly once.
    unsafe { CloseHandle(snapshot) };
    names
}

/// Decode a CP_ACP (system code page) byte slice to a `String`. `OutputDebugStringW`
/// down-converts to CP_ACP before filling the CHAR DBWIN buffer, so this is the
/// faithful decode; a non-decodable byte run falls back to lossy UTF-8.
fn decode_acp(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let len = i32::try_from(bytes.len()).unwrap_or(i32::MAX);
    // SAFETY: a null output buffer with size 0 asks MultiByteToWideChar for the
    // required wide length.
    let needed =
        unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` has `needed` units; same input slice.
    let written =
        unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed) };
    String::from_utf16_lossy(&wide[..written.max(0) as usize])
}

/// Collapse internal `\r`/`\n` runs to single spaces and drop a trailing newline run,
/// so a multi-line debug string is one log line (DbgViewMini's `StrRemoveNewlines`).
fn collapse_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut trailing_start = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' || c == '\n' {
            if trailing_start.is_none() {
                trailing_start = Some(out.len());
            }
            out.push(' ');
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
        } else {
            out.push(c);
            trailing_start = None;
        }
    }
    if let Some(start) = trailing_start {
        out.truncate(start);
    }
    out
}

/// Decode a NUL-terminated UTF-16 buffer (e.g. `PROCESSENTRY32W::szExeFile`).
fn wide_buf_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A NUL-terminated UTF-16 buffer for a `PCWSTR` argument.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_newlines_joins_internal_runs_and_drops_the_trailing_run() {
        assert_eq!(collapse_newlines("[WH] a\r\nb\nc\r\n"), "[WH] a b c");
        assert_eq!(collapse_newlines("no newlines"), "no newlines");
        assert_eq!(collapse_newlines("trailing\n\n"), "trailing");
        assert_eq!(collapse_newlines(""), "");
    }

    #[test]
    fn decode_acp_passes_ascii_through() {
        // ASCII is identical in every ANSI code page; the [WH] marker is preserved.
        assert_eq!(decode_acp(b"[WH] [mod] hello"), "[WH] [mod] hello");
        assert_eq!(decode_acp(b""), "");
    }

    #[test]
    fn flush_delivers_a_nonempty_batch_then_clears_and_resets() {
        let delivered = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let on_lines = |lines: &[String]| delivered.borrow_mut().push(lines.to_vec());

        let mut batch = vec!["a".to_owned(), "b".to_owned()];
        // Start with a stale timer to prove the flush resets it.
        let mut last_flush = Instant::now() - Duration::from_secs(10);
        flush(&mut batch, &on_lines, &mut last_flush);

        assert_eq!(
            *delivered.borrow(),
            vec![vec!["a".to_owned(), "b".to_owned()]]
        );
        assert!(batch.is_empty());
        assert!(last_flush.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn flush_delivers_nothing_on_an_empty_batch_but_still_resets_the_clock() {
        let delivered = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let on_lines = |lines: &[String]| delivered.borrow_mut().push(lines.to_vec());

        let mut batch: Vec<String> = Vec::new();
        let mut last_flush = Instant::now() - Duration::from_secs(10);
        flush(&mut batch, &on_lines, &mut last_flush);

        // The idle-timeout and every-iteration paths call flush unconditionally, so an
        // empty flush must emit nothing yet still advance the coalesce clock.
        assert!(delivered.borrow().is_empty());
        assert!(last_flush.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn should_refresh_forces_the_first_snapshot() {
        // Never snapshotted: refresh regardless of hit/miss.
        assert!(should_refresh(None, false));
        assert!(should_refresh(None, true));
    }

    #[test]
    fn should_refresh_rate_limits_the_miss_path() {
        // A miss within the floor does NOT refresh - the guard against a flood from an
        // absent pid enumerating the whole process table per line.
        assert!(!should_refresh(Some(Duration::ZERO), true));
        // Past the floor, a miss refreshes to pick up a just-started process.
        assert!(should_refresh(Some(NAMES_MISS_REFRESH), true));
    }

    #[test]
    fn should_refresh_keeps_a_fresh_hit_and_honours_the_full_cadence() {
        // A hit on a fresh table, and one just shy of the full cadence: no refresh.
        assert!(!should_refresh(Some(Duration::ZERO), false));
        assert!(!should_refresh(
            Some(NAMES_FULL_REFRESH - Duration::from_millis(1)),
            false
        ));
        // Past the full cadence: refresh even on a hit, so a long-lived pid re-resolves
        // after the table churns.
        assert!(should_refresh(Some(NAMES_FULL_REFRESH), false));
    }
}
