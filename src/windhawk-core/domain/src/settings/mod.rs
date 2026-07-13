//! Initial-settings extraction: the `==WindhawkModSettings==` YAML block,
//! validated against the same schema the TS implementation enforces with
//! jsonschema, then transformed with `$name`/`$description`/`$options`
//! language selection.
//!
//! Split into single-concern submodules, all driven by the
//! `extract_initial_settings_inner` orchestrator in this root:
//! - `extract`: the source-scan helpers (the mod `@id`/`@version` read that
//!   keys the workarounds; the block extraction itself is `crate::scan`).
//! - `workarounds`: the per-(mod id, version) pre-parse string fixups.
//! - `validate`: the schema validator.
//! - `transform`: the YAML -> typed-tree transformer.
//! - `flatten`: the engine name->value flattener.
//!
//! `scalar_key_to_string` and `parse_annotation_key` are the two helpers
//! genuinely shared between the validate and transform passes, so they live
//! here in the root (one home, not a copy per submodule).
//!
//! Validation and transformation run directly over the parsed YAML tree
//! (yaml-rust2 keeps mapping keys in insertion order), so annotation grouping
//! and language fallback see keys in the same order the JS object did. The two
//! passes are kept SEPARATE (not merged into one producing pass):
//! error-detection order is observable (the first-error message rides in
//! `parseModSource`'s result), and one pass would change which error fires
//! first. The full parse-don't-validate merge is the deferred stretch.
//!
//! Numbers are restricted to int32-ranged integers (see
//! `validate::validate_number`): Windhawk stores settings as 32-bit values -
//! the `SettingsBackend` `set_int` takes an `i32`, REG_DWORD in registry mode -
//! and has no floating-point storage, so a YAML float (`1.0`, `1.5`, `1e3`,
//! `.inf`) or an out-of-range integer is REJECTED with an error, where the TS
//! implementation accepted any js-yaml number and coerced it at the engine. The
//! corpus run found a few SHIPPED mods that do use such values (float defaults,
//! uint32 ARGB colors); rather than relax the rule globally,
//! `workarounds::apply_settings_workarounds` rewrites those exact (mod id,
//! version) blocks to the value the mod actually uses (e.g. `1.0` -> `1`), so a
//! future release of the mod is forced to author it cleanly. The general
//! yaml-rust2-vs-js-yaml quoted-scalar divergences it also found
//! (surrogate-pair `\u` escapes, multi-line double-quoted indentation) are
//! handled the same per-version way. These shims are for ALREADY-PUBLISHED
//! store versions only: the engine path gates them by mod origin
//! (`extract_initial_settings_for_engine`'s `apply_workarounds`), off for a
//! locally-authored mod (`local@` storage id) so its author sees the real
//! validation error, not a silent fixup of a shipped version's source.
//!
//! Other known divergences from the TS implementation, accepted and
//! re-examined by the corpus run:
//! - YAML syntax-error and schema-violation messages differ in wording
//!   (js-yaml/jsonschema diagnostics vs ours); the canonical
//!   "Failed to parse settings: not a valid YAML array" is preserved.
//!   Message wording is explicitly not a compatibility concern.
//! - Scalar resolution (yaml-rust2's YAML 1.2 core schema) matches
//!   js-yaml's for everything that occurs in practice: capitalized
//!   booleans (`TRUE`/`True` -> boolean, which several published mods
//!   rely on - js-yaml's bool type accepts the three-case spellings too),
//!   `yes`/`no`/`on`/`off` as strings, and decimal/hex/octal integers.
//!   The two differ only on YAML-1.1-style literals js-yaml still accepts
//!   (binary `0b...`, a number there but a string here, and capitalized
//!   `Null`/`NULL`), which do not appear in real settings and are not
//!   pursued.
//!
//! Duplicate mapping keys ARE rejected, matching js-yaml: yaml-rust2's loader
//! errors on them (the reason it is preferred over saphyr, which silently keeps
//! the last value).
//!
//! Duplicate settings IDS - two sibling items in one settings array sharing a
//! parameter key, so both flatten to the same engine name - are also rejected
//! (`validate::reject_duplicate_ids`). This is a STRICTER rule than the
//! reference, which silently collapses them last-write-wins (in the engine
//! store and the TS object); the ambiguous settings are invalid. The corpus run
//! found one shipped version that relied on it (scroll-window-opacity 1.0.3, a
//! malformed `modifierKey` dropdown), pinned in
//! `workarounds::apply_settings_workarounds` the same per-(mod id, version)
//! way; its engine flatten stays byte-identical.

