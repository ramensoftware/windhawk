//! The pipe's security descriptor, peer identification, and the verification
//! policy.
//!
//! The two ends identify each other by DIFFERENT mechanisms, because they do not
//! have the same instruments available.
//!
//! The listening end reads the peer's TOKEN: it is the pipe server, so
//! `ImpersonateNamedPipeClient` plus `OpenThreadToken` yields a token it can
//! query, at identification level, which answers the integrity question without
//! a handle to the process. It must not go through `OpenProcess`, because an
//! unelevated process opening an elevated one is not reliably permitted - a
//! filtered administrator token carries the Administrators SID as deny-only, and
//! an elevated process's default descriptor commonly grants Administrators
//! rather than the user.
//!
//! The connecting end identifies the peer PROCESS: an elevated process opening a
//! medium-integrity one always works, so it performs the full check including
//! the image on disk. This is the load-bearing direction - it is what stops an
//! arbitrary process from being served privileged operations - and it may not be
//! weakened or made best effort. The listening end's check is anti-spoofing and
//! diagnostics: a peer that somehow passed the descriptor could feed it false
//! data, but gains nothing by doing so, so that direction may degrade to a
//! warning when a token cannot be obtained.
//!
//! **Neither direction compares the peer's token USER, and the policy below has
//! no field for it.** An over-the-shoulder elevation puts the privileged end on
//! an administrator's account while the unprivileged end stays on the standard
//! user's, so a same-user check would reject the one configuration a
//! non-administrator has. Its absence is the design, not an omission.
//!
//! Both directions start from a process ID, and Windows offers no way to obtain
//! the peer's process itself from a pipe - only its id. So everything either side
//! learns about a peer is learned by looking that id up afterwards, and a peer
//! that exits with its pipe end still connected could in principle have its id
//! recycled before the lookup runs, putting an unrelated process in front of the
//! checks. It is the standard caveat of every pid-based identification on this
//! platform rather than a hole here: the checks then apply in full to whatever
//! process was found, so passing them means being elevated, in this session, and
//! (on the load-bearing direction) running this same image, which is not a
//! position an attacker reaches by winning a race.

use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, PSECURITY_DESCRIPTOR,
    RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER,
    TokenIntegrityLevel, TokenUser,
};
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, ImpersonateNamedPipeClient,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, OpenProcess, OpenProcessToken,
    OpenThreadToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};

use crate::pipe::{PipeStream, wide};

/// A Windows integrity level, as the RID of the token's mandatory label.
///
/// Held as the RID rather than as an enumeration of the named levels because the
/// mandatory policy orders levels by exactly this number, so comparison is the
/// natural operation and an unnamed intermediate level compares correctly
/// instead of having to be added here first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Integrity(pub u32);

impl Integrity {
    pub const UNTRUSTED: Integrity = Integrity(0x0000);
    pub const LOW: Integrity = Integrity(0x1000);
    pub const MEDIUM: Integrity = Integrity(0x2000);
    pub const HIGH: Integrity = Integrity(0x3000);
    pub const SYSTEM: Integrity = Integrity(0x4000);
}

impl std::fmt::Display for Integrity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Integrity::UNTRUSTED => write!(f, "untrusted"),
            Integrity::LOW => write!(f, "low"),
            Integrity::MEDIUM => write!(f, "medium"),
            Integrity::HIGH => write!(f, "high"),
            Integrity::SYSTEM => write!(f, "system"),
            Integrity(rid) => write!(f, "integrity {rid:#x}"),
        }
    }
}

/// What a peer has to satisfy to be served or believed.
///
/// A value rather than a hard-coded sequence, so a test can construct a relaxed
/// policy directly and no environment variable or `cfg(test)` branch has to
/// exist in a shipping build to make the checks testable.
///
/// There is no `same_user` field; see this module's header for why, and treat
/// its absence as load-bearing rather than as something to helpfully correct.
#[derive(Debug, Clone)]
pub struct PeerPolicy {
    /// The integrity the peer must have. Read as a FLOOR on the listening side
    /// (the peer must be at least this privileged, since it is the side doing the
    /// privileged work) and as a CEILING on the connecting side (the peer must be
    /// no more privileged than this, since an already elevated peer is not the
    /// unelevated process this side exists to serve).
    pub integrity: Integrity,
    /// Whether the peer must be in this process's logon session. This is what
    /// replaces the user comparison: an over-the-shoulder elevation changes the
    /// account but not the session.
    pub same_session: bool,
    /// Whether the peer's image on disk must be this process's own. Only the
    /// connecting side can answer this - the listening side has no process handle
    /// to ask with - so it is ignored there.
    pub same_image: bool,
    /// The process the caller actually started, when it could learn one.
    ///
    /// Complementary evidence rather than a substitute for the integrity check:
    /// it says nothing about privilege, but it answers a sharper question than
    /// the token can - not "some privileged process" but "the process I asked
    /// for". `Option` because the launch path may not be able to report a pid. It
    /// lives in the policy rather than at the call site so the whole check has
    /// one home and one set of tests.
    pub expected_pid: Option<u32>,
}

