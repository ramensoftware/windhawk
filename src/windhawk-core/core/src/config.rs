//! The session configuration document of `WhCoreSessionCreate`, resolved once
//! at session creation and immutable afterwards. The core never reads the
//! environment; debug overrides arrive only through this document.

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    pub app_root_path: String,
    pub arm64_enabled: bool,
    /// Log the compiler's stdout/stderr (clang's diagnostics on a SUCCESSFUL
    /// compile) as `Warn` records. Off by default - a clean compile's warnings
    /// are noise for the typical install - and enabled by the operator with
    /// `WINDHAWK_LOG_COMPILER_WARNINGS=1`, read at the host edge and threaded in
    /// here (the core never reads the environment). A FAILING compile always
    /// carries its output in the `COMPILER_FAILED` error, independent of this.
    #[serde(default)]
    pub log_compiler_warnings: bool,
    #[serde(default)]
    pub windhawk_version: Option<String>,
    /// The repository `User-Agent` header (the TS composition root's
    /// `userAgentProduct` + ` (portable)` suffix, e.g. `Windhawk/1.7.3`). The
    /// front-end owns the product identity (it distinguishes the GUI from the
    /// CLI), so it is passed in rather than built here; when absent the repo
    /// service falls back to a `Windhawk/<windhawkVersion>` default. The header
    /// is server-visible only (a benign delta).
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub debug_overrides: DebugOverrides,
}

impl SessionConfig {
    /// The resolved debug-only "ignore TLS certificate errors" setting
    /// (`debugOverrides.ignoreCertErrors`). Always `false` in release builds:
    /// certificate validation cannot be disabled in production no matter what
    /// the config document carries, so a release core can never be steered onto
    /// an unauthenticated TLS connection. Debug builds honor the override for
    /// testing against a server with a self-signed certificate.
    pub fn ignore_cert_errors(&self) -> bool {
        cfg!(debug_assertions) && self.debug_overrides.ignore_cert_errors
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DebugOverrides {
    pub mods_url_root: Option<String>,
    pub update_url: Option<String>,
    pub installer_reg_key: Option<String>,
    pub schtasks_path: Option<String>,
    /// Debug-only: ignore TLS certificate errors (unknown CA / CN mismatch) on
    /// repository and update fetches (`WINDHAWK_DEBUG_IGNORE_CERT_ERRORS`).
    /// Read through `SessionConfig::ignore_cert_errors`, which clamps it to
    /// `false` in release builds; never consult this field directly.
    #[serde(default)]
    pub ignore_cert_errors: bool,
}
