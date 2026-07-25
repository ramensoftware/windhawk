//! [`SessionConfig`]: the `WhCoreSessionCreate` config both consumers build,
//! mirror of the TS `dllBackend` config object. The consumer supplies the
//! resolved app root, the user-agent product token (`windhawk-cli` vs
//! `windhawk-ui` - the one config field that differs per consumer) and the
//! `windhawkVersion`; the host reads the env-dependent inputs (the gated
//! `WINDHAWK_DEBUG_*` overrides) and the windhawk.ini portable flag at the
//! process edge, then renders deterministically.
//!
//! Debug-override gating mirrors the front-end's production/dev split: the
//! `WINDHAWK_DEBUG_*` session overrides are honored ONLY in a debug build. They
//! pass through `select_debug_overrides`, which returns them as-is in a debug
//! build and zeroes them in a release build, so a release build cannot be
//! pointed at a mock repo/installer or relax cert validation. The release core
//! additionally clamps `ignoreCertErrors` to false on its side.
//!
//! ARM64 eligibility is NOT auto-detected by the host: the core detects the OS
//! native machine itself at session creation. The host only forwards the
//! optional `--arch` override (`compile_arch`, one of `x64`/`arm64`/`all`) the
//! CLI exposes, so a user (or a test) can pick the compile-arch scope on a host
//! whose architecture would not otherwise exercise it. Unlike the
//! `WINDHAWK_DEBUG_*` overrides, `--arch` is a user-facing flag honored in every
//! build; `auto` (the default, and always the UI) forwards nothing and lets the
//! core detect.

use serde_json::{Value, json};

