//! The `UserAccounts` adapter: the process token's user, the SID string round
//! trip through `ConvertStringSidToSidW` / `ConvertSidToStringSidW`, and the
//! account name from `LookupAccountSidW`.

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows_sys::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, PSID, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use windhawk_core_ports::{OsError, UserAccounts, os_message};

use crate::os;
use crate::wide::{from_wide_nul, from_wide_ptr, to_wide};

pub struct WindowsUserAccounts;

/// An `OsError` for a failed call that set the last error.
fn last_os_error(operation: &'static str, call: &str) -> OsError {
    let code = os::last_error();
    OsError::new(operation, code, format!("{call}: {}", os_message(code)))
}

/// `sid` as its canonical string. The caller keeps `sid` valid for the call.
///
/// # Safety
/// `sid` must point to a valid SID.
unsafe fn sid_to_string(operation: &'static str, sid: PSID) -> Result<String, OsError> {
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is valid per this function's contract; on success the
    // conversion allocates a NUL-terminated string, read once and freed here.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(last_os_error(operation, "ConvertSidToStringSid"));
        }
        let string = from_wide_ptr(text);
        LocalFree(text.cast());
        Ok(string)
    }
}

impl UserAccounts for WindowsUserAccounts {
    fn process_user_sid(&self) -> Result<String, OsError> {
        const OP: &str = "process_user_sid";
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the call;
        // on success `token` receives a real handle, closed exactly once below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_os_error(OP, "OpenProcessToken"));
        }
        let result = (|| {
            let mut length = 0u32;
            // SAFETY: a size query: a null buffer of length 0 with a valid
            // out-parameter for the required size.
            unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length) };
            if os::last_error() != ERROR_INSUFFICIENT_BUFFER {
                return Err(last_os_error(OP, "GetTokenInformation"));
            }
            // A u64 buffer keeps the TOKEN_USER at its pointer alignment.
            let mut buffer = vec![0u64; (length as usize).div_ceil(size_of::<u64>())];
            // SAFETY: `buffer` holds at least `length` bytes, the size the query
            // above asked for, and is aligned for TOKEN_USER.
            if unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    length,
                    &mut length,
                )
            } == 0
            {
                return Err(last_os_error(OP, "GetTokenInformation"));
            }
            // SAFETY: the call above filled `buffer` with a TOKEN_USER whose SID
            // points into `buffer` itself, which outlives the conversion.
            unsafe {
                let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
                sid_to_string(OP, user.User.Sid)
            }
        })();
        // SAFETY: the handle came from the OpenProcessToken above and is closed
        // exactly once, here.
        unsafe { CloseHandle(token) };
        result
    }

    fn canonical_sid(&self, sid: &str) -> Result<String, OsError> {
        const OP: &str = "canonical_sid";
        let wide = to_wide(sid);
        let mut parsed: PSID = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and outlives the call; on success
        // `parsed` receives a LocalAlloc'd SID, freed exactly once below.
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut parsed) } == 0 {
            return Err(last_os_error(OP, "ConvertStringSidToSid"));
        }
        // SAFETY: `parsed` is the valid SID the conversion above allocated.
        let canonical = unsafe { sid_to_string(OP, parsed) };
        // SAFETY: allocated by ConvertStringSidToSidW and freed exactly once.
        unsafe { LocalFree(parsed) };
        canonical
    }

    fn account_name(&self, sid: &str) -> Option<String> {
        let wide = to_wide(sid);
        let mut parsed: PSID = std::ptr::null_mut();
        // SAFETY: as in `canonical_sid`.
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut parsed) } == 0 {
            return None;
        }
        let mut name = vec![0u16; 256];
        let mut domain = vec![0u16; 256];
        let mut name_len = name.len() as u32;
        let mut domain_len = domain.len() as u32;
        let mut kind: SID_NAME_USE = 0;
        let mut found = false;
        for _ in 0..2 {
            // SAFETY: `parsed` is a valid SID; both buffers hold the lengths
            // passed; a null system name looks the account up locally, then
            // through the domain the machine trusts.
            let ok = unsafe {
                LookupAccountSidW(
                    std::ptr::null(),
                    parsed,
                    name.as_mut_ptr(),
                    &mut name_len,
                    domain.as_mut_ptr(),
                    &mut domain_len,
                    &mut kind,
                )
            };
            if ok != 0 {
                found = true;
                break;
            }
            if os::last_error() != ERROR_INSUFFICIENT_BUFFER {
                break;
            }
            // The failed call reported the sizes it needs, NUL included.
            name.resize(name_len as usize, 0);
            domain.resize(domain_len as usize, 0);
        }
        // SAFETY: allocated by ConvertStringSidToSidW and freed exactly once.
        unsafe { LocalFree(parsed) };
        if !found {
            return None;
        }
        let name = from_wide_nul(&name);
        let domain = from_wide_nul(&domain);
        Some(if domain.is_empty() {
            name
        } else {
            format!("{domain}\\{name}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_process_user_is_a_canonical_sid_with_a_name() {
        let accounts = WindowsUserAccounts;
        let sid = accounts.process_user_sid().expect("process token user");
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert_eq!(accounts.canonical_sid(&sid).unwrap(), sid);
        let name = accounts
            .account_name(&sid)
            .expect("the process user has a name");
        assert!(!name.is_empty());
    }

    #[test]
    fn a_sid_round_trips_only_in_its_canonical_form() {
        let accounts = WindowsUserAccounts;
        assert_eq!(accounts.canonical_sid("S-1-5-18").unwrap(), "S-1-5-18");
        // Parses, but formats back differently: a caller comparing the two sees
        // a SID that is not canonical.
        assert_eq!(accounts.canonical_sid("s-1-5-18").unwrap(), "S-1-5-18");
        for malformed in ["", "S-1", "S-1-5-x", "Users", "S-1-5-18\\..", "S-1-5-18 "] {
            assert!(
                accounts.canonical_sid(malformed).is_err()
                    || accounts.canonical_sid(malformed).unwrap() != malformed,
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn a_well_known_sid_is_named_and_an_unknown_one_is_not() {
        let accounts = WindowsUserAccounts;
        assert!(accounts.account_name("S-1-5-18").is_some());
        assert_eq!(
            accounts.account_name("S-1-5-21-1111111111-2222222222-3333333333-4321"),
            None
        );
    }
}
