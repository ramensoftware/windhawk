//! The user-profile document model and reconciliation rules, a faithful port of
//! `services/userProfile.ts`.
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

    /// `deleteMod`: drop the entry, but keep a lone `rating` (so a removed
    /// mod's user rating survives a reinstall).
    pub fn delete_mod(&mut self, mod_id: &str) {
        let rating = self.mod_field(mod_id, "rating").cloned();
        match rating {
            Some(rating) => {
                let mut keep = Map::new();
                keep.insert("rating".to_owned(), rating);
                self.set_mod(mod_id, Value::Object(keep));
            }
            None => {
                if let Some(mods) = self.mods_mut() {
                    mods.shift_remove(mod_id);
                }
            }
        }
    }

    /// A mod counts as deleted if it is absent or carries only a `rating`
    /// (`isModDeleted`).
    fn is_mod_deleted(&self, mod_id: &str) -> bool {
        match self.mods().and_then(|mods| mods.get(mod_id)) {
            None => true,
            Some(value) => value
                .as_object()
                .is_some_and(|m| m.len() == 1 && m.contains_key("rating")),
        }
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
    pub fn update_latest_versions(
        &mut self,
        app_latest_version: Option<&str>,
        app_latest_version_bleeding_edge: Option<&str>,
        app_latest_version_pre_release: Option<&str>,
        mod_latest_versions: &[(String, String)],
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
        );
        assert!(changed);
        assert_eq!(p.app_latest_version(), Some("1.8.0"));
        assert_eq!(p.app_latest_version_bleeding_edge(), Some("1.9.0"));
        assert_eq!(p.app_latest_version_pre_release(), Some("2.0.0-alpha.1"));
        assert_eq!(p.mod_latest_version("m"), Some("2.0"));
        assert!(!p.mods().unwrap().contains_key("not-installed"));
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
