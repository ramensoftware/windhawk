//! The installed-mods projection. The core's `listInstalledMods` result is `{
//! mods, loadErrors }`, and each `mods` entry is ALREADY the front-end's `{
//! metadata, config, updateAvailable, userRating }` shape (the extension
//! forwards it verbatim via `Object.assign`). So the reply is just the `mods`
//! object lifted under `installedMods`; the entries pass through untouched (no
//! typed re-serialize that could drop a field the core adds). The profile
//! watcher re-derives the update-availability + ratings SUBSET through
//! [`installed_mods_details`] in THIS module, so the watcher and the list
//! handler cannot drift into two mappings.

use serde_json::{Map, Value, json};

/// Shape a `listInstalledMods` result into the `getInstalledMods` reply data:
/// `{ installedMods: <result.mods> }`. A missing `mods` (a malformed result)
/// degrades to an empty map rather than failing the reply.
pub fn installed_mods_reply(list_result: &Value) -> Value {
    let mods = list_result
        .get("mods")
        .cloned()
        .unwrap_or_else(|| json!({}));
    json!({ "installedMods": mods })
}

/// Project a `listInstalledMods` result into the `updateInstalledModsDetails`
/// event data the profile watcher pushes: `{ details: { <modId>: {
/// updateAvailable, userRating } } }`. The same source the `getInstalledMods`
/// reply reads, narrowed to the update-availability + rating subset the event
/// carries - so the watcher and the list handler stay one mapping. A
/// missing/!object `mods` degrades to an empty `details` map.
pub fn installed_mods_details(list_result: &Value) -> Value {
    let mut details = Map::new();
    if let Some(mods) = list_result.get("mods").and_then(Value::as_object) {
        for (id, entry) in mods {
            details.insert(
                id.clone(),
                json!({
                    "updateAvailable": entry.get("updateAvailable").and_then(Value::as_bool).unwrap_or(false),
                    "userRating": entry.get("userRating").cloned().unwrap_or_else(|| json!(0)),
                }),
            );
        }
    }
    json!({ "details": Value::Object(details) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_mods_under_installed_mods_verbatim() {
        let result = json!({
            "mods": {
                "alpha@1": {
                    "metadata": { "name": "Alpha", "version": "1.0" },
                    "config": { "libraryFileName": "alpha.dll", "disabled": false },
                    "updateAvailable": true,
                    "userRating": 3,
                }
            },
            "loadErrors": [],
        });
        // The entries pass through unchanged - including the core-only
        // `libraryFileName` the front-end tolerates - under `installedMods`.
        assert_eq!(
            installed_mods_reply(&result),
            json!({
                "installedMods": {
                    "alpha@1": {
                        "metadata": { "name": "Alpha", "version": "1.0" },
                        "config": { "libraryFileName": "alpha.dll", "disabled": false },
                        "updateAvailable": true,
                        "userRating": 3,
                    }
                }
            })
        );
    }

    #[test]
    fn missing_mods_degrades_to_empty_map() {
        assert_eq!(
            installed_mods_reply(&json!({ "loadErrors": [] })),
            json!({ "installedMods": {} })
        );
    }

    #[test]
    fn details_projects_the_update_and_rating_subset() {
        let result = json!({
            "mods": {
                "alpha@1": {
                    "metadata": { "name": "Alpha" },
                    "config": { "disabled": false },
                    "updateAvailable": true,
                    "userRating": 3,
                },
                "beta@2": {
                    "metadata": null,
                    "config": { "disabled": true },
                    "updateAvailable": false,
                    "userRating": 0,
                }
            },
            "loadErrors": [],
        });
        // Only updateAvailable + userRating survive; metadata/config are dropped.
        assert_eq!(
            installed_mods_details(&result),
            json!({
                "details": {
                    "alpha@1": { "updateAvailable": true, "userRating": 3 },
                    "beta@2": { "updateAvailable": false, "userRating": 0 },
                }
            })
        );
    }

    #[test]
    fn details_missing_mods_degrades_to_empty_map() {
        assert_eq!(
            installed_mods_details(&json!({ "loadErrors": [] })),
            json!({ "details": {} })
        );
    }
}
