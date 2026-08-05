//! DTOs of the storage/settings commands, mirroring `AppSettings`, `ModConfig`,
//! `ModSettings`, and `CoreInfo` in `windhawk-vscode`'s
//! `src/coreClient/contract.ts`, and `src/services/types.ts` of the TypeScript
//! implementation they replace, 1:1. camelCase field names match the TS
//! property names so the client does no mapping.
//!
//! Patch DTOs (`AppSettingsPatch`, `ModConfigPatch`) carry every field as an
//! `Option`: absent means "preserve" (the TS `Partial<...>` semantics).
//! `disableRunUIScheduledTask` follows the same shape - a plain `Option<bool>`
//! where both absent AND a present `null` decode to `None` ("not touching it").
//! Portable mode reports the field as `null` from `getAppSettings` (no task
//! there), so a front-end that round-trips the whole settings object sends
//! `null` back; mapping it to `None` makes that a tolerated no-op. A front-end
//! that wants to change the value sends a bool (only ever non-portable); the
//! front-ends omit the field rather than send a `null`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The default UI theme (`'dark'`), matching the front-end's `DEFAULT_THEME`.
/// It is both the `getAppSettings` `Theme` default and the `serde(default)` for
/// `AppSettings::theme`, so a settings object that predates the field still
/// decodes to the dark default rather than failing.
pub const DEFAULT_THEME: &str = "dark";

fn default_theme() -> String {
    DEFAULT_THEME.to_owned()
}

////////////////////////////////////////////////////////////////////////////
// getCoreInfo

/// `CoreFsPaths` of contract.ts: the resolved filesystem paths.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreFsPaths {
    pub app_root_path: String,
    pub app_data_path: String,
    pub engine_path: String,
    pub compiler_path: String,
    pub ui_path: String,
}

/// `CoreInfo` of contract.ts (the `getCoreInfo` result).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreInfo {
    pub contract_version: String,
    pub portable: bool,
    pub arm64_enabled: bool,
    /// Raw installed-Windhawk version string from the host; `null` when
    /// unknown. Serialized even when absent (the TS field is `string | null`).
    pub windhawk_version: Option<String>,
    pub fs_paths: CoreFsPaths,
}

////////////////////////////////////////////////////////////////////////////
// App settings

/// The engine sub-object of `AppSettings`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettings {
    pub logging_verbosity: i64,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub inject_into_critical_processes: bool,
    pub inject_into_incompatible_programs: bool,
    pub inject_into_games: bool,
}

/// `AppSettings` of `src/services/types.ts` (the `getAppSettings` result).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: String,
    /// The UI theme (`"dark"` / `"light"` / `"auto"`). A UI-only preference the
    /// native shell and the webview follow; the engine and tray ignore it.
    /// Always serialized, including the default, so the `getAppSettings` result
    /// is self-describing: a consumer reads the value instead of having to know
    /// what an absent field would have meant. `serde(default)` so a settings
    /// object written before the field existed still decodes, to
    /// `DEFAULT_THEME`.
    #[serde(default = "default_theme")]
    pub theme: String,
    pub disable_update_check: bool,
    /// `null` in portable mode (the scheduled task only exists in
    /// non-portable installs); serialized even when `null`. Explicit rename:
    /// serde's camelCase would lowercase the `UI` acronym.
    #[serde(rename = "disableRunUIScheduledTask")]
    pub disable_run_ui_scheduled_task: Option<bool>,
    pub dev_mode_opt_out: bool,
    pub hide_tray_icon: bool,
    pub always_compile_mods_locally: bool,
    pub dont_auto_show_toolkit: bool,
    pub mod_tasks_dialog_delay: i64,
    pub safe_mode: bool,
    pub logging_verbosity: i64,
    pub engine: EngineSettings,
}

/// The engine sub-object of an `AppSettings` patch: every field optional.
/// `Serialize` + `skip_serializing_if` so a consumer SENDS only the present
/// fields, byte-identical to a hand-built `json!` patch; `Deserialize` stays
/// the core's request-parse side.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_verbosity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject_into_critical_processes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject_into_incompatible_programs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inject_into_games: Option<bool>,
}

impl EngineSettingsPatch {
    /// True if any engine field is present - the `Object.keys(engine).length
    /// > 0` half of `shouldRestartApp`, and the open decision for the engine
    /// tree in `serialize`.
    ///
    /// The exhaustive destructure (no `..`) makes a NEW `EngineSettingsPatch`
    /// field a COMPILE error here, not a silently-skipped engine write (and a
    /// missed `requiresRestart`): the field set is kept in sync by the compiler,
    /// not the author's memory.
    pub fn has_any(&self) -> bool {
        let Self {
            logging_verbosity,
            include,
            exclude,
            inject_into_critical_processes,
            inject_into_incompatible_programs,
            inject_into_games,
        } = self;
        logging_verbosity.is_some()
            || include.is_some()
            || exclude.is_some()
            || inject_into_critical_processes.is_some()
            || inject_into_incompatible_programs.is_some()
            || inject_into_games.is_some()
    }
}

