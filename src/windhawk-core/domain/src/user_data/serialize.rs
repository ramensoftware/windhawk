//! Serialize an archive to its exact on-disk bytes: one pretty-printed UTF-8
//! JSON document (`serde_json` 2-space pretty, matching
//! `JSON.stringify(x, null, 2)` - the same discipline `profile` follows, so the
//! core single-sources the byte format). Field order is the struct declaration
//! order; object-map order (`settings`, `appSettings`) is the caller's insertion
//! order, which `core` fixes to the canonical export order.

use super::UserDataArchive;

/// Serialize `archive` to its on-disk bytes. Infallible for the archive types
/// (no non-string map keys, no unrepresentable floats), so the error arm is
/// unreachable; it falls back to an empty string only to keep the signature
/// total, mirroring `Profile::to_pretty`.
pub fn serialize(archive: &UserDataArchive) -> String {
    serde_json::to_string_pretty(archive).unwrap_or_default()
}
