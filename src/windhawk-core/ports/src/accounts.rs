//! The user accounts port: which Windows account this process runs as, the
//! canonical form of a SID string, and an account's display name. A user
//! configuration of a mod is keyed by the SID, so the core needs the first two
//! to name the key and the third to show whose it is.

use crate::os_error::OsError;

pub trait UserAccounts: Send + Sync {
    /// The SID of the user this process runs as (its process token's user, not
    /// an impersonation token's), in canonical string form (`S-1-5-21-...`).
    fn process_user_sid(&self) -> Result<String, OsError>;

    /// `sid` parsed as a SID and formatted back to its canonical string form, or
    /// an error when it is not a SID. The canonical form can differ from the
    /// input (a lowercase `s`, a leading zero), which is how a caller tells a
    /// canonical SID from one that only parses.
    fn canonical_sid(&self, sid: &str) -> Result<String, OsError>;

    /// The account `sid` names as `DOMAIN\name` (the name alone when there is no
    /// domain), or `None` when it cannot be looked up.
    fn account_name(&self, sid: &str) -> Option<String>;
}
