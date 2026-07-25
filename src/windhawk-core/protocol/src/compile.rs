//! DTOs of the process-execution commands: `compileInstalledMod` (recompile an
//! installed mod's stored source) and `notifyTray`. Mirrors
//! `CompileInstalledModInput` / `CompileInstalledModResult` and `TrayAction` in
//! `windhawk-vscode`'s `src/coreClient/contract.ts` 1:1.
//!
//! `getCompileFlags` (the clangd flag set for `compile_flags.txt`) needs no
//! DTO: it takes no params and its result is a bare JSON array of flag
//! strings.

use serde::{Deserialize, Serialize};

use crate::parse_mod_source::ModMetadata;
use crate::settings::ModConfig;

/// Params of `compileInstalledMod`: the installed mod's storage id, its stored
/// source (read by the caller, CRLF-normalized), and the metadata extracted
/// from that source (id reconciled against the storage id by the caller).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompileInstalledModParams {
    pub storage_id: String,
    pub source: String,
    pub metadata: ModMetadata,
}

/// Result of `compileInstalledMod`: the mod's config read back from storage
/// after the compile, plus the freshly written library file name.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompileInstalledModResult {
    pub config: ModConfig,
    pub target_dll_name: String,
    /// The clang diagnostics of a SUCCESSFUL compile (warnings emitted even
    /// though the mod compiled), tagged per target. Empty on a clean compile;
    /// skipped when empty so a no-warnings recompile keeps the pre-warnings
    /// shape. Mirrors `InstallModResult::warnings`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub warnings: String,
}

/// The tray action of `notifyTray` (the contract's `TrayAction`): which
/// windhawk.exe flag to spawn (`-restart-bg` / `-app-settings-changed`).
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrayAction {
    RestartBg,
    AppSettingsChanged,
}

/// Params of `notifyTray`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NotifyTrayParams {
    pub action: TrayAction,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn tray_action_serializes_as_camel_case() {
        for (action, expected) in [
            (TrayAction::RestartBg, "restartBg"),
            (TrayAction::AppSettingsChanged, "appSettingsChanged"),
        ] {
            assert_eq!(
                serde_json::to_value(action).unwrap(),
                Value::String(expected.into())
            );
            let params: NotifyTrayParams =
                serde_json::from_value(json!({ "action": expected })).unwrap();
            assert_eq!(params.action, action);
        }
    }

    #[test]
    fn compile_params_decode_camel_case() {
        let params: CompileInstalledModParams = serde_json::from_value(json!({
            "storageId": "test-mod",
            "source": "// src",
            "metadata": { "id": "test-mod", "version": "1.0" }
        }))
        .unwrap();
        assert_eq!(params.storage_id, "test-mod");
        assert_eq!(params.metadata.version.as_deref(), Some("1.0"));
    }
}