/// Why a peer was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    Integrity {
        found: Option<Integrity>,
        required: Integrity,
    },
    Session {
        found: Option<u32>,
        expected: Option<u32>,
    },
    Image {
        found: Option<PathBuf>,
        expected: PathBuf,
    },
    Pid {
        found: u32,
        expected: u32,
    },
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::Integrity { found, required } => match found {
                Some(found) => write!(f, "peer integrity is {found}, required {required}"),
                None => write!(f, "peer integrity is unknown, required {required}"),
            },
            RejectReason::Session { found, expected } => {
                write!(f, "peer logon session is {found:?}, expected {expected:?}")
            }
            RejectReason::Image { found, expected } => write!(
                f,
                "peer image is {}, expected {}",
                found
                    .as_ref()
                    .map_or("unknown".into(), |p| p.display().to_string()),
                expected.display()
            ),
            RejectReason::Pid { found, expected } => {
                write!(f, "peer is process {found}, expected {expected}")
            }
        }
    }
}

/// This process's own identity, resolved once and compared against every peer.
#[derive(Debug, Clone)]
pub struct SelfIdentity {
    pub pid: u32,
    pub session: Option<u32>,
    pub image: PathBuf,
}

impl SelfIdentity {
    pub fn resolve() -> io::Result<SelfIdentity> {
        // SAFETY: takes no arguments and only reads this process's id.
        let pid = unsafe { GetCurrentProcessId() };
        Ok(SelfIdentity {
            pid,
            session: session_of(pid),
            image: std::env::current_exe()?,
        })
    }
}

/// A peer identified from the listening side: everything obtainable without a
/// handle to its process.
#[derive(Debug, Clone)]
pub struct ClientPeer {
    pub pid: u32,
    pub session: Option<u32>,
    /// `None` when the peer's token could not be obtained at all, which this
    /// direction degrades on rather than rejects.
    pub integrity: Option<Integrity>,
}

/// A peer identified from the connecting side, through a handle to its process.
#[derive(Debug, Clone)]
pub struct ServerPeer {
    pub pid: u32,
    pub session: Option<u32>,
    pub integrity: Integrity,
    pub image: PathBuf,
}

/// A peer that passed, and what could not be established about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The peer's token was unobtainable, so its integrity was not checked. Only
    /// ever set on the listening side, which is allowed to degrade to a warning.
    pub integrity_unverified: bool,
}

impl PeerPolicy {
    /// Apply the policy from the LISTENING side: integrity as a floor, no image
    /// check, and the launched-pid binding against both the pipe's client pid and
    /// the pid the peer claimed in its handshake.
    pub fn evaluate_client(
        &self,
        peer: &ClientPeer,
        claimed_pid: u32,
        me: &SelfIdentity,
    ) -> Result<Accepted, RejectReason> {
        let integrity_unverified = match peer.integrity {
            Some(found) if found >= self.integrity => false,
            Some(found) => {
                return Err(RejectReason::Integrity {
                    found: Some(found),
                    required: self.integrity,
                });
            }
            // The token could not be read. This direction is anti-spoofing, not
            // the privilege boundary, so the caller is told and the channel
            // proceeds.
            None => true,
        };

        if self.same_session && !same_session(peer.session, me.session) {
            return Err(RejectReason::Session {
                found: peer.session,
                expected: me.session,
            });
        }
        if let Some(expected) = self.expected_pid {
            // Both the pipe's own answer and the peer's claim are checked. The
            // first is what binds the channel to the process that was started; the
            // second catches a peer relaying another process's connection.
            for found in [peer.pid, claimed_pid] {
                if found != expected {
                    return Err(RejectReason::Pid { found, expected });
                }
            }
        }
        Ok(Accepted {
            integrity_unverified,
        })
    }

