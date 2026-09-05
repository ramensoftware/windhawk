//! The security descriptor a directory this process means to own alone is
//! created with, and the elevation question that decides whether it needs one.
//!
//! A process elevated through UAC keeps the environment of the unelevated one
//! that started it, so the OS temp area it resolves is the ordinary user's - a
//! directory every medium-integrity process running as that user can write.
//! Creating a subdirectory exclusively settles its NAME, not the access to it:
//! the ACL is inherited from that parent, so once the name is observable, what
//! is placed inside can be replaced by whoever can write there. A folder whose
//! contents a privileged process will go on to trust cannot be one of those.
//!
//! So the folder is created with a descriptor of its own instead of with the
//! parent's. `P` is the load-bearing flag: it protects the DACL from
//! inheritance, which is what makes the two ACEs below the whole list.
//!
//! **There is no ACE for the current user, and its absence is the design.**
//! Under admin-approval-mode elevation the elevated token carries the SAME user
//! SID as the unelevated process the folder is being protected from, so granting
//! the user would grant exactly whom it excludes. This process reaches its own
//! folder through the Administrators ACE, which admits nobody who was not
//! already omnipotent.
//!
//! An unelevated process gets no descriptor (`None`) and its folder keeps the
//! inherited list: it crosses no boundary, since anything that could tamper with
//! its temp folder can already do whatever the process itself can, and a
//! protected DACL there would only deny a portable install the folder it just
//! made.
//!
//! The list a folder is given up with ([`released_dir_sddl`]) is the same one
//! plus a right to REMOVE what is left, for the owner of the temp area the
//! folder sits in - the ordinary user under either kind of elevation, since the
//! area is that user's whoever consented at the prompt. A folder this process
//! could not remove would otherwise sit in that user's temp area past the
//! lifetime of everything that knew about it, out of reach of the user, of
//! Explorer, and of every disk-cleanup tool, all of which run unelevated.

use std::io;
use std::marker::PhantomData;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree, WIN32_ERROR};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_DELETE_CHILD, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE,
    SYNCHRONIZE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::wide::{from_wide_ptr, path_to_wide, to_wide};

/// The DACL of a directory this process owns alone: full access to SYSTEM and to
/// the Administrators group, inheritable by what is created inside it, and
/// nothing else.
///
/// A constant rather than something only the kernel ever sees, so the ACE list is
/// a value a test can read: what makes this descriptor right is what is NOT in
/// it, and an absence is not visible in a descriptor that merely builds.
pub const PRIVATE_DIR_SDDL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// What a released folder grants on itself and on any subfolder: enough to see
/// what is left and take it away - list it, traverse it, delete a child, delete
/// the folder - and no way to put anything there.
const RELEASED_DIR_RIGHTS: u32 = FILE_LIST_DIRECTORY
    | FILE_TRAVERSE
    | FILE_DELETE_CHILD
    | FILE_READ_ATTRIBUTES
    | DELETE
    | SYNCHRONIZE;

/// The same for the files inside: DELETE, plus what a removal stats on the way.
/// Neither read nor execute - a leftover installer stays as unreachable as it
/// was while it was the one this process had launched.
const RELEASED_FILE_RIGHTS: u32 = FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE;

/// The private list plus a right for `owner_sid` to remove what the folder still
/// holds: the list a folder is given up with, once this process is done with it.
///
/// A right to DELETE and nothing more, deliberately. The folder can still hold
/// the executable this process launched out of it, and a principal who can put a
/// file NEXT to a running installer is the one the private list exists to keep
/// out; deleting the image of a running process is refused by the OS, and
/// renaming it needs the right to create the new name, which this does not
/// grant. So the most this admits is an empty folder.
///
/// Given at the point the folder is given up rather than when it is created: on
/// a fresh, still-empty folder a right to DELETE is a right to remove it and put
/// another of the same name back, before the download it is waiting for ever
/// lands there.
pub fn released_dir_sddl(owner_sid: &str) -> String {
    format!(
        "{PRIVATE_DIR_SDDL}(A;CI;{RELEASED_DIR_RIGHTS:#x};;;{owner_sid})\
         (A;OIIO;{RELEASED_FILE_RIGHTS:#x};;;{owner_sid})"
    )
}

/// The descriptor a private directory is created with, or `None` when this
/// process is unelevated and the folder needs none.
///
/// The error is the caller's to propagate rather than to fall back from: the
/// descriptor is the folder's protection, so a privileged process that cannot
/// build one must not get an unprotected folder instead.
pub fn private_dir_security() -> io::Result<Option<DirSecurity>> {
    if !is_elevated() {
        return Ok(None);
    }
    DirSecurity::from_sddl(PRIVATE_DIR_SDDL).map(Some)
}

