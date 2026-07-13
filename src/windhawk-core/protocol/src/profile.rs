//! DTOs of the mod source and user profile commands, mirroring
//! `ListInstalledModsParams`/`ListInstalledModsResult`, `AppUpdateStatus`,
//! `ProfileWatchInfo`, and `CatalogForProfileSync` in the front-end
//! repository's `src/coreClient/contract.ts` 1:1. camelCase field names match
//! the TS property names so the client does no mapping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::parse_mod_source::ModMetadata;
use crate::settings::ModConfig;

////////////////////////////////////////////////////////////////////////////
// listInstalledMods

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListInstalledModsParams {
    pub language: String,
    pub check_for_updates: bool,
    pub sync_profile: bool,
}

/// One installed-mod entry of the composite listing. `metadata`/`config` are
/// serialized even when `null` (a mod can have a config but no parseable
/// source, or a local source with no config).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModListEntry {
    pub metadata: Option<ModMetadata>,
    pub config: Option<ModConfig>,
    pub update_available: bool,
    pub user_rating: i64,
}

impl InstalledModListEntry {
    /// Whether this entry counts as installed for the catalog overlay: a mod
    /// with a config OR parseable metadata. The catalog side contributes only
    /// the mod-id key, so this reads ONLY the installed side. The one
    /// definition both consumers call - the UI's `getRepositoryMods` overlay
    /// (`shape/catalog.rs`) and the CLI's `repo list --with-installed` - so the
    /// rule cannot drift into two spellings; only the small predicate is
    /// shared, the render shapes stay per-consumer.
    pub fn is_installed(&self) -> bool {
        self.config.is_some() || self.metadata.is_some()
    }
}

/// A per-mod source-parse failure, rendered as `String(error)` (i.e.
/// `Error: <message>`) to match the TS surfacing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModLoadError {
    pub mod_id: String,
    pub error: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListInstalledModsResult {
    /// Installed mods keyed by mod id. This is a `BTreeMap`, so the serialized
    /// JSON object's keys are in lexicographic mod-id order REGARDLESS of
    /// on-disk directory or registry-enumeration order. That sorted order is a
    /// deliberate, stable wire contract (a JSON object's key order is observable
    /// to a client that iterates it), pinned by
    /// `list_installed_mods_are_sorted_by_mod_id`; do not swap the container for
    /// one that leaks filesystem order.
    pub mods: BTreeMap<String, InstalledModListEntry>,
    pub load_errors: Vec<ModLoadError>,
}

////////////////////////////////////////////////////////////////////////////
// setModRating

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetModRatingParams {
    pub mod_id: String,
    /// A nonzero rating is stored; 0 clears the entry (the TS `if (rating)`).
    pub rating: i64,
}

////////////////////////////////////////////////////////////////////////////
// getAppUpdateStatus

/// `AppUpdateStatus` of contract.ts: the cached latest-version strings and
/// their comparison against the session's installed version.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub latest_version: Option<String>,
    pub latest_version_bleeding_edge: Option<String>,
    pub update_available: bool,
    pub update_available_bleeding_edge: bool,
}

////////////////////////////////////////////////////////////////////////////
// getProfileWatchInfo

/// `ProfileWatchInfo` of contract.ts. `lastModifiedByUserMtimeMs` is a JS
/// number (fractional milliseconds); a `serde_json::Number` so an integer
/// capture round-trips as an integer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWatchInfo {
    pub file_path: String,
    pub last_modified_by_user_mtime_ms: Option<serde_json::Number>,
}

////////////////////////////////////////////////////////////////////////////
// syncCatalogToProfile

/// The minimal slice of the catalog the profile sync reads (the TS
/// `CatalogForProfileSync`); unknown catalog fields are tolerated.
#[derive(Deserialize, Debug, Clone)]
pub struct CatalogForProfileSync {
    #[serde(default)]
    pub app: CatalogAppForSync,
    #[serde(default)]
    pub mods: BTreeMap<String, CatalogModForSync>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAppForSync {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub version_bleeding_edge: Option<String>,
    #[serde(default)]
    pub version_pre_release: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CatalogModForSync {
    #[serde(default)]
    pub metadata: CatalogModMetadataForSync,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct CatalogModMetadataForSync {
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SyncCatalogToProfileParams {
    pub catalog: CatalogForProfileSync,
}

/// The SEND side of `syncCatalogToProfile`. Deliberately NOT the parse-side
/// [`SyncCatalogToProfileParams`]: its `catalog` is the lossy
/// `CatalogForProfileSync` projection (it reads only `app.version` +
/// `mods.*.metadata.version`), but the caller holds the FULL catalog as the raw
/// `Value` `fetchCatalog` returned verbatim. Carrying it opaquely keeps the
/// request byte-identical to the old `json!({ catalog })` and never silently
/// drops a catalog field the core may start reading - a drop the output-only
/// parity self-diff could not catch.
#[derive(Serialize, Debug, Clone)]
pub struct SyncCatalogToProfileRequest {
    pub catalog: serde_json::Value,
}

/// Result of `syncCatalogToProfile`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncCatalogToProfileResult {
    pub profile_updated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn list_entry_serializes_null_metadata_and_config() {
        let entry = InstalledModListEntry {
            metadata: None,
            config: None,
            update_available: false,
            user_rating: 0,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            json!({
                "metadata": null,
                "config": null,
                "updateAvailable": false,
                "userRating": 0
            })
        );
    }

    #[test]
    fn is_installed_reads_only_config_or_metadata() {
        let none = InstalledModListEntry {
            metadata: None,
            config: None,
            update_available: false,
            user_rating: 0,
        };
        assert!(!none.is_installed());

        let with_config = InstalledModListEntry {
            config: serde_json::from_value(json!({
                "libraryFileName": "m.dll",
                "disabled": false,
                "loggingEnabled": false,
                "debugLoggingEnabled": false,
                "include": [],
                "exclude": [],
                "includeCustom": [],
                "excludeCustom": [],
                "includeExcludeCustomOnly": false,
                "patternsMatchCriticalSystemProcesses": false,
                "architecture": [],
                "version": "1.0"
            }))
            .ok(),
            ..none.clone()
        };
        assert!(with_config.is_installed());

        let with_metadata = InstalledModListEntry {
            metadata: Some(ModMetadata {
                id: Some("m".to_owned()),
                ..Default::default()
            }),
            ..none
        };
        assert!(with_metadata.is_installed());
    }

    #[test]
    fn profile_watch_info_round_trips_integer_mtime() {
        let v = json!({"filePath": "C:\\x\\userprofile.json", "lastModifiedByUserMtimeMs": 12345});
        let dto: ProfileWatchInfo = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&dto).unwrap(), v);
    }

    #[test]
    fn catalog_tolerates_extra_fields() {
        let v = json!({
            "catalog": {
                "app": {"version": "1.8.0", "versionBleedingEdge": "1.9.0", "versionPreRelease": "2.0.0-alpha.2"},
                "mods": {"m": {"metadata": {"id": "m", "version": "2.0", "name": "M"}, "featured": true}}
            }
        });
        let p: SyncCatalogToProfileParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.catalog.app.version.as_deref(), Some("1.8.0"));
        assert_eq!(
            p.catalog.app.version_bleeding_edge.as_deref(),
            Some("1.9.0")
        );
        assert_eq!(
            p.catalog.app.version_pre_release.as_deref(),
            Some("2.0.0-alpha.2")
        );
        assert_eq!(p.catalog.mods["m"].metadata.version.as_deref(), Some("2.0"));
    }
}
