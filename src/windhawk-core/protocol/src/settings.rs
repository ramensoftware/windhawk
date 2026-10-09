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
//!
//! The numeric app settings (`loggingVerbosity`, `modTasksDialogDelay`) are
//! `i64` here because the TS contract types them as `number`; the settings store
//! behind them holds a 32-bit int. The range is the CORE's rule, not this
//! layer's: a value that does not fit is refused with `INVALID_REQUEST` when the
//! core writes it, naming the setting, never truncated to a different number -
//! the rule `setModSettings` applies to a settings value it is handed.

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
    /// The Windows account the session acts for, whose user configurations
    /// the mod-config commands read and write.
    pub user: SessionUser,
}

/// A Windows account: its SID in canonical string form and `DOMAIN\name`, or
/// the SID again when the account name could not be looked up.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub sid: String,
    pub name: String,
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
    /// Whether the Ctrl+Win+W hotkey that brings up the toolkit is left
    /// unregistered.
    pub disable_toolkit_hotkey: bool,
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
    pub disable_toolkit_hotkey: Option<bool>,
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
    /// Whether update offers for this mod are suppressed, as a matcher over the
    /// offered version rather than a flag. See [`parse_suppression`] for the
    /// grammar; the empty string (which is also what an absent stored value
    /// reads as) means updates are offered normally.
    pub updates_disabled_for_version: String,
    /// Which configuration of the mod this one is for the session's user.
    /// Host-constructed: no patch carries it.
    pub user_scope: UserScope,
    /// Why the mod catalog took the mod off every machine, or `""` when it did
    /// not. A nuked mod does not run whatever `disabled` says, and enabling it
    /// is refused. Read from the global configuration in every scope, and
    /// host-constructed like `user_scope`: the catalog sync and the daily update
    /// check write it, an install of the offered version removes it, and no
    /// patch carries it.
    pub nuked: String,
}

/// Whether the session's user has a configuration of their own for a mod, and
/// whether it is switched on. Only with `Active` is it in use, in the user's
/// programs and in every read and write; with `Inactive` it is kept, and the
/// global one is in use.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UserScope {
    None,
    Inactive,
    Active,
}

/// The `updates_disabled_for_version` value that suppresses every offer. Crate
/// internal: this language only ever DECODES the grammar, so the sentinel is
/// spelled once, here. The TS mirror exports its own for the writers that build
/// a suppression - the front-end's reenable affordance; the CLI hands the user's
/// own word through [`is_valid_suppression`].
const ALL_VERSIONS: &str = "*";

/// What a stored `updatesDisabledForVersion` suppresses. Constructed only by
/// [`parse_suppression`], so a value of this type is a value that came through
/// the grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSuppression<'a> {
    /// `"*"`: every offer for this mod.
    All,
    /// `"=<version>"`: an offer of exactly this (non-empty) version.
    Pinned(&'a str),
}

/// Decode a stored `updatesDisabledForVersion` (the grammar is decoded HERE and
/// nowhere else in this language). `None` is "suppresses nothing", which covers
/// `""`, a bare `"="` (a pin on the empty version, which no offer can be), and
/// every other value outside the grammar.
///
/// Reads fail open on purpose: a value this build does not recognize costs the
/// user an offer they see again, never an update withheld forever by a value no
/// version can match. [`is_valid_suppression`] is the strict half of that split,
/// for writers.
pub fn parse_suppression(stored: &str) -> Option<UpdateSuppression<'_>> {
    if stored == ALL_VERSIONS {
        return Some(UpdateSuppression::All);
    }
    stored
        .strip_prefix('=')
        .filter(|version| !version.is_empty())
        .map(UpdateSuppression::Pinned)
}

/// Whether a stored `updatesDisabledForVersion` suppresses an offer of `latest`.
///
/// The pin arm is equality, matching the `latest != version` rule the update
/// offer itself is decided by: the suppression releases as soon as the offered
/// version is anything other than the pinned one.
///
/// Crate internal, taking the raw stored string so the grammar stays total on
/// its own; [`ModConfig::suppresses_update_offer`] is what a caller holding a
/// config reaches, and the only way in from outside.
fn suppresses_update_offer(stored: &str, latest: &str) -> bool {
    match parse_suppression(stored) {
        Some(UpdateSuppression::All) => true,
        Some(UpdateSuppression::Pinned(version)) => version == latest,
        None => false,
    }
}

/// Whether a value is one a WRITER may store: `""` (updates on) or a value the
/// parser recognizes. The parser accepts anything and suppresses nothing for
/// what it does not recognize; this is the other half of that split, so a writer
/// cannot store a value that can never match - a `"1.2.3"` with the `=`
/// forgotten would otherwise be stored, reported as a success, and honored by
/// nothing.
pub fn is_valid_suppression(value: &str) -> bool {
    value.is_empty() || parse_suppression(value).is_some()
}

impl ModConfig {
    /// Whether this mod's stored suppression refuses an offer of `latest`.
    pub fn suppresses_update_offer(&self, latest: &str) -> bool {
        suppresses_update_offer(&self.updates_disabled_for_version, latest)
    }
}

