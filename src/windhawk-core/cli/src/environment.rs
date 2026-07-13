//! The CLI's product version, the one environment value that stays
//! consumer-side: it is this binary's own build-embedded version (the
//! user-agent product token and `windhawkVersion` the session config carries).
//! DLL resolution, the session-config render, and the windhawk.ini access all
//! live in `windhawk-core-host`, shared with the UI.

/// The product version: the workspace `version`, inherited by this crate via
/// `version.workspace = true` and embedded at build time. Feeds the session's
/// `windhawkVersion` / user agent and `update status`'s installed line.
pub fn product_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