    /// Apply the policy from the CONNECTING side: integrity as a ceiling, plus
    /// the image on disk. This direction never degrades.
    pub fn evaluate_server(
        &self,
        peer: &ServerPeer,
        me: &SelfIdentity,
    ) -> Result<Accepted, RejectReason> {
        if peer.integrity > self.integrity {
            return Err(RejectReason::Integrity {
                found: Some(peer.integrity),
                required: self.integrity,
            });
        }
        if self.same_session && !same_session(peer.session, me.session) {
            return Err(RejectReason::Session {
                found: peer.session,
                expected: me.session,
            });
        }
        if self.same_image && !same_path(&peer.image, &me.image) {
            return Err(RejectReason::Image {
                found: Some(peer.image.clone()),
                expected: me.image.clone(),
            });
        }
        if let Some(expected) = self.expected_pid
            && peer.pid != expected
        {
            return Err(RejectReason::Pid {
                found: peer.pid,
                expected,
            });
        }
        Ok(Accepted {
            integrity_unverified: false,
        })
    }
}

/// Whether two logon sessions are known to be the same one.
///
/// A session that could not be read is not a session that matches. `None` means
/// `ProcessIdToSessionId` failed, and comparing the `Option`s directly would let
/// two failures agree with each other - turning the check that replaces the user
/// comparison into one that passes precisely when it learned nothing.
fn same_session(peer: Option<u32>, me: Option<u32>) -> bool {
    matches!((peer, me), (Some(peer), Some(me)) if peer == me)
}

/// Whether two paths name the same executable.
///
/// The two sides do not obtain the path the same way - one asks the kernel for a
/// running process's image, the other reads its own module path, which is
/// whatever string it was started with - so the same file can arrive here spelled
/// two ways: differently cased, through an 8.3 short name, through a junction, or
/// through a mapped drive. Canonicalizing resolves all four, since it asks the
/// filesystem for the final name of an opened handle rather than comparing text.
///
/// A path that cannot be opened falls back to the textual compare, which at least
/// answers the casing. Both directions of the check are fail-closed - a peer that
/// does not match is refused - so a fallback that is too strict costs a channel,
/// never a wrongly accepted peer.
fn same_path(left: &Path, right: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) {
        return case_insensitive_eq(&left, &right);
    }
    case_insensitive_eq(left, right)
}

fn case_insensitive_eq(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .to_lowercase()
        .eq(&right.as_os_str().to_string_lossy().to_lowercase())
}

/// Identify the peer connected to this listening pipe instance.
///
/// The integrity is read from the peer's own token, obtained by impersonating it
/// and opening the resulting thread token. `open_as_self` is set on that open, so
/// the token is opened in this process's security context rather than in the
/// peer's - the peer may be more privileged, and this side is only trying to
/// look at it.
pub fn identify_client(pipe: &PipeStream) -> io::Result<ClientPeer> {
    let pipe = pipe.raw();
    let mut pid = 0u32;
    // SAFETY: a valid pipe-server handle with a connected client; `pid` receives
    // the client's process id.
    if unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ClientPeer {
        pid,
        session: session_of(pid),
        integrity: client_token_integrity(pipe).ok(),
    })
}

fn client_token_integrity(pipe: HANDLE) -> io::Result<Integrity> {
    // SAFETY: a valid pipe-server handle with a connected client. On success this
    // thread carries the peer's token until RevertToSelf, which the guard below
    // performs on every path out.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _revert = RevertGuard;

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentThread returns a pseudo-handle valid for the call. The
    // third argument opens the token in this process's security context rather
    // than the impersonated one, which is what a less privileged process must do
    // to look at a more privileged peer. `token` receives a real handle, closed
    // by the guard below.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedToken(token);
    token_integrity(token.0)
}

/// Restore this thread's own token, whatever happens to the query in between.
struct RevertGuard;

impl Drop for RevertGuard {
    fn drop(&mut self) {
        // SAFETY: takes no arguments; drops any impersonation token on this
        // thread. Failing means there was none, which is the wanted state.
        unsafe { RevertToSelf() };
    }
}

struct OwnedToken(HANDLE);