/// The SID of `path`'s owner, in the string form an SDDL ACE names a principal
/// by.
pub fn owner_sid_string(path: &Path) -> io::Result<String> {
    let path_w = path_to_wide(path);
    let mut owner: PSID = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `path_w` is NUL-terminated and outlives the call; the group, DACL
    // and SACL out-parameters are unused (null). On success `descriptor` receives
    // a LocalAlloc'd buffer freed exactly once below, and `owner` points inside
    // it, so it is only read while that buffer lives.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_w.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    win32_result(status)?;
    // SAFETY: `owner` points into the live descriptor. On success the conversion
    // hands back a NUL-terminated string it allocated, read once and then freed;
    // the descriptor is freed on both paths.
    unsafe {
        let mut text: *mut u16 = std::ptr::null_mut();
        let converted = ConvertSidToStringSidW(owner, &mut text);
        let sid = if converted == 0 {
            Err(io::Error::last_os_error())
        } else {
            let sid = from_wide_ptr(text);
            LocalFree(text.cast());
            Ok(sid)
        };
        LocalFree(descriptor);
        sid
    }
}

/// Replace `path`'s DACL with the one `sddl` describes, protected from what it
/// would otherwise inherit.
///
/// The inheritable ACEs reach what is already inside `path`, not just what is
/// created there afterwards - which is the half a released folder needs, since
/// the file it could not remove is already sitting in it.
pub fn set_protected_dacl(path: &Path, sddl: &str) -> io::Result<()> {
    let security = DirSecurity::from_sddl(sddl)?;
    let mut path_w = path_to_wide(path);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut present = 0;
    let mut defaulted = 0;
    // SAFETY: `security` owns a well-formed descriptor for the whole block, so
    // `dacl` points into a live buffer; `path_w` is NUL-terminated and outlives
    // the call. Owner, group and SACL are unused (null), which is what confines
    // the write to the DACL - the one part an object's owner may rewrite without
    // SeSecurityPrivilege.
    unsafe {
        if GetSecurityDescriptorDacl(security.descriptor, &mut present, &mut dacl, &mut defaulted)
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        win32_result(SetNamedSecurityInfoW(
            path_w.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        ))
    }
}

/// The security APIs that return a status code rather than setting the last
/// error, as an `io::Result`.
fn win32_result(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status as i32))
    }
}

/// Whether this process runs with an elevated token.
///
/// A token that cannot be read counts as elevated. That case is what the
/// descriptor exists for, and the two ways of being wrong are not symmetric: a
/// folder protected when it did not need to be fails the operation visibly,
/// while one left open when it did would hand an attacker an elevated launch.
pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the call; on
    // success `token` receives a real handle, closed exactly once below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return true;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut length = 0u32;
    // SAFETY: a valid token opened for TOKEN_QUERY and a correctly sized
    // TOKEN_ELEVATION out-parameter for the class being queried.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut length,
        )
    };
    // SAFETY: the handle came from the OpenProcessToken above and is closed
    // exactly once, here.
    unsafe { CloseHandle(token) };
    ok == 0 || elevation.TokenIsElevated != 0
}

/// A security descriptor, owning the buffer the SDDL conversion allocated for
/// it.
pub struct DirSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl DirSecurity {
    pub fn from_sddl(sddl: &str) -> io::Result<DirSecurity> {
        let wide = to_wide(sddl);
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is a NUL-terminated wide string that outlives the call;
        // on success `descriptor` receives a LocalAlloc'd descriptor freed in
        // Drop. The size out-parameter is unused (null).
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
        Ok(DirSecurity { descriptor })
    }

    /// The attributes to create an object with, borrowed from the descriptor
    /// they point at.
    pub fn attributes(&self) -> DirAttributes<'_> {
        DirAttributes {
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor,
                bInheritHandle: 0,
            },
            owner: PhantomData,
        }
    }
}

impl Drop for DirSecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            // SAFETY: the descriptor was allocated by the SDDL conversion above
            // and is freed exactly once, here.
            unsafe { LocalFree(self.descriptor) };
        }
    }
}

/// A `SECURITY_ATTRIBUTES` and the borrow that keeps it valid.
///
/// The struct is a bare pointer into a buffer [`DirSecurity`] frees in `Drop`,
/// and the pointer is dereferenced by the kernel with no `unsafe` at the call
/// site to mark it. So the descriptor is borrowed for as long as the attributes
/// exist, and the compiler is what stops a caller outliving it - the alternative
/// is a dangling pointer nothing in the signature warns about.
pub struct DirAttributes<'a> {
    attributes: SECURITY_ATTRIBUTES,
    owner: PhantomData<&'a DirSecurity>,
}