/// `Partial<ModConfig>`: the patch applied by `updateModConfig`. `Serialize` +
/// `skip_serializing_if` so a consumer SENDS only the present fields,
/// byte-identical to a hand-built `json!` patch; `Deserialize` stays the core's
/// request-parse side.
///
/// An unknown key is refused rather than ignored: `ModConfig` carries the
/// read-only `userScope` and `nuked`, and a writer echoing a whole config back
/// must learn that it did not set them.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates_disabled_for_version: Option<String>,
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
            updates_disabled_for_version,
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
            || updates_disabled_for_version.is_some()
    }

    /// Split into `(machine, scopeable)`: the fields that describe the installed
    /// mod, which live in the global tree whichever configuration is edited, and
    /// the ones a user configuration holds its own copy of (the Advanced tab's
    /// options, the enabled state). An exhaustive destructure like `has_any`, so
    /// a new field does not build until it is classified here.
    pub fn split_by_scope(&self) -> (ModConfigPatch, ModConfigPatch) {
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
            updates_disabled_for_version,
        } = self.clone();
        let machine = ModConfigPatch {
            library_file_name,
            include,
            exclude,
            architecture,
            version,
            updates_disabled_for_version,
            ..Default::default()
        };
        let scopeable = ModConfigPatch {
            disabled,
            logging_enabled,
            debug_logging_enabled,
            include_custom,
            exclude_custom,
            include_exclude_custom_only,
            patterns_match_critical_system_processes,
            ..Default::default()
        };
        (machine, scopeable)
    }
}

impl From<&ModConfig> for ModConfigPatch {
    /// Every writable field of `config`, present. `user_scope` is not a stored
    /// value and `nuked` is the catalog's to write, so neither has a place in a
    /// patch.
    fn from(config: &ModConfig) -> Self {
        let ModConfig {
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
            updates_disabled_for_version,
            user_scope: _,
            nuked: _,
        } = config.clone();
        ModConfigPatch {
            library_file_name: Some(library_file_name),
            disabled: Some(disabled),
            logging_enabled: Some(logging_enabled),
            debug_logging_enabled: Some(debug_logging_enabled),
            include: Some(include),
            exclude: Some(exclude),
            include_custom: Some(include_custom),
            exclude_custom: Some(exclude_custom),
            include_exclude_custom_only: Some(include_exclude_custom_only),
            patterns_match_critical_system_processes: Some(
                patterns_match_critical_system_processes,
            ),
            architecture: Some(architecture),
            version: Some(version),
            updates_disabled_for_version: Some(updates_disabled_for_version),
        }
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
    /// cleared first); a value of any other shape is rejected, as is a name
    /// outside the engine's flat notation (`Scalar`, `Group.child`, `List[0]`,
    /// `Matrix[0].cell`).
    pub settings: serde_json::Map<String, Value>,
}

/// One option of a `$dynamicSelect` setting, as `getModDynamicSelectOptions`
/// reports it: what a selection stores and the text the dropdown shows. The
/// result is a JSON object of setting path -> array of these, both in the order
/// the mod wrote them; it is decoded through `serde_json::Map` (insertion-
/// ordered) and this per-entry DTO, since a keyed struct or a sorted map would
/// lose that order.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DynamicSelectOption {
    pub value: String,
    pub label: String,
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

/// Params of `setModUserScope`: switch the session user's own configuration of
/// the mod on (`active`), creating it from the global one the first time, or
/// off. The result is the mod's `ModConfig` after the switch.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetModUserScopeParams {
    pub mod_id: String,
    pub active: bool,
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
            disable_toolkit_hotkey: false,
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
            "disableToolkitHotkey": false,
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

    fn every_field_patch() -> ModConfigPatch {
        ModConfigPatch {
            library_file_name: Some("m_1.0_1.dll".into()),
            disabled: Some(true),
            logging_enabled: Some(true),
            debug_logging_enabled: Some(true),
            include: Some(vec!["a.exe".into()]),
            exclude: Some(vec!["b.exe".into()]),
            include_custom: Some(vec!["c.exe".into()]),
            exclude_custom: Some(vec!["d.exe".into()]),
            include_exclude_custom_only: Some(true),
            patterns_match_critical_system_processes: Some(true),
            architecture: Some(vec!["x86-64".into()]),
            version: Some("1.0".into()),
            updates_disabled_for_version: Some("*".into()),
        }
    }

