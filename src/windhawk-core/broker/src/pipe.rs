//! The named pipe: creation, connection, and overlapped reads and writes with
//! deadlines.
//!
//! Blocking-mode pipe calls cannot meet two requirements this channel has.
//! `ConnectNamedPipe` takes no timeout, so a listener could not bound how long
//! it waits for its peer; and a thread parked in a blocking `ReadFile` cannot be
//! released safely by closing the handle from another thread, because the handle
//! can be reused underneath the parked call. So every operation is issued
//! overlapped against a per-operation event, and every wait is a
//! `WaitForMultipleObjects` over that event plus a shutdown event, with a
//! timeout.
//!
//! One consequence is worth stating because it is the easy thing to get wrong:
//! when a wait ends for any reason other than the operation completing, the
//! operation is still pending and the kernel still owns the caller's buffer. So
//! every such path cancels the operation and then waits for the cancellation to
//! land before returning, and only then may the buffer be dropped.

use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, NAMED_PIPE_MODE, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};

use crate::security::PipeSecurity;

/// The pipe's kernel buffers. A frame larger than this simply crosses in several
/// kernel-level chunks; the size only trades memory against syscalls.
const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
/// How long a busy-pipe retry parks in `WaitNamedPipeW` before the caller gets a
/// chance to re-check its own deadline. `WaitNamedPipeW` is the one call in this
/// file that cannot be made overlapped, so it is sliced instead of waited out.
const PIPE_BUSY_SLICE_MS: u32 = 50;
/// How long a deadline-bearing wait parks before re-reading its deadline. Bounds
/// how stale a moving deadline can be, and how coarse a fixed one is.
const DEADLINE_SLICE: Duration = Duration::from_millis(250);

/// An owned kernel handle, closed exactly once.
struct OwnedHandle(HANDLE);

// SAFETY: a HANDLE is a process-wide reference to a kernel object, not a
// thread-affine resource: any thread may use it, and the kernel serializes its
// own state. Ownership here is exclusive (the handle is closed exactly once, in
// Drop), so moving one between threads and sharing one by reference are both
// sound. Sharing is what the design needs - the reader thread reads while
// another thread writes, which the kernel supports on a duplex pipe.
unsafe impl Send for OwnedHandle {}
// SAFETY: as above.
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle was produced by a Create*/Open* call in this
            // module and is closed exactly once, here.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A manual-reset event.
///
/// Manual reset, not auto: an operation's completion is observed twice - once by
/// the wait and once by `GetOverlappedResult` - and an auto-reset event consumed
/// by the first observation would leave the second waiting forever. Every
/// operation resets its event before issuing, so a stale signal cannot be
/// mistaken for a completion.
pub struct Event(OwnedHandle);

impl Event {
    fn create() -> io::Result<Event> {
        // SAFETY: null attributes (default security, not inheritable), manual
        // reset, initially unsignaled, unnamed.
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Event(OwnedHandle(handle)))
    }

    fn raw(&self) -> HANDLE {
        self.0.0
    }

    /// Signal it, and leave it signaled.
    pub fn set(&self) {
        // SAFETY: a valid manual-reset event handle owned by this value.
        unsafe { SetEvent(self.raw()) };
    }

    fn reset(&self) {
        // SAFETY: a valid manual-reset event handle owned by this value.
        unsafe { ResetEvent(self.raw()) };
    }
}

/// What an abandoned operation's completed bytes are worth to its caller.
///
/// The cancel races the operation, so a wait that was called off can still find
/// the bytes already transferred, and what to do with them is the caller's
/// question rather than the cancel's.
#[derive(Clone, Copy)]
enum Salvage {
    /// They are data that arrived: a read whose peer answered exactly on the
    /// deadline holds the frame it sent, and reporting the deadline instead
    /// would turn away a peer that wrote in time.
    Bytes,
    /// Nothing. The connect transfers nothing even when it succeeds, so a wait
    /// that was called off must not produce a peer nobody is waiting for any
    /// more; and a write abandoned part way leaves a partial frame on the wire,
    /// which ends the channel whatever the count says - so counting it would
    /// only send the caller round for the next chunk against a deadline that has
    /// already run out.
    Nothing,
}

