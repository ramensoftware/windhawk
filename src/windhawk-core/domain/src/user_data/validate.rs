//! Semantic validation of a decoded archive; `inspectUserData` /
//! `importUserData` run it before acting. The structural type checks are already
//! enforced by deserialization (the top level into the archive struct, each
//! `config` into the seven-field struct), so this pass covers only the rules a
//! type cannot express: the format tag (which encodes the archive version),
//! per-mod identity (`modId`/`version`), a local mod's required embedded
//! source, the settings keys and value types, an object `appSettings`, and a
//! mod-count bound. Validation is pure, so `inspectUserData` doubles as an
//! "is this a valid archive" probe.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{ArchiveError, ArchiveMod, FORMAT_TAG, UserDataArchive};
use crate::mod_id::{ModId, Version};
use crate::settings::is_valid_flat_key;

/// A sanity cap on the mod count, rejecting a corrupt or hostile archive up
/// front. Far above any real install; a safety limit, not a semantic one.
const MAX_MODS: usize = 100_000;

/// Validate `archive`, returning `Ok(())` when it conforms to the format rules
/// and a descriptive [`ArchiveError`] otherwise.
pub fn validate(archive: &UserDataArchive) -> Result<(), ArchiveError> {
    // The format tag encodes the archive version. An unrecognized value - a
    // future format this build does not model, or a file that is not an archive
    // at all - is refused with a user-facing message rather than a raw decode
    // error.
    if archive.format != FORMAT_TAG {
        return Err(ArchiveError::new(
            "this file is not a supported Windhawk user-data file and might require a newer version of Windhawk",
        ));
    }
    if archive.mods.len() > MAX_MODS {
        return Err(ArchiveError::new(format!(
            "archive has too many mods ({}; the maximum is {MAX_MODS})",
            archive.mods.len()
        )));
    }
    if let Some(app_settings) = &archive.app_settings
        && !app_settings.is_object()
    {
        return Err(ArchiveError::new("appSettings must be a JSON object"));
    }
    // A mod id names one entry: a duplicate would make the import process the
    // same mod twice (the second silently winning) and skew the manifest.
    let mut seen = BTreeSet::new();
    for m in &archive.mods {
        if !seen.insert(m.mod_id.as_str()) {
            return Err(ArchiveError::new(format!(
                "mod {:?} appears more than once in the archive",
                m.mod_id
            )));
        }
        validate_mod(m)?;
    }
    Ok(())
}

fn validate_mod(m: &ArchiveMod) -> Result<(), ArchiveError> {
    if m.mod_id.is_empty() {
        return Err(ArchiveError::new("a mod entry has an empty modId"));
    }
    let is_local = ModId::str_is_local(&m.mod_id);
    // Once installed, the id names a source file, a storage directory, a config
    // key, and a profile entry, so an archive's id must obey the same charset a
    // source's `@id` does. Without this an id bearing `\`, `/`, `:`, or `..`
    // would write outside the mods namespace - the storage helpers interpolate
    // it verbatim and sanitize nothing.
    if !ModId::str_is_valid_bare(ModId::str_bare(&m.mod_id)) {
        return Err(ArchiveError::new(format!(
            "mod {:?}: modId must only contain the characters 0-9, a-z, and a hyphen (-)",
            m.mod_id
        )));
    }
    if m.version.is_empty() {
        return Err(ArchiveError::new(format!(
            "mod {:?} has an empty version",
            m.mod_id
        )));
    }
    // A repository mod's version names the source file an import fetches: it is
    // interpolated verbatim into the repository URL, which sanitizes nothing, so
    // a version carrying `/`, `%`, `?`, or `#` would steer that fetch to a path
    // of the archive's choosing rather than the mod's published source. A local
    // mod's version is never fetched by (its source is embedded) and is
    // author-chosen free text, so it stays unconstrained.
    if !is_local && !Version::str_is_valid(&m.version) {
        return Err(ArchiveError::new(format!(
            "mod {:?}: version must only contain the characters 0-9, a-z, A-Z, and . - _ +",
            m.mod_id
        )));
    }
    // A local mod's source lives nowhere but the archive, so it must be present
    // and non-empty. A repository mod may omit it (the reference-only default).
    if is_local && m.source.as_deref().unwrap_or("").is_empty() {
        return Err(ArchiveError::new(format!(
            "local mod {:?} is missing its embedded source",
            m.mod_id
        )));
    }
    if let Some(settings) = &m.settings {
        validate_settings(&m.mod_id, settings)?;
    }
    Ok(())
}

