//! DTOs of the install use-case command: `installMod`. Mirrors
//! `CoreInstallModInput` / `InstallModResult` in `windhawk-vscode`'s
//! `src/coreClient/contract.ts` 1:1.
//!
//! `CoreInstallModInput` is the `InstallModInput` minus `modsFolderUrl`: the
//! repository folder URL for precompiled downloads is core-internal now (the
//! front-ends no longer know repository URLs), so the install service derives
//! it from `debugOverrides.modsUrlRoot` / the default root like
//! `services::repo`. The optional fields carry the absent-means-preserve
//! semantics of the TS object (`ModConfigCodec.serialize` skips undefined
//! fields).

use serde::{Deserialize, Serialize};

use crate::parse_mod_source::ModMetadata;
use crate::settings::ModConfig;

/// Params of `installMod` (the contract's `CoreInstallModInput`). `Serialize`
/// (with `skip_serializing_if` on the optionals) so a consumer SENDS the same
/// shape its `json!` call site built - omitting the absent-means-preserve
/// optionals - byte-identical; `Deserialize` stays the core's parse side.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallModParams {
    /// Storage id the mod is persisted under: the bare repo id, or
    /// `local@<id>` for locally-authored mods.
    pub storage_id: String,
    /// Mod source code, CRLF-normalized by the caller.
    pub source: String,
    /// Metadata already extracted from `source` and validated by the caller.
    pub metadata: ModMetadata,
    /// `true`/`false` sets the disabled state explicitly; absent preserves it
    /// (a fresh install then takes the backend's default of enabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Same absent-means-preserve semantics as `disabled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_enabled: Option<bool>,
    /// The compile-vs-download decision, made by the caller.
    pub compile_locally: bool,
    /// `false` for `local@` mods, which are not tracked in the user profile.
    pub track_in_profile: bool,
    /// Editor compile only: the precompiled-headers folder passed through to
    /// the compiler.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pch_folder: Option<String>,
    /// Editor compile only: when the mod id was renamed in the source, the
    /// previous storage id whose config is moved to `storageId` and whose
    /// source file is deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_from_storage_id: Option<String>,
}

/// Result of `installMod`: the mod's config as read back after the install,
/// plus the freshly placed library file name. (Same shape as
/// `CompileInstalledModResult`, kept as a distinct type to mirror the contract
/// 1:1.)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallModResult {
    pub config: ModConfig,
    pub target_dll_name: String,
    /// The clang diagnostics of a SUCCESSFUL local compile (the mod compiled but
    /// the compiler still emitted warnings), tagged per target. Empty on a clean
    /// compile or a precompiled download; the front-end surfaces a non-empty
    /// value in its compiler-output channel. Skipped when empty so a
    /// no-warnings install serializes to the pre-warnings shape.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warnings: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn install_params_decode_minimal_and_full() {
        let minimal: InstallModParams = serde_json::from_value(json!({
            "storageId": "test-mod",
            "source": "// src",
            "metadata": { "id": "test-mod", "version": "1.0" },
            "compileLocally": true,
            "trackInProfile": true
        }))
        .unwrap();
        assert_eq!(minimal.storage_id, "test-mod");
        assert_eq!(minimal.disabled, None);
        assert_eq!(minimal.logging_enabled, None);
        assert_eq!(minimal.pch_folder, None);
        assert_eq!(minimal.rename_from_storage_id, None);
        assert!(minimal.compile_locally);
        assert!(minimal.track_in_profile);

        let full: InstallModParams = serde_json::from_value(json!({
            "storageId": "local@test-mod",
            "source": "// src",
            "metadata": { "id": "test-mod" },
            "disabled": true,
            "loggingEnabled": false,
            "compileLocally": false,
            "trackInProfile": false,
            "pchFolder": "C:\\pch",
            "renameFromStorageId": "local@old-mod"
        }))
        .unwrap();
        assert_eq!(full.disabled, Some(true));
        assert_eq!(full.logging_enabled, Some(false));
        assert_eq!(full.pch_folder.as_deref(), Some("C:\\pch"));
        assert_eq!(
            full.rename_from_storage_id.as_deref(),
            Some("local@old-mod")
        );
        assert!(!full.compile_locally);
        assert!(!full.track_in_profile);
    }
}
