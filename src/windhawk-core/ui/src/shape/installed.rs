//! The installed-mods projection. The core's `listInstalledMods` result is `{
//! mods, loadErrors }`, and each `mods` entry IS the front-end's `{ metadata,
//! config, latestVersion, userRating }` shape. So the reply is the `mods` object
//! lifted under `installedMods`, entries untouched - no typed re-serialize that
//! could drop a field the core adds. The profile watcher re-derives the per-mod
//! subset through [`installed_mods_details`] in THIS module, so the watcher and
//! the list handler cannot drift into two mappings.
//!
//! Neither boundary carries an update ANSWER, only the terms it is reached from.
//! The front-end holds a cache those terms reach one at a time (a config write
//! turns an offer down, an install takes a version), so an answer held beside
//! them would be stale between listings; it applies the rule itself, over
//! `latestVersion`, the metadata version, and the config's
//! `updatesDisabledForVersion`.
//!
//! That trade only works while every term reaches the cache, which is why
//! [`installed_mods_details`] carries all three and not just the profile-held
//! ones. The other projections carry the mod itself and the front-end reads the
//! installed version and the suppression off it; this event does not carry the
//! mod, and it is the message that arrives when ANOTHER process has been at it -
//! a config it wrote, an install or a recompile it ran.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::shape::webview_ipc::{
    InstalledModDetails, InstalledModProfileFields, UpdateInstalledModsDetails,
    UpdateInstalledModsDetailsEntry, to_wire,
};

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
/// latestVersion, installedVersion, updatesDisabledForVersion, userRating } } }`.
/// The same source the `getInstalledMods` reply reads, narrowed to the subset the
/// event carries - so the watcher and the list handler stay one mapping. A
/// missing/!object `mods` degrades to an empty `details` map.
pub fn installed_mods_details(list_result: &Value) -> Value {
    // A BTreeMap keys the details by mod id in sorted order. That is deterministic but
    // differs from the `getInstalledMods` reply, which forwards `mods` in the core's
    // insertion order (serde_json's `preserve_order`). Benign on the wire: the front-end
    // keys the map by id, and JSON is compared by value, not serialized key order (the
    // rationale the `preserve_order` note in the workspace Cargo.toml already records).
    let mut details = BTreeMap::new();
    if let Some(mods) = list_result.get("mods").and_then(Value::as_object) {
        for (id, entry) in mods {
            details.insert(id.clone(), details_entry(entry));
        }
    }
    to_wire(UpdateInstalledModsDetails { details })
}