/// Read a `WINDHAWK_DEBUG_*` override, treating empty as unset. Ungated: the
/// build-profile gate is `select_debug_overrides`, applied to the read value
/// rather than to the read itself.
fn env_override(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The user-agent the repository sees: `<product>/<version>`, plus a
/// " (portable)" suffix for portable installs - the same composition the TS
/// `coreClient/index.ts` does from the resolved storage. The product token is the
/// one config field that differs per consumer.
fn user_agent(product: &str, version: &str, portable: bool) -> String {
    let base = format!("{product}/{version}");
    if portable {
        format!("{base} (portable)")
    } else {
        base
    }
}

/// The typed `WINDHAWK_DEBUG_*` session overrides, already gated to the build
/// profile by `select_debug_overrides`. Empty strings are normalized to `None`
/// at read time (`env_override`).
#[derive(Default)]
struct DebugOverrides {
    mods_url_root: Option<String>,
    update_url: Option<String>,
    installer_reg_key: Option<String>,
    schtasks_path: Option<String>,
    ignore_cert_errors: bool,
}

/// Apply the production/dev gate to the raw session overrides: a debug build
/// honors them; a release build returns all-`None`/`false`, so it cannot be
/// pointed at a mock repo/installer or relax cert validation. Kept pure (no env
/// read) so the release-strip behavior is unit-testable by passing a populated
/// `raw` and asserting it is zeroed, with no global env mutation.
#[cfg(debug_assertions)]
fn select_debug_overrides(raw: DebugOverrides) -> DebugOverrides {
    raw
}

#[cfg(not(debug_assertions))]
fn select_debug_overrides(_raw: DebugOverrides) -> DebugOverrides {
    DebugOverrides::default()
}

/// The resolved, env-independent inputs `to_json` renders into the
/// `WhCoreSessionCreate` config. Built once at the process edge by
/// [`SessionConfig::resolve`], so the renderer reads no environment and stays
/// pure.
pub struct SessionConfig {
    app_root: String,
    /// Optional `--arch` override for the core's compile-arch scope. `None` means
    /// `auto` (the default, and always the UI): the config omits `compileArch`
    /// and the core resolves the scope from the OS native machine it detects.
    /// `Some("x64"|"arm64"|"all")` is the CLI's `--arch`, honored in every build.
    compile_arch: Option<String>,
    portable: bool,
    user_agent_product: String,
    windhawk_version: String,
    debug: DebugOverrides,
}

impl SessionConfig {
    /// Resolve the env-dependent session inputs at the process edge: the
    /// portable flag and the gated debug overrides. The one place that reads the
    /// environment (and the windhawk.ini portable flag) for the session config;
    /// the consumer supplies the per-consumer `user_agent_product` token
    /// (`windhawk-cli` / `windhawk-ui`), its `windhawk_version`, and the optional
    /// `compile_arch` override (the CLI's `--arch`, one of `x64`/`arm64`/`all`;
    /// `None` for `auto` - the default and always the UI - which lets the core
    /// detect the OS native machine). Unlike the debug overrides, `--arch` is
    /// user-facing and honored in every build.
    pub fn resolve(
        app_root: String,
        user_agent_product: impl Into<String>,
        windhawk_version: impl Into<String>,
        compile_arch: Option<&str>,
    ) -> SessionConfig {
        let raw = DebugOverrides {
            mods_url_root: env_override("WINDHAWK_DEBUG_MODS_URL"),
            update_url: env_override("WINDHAWK_DEBUG_UPDATE_URL"),
            installer_reg_key: env_override("WINDHAWK_DEBUG_INSTALLER_REG_KEY"),
            schtasks_path: env_override("WINDHAWK_DEBUG_SCHTASKS_PATH"),
            ignore_cert_errors: env_override("WINDHAWK_DEBUG_IGNORE_CERT_ERRORS").as_deref()
                == Some("1"),
        };
        SessionConfig {
            portable: crate::windhawk_ini::is_portable(&app_root),
            compile_arch: compile_arch.map(str::to_owned),
            debug: select_debug_overrides(raw),
            app_root,
            user_agent_product: user_agent_product.into(),
            windhawk_version: windhawk_version.into(),
        }
    }

    /// Build the `WhCoreSessionCreate` config JSON from the resolved inputs. Pure:
    /// every env-dependent value was resolved by [`SessionConfig::resolve`], so
    /// this renders deterministically.
    pub fn to_json(&self) -> Value {
        let debug = &self.debug;
        let mut config = json!({
            "appRootPath": self.app_root,
            // Never null here: a build-time embed is always present (the consumer
            // passes its product version). The unknown -> null contract is
            // preserved-but-unreachable.
            "windhawkVersion": self.windhawk_version,
            "userAgent": user_agent(&self.user_agent_product, &self.windhawk_version, self.portable),
            "debugOverrides": {
                "modsUrlRoot": debug.mods_url_root,
                "updateUrl": debug.update_url,
                "installerRegKey": debug.installer_reg_key,
                "schtasksPath": debug.schtasks_path,
                "ignoreCertErrors": debug.ignore_cert_errors,
            }
        });
        // Omit `compileArch` for `auto`: the core detects the OS native machine.
        // Emit it only for an explicit `--arch`, which the core takes over its
        // own detection.
        if let Some(arch) = &self.compile_arch {
            config["compileArch"] = json!(arch);
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(app_root: &str) -> SessionConfig {
        SessionConfig {
            app_root: app_root.to_owned(),
            compile_arch: None,
            portable: false,
            user_agent_product: "windhawk-cli".to_owned(),
            windhawk_version: "1.7.3".to_owned(),
            debug: DebugOverrides::default(),
        }
    }

    #[test]
    fn to_json_has_the_expected_shape() {
        let json = config("C:\\wh").to_json();
        assert_eq!(json["appRootPath"], json!("C:\\wh"));
        assert_eq!(json["windhawkVersion"], json!("1.7.3"));
        assert_eq!(json["userAgent"], json!("windhawk-cli/1.7.3"));
        // `auto` (the default) omits compileArch: the core detects the machine.
        assert!(json.get("compileArch").is_none());
        assert!(json["debugOverrides"]["ignoreCertErrors"].is_boolean());
    }

    #[test]
    fn to_json_renders_the_resolved_inputs() {
        let json = SessionConfig {
            app_root: "C:\\wh".to_owned(),
            compile_arch: Some("all".to_owned()),
            portable: true,
            user_agent_product: "windhawk-cli".to_owned(),
            windhawk_version: "1.7.3".to_owned(),
            debug: DebugOverrides {
                mods_url_root: Some("http://mock/".to_owned()),
                ignore_cert_errors: true,
                ..DebugOverrides::default()
            },
        }
        .to_json();
        // An explicit --arch is emitted for the core to honor.
        assert_eq!(json["compileArch"], json!("all"));
        assert_eq!(json["userAgent"], json!("windhawk-cli/1.7.3 (portable)"));
        assert_eq!(json["debugOverrides"]["modsUrlRoot"], json!("http://mock/"));
        // An absent override serializes to explicit null.
        assert_eq!(json["debugOverrides"]["updateUrl"], json!(null));
        assert_eq!(json["debugOverrides"]["ignoreCertErrors"], json!(true));
    }

    #[test]
    fn resolve_forwards_the_arch_override_in_every_build() {
        // Unlike the WINDHAWK_DEBUG_* overrides (release-stripped), --arch is
        // user-facing and honored regardless of build profile: resolve emits
        // compileArch for an explicit value and omits it for `auto` (None).
        let with_arch =
            SessionConfig::resolve("C:\\wh".to_owned(), "windhawk-cli", "1.7.3", Some("arm64"))
                .to_json();
        assert_eq!(with_arch["compileArch"], json!("arm64"));

        let auto =
            SessionConfig::resolve("C:\\wh".to_owned(), "windhawk-cli", "1.7.3", None).to_json();
        assert!(auto.get("compileArch").is_none());
    }

    #[test]
    fn user_agent_appends_portable_suffix_for_a_portable_install() {
        assert_eq!(
            user_agent("windhawk-ui", "2.0.0", true),
            "windhawk-ui/2.0.0 (portable)"
        );
        assert_eq!(
            user_agent("windhawk-cli", "1.7.3", false),
            "windhawk-cli/1.7.3"
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_build_strips_debug_overrides() {
        // The gate is pure: a fully-populated raw set must come back zeroed in a
        // release build, with no env mutation.
        let gated = select_debug_overrides(DebugOverrides {
            mods_url_root: Some("http://mock/".to_owned()),
            update_url: Some("http://mock/update".to_owned()),
            installer_reg_key: Some("HKCU\\mock".to_owned()),
            schtasks_path: Some("C:\\mock\\schtasks.exe".to_owned()),
            ignore_cert_errors: true,
        });
        assert_eq!(gated.mods_url_root, None);
        assert_eq!(gated.update_url, None);
        assert_eq!(gated.installer_reg_key, None);
        assert_eq!(gated.schtasks_path, None);
        assert!(!gated.ignore_cert_errors);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_build_passes_debug_overrides_through() {
        let gated = select_debug_overrides(DebugOverrides {
            mods_url_root: Some("http://mock/".to_owned()),
            ignore_cert_errors: true,
            ..DebugOverrides::default()
        });
        assert_eq!(gated.mods_url_root.as_deref(), Some("http://mock/"));
        assert!(gated.ignore_cert_errors);
    }
}
