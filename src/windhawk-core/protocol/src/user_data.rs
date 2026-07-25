//! DTOs of the user-data export/import commands (`exportUserData`,
//! `inspectUserData`, and - in a later phase - `importUserData`), mirroring the
//! front-end contract types. The archive bytes themselves are NOT modeled here:
//! they cross the ABI as an opaque string (`ExportUserDataResult::archive` /
//! `InspectUserDataParams::archive`), single-sourced by `domain::user_data`, the
//! same split `getModSource` uses for source text.
//!
//! One `UserDataSelection` shape serves export and import; `offline` is an
//! `options` field on each side rather than part of the shared selection, because
//! it means different things per direction (embed on export, network-free on
//! import).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::settings::AppSettingsIntents;

/// The largest archive the core accepts, in bytes, mirroring the cap
/// `windhawk-core-domain`'s `user_data` enforces (a core test pins the two
/// together). Part of the contract because an archive that arrives as a FILE is
/// the host's to read: sized against this before the read, an oversized document
/// is refused without ever being pulled into memory, whereas the core can only
/// reject it once it holds the whole string.
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

////////////////////////////////////////////////////////////////////////////
// Selection (shared by export and import)

/// The granular selection: which parts of the user data to act on. Identical for
/// export (what to include) and import (what to apply, filtered by what the
/// archive actually carries).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserDataSelection {
    /// Whether to include the Windhawk application settings.
    #[serde(default)]
    pub app_settings: bool,
    /// The mod scope: `all`, `all-except-local`, `none`, or an explicit id list.
    pub mods: ModScope,
    /// The per-mod facet toggles applied to every selected mod (the common
    /// case).
    #[serde(default)]
    pub defaults: FacetToggles,
    /// Per-mod overrides of the defaults, keyed by storage id (the fine-grained
    /// case). An entry omits a facet to leave it at the default.
    #[serde(default)]
    pub per_mod: BTreeMap<String, PerModToggles>,
}

/// The mod scope of a selection. Serializes as the bare string `"all"` /
/// `"all-except-local"` / `"none"`, or the object `{ "ids": [...] }`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum ModScope {
    /// One of the keyword scopes (a bare JSON string).
    Keyword(ModScopeKeyword),
    /// An explicit id list (the JSON object `{ "ids": [...] }`).
    Ids { ids: Vec<String> },
}

/// The keyword scopes: everything, everything but `local@` mods, or nothing (an
/// app-settings-only selection).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModScopeKeyword {
    All,
    AllExceptLocal,
    None,
}

/// The per-mod facet toggles (the `defaults` block): whether to include each
/// selected mod's runtime settings and configuration.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FacetToggles {
    #[serde(default)]
    pub settings: bool,
    #[serde(default)]
    pub config: bool,
}

/// A per-mod override of the `defaults`. A facet left `None` falls back to the
/// default; `Some(bool)` pins it for this mod.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PerModToggles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<bool>,
}

////////////////////////////////////////////////////////////////////////////
// exportUserData

/// Options of `exportUserData`. `offline` embeds every repository mod's source
/// so the archive restores with no network (local mods always embed); off by
/// default (reference-only).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    #[serde(default)]
    pub offline: bool,
}

/// Params of `exportUserData`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportUserDataParams {
    pub selection: UserDataSelection,
    #[serde(default)]
    pub options: ExportOptions,
}

/// Result of `exportUserData`: the archive bytes (a pretty-printed JSON string,
/// the exact file contents the host writes) and a best-effort summary of any
/// per-mod warnings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportUserDataResult {
    pub archive: String,
    pub summary: ExportSummary,
}

/// The export summary: per-mod warnings, empty on a clean export.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub warnings: Vec<ExportWarning>,
}

/// One per-mod export warning: a mod that exported without a facet (e.g. its
/// source would not parse, so its settings were omitted), named so a front-end
/// can surface it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportWarning {
    pub mod_id: String,
    pub message: String,
}

////////////////////////////////////////////////////////////////////////////
// inspectUserData

/// Params of `inspectUserData`: the archive bytes to validate and project.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InspectUserDataParams {
    pub archive: String,
}