    #[test]
    fn the_scope_split_puts_every_field_on_exactly_one_side() {
        let (machine, scopeable) = every_field_patch().split_by_scope();
        let keys = |patch: &ModConfigPatch| -> Vec<String> {
            serde_json::to_value(patch)
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect()
        };
        let mut machine_keys = keys(&machine);
        machine_keys.sort();
        let mut scopeable_keys = keys(&scopeable);
        scopeable_keys.sort();
        assert_eq!(
            machine_keys,
            [
                "architecture",
                "exclude",
                "include",
                "libraryFileName",
                "updatesDisabledForVersion",
                "version",
            ]
        );
        assert_eq!(
            scopeable_keys,
            [
                "debugLoggingEnabled",
                "disabled",
                "excludeCustom",
                "includeCustom",
                "includeExcludeCustomOnly",
                "loggingEnabled",
                "patternsMatchCriticalSystemProcesses",
            ]
        );

        // The two halves carry the values they were given, unchanged.
        let mut merged = serde_json::to_value(&machine).unwrap();
        for (key, value) in serde_json::to_value(&scopeable)
            .unwrap()
            .as_object()
            .unwrap()
        {
            merged[key] = value.clone();
        }
        assert_eq!(merged, serde_json::to_value(every_field_patch()).unwrap());

        let (machine, scopeable) = ModConfigPatch::default().split_by_scope();
        assert!(!machine.has_any() && !scopeable.has_any());
    }

    #[test]
    fn a_patch_naming_a_read_only_field_does_not_decode() {
        let error = serde_json::from_value::<ModConfigPatch>(serde_json::json!({
            "loggingEnabled": true,
            "userScope": "active",
        }))
        .unwrap_err();
        assert!(error.to_string().contains("userScope"), "{error}");

        let params = serde_json::from_value::<UpdateModConfigParams>(serde_json::json!({
            "modId": "m",
            "patch": { "userScope": "none" },
        }));
        assert!(params.is_err());

        let error = serde_json::from_value::<ModConfigPatch>(serde_json::json!({
            "nuked": "",
        }))
        .unwrap_err();
        assert!(error.to_string().contains("nuked"), "{error}");
    }

    #[test]
    fn a_config_converts_to_a_patch_of_every_writable_field() {
        let config: ModConfig = serde_json::from_value(serde_json::json!({
            "libraryFileName": "m_1.0_1.dll",
            "disabled": true,
            "loggingEnabled": true,
            "debugLoggingEnabled": true,
            "include": ["a.exe"],
            "exclude": ["b.exe"],
            "includeCustom": ["c.exe"],
            "excludeCustom": ["d.exe"],
            "includeExcludeCustomOnly": true,
            "patternsMatchCriticalSystemProcesses": true,
            "architecture": ["x86-64"],
            "version": "1.0",
            "updatesDisabledForVersion": "*",
            "userScope": "active",
            "nuked": "Malware found",
        }))
        .unwrap();
        assert_eq!(ModConfigPatch::from(&config), every_field_patch());
    }

    #[test]
    fn update_suppression_grammar() {
        // The whole truth table of `updatesDisabledForVersion`, in the cheapest
        // place to assert it. The TS mirror in the webview IPC contract package
        // asserts the SAME cases, so a divergence between the two
        // implementations of the grammar is a failing assertion rather than a
        // front-end that disagrees with the core about what `=1.2.3` means.
        assert_eq!(parse_suppression(""), None);
        assert_eq!(parse_suppression("*"), Some(UpdateSuppression::All));
        assert_eq!(
            parse_suppression("=1.2.4"),
            Some(UpdateSuppression::Pinned("1.2.4"))
        );
        // A mod published as the literal version `*` pins as `=*`, a different
        // value from the all-versions sentinel - which is what the prefix buys.
        assert_eq!(
            parse_suppression("=*"),
            Some(UpdateSuppression::Pinned("*"))
        );
        // Outside the grammar: a bare version is what a writer produces with the
        // `=` forgotten, and it decodes to nothing rather than to a pin.
        assert_eq!(parse_suppression("1.2.4"), None);
        // Filtered at the PARSER, not left to the caller: read as a pin on the
        // empty version, `=` would be a value no offer could ever match.
        assert_eq!(parse_suppression("="), None);

        assert!(!suppresses_update_offer("", "1.2.4"));
        assert!(suppresses_update_offer("*", "1.2.4"));
        assert!(suppresses_update_offer("*", "1.2.3"));
        assert!(suppresses_update_offer("=1.2.4", "1.2.4"));
        assert!(!suppresses_update_offer("=1.2.4", "1.2.3"));
        assert!(suppresses_update_offer("=*", "*"));
        assert!(suppresses_update_offer("*", "*"));
        assert!(!suppresses_update_offer("1.2.4", "1.2.4"));
        // Against an EMPTY offer specifically: the only offer a pin on the empty
        // version could ever have matched, so this is the case that separates a
        // grammar that is total on its own from one that leans on the decision
        // site guarding an empty latest version first.
        assert!(!suppresses_update_offer("=", ""));
        assert!(!suppresses_update_offer("", ""));

        // The write side of the same grammar. Read against the block above, it
        // is visible that the reader accepts strictly more than a writer emits:
        // `1.2.4` and `=` read as "not suppressed" there and are refused here.
        assert!(is_valid_suppression(""));
        assert!(is_valid_suppression("*"));
        assert!(is_valid_suppression("=1.2.4"));
        assert!(is_valid_suppression("=*"));
        assert!(!is_valid_suppression("1.2.4"));
        assert!(!is_valid_suppression("="));
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
