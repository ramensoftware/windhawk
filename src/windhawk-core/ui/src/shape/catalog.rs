//! Catalog shapers: the featured subset (`getFeaturedMods`) and the
//! catalog+installed overlay (`getRepositoryMods`). Both read the catalog
//! `mods` map verbatim - the core returns the catalog unchanged, so entries
//! pass through untouched rather than round-tripping through a typed DTO that
//! would drop a field the catalog adds. The overlay's installed-join predicate
//! is the shared [`InstalledModListEntry::is_installed()`], single-sourced in
//! `protocol` and shared with the CLI, so the rule cannot drift.

use serde_json::{Map, Value, json};
use windhawk_core_protocol::InstalledModListEntry;

/// `getFeaturedMods`: the subset of `catalog.mods` whose entry is `featured`,
/// each entry verbatim. A missing/!object `mods` yields an empty object (the
/// reply's `featuredMods: null` failure case is the handler's, not this shaper's).
pub fn featured_subset(catalog: &Value) -> Value {
    let mut out = Map::new();
    if let Some(mods) = catalog.get("mods").and_then(Value::as_object) {
        for (id, entry) in mods {
            if entry
                .get("featured")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                out.insert(id.clone(), entry.clone());
            }
        }
    }
    Value::Object(out)
}

/// `getRepositoryMods`: the catalog overlaid with installed state. Each catalog
/// entry becomes `{ repository: <entry> }`; an `installed: { metadata, config,
/// userRating }` is grafted on for catalog mods that are also installed (the
/// shared
/// [`is_installed`](windhawk_core_protocol::InstalledModListEntry::is_installed)
/// predicate). The installed values pass through RAW (looked up from the
/// listInstalledMods result by id) so a config/metadata field the core adds is
/// not dropped; the typed entry is used only to apply the predicate.
///
/// The predicate is applied PER ENTRY: a single installed entry that does not decode
/// (an unexpected config shape) is skipped on its own, so it loses only its own
/// overlay rather than zeroing every mod's installed state - the catalog side still
/// renders it.
pub fn repository_mods_overlay(catalog: &Value, installed_result: &Value) -> Value {
    let mut mods = Map::new();
    if let Some(cat_mods) = catalog.get("mods").and_then(Value::as_object) {
        for (id, entry) in cat_mods {
            mods.insert(id.clone(), json!({ "repository": entry }));
        }
    }

    if let Some(installed_mods) = installed_result.get("mods").and_then(Value::as_object) {
        for (id, raw) in installed_mods {
            // Decode JUST this entry for the shared predicate. A per-entry
            // failure skips this mod only, not the whole overlay.
            let Ok(entry) = serde_json::from_value::<InstalledModListEntry>(raw.clone()) else {
                continue;
            };
            if !entry.is_installed() {
                continue;
            }
            let Some(slot) = mods.get_mut(id).and_then(Value::as_object_mut) else {
                continue;
            };
            slot.insert(
                "installed".to_owned(),
                json!({
                    "metadata": raw.get("metadata").cloned().unwrap_or(Value::Null),
                    "config": raw.get("config").cloned().unwrap_or(Value::Null),
                    "userRating": raw.get("userRating").cloned().unwrap_or(json!(0)),
                }),
            );
        }
    }

    json!({ "mods": Value::Object(mods) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Value {
        json!({
            "app": { "version": "1.8.0" },
            "mods": {
                "alpha": { "metadata": { "name": "Alpha" }, "details": { "users": 5 }, "featured": true },
                "beta": { "metadata": { "name": "Beta" }, "details": { "users": 1 } },
            }
        })
    }

    #[test]
    fn featured_subset_keeps_only_featured_entries_verbatim() {
        let featured = featured_subset(&catalog());
        // Only the featured entry, passed through whole (including `featured`).
        assert_eq!(
            featured,
            json!({
                "alpha": { "metadata": { "name": "Alpha" }, "details": { "users": 5 }, "featured": true }
            })
        );
    }

    #[test]
    fn featured_subset_is_empty_when_none_featured() {
        let none = json!({ "mods": { "beta": { "metadata": {}, "details": {} } } });
        assert_eq!(featured_subset(&none), json!({}));
    }

    /// A complete `ModConfig` JSON (every field present), as the core's
    /// `listInstalledMods` always returns - the overlay deserializes the whole
    /// result to apply the shared predicate, so a partial config would not decode.
    fn full_config() -> Value {
        json!({
            "libraryFileName": "alpha.dll",
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
        })
    }

    #[test]
    fn overlay_grafts_installed_state_onto_catalog_mods() {
        let installed = json!({
            "mods": {
                // Installed AND in the catalog: gets the overlay (raw values, incl.
                // the core-only libraryFileName).
                "alpha": {
                    "metadata": { "name": "Alpha", "version": "1.0" },
                    "config": full_config(),
                    "updateAvailable": true,
                    "userRating": 4
                },
                // Installed but NOT in the catalog: contributes nothing.
                "gamma": {
                    "metadata": { "name": "Gamma" }, "config": null,
                    "updateAvailable": false, "userRating": 0
                },
                // In the catalog but a both-null entry: is_installed() is
                // false, so no overlay.
                "beta": { "metadata": null, "config": null, "updateAvailable": false, "userRating": 0 }
            },
            "loadErrors": []
        });
        let out = repository_mods_overlay(&catalog(), &installed);
        let mods = &out["mods"];
        // Every catalog mod is present under `repository`.
        assert_eq!(
            mods["alpha"]["repository"]["metadata"]["name"],
            json!("Alpha")
        );
        assert_eq!(
            mods["beta"]["repository"]["metadata"]["name"],
            json!("Beta")
        );
        // alpha gets the installed overlay with raw config (libraryFileName kept).
        assert_eq!(
            mods["alpha"]["installed"]["config"]["libraryFileName"],
            json!("alpha.dll")
        );
        assert_eq!(mods["alpha"]["installed"]["userRating"], json!(4));
        // beta's both-null entry is not counted as installed.
        assert_eq!(mods["beta"].get("installed"), None);
        // gamma is not in the catalog, so it does not appear at all.
        assert_eq!(mods.get("gamma"), None);
    }

    #[test]
    fn a_single_undecodable_installed_entry_does_not_drop_the_others() {
        // One installed entry has a partial config (not a full ModConfig), so it does
        // not decode. The overlay must still graft the OTHER installed mod rather than
        // zeroing every mod's installed state (the per-entry predicate, not a single
        // decode of the whole result).
        let installed = json!({
            "mods": {
                "alpha": {
                    "metadata": { "name": "Alpha" },
                    "config": full_config(),
                    "updateAvailable": false,
                    "userRating": 4
                },
                "beta": {
                    "metadata": { "name": "Beta" },
                    "config": { "disabled": false }, // partial: not a full ModConfig
                    "updateAvailable": false,
                    "userRating": 1
                }
            },
            "loadErrors": []
        });
        let out = repository_mods_overlay(&catalog(), &installed);
        let mods = &out["mods"];
        // alpha decoded and is overlaid - it is NOT collateral of beta's bad shape.
        assert_eq!(mods["alpha"]["installed"]["userRating"], json!(4));
        // beta did not decode, so it gets no overlay, but its catalog side survives.
        assert_eq!(
            mods["beta"]["repository"]["metadata"]["name"],
            json!("Beta")
        );
        assert_eq!(mods["beta"].get("installed"), None);
    }
}