/// One end of a duplex named pipe, driven with overlapped I/O.
///
/// Reads happen on one thread and writes are serialized behind a lock, so each
/// direction needs exactly one operation event. Both waits also watch the
/// shutdown event, which is what releases a parked reader without closing the
/// handle underneath it.
pub struct PipeStream {
    handle: OwnedHandle,
    read: Mutex<Event>,
    write: Mutex<Event>,
    shutdown: Arc<Event>,
}

impl PipeStream {
    /// Create the single listening instance of `name`.
    ///
    /// `FILE_FLAG_FIRST_PIPE_INSTANCE` makes creation fail if the name already
    /// exists, so a squatter is detected rather than served, and
    /// `nMaxInstances = 1` means exactly one peer can ever be connected. Byte
    /// mode, because framing is explicit; remote clients are refused outright.
    pub fn create_listener(name: &str, security: &PipeSecurity) -> io::Result<PipeStream> {
        let wide = wide(name);
        let attributes = security.attributes();
        // SAFETY: `wide` is a NUL-terminated wide string that outlives the call,
        // and `attributes` borrows the descriptor it points at, so `security`
        // outlives it. The kernel copies the descriptor into the object it
        // creates.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                listener_open_mode(),
                listener_pipe_mode(),
                1,
                PIPE_BUFFER_BYTES,
                PIPE_BUFFER_BYTES,
                0,
                attributes.as_ptr(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        PipeStream::wrap(handle)
    }

    /// Open the listening instance of `name`, retrying while it is momentarily
    /// occupied.
    ///
    /// The single instance can legitimately be busy: a peer that is in the
    /// process of being rejected still holds it. Treating the first
    /// `ERROR_PIPE_BUSY` as a failure would hand a denial of service to anything
    /// that can open the pipe once, so the open is a loop bounded by `deadline`.
    pub fn connect(name: &str, deadline: Instant) -> io::Result<PipeStream> {
        let wide = wide(name);
        loop {
            // SAFETY: `wide` is a NUL-terminated wide string that outlives the
            // call; the security-attributes and template-file parameters are
            // unused (null) as they are for an open of an existing object.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    connect_flags(),
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return PipeStream::wrap(handle);
            }

            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32)
            {
                return Err(error);
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            // Sliced rather than waited out: this is the one call here that
            // cannot be overlapped, so the slice bounds how long the loop can
            // overshoot its own deadline.
            // SAFETY: `wide` is a NUL-terminated wide string that outlives the
            // call. A timeout return is reported through GetLastError, which the
            // loop simply treats as "retry until the deadline".
            unsafe { WaitNamedPipeW(wide.as_ptr(), PIPE_BUSY_SLICE_MS) };
        }
    }

    fn wrap(handle: HANDLE) -> io::Result<PipeStream> {
        Ok(PipeStream {
            handle: OwnedHandle(handle),
            read: Mutex::new(Event::create()?),
            write: Mutex::new(Event::create()?),
            shutdown: Arc::new(Event::create()?),
        })
    }

    /// The raw handle, for the peer identity queries that take one.
    pub fn raw(&self) -> HANDLE {
        self.handle.0
    }

    /// The event that releases every wait on this pipe. Held by the owner so a
    /// reader parked on its next read can be let go without closing the handle
    /// underneath it.
    pub fn shutdown_signal(&self) -> Arc<Event> {
        Arc::clone(&self.shutdown)
    }

    /// Release every wait on this pipe, now and in future.
    pub fn signal_shutdown(&self) {
        self.shutdown.set();
    }

    /// Wait for a peer to connect to this listening instance.
    pub fn accept(&self, deadline: Instant) -> io::Result<()> {
        self.accept_until(&|| deadline)
    }

    /// As above, against a deadline that may move while the wait is in progress.
    pub fn accept_until(&self, deadline: &dyn Fn() -> Instant) -> io::Result<()> {
        let event = self
            .read
            .lock()
            .expect("the pipe read event lock is poisoned");
        event.reset();
        let mut overlapped = overlapped_for(&event);

        // SAFETY: `overlapped` lives until this function returns, and every path
        // out of the wait below either observes the operation complete or cancels
        // it and waits for the cancellation to land, so the kernel is never left
        // holding it.
        let connected = unsafe { ConnectNamedPipe(self.raw(), &mut overlapped) };
        if connected != 0 {
            return Ok(());
        }
        // SAFETY: reads this thread's last-error code, set by the call above.
        match unsafe { GetLastError() } {
            ERROR_IO_PENDING => self
                .await_overlapped_until(&mut overlapped, &|| Some(deadline()), Salvage::Nothing)
                .map(|_| ()),
            // The peer connected in the window between creating the pipe and
            // asking to be connected. There is nothing left to wait for.
            ERROR_PIPE_CONNECTED => Ok(()),
            error => Err(io::Error::from_raw_os_error(error as i32)),
        }
    }