/// `Partial<AppSettings>`: the patch applied by `applyAppSettings` and
/// previewed by `previewAppSettingsEffects`. `Serialize` +
/// `skip_serializing_if` so a consumer SENDS only the present fields,
/// byte-identical to a hand-built `json!` patch; `Deserialize` stays the core's
/// request-parse side.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_update_check: Option<bool>,
    /// Absent OR a present `null` both decode to `None` ("not touching it");
    /// `Some(b)` sets and writes the value. Portable mode reports this field as
    /// `null` (no task), so a round-tripped settings object sends `null` back -
    /// tolerated here as a no-op; portable `applyAppSettings` rejects only a
    /// present bool. Explicit rename (the `UI` acronym, as in `AppSettings`).
    #[serde(
        rename = "disableRunUIScheduledTask",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub disable_run_ui_scheduled_task: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dev_mode_opt_out: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_tray_icon: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_compile_mods_locally: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dont_auto_show_toolkit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_tasks_dialog_delay: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_verbosity: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineSettingsPatch>,
}

/// `AppSettingsIntents` of contract.ts: what applying a patch demands of the
/// tray program (the `applyAppSettings` / `previewAppSettingsEffects` result).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsIntents {
    pub requires_restart: bool,
    pub requires_notify: bool,
}

/// Params of `applyAppSettings` and `previewAppSettingsEffects`.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AppSettingsPatchParams {
    #[serde(default)]
    pub patch: AppSettingsPatch,
}

////////////////////////////////////////////////////////////////////////////
// Mod config

/// `ModConfig` of `src/services/types.ts` (the `getModConfig` result, `null`
/// when the mod is not installed).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModConfig {
    pub library_file_name: String,
    pub disabled: bool,
    pub logging_enabled: bool,
    pub debug_logging_enabled: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub include_custom: Vec<String>,
    pub exclude_custom: Vec<String>,
    pub include_exclude_custom_only: bool,
    pub patterns_match_critical_system_processes: bool,
    pub architecture: Vec<String>,
    pub version: String,
}

