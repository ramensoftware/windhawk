//! DTOs of the mod source and user profile commands, mirroring
//! `ListInstalledModsParams`/`ListInstalledModsResult`, `AppUpdateStatus`,
//! `ProfileWatchInfo`, and `CatalogForProfileSync` in `windhawk-vscode`'s
//! `src/coreClient/contract.ts` 1:1. camelCase field names match the TS
//! property names so the client does no mapping.

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
///
/// The entry carries the TERMS of the update answer and not the answer:
/// [`is_update_available`](Self::is_update_available) reaches it from the
/// metadata version, the cached `latest_version`, and the config's stored
/// suppression, all three of which are already here. Every consumer holds an
/// entry, so every consumer can ask; one holding the answer instead would hold
/// one reached before the terms beside it moved.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModListEntry {
    pub metadata: Option<ModMetadata>,
    pub config: Option<ModConfig>,
    /// The version the repository holds, as the profile last cached it, or
    /// `None` where the host knows of none (updates not asked for, a local mod,
    /// nothing cached). Not suppression-aware, so a refused offer still names
    /// what was refused - which is what tells that state from a mod that is up
    /// to date. Serialized even when `None`, matching the `string | null` the TS
    /// mirrors declare.
    pub latest_version: Option<String>,
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

    /// Whether an update is being offered: `latest_version` names a version
    /// other than the installed one, and the stored `updatesDisabledForVersion`
    /// does not suppress that version.
    ///
    /// A method rather than a field, because an answer held beside the terms it
    /// was reached from goes stale the moment one of them moves - and they move
    /// one at a time for a holder that caches an entry (a config write turns an
    /// offer down, an install takes a version). Every consumer therefore reaches
    /// it from an entry it is holding: this method, and the TypeScript
    /// `resolveUpdateOffer` for the front-end that gets the terms as separate
    /// messages. The two are the SAME rule, held together by the
    /// `updatesDisabledForVersion` truth table asserted in both languages.
    ///
    /// Nothing is offered on a mod the machine has neither a source nor a config
    /// for. The listing cannot reach that case at all - its ids ARE the sources
    /// and configs on disk - but `getInstalledModDetails` is asked about an id,
    /// and the profile can hold a version for one removed from outside until the
    /// next reconciliation drops the entry. Without the guard such an id would
    /// answer that an update is available for a mod that is not there: the
    /// installed version reads as the empty string, which differs from every
    /// version the repository names.
    pub fn is_update_available(&self) -> bool {
        let Some(latest) = self.latest_version.as_deref() else {
            return false;
        };
        let installed = self
            .metadata
            .as_ref()
            .and_then(|m| m.version.as_deref())
            .unwrap_or_default();
        self.is_installed()
            && latest != installed
            && !self
                .config
                .as_ref()
                .is_some_and(|c| c.suppresses_update_offer(latest))
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
// getInstalledModDetails

/// One mod, on the terms `listInstalledMods` lists them on: `language` for the
/// metadata parse, `checkForUpdates` gating the cached repository version. There
/// is no `syncProfile` twin - the reconciliation is a pass over every installed
/// mod, which is the listing's business and not a single mod's. The result is an
/// [`InstalledModListEntry`], the same entry the listing would carry for this id.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetInstalledModDetailsParams {
    pub mod_id: String,
    pub language: String,
    pub check_for_updates: bool,
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

    fn bare_entry() -> InstalledModListEntry {
        InstalledModListEntry {
            metadata: None,
            config: None,
            latest_version: None,
            user_rating: 0,
        }
    }

    fn config_with_suppression(stored: &str) -> Option<ModConfig> {
        serde_json::from_value(json!({
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
            "version": "1.0",
            "updatesDisabledForVersion": stored
        }))
        .ok()
    }

    #[test]
    fn list_entry_serializes_null_metadata_config_and_latest_version() {
        let entry = bare_entry();
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            json!({
                "metadata": null,
                "config": null,
                "latestVersion": null,
                "userRating": 0
            })
        );
        let back: InstalledModListEntry =
            serde_json::from_value(serde_json::to_value(&entry).unwrap()).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn is_installed_reads_only_config_or_metadata() {
        let none = bare_entry();
        assert!(!none.is_installed());

        let with_config = InstalledModListEntry {
            config: config_with_suppression(""),
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

    /// The update rule over an installed mod at 1.0, across what the cached
    /// version and the stored suppression can be. The two `false` rows a version
    /// alone cannot tell apart - up to date, and nothing cached - are why the
    /// entry carries the version rather than the answer.
    #[test]
    fn is_update_available_reads_the_three_terms() {
        let installed = InstalledModListEntry {
            metadata: Some(ModMetadata {
                id: Some("m".to_owned()),
                version: Some("1.0".to_owned()),
                ..Default::default()
            }),
            config: config_with_suppression(""),
            ..bare_entry()
        };

        for (latest, stored, expected) in [
            (Some("1.1"), "", true),
            (Some("1.0"), "", false),
            (None, "", false),
            // The pin is equality against the version being offered, so one the
            // repository has moved past refuses nothing; `*` refuses everything.
            (Some("1.1"), "=1.1", false),
            (Some("1.2"), "=1.1", true),
            (Some("1.1"), "*", false),
            // Outside the grammar is not a suppression, so the offer stands.
            (Some("1.1"), "1.1", true),
        ] {
            let entry = InstalledModListEntry {
                latest_version: latest.map(str::to_owned),
                config: config_with_suppression(stored),
                ..installed.clone()
            };
            assert_eq!(
                entry.is_update_available(),
                expected,
                "latest {latest:?}, stored {stored:?}"
            );
        }
    }

    /// A mod the machine has neither a source nor a config for is offered
    /// nothing, however the profile still describes it. `getInstalledModDetails`
    /// is the one caller that can be asked about such an id.
    #[test]
    fn is_update_available_offers_nothing_on_a_mod_that_is_not_there() {
        let ghost = InstalledModListEntry {
            latest_version: Some("1.1".to_owned()),
            user_rating: 4,
            ..bare_entry()
        };
        assert!(!ghost.is_installed());
        assert!(!ghost.is_update_available());

        // The same profile row, once a config for the id exists again: the
        // installed version reads as the empty string, which every version the
        // repository names differs from.
        let reinstated = InstalledModListEntry {
            config: config_with_suppression(""),
            ..ghost
        };
        assert!(reinstated.is_update_available());
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