    /// Drop the connected peer and return the instance to the listening state,
    /// so the listener can keep waiting for the peer it is actually after.
    pub fn disconnect(&self) {
        // SAFETY: a valid pipe-server handle. Failure means there was no peer to
        // disconnect, which is the state the caller wanted anyway.
        unsafe { DisconnectNamedPipe(self.raw()) };
    }

    /// Read what is available, up to `buf.len()`. Returns `Ok(0)` at end of
    /// stream, so the codec above can tell a peer that exited from one that
    /// failed.
    pub fn read(&self, buf: &mut [u8], deadline: Option<Instant>) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let event = self
            .read
            .lock()
            .expect("the pipe read event lock is poisoned");
        event.reset();
        let mut overlapped = overlapped_for(&event);

        let mut read = 0u32;
        let length = buf.len().min(u32::MAX as usize) as u32;
        // SAFETY: `buf` and `overlapped` both outlive this call - every path out
        // of the wait below leaves the operation completed or cancelled-and-
        // drained, so the kernel is not still writing into `buf` when this
        // returns. `read` receives the count on synchronous completion.
        let ok = unsafe {
            ReadFile(
                self.raw(),
                buf.as_mut_ptr(),
                length,
                &mut read,
                &mut overlapped,
            )
        };
        if ok != 0 {
            return Ok(read as usize);
        }
        // SAFETY: reads this thread's last-error code, set by the call above.
        match unsafe { GetLastError() } {
            ERROR_IO_PENDING => {
                match self.await_overlapped(&mut overlapped, deadline, Salvage::Bytes) {
                    Ok(transferred) => Ok(transferred as usize),
                    Err(error) if is_end_of_stream(&error) => Ok(0),
                    Err(error) => Err(error),
                }
            }
            error if is_end_of_stream_code(error) => Ok(0),
            error => Err(io::Error::from_raw_os_error(error as i32)),
        }
    }

    /// Write every byte of `buf` as one indivisible unit with respect to other
    /// writers on this pipe: the lock is held for the whole frame, so two
    /// writers never interleave a header with the other's payload.
    ///
    /// A frame that runs out of deadline part way through is a failure and not a
    /// partial success ([`Salvage::Nothing`]): what is on the wire is half a
    /// frame, which the caller answers by ending the channel.
    pub fn write_all(&self, buf: &[u8], deadline: Option<Instant>) -> io::Result<()> {
        let event = self
            .write
            .lock()
            .expect("the pipe write event lock is poisoned");
        let mut written = 0;
        while written < buf.len() {
            event.reset();
            let mut overlapped = overlapped_for(&event);

            let chunk = &buf[written..];
            let length = chunk.len().min(u32::MAX as usize) as u32;
            let mut transferred = 0u32;
            // SAFETY: `chunk` borrows `buf`, which outlives this call, and
            // `overlapped` outlives the wait below; as in `read`, every exit path
            // leaves the operation completed or cancelled-and-drained.
            let ok = unsafe {
                WriteFile(
                    self.raw(),
                    chunk.as_ptr(),
                    length,
                    &mut transferred,
                    &mut overlapped,
                )
            };
            let transferred = if ok != 0 {
                transferred
            } else {
                // SAFETY: reads this thread's last-error code, set by the call
                // above.
                match unsafe { GetLastError() } {
                    ERROR_IO_PENDING => {
                        self.await_overlapped(&mut overlapped, deadline, Salvage::Nothing)?
                    }
                    error => return Err(io::Error::from_raw_os_error(error as i32)),
                }
            };
            if transferred == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            written += transferred as usize;
        }
        Ok(())
    }

    /// Wait for a pending operation, and make sure the kernel is done with it
    /// before returning whatever the outcome was.
    fn await_overlapped(
        &self,
        overlapped: &mut OVERLAPPED,
        deadline: Option<Instant>,
        salvage: Salvage,
    ) -> io::Result<u32> {
        self.await_overlapped_until(overlapped, &|| deadline, salvage)
    }

    /// As above, but re-reading the deadline as it waits, so an operation can
    /// outlive the deadline it was issued under.
    ///
    /// A deadline that MOVES is not a nicety: a listener waiting on the short
    /// deadline of one way of starting a peer has to switch to a long one the
    /// moment a consent dialog goes up, and it learns that while it is already
    /// parked here. Extending it by cancelling and re-issuing the connect would
    /// leave a gap for a peer to arrive into, so the operation stays pending
    /// across the whole wait and only the timeout is sliced.
    fn await_overlapped_until(
        &self,
        overlapped: &mut OVERLAPPED,
        deadline: &dyn Fn() -> Option<Instant>,
        salvage: Salvage,
    ) -> io::Result<u32> {
        let handles = [overlapped.hEvent, self.shutdown.raw()];
        loop {
            let timeout = match deadline() {
                None => INFINITE,
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return self.abandon(overlapped, io::ErrorKind::TimedOut.into(), salvage);
                    }
                    let slice = remaining.min(DEADLINE_SLICE);
                    u32::try_from(slice.as_millis()).unwrap_or(u32::MAX - 1)
                }
            };
            // SAFETY: both handles are valid and outlive the call - the operation
            // event is owned by this pipe's lock guard, the shutdown event by the
            // `Arc` this pipe holds.
            let wait = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) };
            match wait {
                WAIT_OBJECT_0 => return self.overlapped_result(overlapped, false),
                // A slice ended with the operation still pending. Round again, so
                // a deadline that has moved out is honoured and one that has run
                // out is caught at the top.
                WAIT_TIMEOUT => continue,
                // The shutdown event, or a failed wait: either way this operation
                // is being abandoned.
                w if w == WAIT_OBJECT_0 + 1 => {
                    return self.abandon(
                        overlapped,
                        io::ErrorKind::ConnectionAborted.into(),
                        salvage,
                    );
                }
                _ => {
                    let reason = io::Error::last_os_error();
                    return self.abandon(overlapped, reason, salvage);
                }
            }
        }
    }

    /// Give up on a pending operation.
    ///
    /// Abandoning is not returning: the operation is still pending and the kernel
    /// still holds the caller's buffer, so the cancellation has to be waited for
    /// before either can go out of scope.
    ///
    /// The cancel RACES the operation, and the drain is what says which won. A
    /// cancel that landed first reports the operation aborted, which is the
    /// abandonment the caller asked for; one that arrived a moment too late finds
    /// the operation already complete, and whether that is worth anything is
    /// `salvage`'s question. A zero-byte completion is the abandonment either
    /// way - for a read that is end of stream.
    fn abandon(
        &self,
        overlapped: &mut OVERLAPPED,
        reason: io::Error,
        salvage: Salvage,
    ) -> io::Result<u32> {
        // SAFETY: `overlapped` is the pending operation's structure, still alive,
        // on this pipe's handle.
        unsafe { CancelIoEx(self.raw(), overlapped) };
        let drained = self.overlapped_result(overlapped, true);
        match salvage {
            Salvage::Bytes => match drained {
                Ok(transferred) if transferred > 0 => Ok(transferred),
                _ => Err(reason),
            },
            Salvage::Nothing => Err(reason),
        }
    }

    fn overlapped_result(&self, overlapped: &OVERLAPPED, wait: bool) -> io::Result<u32> {
        let mut transferred = 0u32;
        // SAFETY: `overlapped` describes an operation issued on this handle;
        // `transferred` receives the byte count. With `wait` set the call blocks
        // until the operation finishes or is cancelled, which is exactly what the
        // abandon path needs before the structure is dropped.
        let ok = unsafe {
            GetOverlappedResult(
                self.raw(),
                overlapped,
                &mut transferred,
                if wait { 1 } else { 0 },
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred)
    }

    /// A `Read` view with a deadline applied to each read, so the codec can be
    /// driven over this pipe exactly as it is driven over any byte stream.
    pub fn reader(&self, deadline: Option<Instant>) -> PipeReader<'_> {
        PipeReader {
            pipe: self,
            deadline,
        }
    }
}