impl Drop for OwnedToken {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle came from an OpenProcessToken/OpenThreadToken in
            // this module and is closed exactly once, here.
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct OwnedProcess(HANDLE);

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle came from OpenProcess in this module and is
            // closed exactly once, here.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Identify the peer serving this connected pipe, through a handle to its
/// process. Every part of this is required to succeed: this is the direction
/// that decides whether privileged work is done on someone's behalf.
pub fn identify_server(pipe: &PipeStream) -> io::Result<ServerPeer> {
    let pipe = pipe.raw();
    let mut pid = 0u32;
    // SAFETY: a valid connected pipe-client handle; `pid` receives the server's
    // process id.
    if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: an access mask, an inherit flag, and a process id. On success a real
    // handle is returned, closed exactly once by the wrapper.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let process = OwnedProcess(process);

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: `process` is the handle just opened; PROCESS_QUERY_LIMITED_INFORMATION
    // is sufficient for a query-only token open. `token` receives a real handle.
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedToken(token);

    Ok(ServerPeer {
        pid,
        session: session_of(pid),
        integrity: token_integrity(token.0)?,
        image: process_image(process.0)?,
    })
}

fn process_image(process: HANDLE) -> io::Result<PathBuf> {
    // Long paths are permitted, so the buffer starts at the classic limit and
    // grows while the call says it was too small.
    let mut buffer = vec![0u16; 260];
    loop {
        let mut length = buffer.len() as u32;
        // SAFETY: `buffer` holds `length` wide characters; on success the call
        // writes a NUL-terminated path into it and sets `length` to the character
        // count excluding the NUL.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut length,
            )
        };
        if ok != 0 {
            buffer.truncate(length as usize);
            return Ok(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }
        // SAFETY: reads this thread's last-error code, set by the call above.
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || buffer.len() >= 32768 {
            return Err(io::Error::last_os_error());
        }
        buffer.resize(buffer.len() * 2, 0);
    }
}