/// `Partial<ModConfig>`: the patch applied by `updateModConfig`. `Serialize` +
/// `skip_serializing_if` so a consumer SENDS only the present fields,
/// byte-identical to a hand-built `json!` patch; `Deserialize` stays the core's
/// request-parse side.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModConfigPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_logging_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_custom: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_custom: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_exclude_custom_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patterns_match_critical_system_processes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl ModConfigPatch {
    /// True if any field is present. `updateModConfig` uses this to skip opening
    /// (and thus creating) the mod-config tree for a wholly-empty patch, the
    /// same open-only-when-non-empty policy `AppSettingsPatch` follows - so an
    /// empty patch is a no-op rather than materializing an empty key/INI
    /// section. (The install config write always forces fields to `Some`, so it
    /// never observes an empty patch.)
    ///
    /// The exhaustive destructure (no `..`) is deliberate: it makes a NEW
    /// `ModConfigPatch` field a COMPILE error here rather than a silently-skipped
    /// write. Without it, a field added to the struct and to
    /// `write_mod_config_patch` but forgotten here would make
    /// `updateModConfig {patch: {newField: ...}}` look empty and skip the
    /// open+write - a drift the round-trip test cannot catch (it sets every
    /// field, so `has_any` is true regardless). The compiler keeps this in sync
    /// with the field set, not the author's memory.
    pub fn has_any(&self) -> bool {
        let Self {
            library_file_name,
            disabled,
            logging_enabled,
            debug_logging_enabled,
            include,
            exclude,
            include_custom,
            exclude_custom,
            include_exclude_custom_only,
            patterns_match_critical_system_processes,
            architecture,
            version,
        } = self;
        library_file_name.is_some()
            || disabled.is_some()
            || logging_enabled.is_some()
            || debug_logging_enabled.is_some()
            || include.is_some()
            || exclude.is_some()
            || include_custom.is_some()
            || exclude_custom.is_some()
            || include_exclude_custom_only.is_some()
            || patterns_match_critical_system_processes.is_some()
            || architecture.is_some()
            || version.is_some()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModIdParams {
    pub mod_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModConfigParams {
    pub mod_id: String,
    #[serde(default)]
    pub patch: ModConfigPatch,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetModSettingsParams {
    pub mod_id: String,
    /// Per-mod runtime settings: a map of name to a string or a 32-bit integer,
    /// the two forms the settings store holds. Written verbatim (the section is
    /// cleared first); a value of any other shape is rejected.
    pub settings: serde_json::Map<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetModLoggingEnabledParams {
    pub mod_id: String,
    pub enable: bool,
}

/// Params of `setModEnabled` (the use-case enable/disable, which also mirrors
/// the state into the user profile for non-local mods).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetModEnabledParams {
    pub mod_id: String,
    pub enable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_treats_absent_and_null_as_none() {
        // A present `null` decodes to `None` just like an absent field (serde's
        // default `Option` handling), so only a present bool is `Some`.
        let absent: AppSettingsPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.disable_run_ui_scheduled_task, None);

        let null: AppSettingsPatch =
            serde_json::from_str(r#"{"disableRunUIScheduledTask": null}"#).unwrap();
        assert_eq!(null.disable_run_ui_scheduled_task, None);

        let value: AppSettingsPatch =
            serde_json::from_str(r#"{"disableRunUIScheduledTask": true}"#).unwrap();
        assert_eq!(value.disable_run_ui_scheduled_task, Some(true));
    }

    #[test]
    fn app_settings_serializes_scheduled_task_null() {
        let s = AppSettings {
            language: "en".into(),
            theme: "dark".into(),
            disable_update_check: false,
            disable_run_ui_scheduled_task: None,
            dev_mode_opt_out: false,
            hide_tray_icon: false,
            always_compile_mods_locally: false,
            dont_auto_show_toolkit: false,
            mod_tasks_dialog_delay: 2000,
            safe_mode: false,
            logging_verbosity: 0,
            engine: EngineSettings {
                logging_verbosity: 0,
                include: vec![],
                exclude: vec![],
                inject_into_critical_processes: false,
                inject_into_incompatible_programs: false,
                inject_into_games: false,
            },
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["disableRunUIScheduledTask"], Value::Null);
        assert!(
            v.as_object()
                .unwrap()
                .contains_key("disableRunUIScheduledTask")
        );
    }

    #[test]
    fn app_settings_theme_defaults_when_absent() {
        // A settings object written before `theme` existed still decodes: the
        // `serde(default)` fills in the dark default rather than failing.
        let v = serde_json::json!({
            "language": "en",
            "disableUpdateCheck": false,
            "disableRunUIScheduledTask": false,
            "devModeOptOut": false,
            "hideTrayIcon": false,
            "alwaysCompileModsLocally": false,
            "dontAutoShowToolkit": false,
            "modTasksDialogDelay": 2000,
            "safeMode": false,
            "loggingVerbosity": 0,
            "engine": {
                "loggingVerbosity": 0,
                "include": [],
                "exclude": [],
                "injectIntoCriticalProcesses": false,
                "injectIntoIncompatiblePrograms": false,
                "injectIntoGames": false,
            },
        });
        let settings: AppSettings = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(settings.theme, DEFAULT_THEME);

        // The default is serialized explicitly rather than skipped, so a consumer
        // of the raw result never has to infer the theme from an absent field.
        let back = serde_json::to_value(&settings).unwrap();
        assert_eq!(back["theme"], serde_json::json!(DEFAULT_THEME));

        // A non-default theme is carried explicitly.
        let light = AppSettings {
            theme: "light".into(),
            ..serde_json::from_value(v).unwrap()
        };
        assert_eq!(
            serde_json::to_value(&light).unwrap()["theme"],
            serde_json::json!("light")
        );
    }

    #[test]
    fn patch_carries_theme() {
        let patch: AppSettingsPatch = serde_json::from_str(r#"{"theme": "light"}"#).unwrap();
        assert_eq!(patch.theme.as_deref(), Some("light"));
        assert_eq!(
            serde_json::to_value(AppSettingsPatch {
                theme: Some("light".into()),
                ..Default::default()
            })
            .unwrap(),
            serde_json::json!({ "theme": "light" })
        );
    }

    #[test]
    fn engine_patch_has_any() {
        assert!(!EngineSettingsPatch::default().has_any());
        assert!(
            EngineSettingsPatch {
                logging_verbosity: Some(1),
                ..Default::default()
            }
            .has_any()
        );
    }

    #[test]
    fn patch_serialize_omits_absent_fields() {
        // A SENT patch carries only the present fields, byte-identical to the
        // hand-built `json!` the call sites used; an empty patch is `{}`, and
        // the renamed scheduled-task key keeps its spelling.
        assert_eq!(
            serde_json::to_value(AppSettingsPatch::default()).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(AppSettingsPatch {
                safe_mode: Some(true),
                ..Default::default()
            })
            .unwrap(),
            serde_json::json!({ "safeMode": true })
        );
        assert_eq!(
            serde_json::to_value(AppSettingsPatch {
                disable_run_ui_scheduled_task: Some(true),
                ..Default::default()
            })
            .unwrap(),
            serde_json::json!({ "disableRunUIScheduledTask": true })
        );
        assert_eq!(
            serde_json::to_value(AppSettingsPatch {
                engine: Some(EngineSettingsPatch {
                    logging_verbosity: Some(2),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap(),
            serde_json::json!({ "engine": { "loggingVerbosity": 2 } })
        );
        assert_eq!(
            serde_json::to_value(ModConfigPatch {
                logging_enabled: Some(true),
                ..Default::default()
            })
            .unwrap(),
            serde_json::json!({ "loggingEnabled": true })
        );
    }
}