/// A `Read` over a [`PipeStream`], carrying the deadline that applies to each
/// read it performs.
pub struct PipeReader<'a> {
    pipe: &'a PipeStream,
    deadline: Option<Instant>,
}

impl Read for PipeReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.pipe.read(buf, self.deadline)
    }
}

fn overlapped_for(event: &Event) -> OVERLAPPED {
    // SAFETY: OVERLAPPED is plain data (integers, a pointer, and a union of
    // those), for which an all-zero value is the documented initial state.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    overlapped.hEvent = event.raw();
    overlapped
}

/// Whether an error is the peer having gone away rather than a transport
/// failure. A named pipe reports the far end closing as a broken pipe; an
/// operation abandoned by a cancel reports as aborted.
fn is_end_of_stream(error: &io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|code| is_end_of_stream_code(code as u32))
}

fn is_end_of_stream_code(code: u32) -> bool {
    matches!(
        code,
        ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_NO_DATA | ERROR_OPERATION_ABORTED
    )
}

/// The listener's open mode. Pinned by a test rather than by review: dropping
/// `FILE_FLAG_FIRST_PIPE_INSTANCE` would silently start serving a squatter's
/// name, and dropping `FILE_FLAG_OVERLAPPED` would silently remove every
/// deadline in this file.
pub fn listener_open_mode() -> FILE_FLAGS_AND_ATTRIBUTES {
    PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED
}