use std::borrow::Cow;

use yaml_rust2::{Yaml, YamlLoader};

use crate::language::DEFAULT_LANGUAGE;
use crate::model::{EngineSettingValue, SettingItem, SettingsParseError};
use crate::scan::find_comment_block;

mod extract;
mod flatten;
mod transform;
mod validate;
mod workarounds;

/// `extractInitialSettings`: `Ok(None)` when the source has no settings
/// block, the parsed and language-resolved settings otherwise. Applies the
/// per-version compatibility workarounds (the display/preview parse, e.g.
/// `parseModSource`); the engine path gates them by mod origin, see
/// `extract_initial_settings_for_engine`.
pub fn extract_initial_settings(
    mod_source: &str,
    language: &str,
) -> Result<Option<Vec<SettingItem>>, SettingsParseError> {
    extract_initial_settings_inner(mod_source, language, true)
}

/// `extract_initial_settings`, with explicit control over whether the per-version
/// `workarounds::apply_settings_workarounds` shims run. The shims keep
/// ALREADY-PUBLISHED store versions parsing; a locally-authored mod (`local@`
/// storage id) skips them so the author sees the real validation error instead
/// of a silent fixup.
fn extract_initial_settings_inner(
    mod_source: &str,
    language: &str,
    apply_workarounds: bool,
) -> Result<Option<Vec<SettingItem>>, SettingsParseError> {
    let Some(block) = find_comment_block(mod_source, "WindhawkModSettings") else {
        return Ok(None);
    };

    // Apply any per-(mod id, version) compatibility fixup for shipped mods
    // whose settings YAML js-yaml accepts but yaml-rust2 rejects (the corpus
    // run), keyed on the source's own @id/@version - but only for
    // store-installed mods; a locally-authored mod is parsed as written.
    let normalized = if apply_workarounds {
        let (mod_id, mod_version) = extract::mod_id_and_version(mod_source);
        workarounds::apply_settings_workarounds(mod_id.as_ref(), mod_version.as_ref(), block)
    } else {
        Cow::Borrowed(block)
    };

    // A YAML syntax error, including a duplicate mapping key, fails here
    // (js-yaml's yaml.load throws on both).
    let docs = YamlLoader::load_from_str(&normalized)
        .map_err(|e| SettingsParseError::new(format!("Failed to parse settings: {e}")))?;
    let doc = match docs.len() {
        // js-yaml's load() returns undefined for an empty stream, which
        // then fails the array check below.
        0 => Yaml::Null,
        1 => docs.into_iter().next().unwrap_or(Yaml::BadValue),
        // js-yaml's load() message for multi-document input.
        _ => {
            return Err(SettingsParseError::new(
                "expected a single document in the stream, but found more",
            ));
        }
    };

    let Yaml::Array(items) = &doc else {
        return Err(SettingsParseError::new(
            "Failed to parse settings: not a valid YAML array",
        ));
    };

    validate::validate_settings_array(items)
        .map_err(|path| SettingsParseError::new(format!("Failed to parse settings: {path}")))?;

    transform::parse_settings(items, language).map(Some)
}