impl DirAttributes<'_> {
    /// For the creation call, which takes the attributes by pointer.
    pub fn as_ptr(&self) -> *const SECURITY_ATTRIBUTES {
        &self.attributes
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_WRITE_ATTRIBUTES, FILE_WRITE_EA, WRITE_DAC,
        WRITE_OWNER,
    };

    use super::*;

    /// The descriptor read as the list it is. What makes it right is what it does
    /// NOT grant, so the ACEs are asserted individually and the count is asserted
    /// too, which is what turns "these two are present" into "these two and no
    /// others".
    ///
    /// The user aliases are named so that admitting one has to be a deliberate
    /// edit here as well: under admin-approval-mode elevation the unelevated
    /// process this folder is protected from runs as that same user.
    #[test]
    fn the_private_directory_grants_system_and_administrators_and_nobody_else() {
        let sddl = PRIVATE_DIR_SDDL;

        assert!(
            sddl.starts_with("D:P("),
            "the DACL must say it is the whole list: {sddl}"
        );
        assert!(sddl.contains("(A;OICI;FA;;;SY)"), "{sddl}");
        assert!(sddl.contains("(A;OICI;FA;;;BA)"), "{sddl}");
        assert_eq!(
            sddl.matches("(A;").count(),
            2,
            "two allow ACEs, no more: {sddl}"
        );
        assert_eq!(sddl.matches("(D;").count(), 0, "no deny ACEs: {sddl}");

        for wider in ["WD", "AU", "IU", "BU", "AC", "CO", "S-1-1-0"] {
            assert!(
                !sddl.contains(&format!(";{wider})")),
                "{wider} may not appear in a private directory's DACL: {sddl}"
            );
        }

        DirSecurity::from_sddl(sddl).expect("the descriptor must convert");
    }

    /// The inheritance flags are not decoration: the file written into the folder
    /// is the thing that must not be replaceable, and it gets its ACL from these.
    #[test]
    fn the_private_directory_passes_its_list_to_what_is_created_inside_it() {
        assert_eq!(
            PRIVATE_DIR_SDDL.matches("OICI").count(),
            2,
            "every ACE must reach the file inside: {PRIVATE_DIR_SDDL}"
        );
    }

    /// Giving a folder up widens its list, so the widening is read as a list
    /// too: the private ACEs survive intact, and exactly one further principal
    /// is named - by SID, since the point of the release is to reach the one
    /// user the aliases cannot name.
    #[test]
    fn a_released_directory_keeps_the_private_list_and_adds_one_principal() {
        let sid = "S-1-5-21-1-2-3-1001";
        let sddl = released_dir_sddl(sid);

        assert!(
            sddl.starts_with(PRIVATE_DIR_SDDL),
            "the private ACEs must survive the release: {sddl}"
        );
        assert_eq!(
            sddl.matches("(A;").count(),
            4,
            "the two private ACEs and the owner's two, no more: {sddl}"
        );
        assert_eq!(
            sddl.matches(sid).count(),
            2,
            "one principal is named, on the folder and on its files: {sddl}"
        );
        assert_eq!(sddl.matches("(D;").count(), 0, "no deny ACEs: {sddl}");

        DirSecurity::from_sddl(&sddl).expect("the released descriptor must convert");
    }

    /// The release is a right to REMOVE, and the distinction is the whole of its
    /// safety: the folder can still hold an executable a privileged process
    /// launched out of it, so a right to put a file beside that one - to plant a
    /// DLL next to it - would be the hole the private list exists to close,
    /// handed over at cleanup time instead.
    #[test]
    fn a_released_directory_grants_no_way_to_put_anything_in_it() {
        const PLANTING: u32 = FILE_ADD_FILE
            | FILE_ADD_SUBDIRECTORY
            | FILE_WRITE_EA
            | FILE_WRITE_ATTRIBUTES
            | WRITE_DAC
            | WRITE_OWNER;

        assert_eq!(
            RELEASED_DIR_RIGHTS & PLANTING,
            0,
            "the folder's mask may not admit a write: {RELEASED_DIR_RIGHTS:#x}"
        );
        assert_eq!(
            RELEASED_FILE_RIGHTS,
            FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            "a leftover file is stat-able and removable, and nothing else: \
             {RELEASED_FILE_RIGHTS:#x}"
        );
    }
}