/// Result of `inspectUserData`: the manifest a front-end reads to build an
/// import selection over the archive.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectUserDataResult {
    pub manifest: UserDataManifest,
}

/// The archive manifest: what the archive carries at the top level plus a
/// per-mod availability summary. Every field is serialized (no
/// omitted-when-absent), so a front-end sees a stable shape.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserDataManifest {
    pub has_app_settings: bool,
    pub mods: Vec<ManifestModEntry>,
}

/// One mod's manifest row: identity plus which facets the archive carries.
/// `has_source: false` marks a reference-only repository mod (its import needs
/// the network).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestModEntry {
    pub mod_id: String,
    pub is_local: bool,
    pub version: String,
    pub name: Option<String>,
    pub has_source: bool,
    pub has_settings: bool,
    pub has_config: bool,
}

////////////////////////////////////////////////////////////////////////////
// importUserData

/// How import treats a mod that is already installed on the target. `overwrite`
/// (the default) reinstalls and re-applies to the clean baseline (the archive
/// wins); `skip` leaves the target's copy untouched and records a skip.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictPolicy {
    #[default]
    Overwrite,
    Skip,
}

/// Options of `importUserData`. `offline` demands a network-free restore (force
/// local compile, and refuse a reference-only mod that has no embedded source);
/// `no_precompiled` forces local compilation but may still fetch a
/// reference-only mod's source. `confirm_app_restart` acknowledges that applying
/// the archived app settings may require a Windhawk restart (without it, a
/// restart-requiring app-settings import is refused before any change).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub no_precompiled: bool,
    #[serde(default)]
    pub on_conflict: ConflictPolicy,
    #[serde(default)]
    pub confirm_app_restart: bool,
}

/// Params of `importUserData`: the archive bytes, the selection (filtered by
/// what the archive actually carries), and the import options.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportUserDataParams {
    pub archive: String,
    pub selection: UserDataSelection,
    #[serde(default)]
    pub options: ImportOptions,
}

/// The terminal result of `importUserData` (the `completed` event's payload):
/// the per-mod outcomes and the app-settings intents.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportUserDataResult {
    pub summary: ImportSummary,
}

/// The import summary: one outcome per processed mod, plus the app-settings
/// intents when app settings were applied.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub mods: Vec<ImportModOutcome>,
    /// The restart/notify intents the applied app settings reported, or `None`
    /// when app settings were not imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_settings: Option<AppSettingsIntents>,
}

/// One mod's import outcome. `message` carries the failure reason for a
/// `failed` mod (and the skip reason for a `skipped` one); it is absent for an
/// `installed` mod.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportModOutcome {
    pub mod_id: String,
    pub status: ImportModStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The terminal per-mod status carried in the summary.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportModStatus {
    Installed,
    Skipped,
    Failed,
}

/// A per-mod `progress` event payload import emits as it works, carrying the mod
/// dimension the single-operation vocabulary lacks (the `modId` and an
/// `{ index, total }` position), so a front-end can render "mod 3 of 12" even
/// for a precompiled install that emits no sub-progress. `status` marks the
/// start (`installing`) and the terminal per-mod outcome. The same
/// `{ modId, index, total, item }` is stamped onto every event a driven install
/// emits (e.g. a local compile's `compileTarget`), so that sub-progress is
/// attributed to the right mod. `item` is the union discriminant, always `Mod`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub item: ImportProgressItem,
    pub mod_id: String,
    pub index: usize,
    pub total: usize,
    pub status: ImportProgressStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The per-mod progress status: the start marker plus the three terminal
/// outcomes.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportProgressStatus {
    Installing,
    Installed,
    Skipped,
    Failed,
}

/// Which item an import `progress` event describes: `mod` for the per-mod markers
/// ([`ImportProgress`]) and their stamped install sub-events; `appSettings` for the
/// app-settings step ([`ImportAppSettingsProgress`]). The discriminant is present on
/// every import progress event.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportProgressItem {
    Mod,
    AppSettings,
}

/// The app-settings step `progress` event payload. Import applies the archive's
/// global app settings once, before the mod loop, so this marker carries no mod
/// `{ modId, index, total }` position - only the `item` discriminant and a status:
/// `applying` as it starts, `applied` once done. Emitted only when the import
/// applies app settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportAppSettingsProgress {
    pub item: ImportProgressItem,
    pub status: ImportAppSettingsStatus,
}