/// One event entry off a core entry, with the degradations its fields have one by
/// one for an entry missing them; the core emits all four. A `null`
/// `latestVersion` (the core knows of none) reads as absent, which is what it
/// means, and so does a `null` `installedVersion` (no readable source to name
/// one). `userRating` is i64 end to end (see [`InstalledModProfileFields`]);
/// `as_i64` is the exact match and falls back to 0 only for a malformed/absent
/// field the core never emits.
///
/// The two mod-side terms come off the entry's own `metadata`/`config`, the
/// things that OWN them, rather than off the profile's mirror of either. A mod
/// with no config has no suppression, and the empty string IS "suppresses
/// nothing" in the grammar, so that absent case needs no case of its own.
fn details_entry(entry: &Value) -> UpdateInstalledModsDetailsEntry {
    UpdateInstalledModsDetailsEntry {
        profile: InstalledModProfileFields {
            latest_version: entry
                .get("latestVersion")
                .and_then(Value::as_str)
                .map(str::to_owned),
            user_rating: entry.get("userRating").and_then(Value::as_i64).unwrap_or(0),
        },
        installed_version: entry
            .get("metadata")
            .and_then(|metadata| metadata.get("version"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        updates_disabled_for_version: entry
            .get("config")
            .and_then(|config| config.get("updatesDisabledForVersion"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

/// The details an `installMod` / `compileMod` reply carries: the mod's
/// `getInstalledModDetails`, taken after the operation, with the operation's own
/// `metadata` and `config` written over it.
///
/// The entry IS the reply's shape, so it is forwarded rather than re-read field by
/// field: a field the core adds to an entry reaches this reply the way it reaches
/// the listing (the module doc's pass-through). Its metadata and config are the two
/// it does not keep - read back off the machine, where what a reply reports is what
/// the operation did, and those are the values it returned.
///
/// The profile-held pair is backfilled where the entry does not name it, so the
/// reply always declares both keys whatever came back. An entry that is not an
/// object is one the core does not emit; it degrades to the operation's own
/// report, as a follow-up that failed outright does.
///
/// A word on what `latestVersion` will be. An install commit resets the profile's
/// cached repository version (the version it just took IS what the repository
/// held), so the entry behind an `installMod` names none and the reply says the
/// repository side is unknown - which clears a badge rather than inventing one.
/// A recompile takes no version, so behind a `compileMod` the cached version
/// survives and the reply names it.
pub fn installed_mod_details(metadata: Value, config: Value, entry: &Value) -> Value {
    let Value::Object(mut details) = entry.clone() else {
        return installed_mod_details_only(metadata, config);
    };
    // The four keys are the core's own, so these overwrite in place and the
    // entry's key order rides through unchanged.
    details.insert("metadata".to_owned(), metadata);
    details.insert("config".to_owned(), config);
    let unknown = InstalledModProfileFields::default();
    backfill(&mut details, "latestVersion", json!(unknown.latest_version));
    backfill(&mut details, "userRating", json!(unknown.user_rating));
    Value::Object(details)
}

/// Name `key` where the entry does not, at the value that stands for knowing
/// nothing - the same one [`installed_mod_details_only`] answers with. The two
/// keys are required by the contract, and forwarding an entry cannot promise
/// they were in it.
fn backfill(details: &mut Map<String, Value>, key: &str, unknown: Value) {
    if !details.contains_key(key) {
        details.insert(key.to_owned(), unknown);
    }
}

/// The details for an operation that landed but whose follow-up read did not:
/// what the operation itself reports, with the profile-held pair at what is known
/// about a mod nothing is known about. Naming no repository version is what a
/// front-end reads as a repository side that has not arrived, which is the truth
/// here - not as a mod that is up to date.
///
/// The rating is not like that: `userRating: 0` is a value INVENTED for a field
/// the contract requires and the front-end would otherwise take at face value -
/// nothing pushes the rating again (the profile write behind an install is the
/// host's own, so the profile watcher does not fire on it), so a real rating
/// would stand at unrated until the next FULL listing. What keeps that from
/// happening is the error the reply carries beside these values: the operation
/// succeeded and the read after it did not, and the front-end is told both, so
/// it can keep what it already had rather than adopt an invented 0.
pub fn installed_mod_details_only(metadata: Value, config: Value) -> Value {
    to_wire(InstalledModDetails {
        metadata,
        config,
        profile: InstalledModProfileFields::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_mods_under_installed_mods_untouched() {
        let entries = json!({
            "alpha@1": {
                "metadata": { "name": "Alpha", "version": "1.0" },
                "config": { "libraryFileName": "alpha.dll", "disabled": false },
                "latestVersion": "1.1",
                "userRating": 3,
                // A field the core might add: the entries are forwarded, not
                // re-read, so it arrives without a change here.
                "someFutureField": "carried",
            }
        });
        // The entries pass through as they came - including the core-only
        // `libraryFileName` the front-end tolerates - under `installedMods`.
        assert_eq!(
            installed_mods_reply(&json!({ "mods": entries, "loadErrors": [] })),
            json!({ "installedMods": entries })
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
    fn details_projects_the_terms_of_the_update_answer() {
        let result = json!({
            "mods": {
                // An offer: a version held that the mod is not at, and not
                // refused.
                "alpha@1": {
                    "metadata": { "name": "Alpha", "version": "1.0" },
                    "config": { "disabled": false, "updatesDisabledForVersion": "" },
                    "latestVersion": "1.1",
                    "userRating": 3,
                },
                // A version held and no offer, because the version held is the
                // one the user turned down. The fields together are what say so;
                // any of them alone reads as a mod that is up to date.
                "beta@2": {
                    "metadata": { "name": "Beta", "version": "1.9" },
                    "config": { "disabled": true, "updatesDisabledForVersion": "=2.0" },
                    "latestVersion": "2.0",
                    "userRating": 0,
                },
                // The other no-offer state a version alone cannot tell from the
                // one above: the mod is AT the version held. This is the state a
                // consumer's cache would miss, the mod having moved to it since
                // the last listing.
                "gamma@3": {
                    "metadata": { "name": "Gamma", "version": "3.0" },
                    "config": { "disabled": false, "updatesDisabledForVersion": "" },
                    "latestVersion": "3.0",
                    "userRating": 0,
                },
                // A core that named no version at all, for a mod with neither a
                // readable source to be at one nor a config to have refused one.
                "delta@4": {
                    "metadata": null,
                    "config": null,
                    "latestVersion": null,
                    "userRating": 0,
                }
            },
            "loadErrors": [],
        });
        // The four terms survive; the rest of the metadata and the config are
        // dropped.
        assert_eq!(
            installed_mods_details(&result),
            json!({
                "details": {
                    "alpha@1": {
                        "latestVersion": "1.1",
                        "installedVersion": "1.0",
                        "updatesDisabledForVersion": "",
                        "userRating": 3,
                    },
                    "beta@2": {
                        "latestVersion": "2.0",
                        "installedVersion": "1.9",
                        "updatesDisabledForVersion": "=2.0",
                        "userRating": 0,
                    },
                    "gamma@3": {
                        "latestVersion": "3.0",
                        "installedVersion": "3.0",
                        "updatesDisabledForVersion": "",
                        "userRating": 0,
                    },
                    "delta@4": {
                        "latestVersion": null,
                        "installedVersion": null,
                        "updatesDisabledForVersion": "",
                        "userRating": 0,
                    },
                }
            })
        );
    }

    #[test]
    fn details_absent_fields_project_as_the_answer_they_stand_for() {
        // The degradations the fields have one by one, for an entry the core does
        // not emit: neither version named, and no suppression stored - the empty
        // string, which is what "suppresses nothing" is in the grammar. Every key
        // still rides the event.
        let result = json!({
            "mods": {
                "alpha@1": { "userRating": 0 }
            },
        });
        assert_eq!(
            installed_mods_details(&result),
            json!({
                "details": {
                    "alpha@1": {
                        "latestVersion": null,
                        "installedVersion": null,
                        "updatesDisabledForVersion": "",
                        "userRating": 0,
                    },
                }
            })
        );
    }

    #[test]
    fn mod_details_take_the_profile_fields_off_the_entry() {
        // The entry read back for the mod, which names a metadata and a config of
        // its own - and they are not what the reply reports.
        let entry = json!({
            "metadata": { "name": "Alpha", "version": "1.0" },
            "config": { "libraryFileName": "read-back.dll", "disabled": false },
            "latestVersion": "1.3",
            "userRating": 4,
        });
        // The metadata and config are the operation's, forwarded as they came; the
        // profile-held pair is the entry's. An install of 1.0 while the repository
        // holds 1.3 is an offer that stands - and what says so is the version, the
        // front-end reaching the answer from it and the two fields beside it.
        assert_eq!(
            installed_mod_details(
                json!({ "name": "Alpha", "version": "1.0" }),
                json!({ "libraryFileName": "alpha_1.0_1.dll", "disabled": false }),
                &entry,
            ),
            json!({
                "metadata": { "name": "Alpha", "version": "1.0" },
                "config": { "libraryFileName": "alpha_1.0_1.dll", "disabled": false },
                "latestVersion": "1.3",
                "userRating": 4,
            })
        );
    }

    #[test]
    fn mod_details_carry_a_field_the_core_adds_to_the_entry() {
        // The entry is forwarded, not re-read field by field, so a field the core
        // starts carrying reaches this reply without a change here - the same
        // additivity `installed_mods_reply` gives the listing.
        let details = installed_mod_details(
            json!({ "name": "Alpha" }),
            json!({ "disabled": false }),
            &json!({
                "metadata": null,
                "config": null,
                "latestVersion": null,
                "userRating": 0,
                "someFutureField": "carried",
            }),
        );
        assert_eq!(details["someFutureField"], json!("carried"));
        assert_eq!(details["metadata"], json!({ "name": "Alpha" }));
    }

    #[test]
    fn mod_details_name_a_profile_field_the_entry_left_out() {
        // The entry is forwarded rather than re-read, so nothing about it
        // guarantees the two keys the contract requires. An entry naming neither
        // - which the core does not emit - still answers under both, at the value
        // the follow-up-failed reply uses for "nothing known".
        let details = installed_mod_details(
            json!({ "name": "Alpha" }),
            json!({ "disabled": false }),
            &json!({ "metadata": null, "config": null }),
        );
        assert_eq!(details["latestVersion"], json!(null));
        assert_eq!(details["userRating"], json!(0));
    }

    #[test]
    fn mod_details_of_an_entry_that_is_not_an_object_claim_nothing() {
        // A core that answered with something other than an entry - which it does
        // not - degrades to the operation's own report rather than to a reply
        // shaped like neither.
        assert_eq!(
            installed_mod_details(json!(null), json!(null), &Value::Null),
            installed_mod_details_only(json!(null), json!(null))
        );
    }

    #[test]
    fn mod_details_without_an_entry_claim_nothing() {
        // What an operation whose follow-up read failed reports: itself, and
        // nothing about the profile.
        assert_eq!(
            installed_mod_details_only(json!(null), json!(null)),
            json!({
                "metadata": null,
                "config": null,
                "latestVersion": null,
                "userRating": 0,
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