/// `extractInitialSettingsForEngine`: the same block parsed and validated as
/// `extract_initial_settings`, then FLATTENED into the engine's name->value
/// store form (the install flow's settings migration). `Ok(None)` when there is
/// no settings block. Keys are dotted/indexed paths (`group.inner`, `list[0]`,
/// `matrix[0][1].cell`), booleans become 0/1, in the source's declaration
/// order. Language is irrelevant here (the `$name`/`$description`/`$options`
/// annotations are dropped by the flattening), so it resolves with a fixed
/// language; the leaf values do not depend on it.
///
/// `apply_workarounds` is the mod-origin gate: `true` for a store-installed mod
/// (the per-version compatibility shims run, keeping shipped versions working),
/// `false` for a locally-authored mod (`local@`), whose settings are parsed as
/// written so the author sees the real error.
pub fn extract_initial_settings_for_engine(
    mod_source: &str,
    apply_workarounds: bool,
) -> Result<Option<Vec<(String, EngineSettingValue)>>, SettingsParseError> {
    let Some(items) =
        extract_initial_settings_inner(mod_source, DEFAULT_LANGUAGE, apply_workarounds)?
    else {
        return Ok(None);
    };
    let mut out = Vec::new();
    flatten::flatten_settings(&items, "", &mut out);
    Ok(Some(out))
}

/// Mapping keys are scalars; JS object keys are their string forms. The one
/// helper genuinely used by BOTH the validate and transform passes, so it lives
/// in the root.
fn scalar_key_to_string(key: &Yaml) -> String {
    match key {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".to_owned(),
        // Raw float text; only reached when checking a (nonsensical) float
        // key against the key-name patterns, which reject it.
        Yaml::Real(s) => s.clone(),
        // Complex keys do not occur in schema-valid documents.
        _ => String::new(),
    }
}