/// The app-settings step status: the start marker and its terminal outcome.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImportAppSettingsStatus {
    Applying,
    Applied,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mod_scope_round_trips_keywords_and_ids() {
        // A keyword scope is a bare string.
        let all: ModScope = serde_json::from_value(json!("all")).unwrap();
        assert_eq!(all, ModScope::Keyword(ModScopeKeyword::All));
        assert_eq!(serde_json::to_value(&all).unwrap(), json!("all"));

        let except: ModScope = serde_json::from_value(json!("all-except-local")).unwrap();
        assert_eq!(except, ModScope::Keyword(ModScopeKeyword::AllExceptLocal));
        assert_eq!(
            serde_json::to_value(&except).unwrap(),
            json!("all-except-local")
        );

        let none: ModScope = serde_json::from_value(json!("none")).unwrap();
        assert_eq!(none, ModScope::Keyword(ModScopeKeyword::None));

        // An explicit id list is an object.
        let ids: ModScope = serde_json::from_value(json!({ "ids": ["a", "b"] })).unwrap();
        assert_eq!(
            ids,
            ModScope::Ids {
                ids: vec!["a".to_owned(), "b".to_owned()]
            }
        );
        assert_eq!(
            serde_json::to_value(&ids).unwrap(),
            json!({ "ids": ["a", "b"] })
        );
    }

    #[test]
    fn selection_defaults_fill_in() {
        // A minimal selection (only `mods`) fills app_settings/defaults/per_mod
        // with their zero values.
        let selection: UserDataSelection =
            serde_json::from_value(json!({ "mods": "all" })).unwrap();
        assert!(!selection.app_settings);
        assert_eq!(selection.defaults, FacetToggles::default());
        assert!(selection.per_mod.is_empty());
    }

    #[test]
    fn per_mod_toggle_omits_absent_facets() {
        assert_eq!(
            serde_json::to_value(PerModToggles {
                settings: Some(false),
                config: None,
            })
            .unwrap(),
            json!({ "settings": false })
        );
    }

    #[test]
    fn export_result_round_trips_losslessly() {
        let result = ExportUserDataResult {
            archive: "{\n  \"format\": \"windhawk-user-data\"\n}".to_owned(),
            summary: ExportSummary {
                warnings: vec![ExportWarning {
                    mod_id: "local@x".to_owned(),
                    message: "settings omitted".to_owned(),
                }],
            },
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(
            value,
            json!({
                "archive": "{\n  \"format\": \"windhawk-user-data\"\n}",
                "summary": { "warnings": [{ "modId": "local@x", "message": "settings omitted" }] }
            })
        );
        let back: ExportUserDataResult = serde_json::from_value(value).unwrap();
        assert_eq!(back, result);
    }

    #[test]
    fn conflict_policy_defaults_to_overwrite_and_round_trips() {
        assert_eq!(ConflictPolicy::default(), ConflictPolicy::Overwrite);
        assert_eq!(
            serde_json::to_value(ConflictPolicy::Overwrite).unwrap(),
            json!("overwrite")
        );
        assert_eq!(
            serde_json::to_value(ConflictPolicy::Skip).unwrap(),
            json!("skip")
        );
        // A minimal params object fills the options with their defaults.
        let params: ImportUserDataParams =
            serde_json::from_value(json!({ "archive": "{}", "selection": { "mods": "all" } }))
                .unwrap();
        assert_eq!(params.options, ImportOptions::default());
        assert_eq!(params.options.on_conflict, ConflictPolicy::Overwrite);
    }

    #[test]
    fn import_summary_omits_app_settings_when_absent() {
        let summary = ImportSummary {
            mods: vec![ImportModOutcome {
                mod_id: "taskbar-clock".to_owned(),
                status: ImportModStatus::Installed,
                message: None,
            }],
            app_settings: None,
        };
        assert_eq!(
            serde_json::to_value(&summary).unwrap(),
            json!({ "mods": [{ "modId": "taskbar-clock", "status": "installed" }] })
        );
    }
}
