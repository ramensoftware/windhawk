//! The user-profile document model and reconciliation rules, a faithful port of
//! `services/userProfile.ts`, with two documented extensions:
//! [`Profile::set_mod_updates_disabled_for_version`] mirrors a mod-config field
//! the TS profile never carried, and [`Profile::add_mod_review_vote`] and
//! [`Profile::retract_mod_review_vote`] keep the review upvotes
//! (`reviewVotes`) the update server counts from the posted profile.
//!
//! The profile is a `serde_json::Value` parsed and serialized with the
//! `preserve_order` feature, so a read-modify-write reproduces
//! `JSON.stringify(profile, null, 2)` byte for byte: object keys keep their
//! insertion order and unknown top-level and per-mod fields survive untouched
//! (which a typed struct cannot do - the order is the *input file's*, not a
//! declaration order). The mutators mirror the TypeScript in-place object
//! semantics exactly: updating a field keeps its position (`Map::insert`),
//! deleting removes it without reordering the rest (`Map::shift_remove`, not
//! the swap-removing `remove`), and a fresh mod entry is appended. The service
//! layer owns the I/O, the named lock, and the last-own-write mtime
//! bookkeeping; this module is pure.

use std::collections::HashSet;

use serde_json::{Map, Value};

/// The per-mod fields that outlive the mod's removal. Both are about the
/// repository mod rather than the copy on the machine, so a reinstall finds
/// them, and an entry holding nothing else counts as not installed.
const PROFILE_KEEP_ON_REMOVE: [&str; 2] = ["rating", "reviewVotes"];

/// One upvote on a review, stored in the profile as the pair
/// `[timestamp, reviewId]` under a mod's `reviewVotes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewVote {
    /// When the vote was cast, in unix seconds.
    pub timestamp: i64,
    pub review_id: i64,
}

impl ReviewVote {
    /// The vote a stored pair spells, or `None` for anything that is not
    /// `[integer, integer]`.
    pub fn from_pair(value: &Value) -> Option<Self> {
        match value.as_array()?.as_slice() {
            [timestamp, review_id] => Some(Self {
                timestamp: timestamp.as_i64()?,
                review_id: review_id.as_i64()?,
            }),
            _ => None,
        }
    }

    pub fn to_pair(&self) -> Value {
        Value::Array(vec![
            Value::Number(self.timestamp.into()),
            Value::Number(self.review_id.into()),
        ])
    }
}