/// The RID of a token's mandatory label, which is its integrity level.
fn token_integrity(token: HANDLE) -> io::Result<Integrity> {
    let mut needed = 0u32;
    // SAFETY: a valid token handle opened for TOKEN_QUERY; a null buffer of length
    // zero only writes the required size into `needed`.
    unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` holds `needed` bytes; on success it receives a
    // TOKEN_MANDATORY_LABEL whose Label.Sid points inside the same buffer.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: the buffer starts with a TOKEN_MANDATORY_LABEL written by the call
    // above, and its Label.Sid points to a well-formed SID within it. The
    // integrity RID is the last sub-authority.
    let rid = unsafe {
        let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err(io::Error::other("the mandatory label SID has no RID"));
        }
        *GetSidSubAuthority(label.Label.Sid, u32::from(count) - 1)
    };
    Ok(Integrity(rid))
}

fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0u32;
    // SAFETY: takes a process id and an out-parameter; queries no handle.
    let ok = unsafe { ProcessIdToSessionId(pid, &mut session) };
    (ok != 0).then_some(session)
}

/// The security descriptor the listening pipe is created with.
///
/// Two allow ACEs and nothing else: the creating user, and the built-in
/// Administrators group. No Everyone, no Anonymous, no application packages, and
/// no mandatory label - the default (medium, no-write-up) is what is wanted here,
/// since a higher-integrity peer writing to a medium-integrity object is a
/// write-down and is allowed while a low-integrity process cannot write at all.
///
/// The Administrators ACE is load-bearing rather than habit: under
/// over-the-shoulder elevation the connecting peer runs on the administrator
/// account whose credentials were supplied, not on the account that created the
/// pipe, so the user ACE alone would lock out the very peer the channel exists
/// for. It admits nobody who was not already omnipotent - a member of that group
/// can elevate to anything on the machine without going near this pipe.
pub struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: the descriptor is a plain LocalAlloc'd buffer this value owns
// exclusively and frees exactly once; nothing about it is thread-affine, and it
// is only ever read (by the kernel, during pipe creation).
unsafe impl Send for PipeSecurity {}
// SAFETY: as above.
unsafe impl Sync for PipeSecurity {}

impl PipeSecurity {
    pub fn for_current_user() -> io::Result<PipeSecurity> {
        PipeSecurity::from_sddl(&sddl_for(&current_user_sid_string()?))
    }

    fn from_sddl(sddl: &str) -> io::Result<PipeSecurity> {
        let wide = wide(sddl);
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is a NUL-terminated wide string that outlives the call;
        // on success `descriptor` receives a LocalAlloc'd descriptor freed in Drop.
        // The size out-parameter is unused (null).
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PipeSecurity { descriptor })
    }

    /// The attributes to create an object with, borrowed from the descriptor they
    /// point at.
    pub(crate) fn attributes(&self) -> PipeAttributes<'_> {
        PipeAttributes {
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor,
                bInheritHandle: 0,
            },
            owner: PhantomData,
        }
    }
}

/// A `SECURITY_ATTRIBUTES` and the borrow that keeps it valid.
///
/// The struct is a bare pointer into a buffer [`PipeSecurity`] frees in `Drop`,
/// and the pointer is dereferenced by the kernel with no `unsafe` at the call
/// site to mark it. So the descriptor is borrowed for as long as the attributes
/// exist, and the compiler is what stops a caller outliving it - the alternative
/// is a dangling pointer nothing in the signature warns about.
pub(crate) struct PipeAttributes<'a> {
    attributes: SECURITY_ATTRIBUTES,
    owner: PhantomData<&'a PipeSecurity>,
}

impl PipeAttributes<'_> {
    /// For the creation call, which takes the attributes by pointer.
    pub(crate) fn as_ptr(&self) -> *const SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            // SAFETY: the descriptor was allocated by the SDDL conversion above
            // and is freed exactly once, here.
            unsafe { LocalFree(self.descriptor) };
        }
    }
}

/// The descriptor as SDDL, for a user SID.
///
/// A function of its own so the ACE list is a value a test can read, rather than
/// something only the kernel ever sees: what makes this descriptor right is what
/// is NOT in it, and an absence is not visible in a descriptor that merely builds.
///
/// `P` protects the DACL from inheritance, which a pipe has none of, but says
/// plainly that this list is the whole list. `GRGW` is the generic read/write a
/// duplex client needs and nothing more.
fn sddl_for(user: &str) -> String {
    format!("O:{user}G:{user}D:P(A;;GRGW;;;{user})(A;;GRGW;;;BA)")
}

/// The current process user's SID in string form, for the descriptor above.
fn current_user_sid_string() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the call; on
    // success `token` receives a real handle, closed exactly once by the wrapper.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedToken(token);

    let mut needed = 0u32;
    // SAFETY: a valid token handle; a null buffer of length zero only writes the
    // required size into `needed`.
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` holds `needed` bytes; on success it receives a TOKEN_USER
    // whose User.Sid points inside the same buffer.
    let ok = unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: the buffer starts with a TOKEN_USER written by the call above, and
    // User.Sid points to a well-formed SID within it, which outlives this call. On
    // success `text` receives a LocalAlloc'd string freed below.
    let ok = unsafe {
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        ConvertSidToStringSidW(user.User.Sid, &mut text)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: `text` is the NUL-terminated string just produced; the length is
    // found by scanning to the terminator, and the buffer is freed once after.
    let sid = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        sid
    };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> SelfIdentity {
        SelfIdentity {
            pid: 100,
            session: Some(1),
            image: PathBuf::from(r"C:\Program Files\Windhawk\windhawk-ui.exe"),
        }
    }

    /// The struct literal is exhaustive on purpose: it stops compiling the day a
    /// `same_user` field is added, which is the regression this guards against.
    /// A same-user check would reject the one configuration a standard user has -
    /// an administrator's credentials supplied at the prompt put the privileged
    /// end on a different account.
    fn strict() -> PeerPolicy {
        PeerPolicy {
            integrity: Integrity::HIGH,
            same_session: true,
            same_image: true,
            expected_pid: None,
        }
    }

    #[test]
    fn a_peer_on_another_account_is_accepted_when_everything_checked_passes() {
        // Nothing in either identity names a user, so nothing can compare one.
        let peer = ClientPeer {
            pid: 200,
            session: Some(1),
            integrity: Some(Integrity::HIGH),
        };
        assert_eq!(
            strict().evaluate_client(&peer, 200, &me()),
            Ok(Accepted {
                integrity_unverified: false
            })
        );

        let peer = ServerPeer {
            pid: 200,
            session: Some(1),
            integrity: Integrity::MEDIUM,
            image: me().image.clone(),
        };
        let connector = PeerPolicy {
            integrity: Integrity::MEDIUM,
            ..strict()
        };
        assert_eq!(
            connector.evaluate_server(&peer, &me()),
            Ok(Accepted {
                integrity_unverified: false
            })
        );
    }

    #[test]
    fn the_listening_side_reads_integrity_as_a_floor() {
        let policy = strict();
        let medium = ClientPeer {
            pid: 200,
            session: Some(1),
            integrity: Some(Integrity::MEDIUM),
        };
        assert!(matches!(
            policy.evaluate_client(&medium, 200, &me()),
            Err(RejectReason::Integrity { .. })
        ));

        let system = ClientPeer {
            integrity: Some(Integrity::SYSTEM),
            ..medium
        };
        assert!(policy.evaluate_client(&system, 200, &me()).is_ok());
    }

    #[test]
    fn the_connecting_side_reads_integrity_as_a_ceiling() {
        let policy = PeerPolicy {
            integrity: Integrity::MEDIUM,
            ..strict()
        };
        let elevated = ServerPeer {
            pid: 200,
            session: Some(1),
            integrity: Integrity::HIGH,
            image: me().image.clone(),
        };
        assert!(matches!(
            policy.evaluate_server(&elevated, &me()),
            Err(RejectReason::Integrity { .. })
        ));
    }

    #[test]
    fn an_unreadable_peer_token_degrades_on_the_listening_side_only() {
        let peer = ClientPeer {
            pid: 200,
            session: Some(1),
            integrity: None,
        };
        assert_eq!(
            strict().evaluate_client(&peer, 200, &me()),
            Ok(Accepted {
                integrity_unverified: true
            }),
            "this direction is anti-spoofing, not the privilege boundary"
        );
    }

    #[test]
    fn a_peer_in_another_logon_session_is_refused() {
        let peer = ClientPeer {
            pid: 200,
            session: Some(2),
            integrity: Some(Integrity::HIGH),
        };
        assert!(matches!(
            strict().evaluate_client(&peer, 200, &me()),
            Err(RejectReason::Session { .. })
        ));
    }

    /// A session that could not be read is not a session that matches. Comparing
    /// the `Option`s directly would let two failed lookups agree, which is the
    /// check passing exactly when it established nothing.
    #[test]
    fn an_unknown_logon_session_is_refused_on_both_sides() {
        let unknown = SelfIdentity {
            session: None,
            ..me()
        };
        for (peer, me) in [(None, Some(1)), (Some(1), None), (None, None)] {
            let me = SelfIdentity {
                session: me,
                ..unknown.clone()
            };
            let client = ClientPeer {
                pid: 200,
                session: peer,
                integrity: Some(Integrity::HIGH),
            };
            assert!(
                matches!(
                    strict().evaluate_client(&client, 200, &me),
                    Err(RejectReason::Session { .. })
                ),
                "listening side accepted sessions {peer:?} and {:?}",
                me.session
            );

            let server = ServerPeer {
                pid: 200,
                session: peer,
                integrity: Integrity::MEDIUM,
                image: me.image.clone(),
            };
            let policy = PeerPolicy {
                integrity: Integrity::MEDIUM,
                ..strict()
            };
            assert!(
                matches!(
                    policy.evaluate_server(&server, &me),
                    Err(RejectReason::Session { .. })
                ),
                "connecting side accepted sessions {peer:?} and {:?}",
                me.session
            );
        }
    }

    #[test]
    fn the_connecting_side_refuses_a_peer_running_another_image() {
        let peer = ServerPeer {
            pid: 200,
            session: Some(1),
            integrity: Integrity::MEDIUM,
            image: PathBuf::from(r"C:\Windows\explorer.exe"),
        };
        let policy = PeerPolicy {
            integrity: Integrity::MEDIUM,
            ..strict()
        };
        assert!(matches!(
            policy.evaluate_server(&peer, &me()),
            Err(RejectReason::Image { .. })
        ));
    }

    #[test]
    fn a_differently_cased_image_path_is_the_same_image() {
        let peer = ServerPeer {
            pid: 200,
            session: Some(1),
            integrity: Integrity::MEDIUM,
            image: PathBuf::from(r"c:\program files\windhawk\WINDHAWK-UI.EXE"),
        };
        let policy = PeerPolicy {
            integrity: Integrity::MEDIUM,
            ..strict()
        };
        assert!(policy.evaluate_server(&peer, &me()).is_ok());
    }

    /// The two sides reach the image by different calls, so the same file can
    /// arrive spelled two ways - a short name, a junction, a mapped drive - and a
    /// textual compare would refuse the peer this channel exists for. Asserted
    /// against a real file, because what resolves the spelling is the filesystem
    /// rather than anything this module can compute.
    #[test]
    fn the_same_file_reached_by_another_spelling_is_the_same_image() {
        let plain = std::env::current_exe().expect("this test binary's path");
        let verbatim = plain.canonicalize().expect("the test binary is openable");
        assert_ne!(
            plain, verbatim,
            "the two spellings must differ for this to be testing anything"
        );
        assert!(same_path(&plain, &verbatim));
    }

    /// The fallback for a path nothing can be opened at: it still answers the
    /// casing, and it still says no to a different file.
    #[test]
    fn an_unopenable_path_falls_back_to_the_textual_compare() {
        let missing = Path::new(r"C:\nowhere\Windhawk\windhawk-ui.exe");
        assert!(same_path(
            missing,
            Path::new(r"c:\NOWHERE\windhawk\WINDHAWK-UI.EXE")
        ));
        assert!(!same_path(
            missing,
            Path::new(r"C:\nowhere\Windhawk\other.exe")
        ));
    }

    #[test]
    fn the_pid_binding_checks_both_the_pipe_and_the_peers_claim() {
        let policy = PeerPolicy {
            expected_pid: Some(200),
            ..strict()
        };
        let peer = ClientPeer {
            pid: 200,
            session: Some(1),
            integrity: Some(Integrity::HIGH),
        };
        assert!(policy.evaluate_client(&peer, 200, &me()).is_ok());
        // A peer relaying another process's connection: the pipe says one thing
        // and the handshake another.
        assert!(matches!(
            policy.evaluate_client(&peer, 999, &me()),
            Err(RejectReason::Pid { .. })
        ));

        let other = ClientPeer { pid: 999, ..peer };
        assert!(matches!(
            policy.evaluate_client(&other, 999, &me()),
            Err(RejectReason::Pid { .. })
        ));
    }

    #[test]
    fn this_process_can_describe_itself_and_build_a_descriptor() {
        let me = SelfIdentity::resolve().unwrap();
        assert!(me.session.is_some());
        assert!(me.image.exists());
        PipeSecurity::for_current_user().expect("the pipe descriptor must build");
    }

    /// The pipe's descriptor, read as the list it is. What makes it right is what
    /// it does NOT grant, and a descriptor that merely converts says nothing about
    /// that - so the ACEs are asserted individually and the count is asserted too,
    /// which is what turns "these two are present" into "these two and no others".
    ///
    /// The Administrators ACE is not decoration and must not be tidied away: under
    /// over-the-shoulder elevation the connecting peer runs on the administrator
    /// account whose credentials were supplied, not on the account that created the
    /// pipe. It admits nobody who was not already omnipotent.
    #[test]
    fn the_pipe_grants_the_creating_user_and_administrators_and_nobody_else() {
        let user = "S-1-5-21-1-2-3-1001";
        let sddl = sddl_for(user);

        assert!(sddl.contains("D:P("), "the DACL says it is the whole list");
        assert!(sddl.contains(&format!("(A;;GRGW;;;{user})")));
        assert!(sddl.contains("(A;;GRGW;;;BA)"));
        assert_eq!(
            sddl.matches("(A;;").count(),
            2,
            "two allow ACEs, no more: {sddl}"
        );
        assert_eq!(sddl.matches("(D;").count(), 0, "no deny ACEs: {sddl}");

        // The aliases that would open the pipe to the machine rather than to the
        // pair, named so that adding one has to be a deliberate edit here as well.
        for wider in ["WD", "AN", "AU", "IU", "AC", "S-1-1-0"] {
            assert!(
                !sddl.contains(&format!(";{wider})")),
                "{wider} may not appear in the pipe's DACL: {sddl}"
            );
        }
        // No mandatory label: the default (medium, no-write-up) is what is wanted,
        // since a higher-integrity peer writing to a medium-integrity object is a
        // write-down and is allowed, while a low-integrity process cannot write at
        // all. An `S:` clause here would be someone lowering that.
        assert!(!sddl.contains("S:"), "no SACL or label: {sddl}");

        PipeSecurity::from_sddl(&sddl).expect("the descriptor must convert");
    }
}