/// `settings` is a flat map of key to a string or a 32-bit integer - no
/// booleans (stored as `0`/`1`), no floats, no nested objects or arrays.
fn validate_settings(mod_id: &str, settings: &Value) -> Result<(), ArchiveError> {
    let Some(map) = settings.as_object() else {
        return Err(ArchiveError::new(format!(
            "mod {mod_id:?}: settings must be a JSON object"
        )));
    };
    for (key, value) in map {
        // Import writes each key into the settings store verbatim, so the key
        // must be the flat notation the flattener emits (`Scalar`,
        // `Group.child`, `List[0]`) - which is also the only shape an export can
        // produce, since export drops any key that does not resolve against the
        // mod's template. Without this an arbitrary key would reach the store as
        // a value name: in portable mode `[Settings]` shares one INI file with
        // the mod's `[Mod]` config, and a key carrying a line break or a leading
        // `[` writes extra lines - a section header of the archive's choosing
        // among them - into that file.
        if !is_valid_flat_key(key) {
            return Err(ArchiveError::new(format!(
                "mod {mod_id:?}: setting key {key:?} is not a valid settings key"
            )));
        }
        let ok = match value {
            Value::String(_) => true,
            Value::Number(n) => n.as_i64().is_some_and(|v| i32::try_from(v).is_ok()),
            _ => false,
        };
        if !ok {
            return Err(ArchiveError::new(format!(
                "mod {mod_id:?}: setting {key:?} must be a string or a 32-bit integer"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{ArchiveModConfig, FORMAT_TAG};
    use super::*;

    /// A minimal valid archive; tests mutate one aspect to exercise a rule.
    fn valid() -> UserDataArchive {
        UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: None,
            mods: vec![ArchiveMod {
                mod_id: "taskbar-clock".to_owned(),
                version: "1.0".to_owned(),
                name: None,
                source: None,
                settings: None,
                config: None,
            }],
        }
    }

    fn local_mod() -> ArchiveMod {
        ArchiveMod {
            mod_id: "local@my-mod".to_owned(),
            version: "1.0".to_owned(),
            name: None,
            source: Some("// ==WindhawkMod==\n".to_owned()),
            settings: None,
            config: None,
        }
    }

    #[test]
    fn a_minimal_valid_archive_passes() {
        assert!(validate(&valid()).is_ok());
    }

    #[test]
    fn a_local_mod_with_embedded_source_passes() {
        let mut a = valid();
        a.mods = vec![local_mod()];
        assert!(validate(&a).is_ok());
    }

    #[test]
    fn wrong_format_tag_is_rejected() {
        // An unrecognized format tag - a future version, or a non-archive file -
        // is refused with the user-facing "newer version" message.
        let mut a = valid();
        a.format = "windhawk-user-data-v2".to_owned();
        let err = validate(&a).unwrap_err().to_string();
        assert!(
            err.contains("not a supported Windhawk user-data file"),
            "{err}"
        );
        assert!(err.contains("newer version of Windhawk"), "{err}");

        // An empty (missing) tag is rejected the same way.
        let mut a = valid();
        a.format = String::new();
        assert!(validate(&a).is_err());
    }

    #[test]
    fn an_empty_mod_id_is_rejected() {
        let mut a = valid();
        a.mods[0].mod_id = String::new();
        assert!(validate(&a).unwrap_err().to_string().contains("modId"));
    }

    #[test]
    fn a_mod_id_outside_the_id_charset_is_rejected() {
        // The id is interpolated verbatim into paths and registry keys, so a
        // traversal or separator must not survive validation.
        for id in [
            "..\\..\\evil",
            "../../evil",
            "C:\\evil",
            "has space",
            "Uppercase",
            "under_score",
        ] {
            let mut a = valid();
            a.mods[0].mod_id = id.to_owned();
            let err = validate(&a).unwrap_err().to_string();
            assert!(err.contains("modId must only contain"), "{id}: {err}");
        }

        // The same rule applies past the `local@` prefix.
        let mut a = valid();
        a.mods = vec![ArchiveMod {
            mod_id: "local@..\\evil".to_owned(),
            ..local_mod()
        }];
        let err = validate(&a).unwrap_err().to_string();
        assert!(err.contains("modId must only contain"), "{err}");

        // A bare `local@` has no id after the prefix.
        let mut a = valid();
        a.mods = vec![ArchiveMod {
            mod_id: "local@".to_owned(),
            ..local_mod()
        }];
        assert!(validate(&a).is_err());
    }

    #[test]
    fn a_well_formed_id_passes_in_both_forms() {
        let mut a = valid();
        a.mods[0].mod_id = "taskbar-clock-2".to_owned();
        assert!(validate(&a).is_ok());

        let mut a = valid();
        a.mods = vec![ArchiveMod {
            mod_id: "local@my-mod-2".to_owned(),
            ..local_mod()
        }];
        assert!(validate(&a).is_ok());
    }

    #[test]
    fn an_empty_version_is_rejected() {
        let mut a = valid();
        a.mods[0].version = String::new();
        assert!(validate(&a).unwrap_err().to_string().contains("version"));
    }

    #[test]
    fn a_repository_version_outside_the_charset_is_rejected() {
        // The version is interpolated verbatim into the fetch URL
        // (`<mods>/<modId>/<version>.wh.cpp`), so a separator, an escape, or a
        // query/fragment start must not survive validation.
        for version in [
            "../../evil",
            "..\\evil",
            "1.0/../../evil",
            "%2e%2e%2fevil",
            "?x=",
            "#frag",
            "1.0 beta",
            "1.0\r\n",
        ] {
            let mut a = valid();
            a.mods[0].version = version.to_owned();
            let err = validate(&a).unwrap_err().to_string();
            assert!(
                err.contains("version must only contain"),
                "{version:?}: {err}"
            );
        }

        // The shapes a published version really takes pass.
        for version in ["1.0", "1.2.3", "2024.01", "1.0.0-beta.1+build_5"] {
            let mut a = valid();
            a.mods[0].version = version.to_owned();
            assert!(validate(&a).is_ok(), "{version:?} must be accepted");
        }

        // A local mod's version is never fetched by, so it stays free text.
        let mut a = valid();
        a.mods = vec![ArchiveMod {
            version: "1.0 (dev build)".to_owned(),
            ..local_mod()
        }];
        assert!(validate(&a).is_ok());
    }

    #[test]
    fn a_local_mod_missing_source_is_rejected() {
        let mut a = valid();
        let mut m = local_mod();
        m.source = None;
        a.mods = vec![m];
        let err = validate(&a).unwrap_err().to_string();
        assert!(err.contains("missing its embedded source"), "{err}");

        // An empty (whitespace-free) source is treated as missing too.
        let mut a = valid();
        let mut m = local_mod();
        m.source = Some(String::new());
        a.mods = vec![m];
        assert!(validate(&a).is_err());
    }

    #[test]
    fn settings_values_must_be_string_or_i32_integer() {
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({ "ok": 1, "also": "text" }));
        assert!(validate(&a).is_ok());

        // A boolean value is not allowed (a bool setting is stored as 0/1).
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({ "bad": true }));
        assert!(validate(&a).unwrap_err().to_string().contains("bad"));

        // A float is not allowed.
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({ "bad": 1.5 }));
        assert!(validate(&a).is_err());

        // An integer outside i32 range is not allowed.
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({ "bad": 3_000_000_000_i64 }));
        assert!(validate(&a).is_err());

        // A nested object is not allowed.
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({ "bad": { "nested": 1 } }));
        assert!(validate(&a).is_err());
    }

    #[test]
    fn settings_keys_must_be_flat_notation() {
        // The notation the flattener emits passes, including subscripts and
        // nesting.
        let mut a = valid();
        a.mods[0].settings = Some(serde_json::json!({
            "Scalar": 1,
            "group.inner": "x",
            "matrix[2].cell": "y",
        }));
        assert!(validate(&a).is_ok());

        // A key outside the notation is refused before it can reach the store.
        // In portable mode a mod's `[Settings]` shares one INI file with its
        // `[Mod]` config, and the value name is written verbatim, so a key
        // carrying a line break or a leading `[` would inject a section header
        // pointing `LibraryFileName` at a library of the archive's choosing.
        for key in [
            "a\r\n[Mod]\r\nLibraryFileName=evil.dll\r\nb",
            "[Mod]",
            "a=b",
            " padded",
        ] {
            let mut a = valid();
            a.mods[0].settings = Some(serde_json::json!({ key: 1 }));
            let err = validate(&a).unwrap_err().to_string();
            assert!(
                err.contains("is not a valid settings key"),
                "{key:?}: {err}"
            );
        }
    }

    #[test]
    fn a_duplicate_mod_id_is_rejected() {
        let mut a = valid();
        a.mods.push(a.mods[0].clone());
        let err = validate(&a).unwrap_err().to_string();
        assert!(err.contains("more than once"), "{err}");
    }

    #[test]
    fn app_settings_must_be_an_object() {
        let mut a = valid();
        a.app_settings = Some(serde_json::json!([1, 2, 3]));
        let err = validate(&a).unwrap_err().to_string();
        assert!(err.contains("appSettings must be a JSON object"), "{err}");
    }

    #[test]
    fn config_presence_does_not_affect_validity() {
        // The config struct is validated by decoding; validate does not re-check
        // its fields, so a present config is fine here.
        let mut a = valid();
        a.mods[0].config = Some(ArchiveModConfig::default());
        assert!(validate(&a).is_ok());
    }
}