pub struct Profile {
    /// Always a `Value::Object`.
    root: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl Profile {
    /// Parse `text` (the file's UTF-8 contents, or `None` if the file is
    /// absent) into a profile, matching the TS constructor: a missing file or
    /// unparseable/non-object JSON yields an empty profile, and `app`/`mods`
    /// are ensured to be objects (appended at the end if missing).
    pub fn parse(text: Option<&str>) -> Profile {
        let mut root = match text {
            Some(t) => serde_json::from_str::<Value>(t).unwrap_or_else(|_| empty_object()),
            None => empty_object(),
        };
        if !root.is_object() {
            root = empty_object();
        }
        if let Some(obj) = root.as_object_mut() {
            // `userProfile.app = userProfile.app || {}` / `.mods = .mods || {}`:
            // replace a missing or non-object value with an empty object.
            for key in ["app", "mods"] {
                if !obj.get(key).is_some_and(Value::is_object) {
                    obj.insert(key.to_owned(), empty_object());
                }
            }
        }
        Profile { root }
    }

    /// Serialize as `JSON.stringify(profile, null, 2)`.
    pub fn to_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.root).unwrap_or_else(|_| "{}".to_owned())
    }

    fn mods(&self) -> Option<&Map<String, Value>> {
        self.root.get("mods")?.as_object()
    }

    fn mods_mut(&mut self) -> Option<&mut Map<String, Value>> {
        self.root.get_mut("mods")?.as_object_mut()
    }

    fn mod_field(&self, mod_id: &str, field: &str) -> Option<&Value> {
        self.mods()?.get(mod_id)?.get(field)
    }

    /// Set `mods[modId]` to `value` (in place if present, appended if new),
    /// the JS `this.userProfile.mods[modId] = value`.
    fn set_mod(&mut self, mod_id: &str, value: Value) {
        if let Some(mods) = self.mods_mut() {
            mods.insert(mod_id.to_owned(), value);
        }
    }

    /// A working copy of `mods[modId]`, or a fresh empty object - the JS
    /// `const mod = this.userProfile.mods[modId] || {}`.
    fn mod_or_new(&self, mod_id: &str) -> Value {
        self.mods()
            .and_then(|mods| mods.get(mod_id))
            .cloned()
            .unwrap_or_else(empty_object)
    }

    /// Apply `f` to the object body of `mods[modId]`, then write it back - the
    /// get-or-new -> `as_object_mut` -> write-back shape the per-mod mutators
    /// share. Generic over the closure's return with a `Default` bound so it
    /// carries both a `()` mutator and `update_mod_details`'s `bool` "changed"
    /// flag. When `mods[modId]` holds a NON-object `Value` (reachable -
    /// `Profile::parse` preserves arbitrary JSON under `mods`), the closure is
    /// SKIPPED and `R::default()` returned, but the entry is STILL written back
    /// unchanged (mirroring the old mutators, which called `set_mod` OUTSIDE
    /// the `if let Some(obj)`, so a non-object entry round-trips untouched).
    fn modify_mod<R: Default>(
        &mut self,
        mod_id: &str,
        f: impl FnOnce(&mut Map<String, Value>) -> R,
    ) -> R {
        let mut m = self.mod_or_new(mod_id);
        let result = match m.as_object_mut() {
            Some(obj) => f(obj),
            None => R::default(),
        };
        self.set_mod(mod_id, m);
        result
    }

    // --- reads ---

    pub fn app_latest_version(&self) -> Option<&str> {
        self.root.get("app")?.get("latestVersion")?.as_str()
    }

    pub fn app_latest_version_bleeding_edge(&self) -> Option<&str> {
        self.root
            .get("app")?
            .get("latestVersionBleedingEdge")?
            .as_str()
    }

    /// The cached latest version on the pre-release channel (`latestVersionPreRelease`).
    /// Consumed only by a running pre-release build's update check, which folds
    /// it into the stable and bleeding-edge caches.
    pub fn app_latest_version_pre_release(&self) -> Option<&str> {
        self.root
            .get("app")?
            .get("latestVersionPreRelease")?
            .as_str()
    }

    pub fn mod_rating(&self, mod_id: &str) -> Option<i64> {
        self.mod_field(mod_id, "rating")?.as_i64()
    }

    pub fn mod_latest_version(&self, mod_id: &str) -> Option<&str> {
        self.mod_field(mod_id, "latestVersion")?.as_str()
    }

    /// The mirrored `updatesDisabledForVersion`, absent when the mod's updates
    /// are offered normally (see
    /// [`set_mod_updates_disabled_for_version`](Self::set_mod_updates_disabled_for_version)).
    pub fn mod_updates_disabled_for_version(&self, mod_id: &str) -> Option<&str> {
        self.mod_field(mod_id, "updatesDisabledForVersion")?
            .as_str()
    }

    /// The review votes recorded for a mod, in file order. A pair that is not
    /// `[integer, integer]` is skipped rather than failing the read, so a
    /// hand-edited file loses that pair's meaning and nothing else.
    pub fn mod_review_votes(&self, mod_id: &str) -> Vec<ReviewVote> {
        self.mod_field(mod_id, "reviewVotes")
            .and_then(Value::as_array)
            .map(|pairs| pairs.iter().filter_map(ReviewVote::from_pair).collect())
            .unwrap_or_default()
    }

    // --- writes ---

    /// `setModVersion`: set the version (in place) and, by default, drop the
    /// cached `latestVersion`.
    pub fn set_mod_version(&mut self, mod_id: &str, version: &str, reset_latest_version: bool) {
        self.modify_mod(mod_id, |obj| {
            obj.insert("version".to_owned(), Value::String(version.to_owned()));
            if reset_latest_version {
                obj.shift_remove("latestVersion");
            }
        });
    }

    /// Remove a mod's cached `latestVersion` in place, so an absent mod gets no
    /// entry, returning whether there was one.
    pub fn remove_mod_latest_version(&mut self, mod_id: &str) -> bool {
        self.mods_mut()
            .and_then(|mods| mods.get_mut(mod_id))
            .and_then(Value::as_object_mut)
            .is_some_and(|entry| entry.shift_remove("latestVersion").is_some())
    }

    /// `setModDisabled`: set `disabled` to `true`, or delete it when enabling.
    pub fn set_mod_disabled(&mut self, mod_id: &str, disabled: bool) {
        self.modify_mod(mod_id, |obj| {
            if disabled {
                obj.insert("disabled".to_owned(), Value::Bool(true));
            } else {
                obj.shift_remove("disabled");
            }
        });
    }

    /// `setModRating`: store a nonzero rating, or clear the entry (the JS
    /// `if (rating)` is a nonzero test).
    pub fn set_mod_rating(&mut self, mod_id: &str, rating: i64) {
        self.modify_mod(mod_id, |obj| {
            if rating != 0 {
                obj.insert("rating".to_owned(), Value::Number(rating.into()));
            } else {
                obj.shift_remove("rating");
            }
        });
    }

    /// Record an upvote on `review_id`, cast at `now_seconds`, by appending
    /// the pair to the mod's `reviewVotes` (created, like the entry, when
    /// absent). Returns whether anything was written: a vote is one way, so
    /// an id already present is left as it is. A `reviewVotes` that is not
    /// an array holds no votes and is replaced.
    pub fn add_mod_review_vote(&mut self, mod_id: &str, review_id: i64, now_seconds: i64) -> bool {
        self.modify_mod(mod_id, |obj| {
            if !obj.get("reviewVotes").is_some_and(Value::is_array) {
                obj.insert("reviewVotes".to_owned(), Value::Array(Vec::new()));
            }
            let Some(votes) = obj.get_mut("reviewVotes").and_then(Value::as_array_mut) else {
                return false;
            };
            if votes
                .iter()
                .filter_map(ReviewVote::from_pair)
                .any(|vote| vote.review_id == review_id)
            {
                return false;
            }
            votes.push(
                ReviewVote {
                    timestamp: now_seconds,
                    review_id,
                }
                .to_pair(),
            );
            true
        })
    }

    /// Take back the vote on `review_id`: its pair is removed from the mod's
    /// `reviewVotes`, a `reviewVotes` left empty is dropped from the entry,
    /// and an entry left `{}` is dropped from `mods`, so the document reads as
    /// it did before the vote. Returns whether anything was written: an id no
    /// `[integer, integer]` pair carries changes nothing, so a malformed pair
    /// stays where it is.
    pub fn retract_mod_review_vote(&mut self, mod_id: &str, review_id: i64) -> bool {
        let Some(mods) = self.mods_mut() else {
            return false;
        };
        let Some(entry) = mods.get_mut(mod_id).and_then(Value::as_object_mut) else {
            return false;
        };
        let Some(votes) = entry.get_mut("reviewVotes").and_then(Value::as_array_mut) else {
            return false;
        };
        let count = votes.len();
        votes.retain(|pair| {
            ReviewVote::from_pair(pair).is_none_or(|vote| vote.review_id != review_id)
        });
        if votes.len() == count {
            return false;
        }
        if votes.is_empty() {
            entry.shift_remove("reviewVotes");
            if entry.is_empty() {
                mods.shift_remove(mod_id);
            }
        }
        true
    }

    /// `deleteMod`: drop the entry, keeping only its [`PROFILE_KEEP_ON_REMOVE`]
    /// fields, in place and in their existing order; an entry with none of
    /// them is removed.
    pub fn delete_mod(&mut self, mod_id: &str) {
        let Some(mods) = self.mods_mut() else {
            return;
        };
        let kept = mods
            .get_mut(mod_id)
            .and_then(Value::as_object_mut)
            .is_some_and(|entry| {
                entry.retain(|key, _| PROFILE_KEEP_ON_REMOVE.contains(&key.as_str()));
                !entry.is_empty()
            });
        if !kept {
            mods.shift_remove(mod_id);
        }
    }

    /// A mod counts as deleted if it is absent or carries only
    /// [`PROFILE_KEEP_ON_REMOVE`] fields (`isModDeleted`). An empty entry is
    /// not deleted, so the reconciliation drops it rather than keeping it.
    fn is_mod_deleted(&self, mod_id: &str) -> bool {
        match self.mods().and_then(|mods| mods.get(mod_id)) {
            None => true,
            Some(value) => value.as_object().is_some_and(|m| {
                !m.is_empty()
                    && m.keys()
                        .all(|key| PROFILE_KEEP_ON_REMOVE.contains(&key.as_str()))
            }),
        }
    }

    /// Mirror the mod config's `updatesDisabledForVersion` into the profile,
    /// returning whether the stored value changed (so a caller can skip a
    /// profile write that would change nothing). An empty value removes the
    /// key, keeping a mod whose updates are offered normally free of it.
    ///
    /// The config tree remains authoritative; this is a copy for a reader that
    /// has the profile but not the settings backend. Every writer of the config
    /// field calls this, since the copy is only as good as its staleness.
    pub fn set_mod_updates_disabled_for_version(&mut self, mod_id: &str, stored: &str) -> bool {
        self.modify_mod(mod_id, |obj| {
            let current = obj
                .get("updatesDisabledForVersion")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if current == stored {
                return false;
            }
            if stored.is_empty() {
                obj.shift_remove("updatesDisabledForVersion");
            } else {
                obj.insert(
                    "updatesDisabledForVersion".to_owned(),
                    Value::String(stored.to_owned()),
                );
            }
            true
        })
    }

    /// `updateModDetails`: reconcile a mod's stored `version`/`disabled` to the
    /// installed state, returning whether anything changed.
    pub fn update_mod_details(&mut self, mod_id: &str, version: &str, disabled: bool) -> bool {
        self.modify_mod(mod_id, |obj| {
            let mut updated = false;
            if obj.get("version").and_then(Value::as_str) != Some(version) {
                obj.insert("version".to_owned(), Value::String(version.to_owned()));
                updated = true;
            }
            let current_disabled = obj
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if current_disabled != disabled {
                obj.insert("disabled".to_owned(), Value::Bool(disabled));
                updated = true;
            }
            updated
        })
    }

    /// `cleanupRemovedMods`: drop profile mods not in `current_mod_ids` (unless
    /// already deleted), returning whether anything changed.
    pub fn cleanup_removed_mods(&mut self, current_mod_ids: &HashSet<String>) -> bool {
        let mut updated = false;
        let ids: Vec<String> = self
            .mods()
            .map(|mods| mods.keys().cloned().collect())
            .unwrap_or_default();
        for mod_id in ids {
            if !current_mod_ids.contains(&mod_id) && !self.is_mod_deleted(&mod_id) {
                self.delete_mod(&mod_id);
                updated = true;
            }
        }
        updated
    }

    /// `updateLatestVersions`: record the catalog's latest app and per-mod
    /// versions (only for existing, non-deleted mods), returning whether
    /// anything changed. Empty/absent versions are skipped (the JS truthiness
    /// test).
    ///
    /// A mod the catalog flags has its cached `latestVersion` removed rather
    /// than recorded, so no flagged version is offered, and a cached version is
    /// always one that was seen unflagged - which is what lets an install of it
    /// restore the mod. The daily update check does the same.
    pub fn update_latest_versions(
        &mut self,
        app_latest_version: Option<&str>,
        app_latest_version_bleeding_edge: Option<&str>,
        app_latest_version_pre_release: Option<&str>,
        mod_latest_versions: &[(String, String)],
        flagged_mods: &[String],
    ) -> bool {
        let mut updated = false;

        if let Some(v) = app_latest_version.filter(|s| !s.is_empty())
            && self.app_latest_version() != Some(v)
        {
            self.set_app_field("latestVersion", v);
            updated = true;
        }

        if let Some(v) = app_latest_version_bleeding_edge.filter(|s| !s.is_empty())
            && self.app_latest_version_bleeding_edge() != Some(v)
        {
            self.set_app_field("latestVersionBleedingEdge", v);
            updated = true;
        }

        if let Some(v) = app_latest_version_pre_release.filter(|s| !s.is_empty())
            && self.app_latest_version_pre_release() != Some(v)
        {
            self.set_app_field("latestVersionPreRelease", v);
            updated = true;
        }

        for (mod_id, latest_version) in mod_latest_versions {
            if self.is_mod_deleted(mod_id) {
                continue;
            }
            // Only an existing mod is updated (the JS `const mod = mods[modId];
            // if (mod && ...)`); a catalog mod not installed is not created.
            if self.mods().is_some_and(|mods| mods.contains_key(mod_id))
                && self.mod_latest_version(mod_id) != Some(latest_version.as_str())
            {
                self.modify_mod(mod_id, |obj| {
                    obj.insert(
                        "latestVersion".to_owned(),
                        Value::String(latest_version.clone()),
                    );
                });
                updated = true;
            }
        }

        for mod_id in flagged_mods {
            updated |= self.remove_mod_latest_version(mod_id);
        }

        updated
    }

    fn set_app_field(&mut self, field: &str, value: &str) {
        if let Some(app) = self.root.get_mut("app").and_then(Value::as_object_mut) {
            app.insert(field.to_owned(), Value::String(value.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_profile_has_app_and_mods() {
        let p = Profile::parse(None);
        assert_eq!(p.to_pretty(), "{\n  \"app\": {},\n  \"mods\": {}\n}");
    }

    #[test]
    fn set_mod_version_preserves_order_and_unknown_fields() {
        // A recorded byte-format golden: setModVersion overwrites version in
        // place, drops latestVersion, preserves everything else byte for byte -
        // including the input order `id, customTopLevel, app, mods`, which a
        // typed struct would not keep.
        let seeded = "{\n  \"id\": \"fixture-profile-id\",\n  \"customTopLevel\": {\n    \"nested\": true\n  },\n  \"app\": {\n    \"version\": \"1.7.0\",\n    \"latestVersion\": \"1.8.0\"\n  },\n  \"mods\": {\n    \"test-mod\": {\n      \"version\": \"1.0\",\n      \"latestVersion\": \"1.1\",\n      \"customPerMod\": \"preserved\"\n    }\n  }\n}";
        let mut p = Profile::parse(Some(seeded));
        p.set_mod_version("test-mod", "2.0", true);
        let expected = "{\n  \"id\": \"fixture-profile-id\",\n  \"customTopLevel\": {\n    \"nested\": true\n  },\n  \"app\": {\n    \"version\": \"1.7.0\",\n    \"latestVersion\": \"1.8.0\"\n  },\n  \"mods\": {\n    \"test-mod\": {\n      \"version\": \"2.0\",\n      \"customPerMod\": \"preserved\"\n    }\n  }\n}";
        assert_eq!(p.to_pretty(), expected);
    }

    #[test]
    fn pretty_output_matches_json_stringify_indent_2() {
        // Empty containers ({} and []), escaping (\n), and a nested object must
        // match JSON.stringify(x, null, 2) exactly. The input already has
        // app/mods so parse adds nothing and the bytes round-trip identically.
        let src = "{\n  \"app\": {},\n  \"mods\": {},\n  \"note\": \"x\\ny\",\n  \"list\": [],\n  \"nested\": {\n    \"k\": 1\n  }\n}";
        let p = Profile::parse(Some(src));
        assert_eq!(p.to_pretty(), src);
    }

    #[test]
    fn set_mod_updates_disabled_for_version_stores_clears_and_reports_change() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"version\": \"1.0\"\n    }\n  }\n}",
        ));

        assert!(p.set_mod_updates_disabled_for_version("m", "=1.1"));
        assert_eq!(p.mod_updates_disabled_for_version("m"), Some("=1.1"));

        // A write of the value already stored changes nothing and says so, which
        // is what keeps a reconciliation pass from marking the profile dirty.
        assert!(!p.set_mod_updates_disabled_for_version("m", "=1.1"));

        // Empty removes the key rather than storing "", so a mod whose updates
        // are offered normally carries no trace of the setting.
        assert!(p.set_mod_updates_disabled_for_version("m", ""));
        assert_eq!(p.mod_updates_disabled_for_version("m"), None);
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"version\": \"1.0\"\n    }\n  }\n}"
        );
        assert!(!p.set_mod_updates_disabled_for_version("m", ""));
    }

    #[test]
    fn set_mod_rating_clears_on_zero() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"rating\": 4\n    }\n  }\n}",
        ));
        p.set_mod_rating("m", 5);
        assert_eq!(p.mod_rating("m"), Some(5));
        p.set_mod_rating("m", 0);
        assert_eq!(p.mod_rating("m"), None);
    }

    #[test]
    fn delete_mod_keeps_rating_only_without_reordering() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"keep\": {\n      \"version\": \"1\"\n    },\n    \"m\": {\n      \"version\": \"1.0\",\n      \"rating\": 3\n    }\n  }\n}",
        ));
        p.delete_mod("m");
        // `keep` stays first, `m` reduced to rating-only in place (shift_remove,
        // not swap_remove, so order is preserved).
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"keep\": {\n      \"version\": \"1\"\n    },\n    \"m\": {\n      \"rating\": 3\n    }\n  }\n}"
        );
        assert!(p.is_mod_deleted("m"));
    }

    #[test]
    fn delete_mod_keeps_the_keep_set_in_place() {
        // A removed mod keeps its rating AND its votes, in the order the entry
        // had them, with everything else shifted out.
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"version\": \"1.0\",\n      \"rating\": 3,\n      \"latestVersion\": \"1.1\",\n      \"reviewVotes\": [\n        [\n          1757900000,\n          112\n        ]\n      ]\n    }\n  }\n}",
        ));
        p.delete_mod("m");
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"rating\": 3,\n      \"reviewVotes\": [\n        [\n          1757900000,\n          112\n        ]\n      ]\n    }\n  }\n}"
        );
        assert!(p.is_mod_deleted("m"));

        // Votes alone are kept too, and an entry with no keep-set field goes.
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{"voted":{"version":"1.0","reviewVotes":[[1757900000,112]]},"plain":{"version":"1.0"}}}"#,
        ));
        p.delete_mod("voted");
        p.delete_mod("plain");
        assert_eq!(
            p.mod_review_votes("voted"),
            vec![ReviewVote {
                timestamp: 1757900000,
                review_id: 112
            }]
        );
        assert_eq!(
            p.mods().unwrap()["voted"],
            json!({"reviewVotes": [[1757900000, 112]]})
        );
        assert!(!p.mods().unwrap().contains_key("plain"));
    }

    #[test]
    fn is_mod_deleted_is_every_key_in_the_keep_set() {
        let p = Profile::parse(Some(
            r#"{"app":{},"mods":{
                "rated":{"rating":4},
                "voted":{"reviewVotes":[[1757900000,112]]},
                "both":{"rating":4,"reviewVotes":[[1757900000,112]]},
                "stamped":{"reviewVotes":[[1757900000,112]],"latestVersion":"1.1"},
                "empty":{}
            }}"#,
        ));
        assert!(p.is_mod_deleted("absent"));
        assert!(p.is_mod_deleted("rated"));
        assert!(p.is_mod_deleted("voted"));
        assert!(p.is_mod_deleted("both"));
        // Anything beyond the keep-set is an entry the reconciliation reduces.
        assert!(!p.is_mod_deleted("stamped"));
        assert!(!p.is_mod_deleted("empty"));
    }

    #[test]
    fn cleanup_leaves_a_votes_only_entry_and_keeps_votes_of_a_removed_mod() {
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{
                "keep":{"version":"1"},
                "voted-only":{"reviewVotes":[[1757900000,112]]},
                "gone":{"version":"1.0","reviewVotes":[[1758000000,222]]}
            }}"#,
        ));
        let current: HashSet<String> = ["keep".to_owned()].into_iter().collect();
        assert!(p.cleanup_removed_mods(&current));
        // The votes-only entry is already deleted-by-definition, so the pass
        // leaves it exactly as it was.
        assert_eq!(
            p.mods().unwrap()["voted-only"],
            json!({"reviewVotes": [[1757900000, 112]]})
        );
        // The removed mod's entry is reduced to its votes, not dropped.
        assert_eq!(
            p.mods().unwrap()["gone"],
            json!({"reviewVotes": [[1758000000, 222]]})
        );

        // A second pass has nothing left to change.
        assert!(!p.cleanup_removed_mods(&current));
    }

    #[test]
    fn add_mod_review_vote_appends_once_per_id() {
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{"m":{"version":"1.0","reviewVotes":[[1757900000,112]]}}}"#,
        ));
        assert!(p.add_mod_review_vote("m", 222, 1758000000));
        assert_eq!(
            p.mod_review_votes("m"),
            vec![
                ReviewVote {
                    timestamp: 1757900000,
                    review_id: 112
                },
                ReviewVote {
                    timestamp: 1758000000,
                    review_id: 222
                },
            ]
        );

        // A repeat writes nothing: the first vote's timestamp stands.
        let before = p.to_pretty();
        assert!(!p.add_mod_review_vote("m", 112, 1759000000));
        assert_eq!(p.to_pretty(), before);
    }

    #[test]
    fn review_votes_skip_a_malformed_pair_on_read_and_keep_it_on_write() {
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{"m":{"reviewVotes":[[1757900000,112],"junk",[1,2,3],[1758000000,"x"],[1759000000,333]]}}}"#,
        ));
        assert_eq!(
            p.mod_review_votes("m"),
            vec![
                ReviewVote {
                    timestamp: 1757900000,
                    review_id: 112
                },
                ReviewVote {
                    timestamp: 1759000000,
                    review_id: 333
                },
            ]
        );

        // The write appends after the malformed pairs and leaves them in place.
        assert!(p.add_mod_review_vote("m", 444, 1760000000));
        assert_eq!(
            p.mods().unwrap()["m"]["reviewVotes"],
            json!([
                [1757900000, 112],
                "junk",
                [1, 2, 3],
                [1758000000, "x"],
                [1759000000, 333],
                [1760000000, 444]
            ])
        );

        // A `reviewVotes` that is not an array holds no votes and is replaced.
        let mut p = Profile::parse(Some(r#"{"app":{},"mods":{"m":{"reviewVotes":"junk"}}}"#));
        assert!(p.mod_review_votes("m").is_empty());
        assert!(p.add_mod_review_vote("m", 1, 1757900000));
        assert_eq!(
            p.mods().unwrap()["m"]["reviewVotes"],
            json!([[1757900000, 1]])
        );
    }

    #[test]
    fn first_vote_serializes_the_pair_pretty_printed() {
        // The pair takes the JSON.stringify(x, null, 2) layout of the rest of
        // the profile: each number on its own line. Appended after the entry's
        // existing fields; a fresh entry holds only the votes.
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"rated\": {\n      \"rating\": 4\n    },\n    \"installed\": {\n      \"latestVersion\": \"1.2\",\n      \"version\": \"1.2\"\n    }\n  }\n}",
        ));
        assert!(p.add_mod_review_vote("rated", 112, 1757900000));
        assert!(p.add_mod_review_vote("installed", 112, 1757900000));
        assert!(p.add_mod_review_vote("absent", 112, 1757900000));
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"rated\": {\n      \"rating\": 4,\n      \"reviewVotes\": [\n        [\n          1757900000,\n          112\n        ]\n      ]\n    },\n    \"installed\": {\n      \"latestVersion\": \"1.2\",\n      \"version\": \"1.2\",\n      \"reviewVotes\": [\n        [\n          1757900000,\n          112\n        ]\n      ]\n    },\n    \"absent\": {\n      \"reviewVotes\": [\n        [\n          1757900000,\n          112\n        ]\n      ]\n    }\n  }\n}"
        );
    }

    #[test]
    fn retract_mod_review_vote_restores_the_bytes_before_the_vote() {
        // A vote then its retraction leaves the document byte for byte as it
        // was: the emptied field goes from the entry, and the entry the vote
        // created goes from `mods`.
        let seeded = "{\n  \"app\": {},\n  \"mods\": {\n    \"rated\": {\n      \"rating\": 4\n    },\n    \"installed\": {\n      \"latestVersion\": \"1.2\",\n      \"version\": \"1.2\"\n    }\n  }\n}";
        let mut p = Profile::parse(Some(seeded));
        assert!(p.add_mod_review_vote("rated", 112, 1757900000));
        assert!(p.add_mod_review_vote("installed", 112, 1757900000));
        assert!(p.add_mod_review_vote("absent", 112, 1757900000));
        assert_ne!(p.to_pretty(), seeded);

        assert!(p.retract_mod_review_vote("rated", 112));
        assert!(p.retract_mod_review_vote("installed", 112));
        assert!(p.retract_mod_review_vote("absent", 112));
        assert_eq!(p.to_pretty(), seeded);
        assert!(!p.mods().unwrap().contains_key("absent"));

        // With another vote recorded, the field stays and keeps its order.
        assert!(p.add_mod_review_vote("installed", 112, 1757900000));
        assert!(p.add_mod_review_vote("installed", 222, 1758000000));
        assert!(p.retract_mod_review_vote("installed", 112));
        assert_eq!(
            p.mod_review_votes("installed"),
            vec![ReviewVote {
                timestamp: 1758000000,
                review_id: 222
            }]
        );
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"rated\": {\n      \"rating\": 4\n    },\n    \"installed\": {\n      \"latestVersion\": \"1.2\",\n      \"version\": \"1.2\",\n      \"reviewVotes\": [\n        [\n          1758000000,\n          222\n        ]\n      ]\n    }\n  }\n}"
        );
    }

    #[test]
    fn retract_mod_review_vote_of_an_unrecorded_id_writes_nothing() {
        // An id not in the list, an entry without the field (or with one that
        // is not an array), and a mod with no entry: false, and the bytes are
        // as they were - in particular no entry is created for the absent mod.
        let seeded = r#"{"app":{},"mods":{"voted":{"version":"1.0","reviewVotes":[[1757900000,112]]},"plain":{"version":"1.0"},"junk":{"reviewVotes":"junk"}}}"#;
        let mut p = Profile::parse(Some(seeded));
        let before = p.to_pretty();
        assert!(!p.retract_mod_review_vote("voted", 222));
        assert!(!p.retract_mod_review_vote("plain", 112));
        assert!(!p.retract_mod_review_vote("junk", 112));
        assert!(!p.retract_mod_review_vote("absent", 112));
        assert_eq!(p.to_pretty(), before);
        assert!(!p.mods().unwrap().contains_key("absent"));
    }

    #[test]
    fn retract_mod_review_vote_leaves_a_malformed_pair_in_place() {
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{"m":{"reviewVotes":[[1757900000,112],"junk",[1,2,3],[1758000000,"x"],[1759000000,333]]}}}"#,
        ));
        // A malformed pair is never matched, whatever numbers it holds.
        assert!(!p.retract_mod_review_vote("m", 2));
        assert!(!p.retract_mod_review_vote("m", 3));

        assert!(p.retract_mod_review_vote("m", 112));
        assert!(p.retract_mod_review_vote("m", 333));
        assert_eq!(p.mod_review_votes("m"), Vec::new());
        // The field is not empty, so it and the entry stay.
        assert_eq!(
            p.mods().unwrap()["m"]["reviewVotes"],
            json!(["junk", [1, 2, 3], [1758000000, "x"]])
        );
    }

    #[test]
    fn retraction_leaves_the_keep_set_rules_as_they_are() {
        // What a retraction leaves behind is read by the existing rules: a
        // rating-only entry is deleted, a versioned one is installed, and the
        // entry the vote alone created is gone.
        let mut p = Profile::parse(Some(
            r#"{"app":{},"mods":{
                "rated":{"rating":4,"reviewVotes":[[1757900000,112]]},
                "installed":{"version":"1.0","reviewVotes":[[1757900000,112]]},
                "voted":{"reviewVotes":[[1757900000,112]]}
            }}"#,
        ));
        for mod_id in ["rated", "installed", "voted"] {
            assert!(p.retract_mod_review_vote(mod_id, 112));
        }
        assert!(p.is_mod_deleted("rated"));
        assert!(!p.is_mod_deleted("installed"));
        assert!(p.is_mod_deleted("voted"));
        assert!(!p.mods().unwrap().contains_key("voted"));

        // The reconciliation then has only the installed entry to reduce, and
        // nothing of it to keep.
        assert!(p.cleanup_removed_mods(&HashSet::new()));
        assert!(!p.mods().unwrap().contains_key("installed"));
        assert_eq!(p.mods().unwrap()["rated"], json!({"rating": 4}));
        assert!(!p.cleanup_removed_mods(&HashSet::new()));
    }

    #[test]
    fn cleanup_removes_uninstalled_keeps_installed_and_rated() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"keep\": {\n      \"version\": \"1\"\n    },\n    \"rated\": {\n      \"version\": \"1\",\n      \"rating\": 2\n    },\n    \"gone\": {\n      \"version\": \"1\"\n    }\n  }\n}",
        ));
        let current: HashSet<String> = ["keep".to_owned()].into_iter().collect();
        assert!(p.cleanup_removed_mods(&current));
        assert!(p.mods().unwrap().contains_key("keep"));
        assert_eq!(p.mod_rating("rated"), Some(2));
        assert!(!p.mods().unwrap().contains_key("gone"));
    }

    #[test]
    fn update_latest_versions_only_touches_existing_non_deleted() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"version\": \"1.0\",\n      \"rating\": 4\n    }\n  }\n}",
        ));
        let changed = p.update_latest_versions(
            Some("1.8.0"),
            Some("1.9.0"),
            Some("2.0.0-alpha.1"),
            &[
                ("m".to_owned(), "2.0".to_owned()),
                ("not-installed".to_owned(), "9.0".to_owned()),
            ],
            &[],
        );
        assert!(changed);
        assert_eq!(p.app_latest_version(), Some("1.8.0"));
        assert_eq!(p.app_latest_version_bleeding_edge(), Some("1.9.0"));
        assert_eq!(p.app_latest_version_pre_release(), Some("2.0.0-alpha.1"));
        assert_eq!(p.mod_latest_version("m"), Some("2.0"));
        assert!(!p.mods().unwrap().contains_key("not-installed"));
    }

    #[test]
    fn update_latest_versions_removes_a_flagged_mods_cached_version_in_place() {
        let seeded = "{\n  \"app\": {},\n  \"mods\": {\n    \"flagged\": {\n      \"version\": \"1.0\",\n      \"latestVersion\": \"1.1\",\n      \"rating\": 4\n    },\n    \"clean\": {\n      \"version\": \"1.0\"\n    }\n  }\n}";
        let mut p = Profile::parse(Some(seeded));
        assert!(p.update_latest_versions(
            None,
            None,
            None,
            &[("clean".to_owned(), "1.2".to_owned())],
            &["flagged".to_owned(), "not-installed".to_owned()],
        ));
        // Only the cached version goes, and the entry keeps its place; a
        // flagged mod with no entry gets none.
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"flagged\": {\n      \"version\": \"1.0\",\n      \"rating\": 4\n    },\n    \"clean\": {\n      \"version\": \"1.0\",\n      \"latestVersion\": \"1.2\"\n    }\n  }\n}"
        );

        // With nothing cached for the flagged mod, nothing changes.
        assert!(!p.update_latest_versions(
            None,
            None,
            None,
            &[("clean".to_owned(), "1.2".to_owned())],
            &["flagged".to_owned()],
        ));
    }

    #[test]
    fn update_mod_details_reports_change() {
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": {\n      \"version\": \"0.9\"\n    }\n  }\n}",
        ));
        assert!(p.update_mod_details("m", "1.0", false));
        assert!(!p.update_mod_details("m", "1.0", false));
    }

    #[test]
    fn modify_mod_on_a_non_object_entry_skips_body_and_writes_back_unchanged() {
        // `Profile::parse` preserves arbitrary JSON under `mods`, so a mod entry
        // can be a non-object Value. modify_mod (the shared helper behind the
        // five wrapped mutators) then skips the closure, returns R::default(),
        // and still writes the entry back byte-identical - exercised here via
        // update_mod_details, whose bool return is `false` on the skip.
        let mut p = Profile::parse(Some(
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": 42\n  }\n}",
        ));
        assert!(!p.update_mod_details("m", "1.0", false));
        assert_eq!(
            p.to_pretty(),
            "{\n  \"app\": {},\n  \"mods\": {\n    \"m\": 42\n  }\n}"
        );
    }
}