/// Split a `$base[:lang]` annotation key into its base name and optional
/// language tag; `None` when the key has no `$` prefix. The SHARED unit of the
/// annotation grammar: validate's `is_annotation_key` adds the lang-shape and
/// name-set checks on top, transform's `parse_item_annotated` groups by the
/// base - the `$`-prefix + `split_once(':')` split is the same in both, so it
/// has one implementation here.
fn parse_annotation_key(key: &str) -> Option<(&str, Option<&str>)> {
    let rest = key.strip_prefix('$')?;
    Some(match rest.split_once(':') {
        Some((base, lang)) => (base, Some(lang)),
        None => (rest, None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SettingValue;

    fn settings_src(yaml: &str) -> String {
        format!("// ==WindhawkModSettings==\n/*\n{yaml}\n*/\n// ==/WindhawkModSettings==\n")
    }

    #[test]
    fn absent_block_is_none() {
        assert_eq!(extract_initial_settings("// code", "en"), Ok(None));
    }

    #[test]
    fn parses_scalars_annotations_and_options() {
        let src = settings_src(
            "- opt: 1\n  $name: Option\n  $name:fr: Choix\n  $description: An option\n  $options:\n  - a: Label A\n  - b: Label B",
        );
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.key, "opt");
        assert_eq!(item.value, SettingValue::Number(1.into()));
        assert_eq!(item.name.as_deref(), Some("Option"));
        assert_eq!(item.description.as_deref(), Some("An option"));
        assert_eq!(
            item.options.as_deref(),
            Some(
                &[
                    ("a".to_owned(), "Label A".to_owned()),
                    ("b".to_owned(), "Label B".to_owned())
                ][..]
            )
        );

        let items = extract_initial_settings(&src, "fr").unwrap().unwrap();
        assert_eq!(items[0].name.as_deref(), Some("Choix"));
    }

    #[test]
    fn parses_nested_settings_and_arrays() {
        let src = settings_src(
            "- group:\n  - inner: true\n- list:\n  - 1\n  - 2\n- names:\n  - x\n  - y\n- matrix:\n  - - cell: a\n  - - cell: b",
        );
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        match &items[0].value {
            SettingValue::Settings(inner) => {
                assert_eq!(inner[0].key, "inner");
                assert_eq!(inner[0].value, SettingValue::Bool(true));
            }
            other => panic!("expected nested settings, got {other:?}"),
        }
        assert_eq!(
            items[1].value,
            SettingValue::NumberArray(vec![1.into(), 2.into()])
        );
        assert_eq!(
            items[2].value,
            SettingValue::StringArray(vec!["x".into(), "y".into()])
        );
        match &items[3].value {
            SettingValue::SettingsArray(arrays) => {
                assert_eq!(arrays.len(), 2);
                assert_eq!(arrays[0][0].key, "cell");
            }
            other => panic!("expected settings array, got {other:?}"),
        }
    }

    #[test]
    fn non_array_yaml_is_the_canonical_error() {
        let src = settings_src("just a string");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: not a valid YAML array"
        );
    }

    #[test]
    fn schema_violations_are_reported() {
        // Empty array.
        let src = settings_src("[]");
        assert!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string()
                .starts_with("Failed to parse settings:")
        );
        // Disallowed property name.
        let src = settings_src("- 'bad key!': 1");
        assert!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string()
                .starts_with("Failed to parse settings:")
        );
        // Null value is not a valid parameter type.
        let src = settings_src("- opt: ~");
        assert!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string()
                .starts_with("Failed to parse settings:")
        );
        // $options with fewer than two entries.
        let src = settings_src("- opt: 1\n  $options:\n  - a: A");
        assert!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string()
                .starts_with("Failed to parse settings:")
        );
    }

    #[test]
    fn meta_only_item_is_missing_settings_key() {
        let src = settings_src("- $name: X");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Missing settings key"
        );
    }

    #[test]
    fn yaml_scalar_resolution_follows_yaml_1_2() {
        let src = settings_src("- a: true\n- b: 'true'\n- c: yes\n- d: 0x1A");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::Bool(true));
        assert_eq!(items[1].value, SettingValue::String("true".into()));
        // YAML 1.2 core schema: `yes` is a string, 0x1A is the integer 26.
        assert_eq!(items[2].value, SettingValue::String("yes".into()));
        assert_eq!(items[3].value, SettingValue::Number(26.into()));
    }

    #[test]
    fn floating_point_numbers_are_rejected() {
        // Windhawk stores settings as 32-bit integers; floats are not
        // supported - including integral-valued ones like 1.0 and 1e3,
        // since the rejection is on the YAML float TYPE, not the value.
        for value in ["1.5", "1.0", "1e3", ".inf"] {
            let src = settings_src(&format!("- opt: {value}"));
            let err = extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string();
            assert!(
                err.starts_with("Failed to parse settings:") && err.contains("floating-point"),
                "{value} must be rejected as a float, got: {err}"
            );
        }
        // Also inside a number array.
        let src = settings_src("- opt:\n  - 1\n  - 2.5");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("floating-point"), "got: {err}");
    }

    #[test]
    fn out_of_int32_range_integers_are_rejected_at_the_bounds() {
        let src = settings_src("- opt: 3000000000"); // > i32::MAX
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("32-bit integer"), "got: {err}");

        // The int32 bounds themselves are accepted.
        let src = settings_src("- lo: -2147483648\n- hi: 2147483647");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[0].value,
            SettingValue::Number((-2147483648i64).into())
        );
        assert_eq!(items[1].value, SettingValue::Number(2147483647i64.into()));
    }

    #[test]
    fn capitalized_booleans_resolve_to_booleans_like_js_yaml() {
        // js-yaml's bool type (used by its JSON_SCHEMA) accepts
        // true/True/TRUE/false/False/FALSE, and yaml-rust2's core schema
        // resolves the same set the same way. Several published mods write
        // TRUE/FALSE and rely on the boolean result, so this is parity,
        // not a divergence.
        let src = settings_src("- a: true\n- b: True\n- c: TRUE\n- d: FALSE");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::Bool(true));
        assert_eq!(items[1].value, SettingValue::Bool(true));
        assert_eq!(items[2].value, SettingValue::Bool(true));
        assert_eq!(items[3].value, SettingValue::Bool(false));
    }

    #[test]
    fn multiple_documents_are_rejected_like_js_yaml() {
        let src = settings_src("- a: 1\n---\n- b: 2");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "expected a single document in the stream, but found more"
        );
    }

    #[test]
    fn engine_flattening_matches_the_ts_keys_and_bool_to_int() {
        // Scalars (bool -> 0/1), scalar arrays (key[i]), a nested settings
        // group (key.child), and an array of settings arrays (key[i].child),
        // in source order - the `extractInitialSettingsForEngine` shape.
        let src = settings_src(
            "- boolOpt: true\n- numberOpt: 5\n- stringOpt: hi\n- list:\n  - 1\n  - 2\n- names:\n  - x\n  - y\n- group:\n  - inner: false\n- matrix:\n  - - cell: a\n  - - cell: b",
        );
        let flat = extract_initial_settings_for_engine(&src, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            flat,
            vec![
                ("boolOpt".to_owned(), EngineSettingValue::Int(1)),
                ("numberOpt".to_owned(), EngineSettingValue::Int(5)),
                (
                    "stringOpt".to_owned(),
                    EngineSettingValue::Str("hi".to_owned())
                ),
                ("list[0]".to_owned(), EngineSettingValue::Int(1)),
                ("list[1]".to_owned(), EngineSettingValue::Int(2)),
                (
                    "names[0]".to_owned(),
                    EngineSettingValue::Str("x".to_owned())
                ),
                (
                    "names[1]".to_owned(),
                    EngineSettingValue::Str("y".to_owned())
                ),
                ("group.inner".to_owned(), EngineSettingValue::Int(0)),
                (
                    "matrix[0].cell".to_owned(),
                    EngineSettingValue::Str("a".to_owned())
                ),
                (
                    "matrix[1].cell".to_owned(),
                    EngineSettingValue::Str("b".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn engine_flattening_is_none_without_a_block() {
        assert_eq!(
            extract_initial_settings_for_engine("// code", true),
            Ok(None)
        );
    }

    #[test]
    fn duplicate_mapping_keys_are_rejected_like_js_yaml() {
        // js-yaml's yaml.load throws on duplicate keys; yaml-rust2's loader
        // errors too (saphyr would silently keep the last value). The
        // message wording differs (documented divergence), but the
        // accept/reject behavior matches.
        let src = settings_src("- opt: 1\n  opt: 2");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("Failed to parse settings:"),
            "duplicate key must be rejected at parse time, got: {err}"
        );
    }

    #[test]
    fn duplicate_settings_id_at_top_level_is_rejected() {
        // Two sibling items with the same parameter key both flatten to the one
        // engine name `opt` - the store would keep only one, so it is invalid
        // and rejected (the reference silently collapses it last-write-wins).
        let src = settings_src("- opt: 1\n- opt: 2");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate settings id 'opt'"), "got: {err}");
    }

    #[test]
    fn duplicate_settings_id_in_a_nested_group_is_rejected() {
        // The scroll-window-opacity shape: a nested settings group whose items
        // all carry the same key `value` (every entry flattening to
        // `modifierKey.value`).
        let src = settings_src("- modifierKey:\n  - value: a\n  - value: b");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate settings id 'value'"), "got: {err}");
    }

    #[test]
    fn indexed_settings_arrays_reusing_a_key_are_not_duplicates() {
        // Two settings arrays at different indices flatten to distinct
        // `matrix[0].cell` / `matrix[1].cell` names, so reusing the inner key is
        // NOT a duplicate and must parse.
        let src = settings_src("- matrix:\n  - - cell: a\n  - - cell: b");
        extract_initial_settings(&src, "en").expect("indexed reuse is not a duplicate");
    }
}
