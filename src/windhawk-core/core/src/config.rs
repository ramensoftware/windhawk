//! The session configuration document of `WhCoreSessionCreate`, resolved once
//! at session creation and immutable afterwards. The core never reads the
//! environment; debug overrides arrive only through this document.

use serde::{Deserialize, Deserializer};
use windhawk_core_domain::CompileArch;

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    pub app_root_path: String,
    /// Optional override for the compile-arch scope (the CLI's `--arch`, one of
    /// `x64`/`arm64`/`all`). Absent means `auto`: the core resolves the scope
    /// from the OS native machine it detects at session creation
    /// ([`Session::create`]'s `detected_arm64`). The resolved scope is read
    /// through `Session::compile_arch` / `Session::arm64_enabled`, never this
    /// field.
    #[serde(default, rename = "compileArch")]
    pub compile_arch_override: Option<CompileArch>,
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
    #[serde(default, deserialize_with = "gated_debug_overrides")]
    pub debug_overrides: DebugOverrides,
}

impl SessionConfig {
    /// The resolved debug-only "ignore TLS certificate errors" setting
    /// (`debugOverrides.ignoreCertErrors`). Always `false` in release builds,
    /// like every other override ([`gated_debug_overrides`]): certificate
    /// validation cannot be disabled in production no matter what the config
    /// document carries, so a release core can never be steered onto an
    /// unauthenticated TLS connection. Debug builds honor the override for
    /// testing against a server with a self-signed certificate.
    pub fn ignore_cert_errors(&self) -> bool {
        self.debug_overrides.ignore_cert_errors
    }
}

/// Decode `debugOverrides` under the build-profile gate: a debug build takes the
/// document's overrides as given, a release build discards them for the default
/// (every override off). A release core therefore cannot be aimed at a
/// substitute mod repository or installer URL (the installer it downloads is run
/// elevated), at a substitute `schtasks.exe` or installer registry key, or told
/// to skip certificate validation - whichever front-end built the config
/// document, and whether or not that front-end applied its own gate.
///
/// Session creation is the only place a [`SessionConfig`] comes from, so this
/// covers every field of [`DebugOverrides`], including ones added later.
fn gated_debug_overrides<'de, D>(deserializer: D) -> Result<DebugOverrides, D::Error>
where
    D: Deserializer<'de>,
{
    let overrides = DebugOverrides::deserialize(deserializer)?;
    Ok(if cfg!(debug_assertions) {
        overrides
    } else {
        DebugOverrides::default()
    })
}

/// The debug-only overrides of the config document, already clamped to the build
/// profile by [`gated_debug_overrides`]: in a release build every field holds
/// its default.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct DebugOverrides {
    pub mods_url_root: Option<String>,
    pub update_url: Option<String>,
    pub installer_reg_key: Option<String>,
    pub schtasks_path: Option<String>,
    /// Debug-only: ignore TLS certificate errors (unknown CA / CN mismatch) on
    /// repository and update fetches (`WINDHAWK_DEBUG_IGNORE_CERT_ERRORS`).
    /// Read through `SessionConfig::ignore_cert_errors`.
    #[serde(default)]
    pub ignore_cert_errors: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config document setting every override, so the gate is asserted over
    /// the whole set rather than one field.
    const ALL_OVERRIDES: &str = r#"{
        "appRootPath": "C:\\Windhawk",
        "debugOverrides": {
            "modsUrlRoot": "http://mock/",
            "updateUrl": "http://mock/setup.exe",
            "installerRegKey": "HKCU\\Software\\Mock",
            "schtasksPath": "C:\\mock\\schtasks.exe",
            "ignoreCertErrors": true
        }
    }"#;

    fn parse(json: &str) -> SessionConfig {
        serde_json::from_str(json).unwrap()
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_build_drops_every_debug_override() {
        let config = parse(ALL_OVERRIDES);
        assert_eq!(config.debug_overrides.mods_url_root, None);
        assert_eq!(config.debug_overrides.update_url, None);
        assert_eq!(config.debug_overrides.installer_reg_key, None);
        assert_eq!(config.debug_overrides.schtasks_path, None);
        assert!(!config.ignore_cert_errors());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_honors_every_debug_override() {
        let config = parse(ALL_OVERRIDES);
        let debug = &config.debug_overrides;
        assert_eq!(debug.mods_url_root.as_deref(), Some("http://mock/"));
        assert_eq!(debug.update_url.as_deref(), Some("http://mock/setup.exe"));
        assert_eq!(
            debug.installer_reg_key.as_deref(),
            Some("HKCU\\Software\\Mock")
        );
        assert_eq!(
            debug.schtasks_path.as_deref(),
            Some("C:\\mock\\schtasks.exe")
        );
        assert!(config.ignore_cert_errors());
    }

    #[test]
    fn an_absent_debug_overrides_block_leaves_every_override_off() {
        let config = parse(r#"{"appRootPath": "C:\\Windhawk"}"#);
        assert_eq!(config.debug_overrides.mods_url_root, None);
        assert_eq!(config.debug_overrides.update_url, None);
        assert!(!config.ignore_cert_errors());
    }
}
