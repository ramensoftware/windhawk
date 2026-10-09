//! In-memory `UserAccounts`: a fixed process user, a syntactic SID round trip
//! that formats the way `ConvertSidToStringSidW` does, and a name table.

use std::collections::BTreeMap;

use windhawk_core_ports::{OsError, UserAccounts};

/// The SID the fake reports as the process user.
pub const TEST_USER_SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";

/// The name the fake reports for [`TEST_USER_SID`].
pub const TEST_USER_NAME: &str = "TESTHOST\\tester";

#[derive(Clone)]
pub struct FakeUserAccounts {
    process_user: Result<String, String>,
    names: BTreeMap<String, String>,
}

impl Default for FakeUserAccounts {
    fn default() -> Self {
        Self {
            process_user: Ok(TEST_USER_SID.to_owned()),
            names: BTreeMap::from([(TEST_USER_SID.to_owned(), TEST_USER_NAME.to_owned())]),
        }
    }
}

impl FakeUserAccounts {
    /// Make `process_user_sid` fail with `message`.
    pub fn failing_process_user(message: &str) -> Self {
        Self {
            process_user: Err(message.to_owned()),
            ..Self::default()
        }
    }

    /// Name the account of `sid`.
    pub fn with_name(mut self, sid: &str, name: &str) -> Self {
        self.names.insert(sid.to_owned(), name.to_owned());
        self
    }
}

impl UserAccounts for FakeUserAccounts {
    fn process_user_sid(&self) -> Result<String, OsError> {
        self.process_user
            .clone()
            .map_err(|message| OsError::new("process_user_sid", 5, message))
    }

    /// `S-<revision>-<authority>(-<subauthority>)*`, case-insensitive on the
    /// `S`, each part a decimal number, printed back with an uppercase `S` and
    /// no leading zeros.
    fn canonical_sid(&self, sid: &str) -> Result<String, OsError> {
        let invalid = || {
            OsError::new(
                "canonical_sid",
                1337,
                "The security ID structure is invalid.",
            )
        };
        let mut parts = sid.split('-');
        if !parts.next().is_some_and(|s| s.eq_ignore_ascii_case("S")) {
            return Err(invalid());
        }
        let numbers = parts
            .map(|part| {
                (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| part.parse::<u64>().ok())
                    .flatten()
            })
            .collect::<Option<Vec<u64>>>()
            .ok_or_else(invalid)?;
        if numbers.len() < 2 || numbers[0] != 1 {
            return Err(invalid());
        }
        Ok(std::iter::once("S".to_owned())
            .chain(numbers.iter().map(u64::to_string))
            .collect::<Vec<_>>()
            .join("-"))
    }

    fn account_name(&self, sid: &str) -> Option<String> {
        self.names.get(sid).cloned()
    }
}