/// The listener's pipe mode. Byte mode, because framing is explicit; remote
/// clients are refused outright.
pub fn listener_pipe_mode() -> NAMED_PIPE_MODE {
    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS
}

/// The connector's flags, and the single most important line in this transport.
///
/// A named-pipe client that specifies no security quality of service grants the
/// SERVER impersonation-level access to its token. The server here is the
/// unelevated process, so without `SECURITY_IDENTIFICATION` a server that got
/// this side to connect could call `ImpersonateNamedPipeClient` and execute with
/// this side's token - a complete elevation of privilege, independent of
/// anything the frames say. At identification level the server can still obtain
/// and query the token, which is what the listener's own verification needs, but
/// cannot act with it.
pub fn connect_flags() -> FILE_FLAGS_AND_ATTRIBUTES {
    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION
}

/// A single-use pipe name: `\\.\pipe\<prefix>.<32 lowercase hex>`.
///
/// Fresh per channel, never reused, never written to disk. The randomness is not
/// relied on as a secret - the descriptor and the peer checks are what keep
/// strangers out - it is what stops the name being squatted ahead of the process
/// that means to create it.
pub fn channel_name(prefix: &str) -> io::Result<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: a null algorithm handle with BCRYPT_USE_SYSTEM_PREFERRED_RNG asks
    // for the system RNG; `bytes` is a live buffer of exactly the length passed.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "BCryptGenRandom failed with status {status:#x}"
        )));
    }

    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(format!(r"\\.\pipe\{prefix}.{hex}"))
}

pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connect_flags_carry_identification_level_impersonation() {
        let flags = connect_flags();
        assert_eq!(
            flags & (SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION),
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            "without SQOS at identification level the pipe server could act with \
             this side's token"
        );
        assert_eq!(
            flags & FILE_FLAG_OVERLAPPED,
            FILE_FLAG_OVERLAPPED,
            "every deadline in this transport needs the handle to be overlapped"
        );
    }

    #[test]
    fn the_listener_refuses_a_squatted_name_and_remote_clients() {
        let open = listener_open_mode();
        assert_eq!(
            open & FILE_FLAG_FIRST_PIPE_INSTANCE,
            FILE_FLAG_FIRST_PIPE_INSTANCE
        );
        assert_eq!(open & FILE_FLAG_OVERLAPPED, FILE_FLAG_OVERLAPPED);
        assert_eq!(open & PIPE_ACCESS_DUPLEX, PIPE_ACCESS_DUPLEX);
        assert_eq!(
            listener_pipe_mode() & PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_REJECT_REMOTE_CLIENTS
        );
    }

    #[test]
    fn a_channel_name_is_fresh_every_time() {
        let first = channel_name("Windhawk.Test").unwrap();
        let second = channel_name("Windhawk.Test").unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(r"\\.\pipe\Windhawk.Test."));
        let suffix = first.rsplit('.').next().unwrap();
        assert_eq!(suffix.len(), 32);
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }
}
