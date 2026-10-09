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
//! - `marker`: the `#!` spelling of the annotations, a pre-parse text pass
//!   plus the second reading of a marked block and its comparison.
//! - `nesting`: the pre-load bound on how deep the document nests.
//! - `aliases`: the pre-load bound on YAML alias expansion.
//! - `validate`: the schema validator.
//! - `transform`: the YAML -> typed-tree transformer.
//! - `conditions`: the `$showIf` / `$hideIf` reference resolver, on the
//!   typed tree.
//! - `strict`: the opt-in rules that take back the grammar's
//!   forward-compatible leniency.
//! - `flatten`: the engine name->value flattener.
//!
//! `scalar_key_to_string`, `parse_annotation_key` and `float_literal` are the
//! helpers genuinely shared between the validate and transform passes, so they
//! live here in the root (one home, not a copy per submodule).
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
//! implementation accepted any js-yaml number and coerced it at the engine. A
//! sweep over the published mods found a few SHIPPED ones that do use such
//! values (float defaults, uint32 ARGB colors); rather than relax the rule
//! globally, `workarounds::apply_settings_workarounds` rewrites those exact (mod
//! id, version) blocks to the value the mod actually uses (e.g. `1.0` -> `1`),
//! so a future release of the mod is forced to author it cleanly. The general
//! yaml-rust2-vs-js-yaml quoted-scalar divergences it also found
//! (surrogate-pair `\u` escapes, multi-line double-quoted indentation) are
//! handled the same per-version way. These shims are for ALREADY-PUBLISHED
//! store versions only: the engine path gates them by mod origin
//! (`extract_initial_settings_for_engine`'s `apply_workarounds`), off for a
//! locally-authored mod (`local@` storage id) so its author sees the real
//! validation error, not a silent fixup of a shipped version's source. The one
//! escape hatch is the `$float: true` annotation, which lifts the rule for that
//! item alone: the number (integer or float literal, finite) is parsed into its
//! canonical decimal text and carried as a STRING setting the mod reads with
//! `Wh_GetStringSetting`. A number item (integer or `$float`) may also declare
//! `$min` / `$max` bounds, checked against its default here and enforced by
//! the editor and the CLI, never by the store. Any item may declare `$showIf`
//! / `$hideIf`, the settings and values under which the editor shows it: the
//! validator checks the shape, and `conditions` resolves each reference,
//! written relative to the item, to the named setting's absolute declaration
//! path, checks the values against that setting, and rejects a cycle. The
//! engine never sees the conditions: a hidden setting is stored and read like
//! any other. Every one of these keys fails the parse of a Windhawk before
//! it, so each has a second spelling that older core reads as a comment: a
//! line beginning with `#! ` is read without the marker (`marker`), and a
//! block carrying one is also read as written and compared, so the marker can
//! only ever add an annotation. A `$float` default may be a string holding
//! the number (`"0.85"`), which is what a marked `$float` needs: the older
//! core stores it as the string it is. The annotation grammar is `$format`,
//! `$float`, `$dynamicSelect`, `$min`, `$max`, `$showIf`, `$hideIf`, none with
//! a language suffix, and the marker.
//!
//! Other known divergences from the TS implementation, accepted and
//! re-examined against the published mods:
//! - YAML syntax-error and schema-violation messages differ in wording
//!   (js-yaml/jsonschema diagnostics vs ours); the canonical
//!   "Failed to parse settings: not a valid YAML array" is preserved (the
//!   prefix comes from `SettingsParseError`'s `Display`, which is the one
//!   place that carries it, so no producer here spells it out).
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
//! - A block whose YAML aliases expand past a node bound is REFUSED
//!   (`aliases`), where js-yaml shares an aliased node by reference and parses
//!   it for free. No published block uses an alias at all.
//! - A block that nests past a depth bound is REFUSED (`nesting`), where the
//!   reference has no such limit. The deepest published block nests 15 levels.
//!
//! Duplicate mapping keys ARE rejected, matching js-yaml: yaml-rust2's loader
//! errors on them (the reason it is preferred over saphyr, which silently keeps
//! the last value).
//!
//! Duplicate settings IDS - two sibling items in one settings array sharing a
//! parameter key, so both flatten to the same engine name - are also rejected
//! (`validate::reject_duplicate_ids`). This is a STRICTER rule than the
//! reference, which silently collapses them last-write-wins (in the engine
//! store and the TS object); the ambiguous settings are invalid. One shipped
//! version relied on it (scroll-window-opacity 1.0.3, a malformed `modifierKey`
//! dropdown), pinned in `workarounds::apply_settings_workarounds` the same
//! per-(mod id, version) way; its engine flatten stays byte-identical.
//!
//! A settings item whose `$options[:lang]` variants do not offer the SAME set of
//! option VALUES is rejected (`validate::reject_mismatched_option_languages`).
//! Localization translates the LABELS; the value is what a selection stores and
//! what the mod reads back, so a language-dependent value set would store a
//! value the other languages cannot render and the mod does not handle. Option
//! ORDER may still differ per language. A STRICTER rule than the reference,
//! which validates each list on its own; NO shipped version violates it, so it
//! needs no `workarounds` pin.
//!
//! Object arrays whose groups declare a key their TEMPLATE group does not - or
//! with a conflicting type - are rejected
//! (`validate::reject_incompatible_object_arrays`, a post-transform pass on the
//! typed tree). The settings UI takes an object array's whole schema from the
//! first element at the template path (`items[0]`, `items[0].subItems[0]`, ...)
//! and applies it to every row, so such a key is dead in the form and mistyped
//! in the store. A reordered or partial (subset) default row stays valid - only
//! an extra or type-conflicting key is rejected. Another STRICTER-than-reference
//! rule; NO shipped version violates it, so it needs no `workarounds` pin.

use std::borrow::Cow;

use yaml_rust2::{Yaml, YamlLoader};

use crate::language::DEFAULT_LANGUAGE;
use crate::model::{EngineSettingValue, SettingItem, SettingsParseError};
use crate::scan::find_comment_block;

mod aliases;
mod conditions;
mod extract;
mod flat_key;
mod flatten;
mod marker;
mod nesting;
mod strict;
mod transform;
mod validate;
mod workarounds;

pub use flat_key::{
    FlatSetting, FlatSettingType, is_valid_flat_key, resolve_flat_setting,
    resolve_flat_setting_type,
};

/// `extractInitialSettings`: `Ok(None)` when the source has no settings
/// block, the parsed and language-resolved settings otherwise. Applies the
/// per-version compatibility workarounds (the display/preview parse, e.g.
/// `parseModSource`); the engine path gates them by mod origin, see
/// `extract_initial_settings_for_engine`.
pub fn extract_initial_settings(
    mod_source: &str,
    language: &str,
) -> Result<Option<Vec<SettingItem>>, SettingsParseError> {
    extract_initial_settings_inner(mod_source, language, true, false)
}

/// [`extract_initial_settings`] held to the `strict` rules as well, which
/// reject what the grammar otherwise accepts for forward compatibility. The
/// block is judged as written: the per-version workarounds do not run.
pub fn extract_initial_settings_strict(
    mod_source: &str,
    language: &str,
) -> Result<Option<Vec<SettingItem>>, SettingsParseError> {
    extract_initial_settings_inner(mod_source, language, false, true)
}

/// `extract_initial_settings`, with explicit control over whether the per-version
/// `workarounds::apply_settings_workarounds` shims run and whether the `strict`
/// rules apply. The shims keep ALREADY-PUBLISHED store versions parsing; a
/// locally-authored mod (`local@` storage id) skips them so the author sees the
/// real validation error instead of a silent fixup.
fn extract_initial_settings_inner(
    mod_source: &str,
    language: &str,
    apply_workarounds: bool,
    strict: bool,
) -> Result<Option<Vec<SettingItem>>, SettingsParseError> {
    let Some(block) = find_comment_block(mod_source, "WindhawkModSettings") else {
        return Ok(None);
    };

    // Apply any per-(mod id, version) compatibility fixup for shipped mods
    // whose settings YAML js-yaml accepts but yaml-rust2 rejects, keyed on the
    // source's own @id/@version - but only for store-installed mods; a
    // locally-authored mod is parsed as written.
    let normalized = if apply_workarounds {
        let (mod_id, mod_version) = extract::mod_id_and_version(mod_source);
        workarounds::apply_settings_workarounds(mod_id.as_ref(), mod_version.as_ref(), block)
    } else {
        Cow::Borrowed(block)
    };

    // The marker pass is grammar, not a shim: it runs for a locally-authored
    // mod as for a store one, since the author of a block written for the
    // older core is the first reader who needs it to parse, and the first who
    // needs to hear that it would not there.
    let (stripped, marked) = marker::strip_markers(&normalized).map_err(SettingsParseError::new)?;
    let items = load_settings_array(&stripped).map_err(SettingsParseError::new)?;
    validate::validate_settings_array(&items).map_err(SettingsParseError::new)?;

    // A marker anywhere is a statement that the whole block installs on
    // Windhawk 1.7.3, which reads the marked lines as comments: read the
    // block that way too, hold that reading to 1.7.3's key set and to the
    // grammar, and require the two readings to differ only by the annotations
    // the marker is for. The full tree's own errors come first, so a block
    // that is simply wrong is reported as such before its compatibility is.
    if marked {
        let legacy = load_settings_array(&normalized)
            .map_err(|e| SettingsParseError::new(format!("{LEGACY_READING}{e}")))?;
        marker::reject_non_legacy_keys(&legacy).map_err(SettingsParseError::new)?;
        validate::validate_settings_array(&legacy)
            .map_err(|e| SettingsParseError::new(format!("{LEGACY_READING}{e}")))?;
        marker::compare_readings(&legacy, &items, "instance").map_err(SettingsParseError::new)?;
    }

    let mut parsed = transform::parse_settings(&items, language)?;
    // The post-transform rules, which need the typed tree and so run here
    // rather than in the pre-transform validate pass: an object array's groups
    // must be type-compatible subsets of the template group at their path (the
    // UI schema), and every `$showIf` / `$hideIf` reference must name a
    // setting it may depend on.
    validate::reject_incompatible_object_arrays(&parsed).map_err(SettingsParseError::new)?;
    conditions::resolve_conditions(&mut parsed).map_err(SettingsParseError::new)?;
    if strict {
        strict::check_settings_array(&items).map_err(SettingsParseError::new)?;
    }
    Ok(Some(parsed))
}

/// The prefix on an error of a marked block's second reading, the one as
/// Windhawk 1.7.3 reads it.
const LEGACY_READING: &str = "as Windhawk 1.7.3 reads this block: ";

/// One reading of the block's text, up to the settings array the validator
/// judges: the two bounds, the loader, and the array check.
fn load_settings_array(block: &str) -> Result<Vec<Yaml>, String> {
    // The loader materializes the whole document before any rule here gets to
    // reject it: it copies an anchored node into every alias site, and it
    // descends a stack frame per nesting level. Bound both off the parser's
    // event stream first, which materializes nothing. Depth leads, because the
    // alias scan reads that stream through the same recursive descent the
    // loader does.
    nesting::reject_deep_nesting(block)?;
    aliases::reject_runaway_aliases(block)?;

    // A YAML syntax error, including a duplicate mapping key, fails here
    // (js-yaml's yaml.load throws on both).
    let docs = YamlLoader::load_from_str(block).map_err(|e| e.to_string())?;
    let doc = match docs.len() {
        // js-yaml's load() returns undefined for an empty stream, which
        // then fails the array check below.
        0 => Yaml::Null,
        1 => docs.into_iter().next().unwrap_or(Yaml::BadValue),
        // js-yaml's load() message for multi-document input.
        _ => return Err("expected a single document in the stream, but found more".to_owned()),
    };
    match doc {
        Yaml::Array(items) => Ok(items),
        _ => Err("not a valid YAML array".to_owned()),
    }
}

/// `extractInitialSettingsForEngine`: the same block parsed and validated as
/// `extract_initial_settings`, then FLATTENED into the engine's name->value
/// store form (the install flow's settings migration). `Ok(None)` when there is
/// no settings block. Keys are dotted/indexed paths (`group.inner`, `list[0]`,
/// `matrix[0].cell`), booleans become 0/1, in the source's declaration order.
/// Language is irrelevant here (the `$name`/`$description`/`$options`
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
        extract_initial_settings_inner(mod_source, DEFAULT_LANGUAGE, apply_workarounds, false)?
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

/// The `f64` a `$float` number literal denotes; `None` for a node that is not
/// a number. A `Real` resolves through yaml-rust2's own `as_f64`, the one
/// parser that understands the `.inf`/`.nan` spellings (their raw text fails
/// `f64::from_str`), so a non-finite literal comes back as such for the caller
/// to reject. Shared by the validate pass (the finite check) and the transform
/// pass (the canonical text) so the two agree on what a float literal is.
fn float_literal(value: &Yaml) -> Option<f64> {
    match value {
        Yaml::Integer(i) => Some(*i as f64),
        Yaml::Real(_) => value.as_f64(),
        _ => None,
    }
}

/// The number node a string `$float` default holds: its text resolved as a
/// bare scalar is (`Yaml::from_str`, so `"0.85"` is a `Real` and `"1"`,
/// `"0x10"` are `Integer`s, exactly as the bare literals read), or `None` when
/// the text resolves to anything else (`""` is a `Null`, `"abc"` a `String`).
/// Shared by validate and transform so the two agree on which strings hold a
/// number.
fn number_in_text(text: &str) -> Option<Yaml> {
    match Yaml::from_str(text) {
        node @ (Yaml::Integer(_) | Yaml::Real(_)) => Some(node),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Condition, SettingValue};

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
            "- opt: a\n  $name: Option\n  $name:fr: Choix\n  $description: An option\n  $options:\n  - a: Label A\n  - b: Label B",
        );
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.key, "opt");
        assert_eq!(item.value, SettingValue::String("a".into()));
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
    fn aliases_resolve_as_they_always_did() {
        // The bound on alias expansion is on the count, not on the feature: a
        // block that anchors a value and aliases it reads the same as one that
        // spells the value out.
        let src = settings_src("- a: &v hello\n  $name: A\n- b: *v\n  $name: B");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[1].value, SettingValue::String("hello".into()));
    }

    #[test]
    fn every_error_path_carries_the_prefix_exactly_once() {
        // `SettingsParseError`'s `Display` is the only thing that spells the
        // prefix out, so every producer reachable through the entry points is
        // labeled once: the marker pass, the alias bound, the YAML loader, the
        // multi-document check, the array check, the schema validation pass,
        // the second reading of a marked block with its key check and
        // comparison, the transformer, and the post-transform object-array and
        // condition passes. A producer that also stored the prefix - or a
        // consumer that added it - shows up here as a doubled one.
        const PREFIX: &str = "Failed to parse settings: ";
        // Nesting past the depth bound.
        let deep = format!("- a: {}1{}", "[".repeat(64), "]".repeat(64));
        let cases = [
            // A YAML syntax error.
            "- 'unterminated",
            // More than one document in the stream.
            "- a: 1\n---\n- b: 2",
            // Not an array.
            "just a string",
            // A schema violation (the empty array).
            "[]",
            // An item carrying annotations but no parameter (the transformer).
            "- $name: X",
            // An object array whose second entry declares a key the first
            // (the UI schema) does not.
            "- matrix:\n  - - a: 1\n  - - b: 2",
            // The annotation shape checks: an empty `$format`, a non-boolean
            // `$float`, a non-finite `$float` number.
            "- a: x\n  $format: ''",
            "- a: 1\n  $float: yes",
            "- a: .inf\n  $float: true",
            // The annotation cross-checks against the value type, and a string
            // under `$float` that holds no number.
            "- a: true\n  $float: true",
            "- a: x\n  $float: true",
            "- a: 1\n  $dynamicSelect: true",
            "- a: x\n  $min: 1",
            // The bound checks: a non-number bound, a float bound on an integer
            // item, inverted bounds, a default outside them.
            "- a: 1\n  $max: x",
            "- a: 1\n  $min: 0.5",
            "- a: 1\n  $min: 2\n  $max: 1",
            "- a: 9\n  $max: 5",
            // The condition shape checks: not a map, a bad reference, a bad
            // value.
            "- a: 1\n  $showIf: x",
            "- a: 1\n  $showIf: {'b c': 1}",
            "- a: 1\n  $showIf: {b: []}",
            // The condition resolver: an unresolvable name, a path into an
            // array, a non-scalar target, a value of the wrong kind, a value
            // outside the target's options, self, a group naming its own
            // member, one setting named twice, a cycle.
            "- a: 1\n  $showIf: {b: 1}",
            "- rows:\n  - - b: 1\n- a: 1\n  $showIf: {rows.b: 1}",
            "- g:\n  - b: 1\n- a: 1\n  $showIf: {g: 1}",
            "- b: 1\n- a: 1\n  $showIf: {b: x}",
            "- b: x\n  $options:\n  - x: X\n  - y: Y\n- a: 1\n  $showIf: {b: z}",
            "- a: 1\n  $showIf: {a: 1}",
            "- g:\n  - b: 1\n  $showIf: {g.b: 1}",
            "- g:\n  - b: 1\n  - a: 1\n    $showIf: {b: 1, g.b: 2}",
            "- a: 1\n  $showIf: {b: 1}\n- b: 1\n  $showIf: {a: 1}",
            // The `#!` marker: a malformed one, and the second reading's
            // producers in their order - its loader (an anchor on a marked
            // line, aliased on a bare one), its key check, its grammar, and
            // the comparison of the two readings.
            "- a: x\n  #!$format: y",
            "- a: x\n  #! $format: &f y\n- b: *f",
            "- a: 1\n  $min: 0\n  #! $format: y",
            "- a: 0.85\n  #! $float: true",
            "- a: x\n  #! $name: A",
            // Aliases that expand past the node bound.
            "- a0: &a0 x\n\
             - a1: &a1 [*a0,*a0,*a0,*a0,*a0,*a0,*a0,*a0,*a0]\n\
             - a2: &a2 [*a1,*a1,*a1,*a1,*a1,*a1,*a1,*a1,*a1]\n\
             - a3: &a3 [*a2,*a2,*a2,*a2,*a2,*a2,*a2,*a2,*a2]\n\
             - a4: &a4 [*a3,*a3,*a3,*a3,*a3,*a3,*a3,*a3,*a3]\n\
             - a5: [*a4,*a4,*a4,*a4,*a4,*a4,*a4,*a4,*a4]",
            deep.as_str(),
        ];
        let mut causes = Vec::new();
        for yaml in cases {
            let src = settings_src(yaml);
            let err = extract_initial_settings(&src, "en")
                .expect_err("expected a parse error")
                .to_string();
            let cause = err
                .strip_prefix(PREFIX)
                .unwrap_or_else(|| panic!("unprefixed error: {err:?}"));
            assert!(!cause.contains(PREFIX), "doubled prefix: {err:?}");
            // The second reading runs only for a marked block, so no other
            // block's error is attributed to it.
            assert!(
                yaml.contains("#!") || !cause.contains(LEGACY_READING),
                "the second reading ran without a marker: {err:?}"
            );
            // The engine entry point shares the producers, so it reads the same.
            assert_eq!(
                extract_initial_settings_for_engine(&src, false)
                    .expect_err("expected a parse error")
                    .to_string(),
                err
            );
            causes.push(cause.to_owned());
        }
        // Distinct causes, so the sources really did reach distinct producers
        // rather than all failing the same check.
        causes.sort();
        causes.dedup();
        assert_eq!(causes.len(), cases.len(), "overlapping cases: {causes:?}");
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
    fn options_with_non_string_label_is_rejected() {
        // A `$options` entry maps an option value (the key) to a display label
        // (the value); the label must be a string, mirroring the reference
        // schema's `additionalProperties: {type: string}`. A numeric or boolean
        // label is rejected. The key is coerced to its string form and is not
        // type-checked, so an integer-keyed dropdown (`0: Off`) stays valid.
        for label in ["1", "true"] {
            let src = settings_src(&format!("- opt: x\n  $options:\n  - a: {label}\n  - b: ok"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].$options[0] must map to a string",
            );
        }
    }

    #[test]
    fn options_on_a_number_or_bool_value_is_rejected() {
        // `$options` is a dropdown the UI renders ONLY for a string setting; on a
        // number or boolean value the UI shows a plain number input / switch and
        // never reads it, so it is dead metadata and rejected (a stricter rule
        // than the reference). A string value keeps its dropdown (see
        // `parses_scalars_annotations_and_options`).
        for value in ["1", "true"] {
            let src = settings_src(&format!("- opt: {value}\n  $options:\n  - a: A\n  - b: B"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be a string or array of strings to use $options",
            );
        }
    }

    #[test]
    fn options_on_a_number_array_is_rejected() {
        // A number array renders per-element number inputs that ignore `$options`,
        // so its dropdown is dead metadata like a scalar number - unlike a string
        // array, whose elements ARE dropdowns.
        let src = settings_src("- levels: [1, 2]\n  $options:\n  - 1: One\n  - 2: Two");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].levels must be a string or array of strings to use $options",
        );
    }

    #[test]
    fn options_on_a_string_array_value_is_allowed() {
        // A string array with `$options` renders each element as a dropdown (the
        // UI recurses into the array), so it is valid - only a string scalar and
        // a string array carry a dropdown.
        let src = settings_src("- buttons: [x, y]\n  $options:\n  - x: X\n  - y: Y");
        let item = &extract_initial_settings(&src, "en").unwrap().unwrap()[0];
        assert_eq!(
            item.value,
            SettingValue::StringArray(vec!["x".into(), "y".into()])
        );
        assert!(item.options.is_some());
    }

    #[test]
    fn localized_options_that_do_not_match_are_rejected() {
        // Only the LABELS of a dropdown are translated; the option VALUES are
        // what a selection stores, so a `$options:<lang>` variant that renames,
        // adds or drops one is rejected. Here the Russian list offers `over`
        // where the base offers `near`, so a Russian user's selection would
        // store a value no other language can render and the mod does not
        // handle.
        let src = settings_src(
            "- placement: near\n  $options:\n  - near: Near the media player\n  - screen: Placement on the screen\n  \
             $options:ru-RU:\n  - over: Ryadom\n  - screen: Na ekrane",
        );
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$options:ru-RU has option 'over' \
             that instance[0].$options does not declare",
        );

        // A variant that only DROPS an option is rejected the same way: the
        // value an English user stored would have no entry to render in French.
        let src = settings_src(
            "- placement: near\n  $options:\n  - near: Near\n  - screen: Screen\n  - both: Both\n  \
             $options:fr:\n  - near: Pres\n  - screen: Ecran",
        );
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$options:fr is missing option 'both' \
             that instance[0].$options declares",
        );
    }

    #[test]
    fn localized_options_may_reorder_and_relabel() {
        // The same option VALUES in a different order is fine: order is
        // presentation only, and each label travels with its own value. The
        // resolved list is the matching language's, in ITS order.
        let src = settings_src(
            "- placement: near\n  $options:\n  - near: Near\n  - screen: Screen\n  \
             $options:fr:\n  - screen: Ecran\n  - near: Pres",
        );
        let item = &extract_initial_settings(&src, "fr").unwrap().unwrap()[0];
        assert_eq!(
            item.options.as_deref(),
            Some(
                &[
                    ("screen".to_owned(), "Ecran".to_owned()),
                    ("near".to_owned(), "Pres".to_owned())
                ][..]
            )
        );
    }

    #[test]
    fn options_declared_only_in_localized_variants_must_still_match() {
        // With no unlocalized `$options`, the first variant in declaration order
        // is the one the others are compared against, so every variant is
        // consistent with every other however the item is authored.
        let src = settings_src(
            "- placement: near\n  $options:en:\n  - near: Near\n  - screen: Screen\n  \
             $options:fr:\n  - near: Pres\n  - ecran: Ecran",
        );
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$options:fr has option 'ecran' \
             that instance[0].$options:en does not declare",
        );
    }

    #[test]
    fn meta_only_item_is_missing_settings_key() {
        let src = settings_src("- $name: X");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: Missing settings key"
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
    fn unmarked_floats_keep_the_exact_rejection_message() {
        // The wording is unchanged by `$float`: the `workarounds` pins and the
        // parity over the published mods depend on the rule standing without
        // the marker. An integer literal past i64 resolves as a Real, so it
        // fails the same way.
        for value in ["1.5", "99999999999999999999"] {
            let src = settings_src(&format!("- opt: {value}"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be an integer; \
                 floating-point numbers are not supported",
                "{value}"
            );
        }
    }

    #[test]
    fn parses_format_float_and_dynamic_select() {
        let src = settings_src(concat!(
            "- accent: \"#3399FF\"\n",
            "  $format: colorRgb\n",
            "- opacity: 0.85\n",
            "  $float: true\n",
            "- scale: 1\n",
            "  $float: true\n",
            "- weights: [0.25, 1, 1e3]\n",
            "  $float: true\n",
            "- device: ''\n",
            "  $dynamicSelect: true\n",
            "- monitor: primary\n",
            "  $options:\n",
            "  - primary: Primary\n",
            "  - secondary: Secondary\n",
            "  $dynamicSelect: true\n",
            "- plain: 1",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();

        // `$format` is forwarded verbatim on a string; nothing else is set.
        assert_eq!(items[0].value, SettingValue::String("#3399FF".into()));
        assert_eq!(items[0].format.as_deref(), Some("colorRgb"));
        assert!(!items[0].float && !items[0].dynamic_select);

        // `$float`: the number becomes its canonical decimal text, as a string.
        assert_eq!(items[1].value, SettingValue::String("0.85".into()));
        assert!(items[1].float);
        assert_eq!(items[2].value, SettingValue::String("1".into()));
        assert!(items[2].float);
        assert_eq!(
            items[3].value,
            SettingValue::StringArray(vec!["0.25".into(), "1".into(), "1000".into()])
        );
        assert!(items[3].float);

        // `$dynamicSelect`, alone and beside static `$options`.
        assert_eq!(items[4].value, SettingValue::String(String::new()));
        assert!(items[4].dynamic_select);
        assert!(items[4].options.is_none());
        assert!(items[5].dynamic_select);
        assert_eq!(items[5].options.as_ref().map(Vec::len), Some(2));

        // An unannotated item carries none of the three.
        assert_eq!(items[6].value, SettingValue::Number(1.into()));
        assert_eq!(items[6].format, None);
        assert!(!items[6].float && !items[6].dynamic_select);
    }

    #[test]
    fn float_array_is_classified_by_a_float_or_an_int_first_element() {
        // The transformer classifies an array by its first element; under
        // `$float` a Real first element must read as a number array, not fall
        // through to a settings group.
        for (value, expected) in [
            ("[0.5, 1]", vec!["0.5", "1"]),
            ("[1, 0.5]", vec!["1", "0.5"]),
        ] {
            let src = settings_src(&format!("- w: {value}\n  $float: true"));
            let items = extract_initial_settings(&src, "en").unwrap().unwrap();
            assert_eq!(
                items[0].value,
                SettingValue::StringArray(expected.into_iter().map(String::from).collect()),
                "{value}"
            );
        }
    }

    #[test]
    fn float_annotation_written_before_or_after_the_value_reads_the_same() {
        // The flag is peeked ahead of the key loop, so the value validates the
        // same way wherever the annotation sits in the mapping.
        for yaml in [
            "- opacity: 0.85\n  $float: true",
            "- $float: true\n  opacity: 0.85",
        ] {
            let items = extract_initial_settings(&settings_src(yaml), "en")
                .unwrap()
                .unwrap();
            assert_eq!(
                items[0].value,
                SettingValue::String("0.85".into()),
                "{yaml}"
            );
        }
    }

    #[test]
    fn float_false_is_accepted_and_inert() {
        let src = settings_src("- scale: 1\n  $float: false");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::Number(1.into()));
        assert!(!items[0].float);
        // Without the flag set, the no-float rule stands on the value.
        let src = settings_src("- scale: 1.5\n  $float: false");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("floating-point"), "got: {err}");
    }

    #[test]
    fn float_on_a_non_number_value_is_rejected() {
        for value in ["true", "\n  - inner: 1"] {
            let src = settings_src(&format!("- opt: {value}\n  $float: true"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be a number or array of numbers to use $float",
                "{value:?}"
            );
        }
    }

    #[test]
    fn float_takes_a_string_default_that_holds_a_number() {
        // The text is resolved as a bare scalar would be, then read as that
        // literal, so the two spellings of a default produce one item. The
        // string spelling is what a marked `$float` needs: a Windhawk before
        // the annotation stores the string as it is.
        for (value, text) in [
            ("'0.85'", "0.85"),
            ("\"1.0\"", "1"),
            ("'1e3'", "1000"),
            ("'0x10'", "16"),
            ("'-7'", "-7"),
        ] {
            let src = settings_src(&format!("- opt: {value}\n  $float: true"));
            let items = extract_initial_settings(&src, "en").unwrap().unwrap();
            assert_eq!(items[0].value, SettingValue::String(text.into()), "{value}");
            assert!(items[0].float);
        }
        let src = settings_src("- opt: ['0.25', '1']\n  $float: true");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[0].value,
            SettingValue::StringArray(vec!["0.25".into(), "1".into()])
        );
        // The engine store gets the same text either way.
        let bare = settings_src("- opt: 0.85\n  $float: true");
        let quoted = settings_src("- opt: '0.85'\n  $float: true");
        assert_eq!(
            extract_initial_settings_for_engine(&quoted, false).unwrap(),
            extract_initial_settings_for_engine(&bare, false).unwrap()
        );
    }

    #[test]
    fn float_on_a_string_that_holds_no_number_is_rejected() {
        for value in ["x", "abc", "'.inf'", "''", "'1 2'", "'true'"] {
            let src = settings_src(&format!("- opt: {value}\n  $float: true"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be a number, or a string holding one, to use $float",
                "{value}"
            );
        }
        // Inside an array the element path is named.
        let src = settings_src("- opt: ['1', a]\n  $float: true");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].opt[1] must be a number, or a string holding one, to use $float"
        );
    }

    #[test]
    fn bounds_read_the_number_a_string_default_holds() {
        let src = settings_src("- opt: '0.5'\n  $float: true\n  $min: 0\n  $max: 1");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::String("0.5".into()));
        assert_eq!(items[0].min, Some(serde_json::Number::from(0)));
        assert_eq!(items[0].max, Some(serde_json::Number::from(1)));
        for (yaml, path) in [
            ("- opt: '1.5'\n  $float: true\n  $max: 1", "instance[0].opt"),
            (
                "- opt: ['0.5', '2']\n  $float: true\n  $max: 1",
                "instance[0].opt[1]",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {path} default is outside its $min/$max range"),
                "{yaml}"
            );
        }
        // Without `$float` a string holds no number to bound.
        assert_eq!(
            settings_error("- opt: '0.5'\n  $min: 0"),
            "Failed to parse settings: instance[0].opt must be a number or array of numbers to use $min"
        );
    }

    #[test]
    fn non_finite_floats_are_rejected_even_when_marked() {
        for value in [".inf", "-.inf", ".nan"] {
            let src = settings_src(&format!("- opt: {value}\n  $float: true"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be a finite number",
                "{value}"
            );
        }
        // Inside an array the element path is named.
        let src = settings_src("- opt: [1, .nan]\n  $float: true");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].opt[1] must be a finite number"
        );
    }

    #[test]
    fn format_is_accepted_on_any_value_type_but_must_be_a_non_empty_string() {
        // The core forwards `$format` without interpreting it, on every value
        // type: the front-end decides what (if anything) a format means there.
        for value in ["true", "1", "x", "[1, 2]", "\n  - inner: 1"] {
            let src = settings_src(&format!("- opt: {value}\n  $format: fontFamily"));
            let items = extract_initial_settings(&src, "en")
                .unwrap_or_else(|e| panic!("{value:?}: {e}"))
                .unwrap();
            assert_eq!(items[0].format.as_deref(), Some("fontFamily"), "{value:?}");
        }
        for (value, cause) in [
            ("''", "instance[0].$format must be a non-empty string"),
            ("1", "instance[0].$format must be a non-empty string"),
            ("[a]", "instance[0].$format must be a non-empty string"),
        ] {
            let src = settings_src(&format!("- opt: x\n  $format: {value}"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                format!("Failed to parse settings: {cause}"),
                "{value}"
            );
        }
    }

    #[test]
    fn boolean_annotations_must_be_booleans() {
        for key in ["$float", "$dynamicSelect"] {
            for value in ["yes", "1", "'true'"] {
                let src = settings_src(&format!("- opt: x\n  {key}: {value}"));
                assert_eq!(
                    extract_initial_settings(&src, "en")
                        .unwrap_err()
                        .to_string(),
                    format!("Failed to parse settings: instance[0].{key} must be a boolean"),
                    "{key}: {value}"
                );
            }
        }
    }

    #[test]
    fn new_annotations_take_no_language_suffix() {
        // `$format`, `$float` and `$dynamicSelect` govern the stored value, not a
        // display string, so a per-language variant is an unknown key - the
        // same rejection as any misspelled or miscased annotation.
        for key in [
            "$format:fr",
            "$float:en",
            "$dynamicSelect:en-US",
            "$Float",
            "$showIf:fr",
            "$hideIf:en",
            "$ShowIf",
        ] {
            let src = settings_src(&format!("- opt: x\n  {key}: true"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                format!("Failed to parse settings: instance[0].{key} is not an allowed property"),
                "{key}"
            );
        }
    }

    #[test]
    fn underscored_annotations_are_ignored_not_aliased() {
        // Any `$_` key is accepted with any value and contributes nothing;
        // `$_name` is not `$name`.
        let src = settings_src(
            "- key: Value\n  $name: The key\n  $_new: green\n  $_name: X\n  $_name:fr: Y\n  $_list:\n  - 1\n  - 2",
        );
        let items = extract_initial_settings(&src, "fr").unwrap().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, "key");
        assert_eq!(items[0].name.as_deref(), Some("The key"));

        let src = settings_src("- key: Value\n  $_new: green");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].name, None);

        // The bare `$_` is the prefix alone, ignored like any other.
        let src = settings_src("- key: Value\n  $_: 1");
        assert!(extract_initial_settings(&src, "en").is_ok());
    }

    #[test]
    fn dynamic_select_on_a_non_string_value_is_rejected() {
        for value in ["1", "true", "[1, 2]"] {
            let src = settings_src(&format!("- opt: {value}\n  $dynamicSelect: true"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt must be a string or array of strings to use $dynamicSelect",
                "{value}"
            );
        }
        // A string array takes it, like `$options`.
        let src = settings_src("- devices: [a, b]\n  $dynamicSelect: true");
        let item = &extract_initial_settings(&src, "en").unwrap().unwrap()[0];
        assert!(item.dynamic_select);
    }

    #[test]
    fn parses_min_and_max_on_integer_and_float_items() {
        let src = settings_src(concat!(
            "- rating: 3\n",
            "  $min: 1\n",
            "  $max: 5\n",
            "- weights: [1, 2, 3]\n",
            "  $min: 0\n",
            "  $max: 10\n",
            "- opacity: 0.85\n",
            "  $float: true\n",
            "  $min: 0\n",
            "  $max: 1\n",
            "- scales: [0.5, 1.5]\n",
            "  $float: true\n",
            "  $min: 0.25\n",
            "  $max: 2.5\n",
            "- retries: 3\n",
            "  $min: 0\n",
            "- limit: 3\n",
            "  $max: 10\n",
            "- plain: 1",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        let bounds = |i: usize| (items[i].min.clone(), items[i].max.clone());
        let int = |v: i64| Some(serde_json::Number::from(v));
        let real = |v: f64| serde_json::Number::from_f64(v);

        // The value itself is unchanged by the bounds.
        assert_eq!(items[0].value, SettingValue::Number(3.into()));
        assert_eq!(bounds(0), (int(1), int(5)));
        assert_eq!(
            items[1].value,
            SettingValue::NumberArray(vec![1.into(), 2.into(), 3.into()])
        );
        assert_eq!(bounds(1), (int(0), int(10)));
        // A `$float` item keeps an integer bound as an integer and a real
        // bound as a real: each literal in its own kind.
        assert_eq!(items[2].value, SettingValue::String("0.85".into()));
        assert_eq!(bounds(2), (int(0), int(1)));
        assert_eq!(
            items[3].value,
            SettingValue::StringArray(vec!["0.5".into(), "1.5".into()])
        );
        assert_eq!(bounds(3), (real(0.25), real(2.5)));
        // Either bound alone.
        assert_eq!(bounds(4), (int(0), None));
        assert_eq!(bounds(5), (None, int(10)));
        assert_eq!(bounds(6), (None, None));
    }

    #[test]
    fn bounds_written_before_or_after_the_value_read_the_same() {
        // The `$float` peek covers the bounds too: a `$min` before the value
        // and before its `$float` still takes the float rule.
        for yaml in [
            "- opacity: 0.85\n  $float: true\n  $min: 0.5",
            "- $min: 0.5\n  $float: true\n  opacity: 0.85",
            "- $min: 0.5\n  opacity: 0.85\n  $float: true",
        ] {
            let items = extract_initial_settings(&settings_src(yaml), "en")
                .unwrap()
                .unwrap();
            assert_eq!(
                items[0].value,
                SettingValue::String("0.85".into()),
                "{yaml}"
            );
            assert_eq!(items[0].min, serde_json::Number::from_f64(0.5), "{yaml}");
        }
    }

    #[test]
    fn bounds_on_a_non_number_value_are_rejected() {
        for (value, key) in [
            ("x", "$min"),
            ("true", "$max"),
            ("[a, b]", "$min"),
            ("\n  - inner: 1", "$max"),
        ] {
            let src = settings_src(&format!("- opt: {value}\n  {key}: 1"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                format!(
                    "Failed to parse settings: instance[0].opt must be a number or array of numbers to use {key}"
                ),
                "{value:?} {key}"
            );
        }
    }

    #[test]
    fn a_bound_takes_the_items_own_number_rule() {
        // A non-number bound.
        for value in ["x", "true", "[1]"] {
            let src = settings_src(&format!("- opt: 1\n  $min: {value}"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].$min must be a number",
                "{value}"
            );
        }
        // A float bound on an integer item fails the way the default would,
        // at the annotation's own path; an out-of-int32 one likewise.
        let src = settings_src("- opt: 1\n  $min: 0.5");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$min must be an integer; \
             floating-point numbers are not supported"
        );
        let src = settings_src("- opt: 1\n  $max: 2147483648");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$max must be a 32-bit integer; \
             2147483648 is out of range"
        );
        // On a `$float` item a bound may be a real, but must be finite.
        let src = settings_src("- opt: 1\n  $float: true\n  $max: .inf");
        assert_eq!(
            extract_initial_settings(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$max must be a finite number"
        );
    }

    #[test]
    fn inverted_bounds_are_rejected() {
        for yaml in [
            "- opt: 1\n  $min: 2\n  $max: 1",
            "- opt: 1\n  $float: true\n  $min: 1.5\n  $max: 0.5",
        ] {
            assert_eq!(
                extract_initial_settings(&settings_src(yaml), "en")
                    .unwrap_err()
                    .to_string(),
                "Failed to parse settings: instance[0].opt declares a $min greater than its $max",
                "{yaml}"
            );
        }
        // Equal bounds are a range of one value.
        let src = settings_src("- opt: 1\n  $min: 1\n  $max: 1");
        assert!(extract_initial_settings(&src, "en").is_ok());
    }

    #[test]
    fn a_default_outside_the_bounds_is_rejected() {
        for (yaml, path) in [
            ("- opt: 0\n  $min: 1", "instance[0].opt"),
            ("- opt: 6\n  $max: 5", "instance[0].opt"),
            (
                "- opt: 0.25\n  $float: true\n  $min: 0.5",
                "instance[0].opt",
            ),
            // An array names the offending element.
            ("- opt: [1, 2, 9]\n  $max: 5", "instance[0].opt[2]"),
            (
                "- opt: [0.5, 0.1]\n  $float: true\n  $min: 0.25",
                "instance[0].opt[1]",
            ),
        ] {
            assert_eq!(
                extract_initial_settings(&settings_src(yaml), "en")
                    .unwrap_err()
                    .to_string(),
                format!("Failed to parse settings: {path} default is outside its $min/$max range"),
                "{yaml}"
            );
        }
        // The bounds are inclusive.
        for yaml in [
            "- opt: 1\n  $min: 1\n  $max: 5",
            "- opt: 5\n  $min: 1\n  $max: 5",
            "- opt: [1, 5]\n  $min: 1\n  $max: 5",
        ] {
            assert!(
                extract_initial_settings(&settings_src(yaml), "en").is_ok(),
                "{yaml}"
            );
        }
    }

    #[test]
    fn bounds_take_no_language_suffix() {
        for key in ["$min:fr", "$max:en", "$Min"] {
            let src = settings_src(&format!("- opt: 1\n  {key}: 1"));
            assert_eq!(
                extract_initial_settings(&src, "en")
                    .unwrap_err()
                    .to_string(),
                format!("Failed to parse settings: instance[0].{key} is not an allowed property"),
                "{key}"
            );
        }
    }

    #[test]
    fn float_flag_is_per_item() {
        // A `$float` on a group does not reach the nested items, and a nested
        // `$float` does not reach its siblings: each item reads its own flag.
        let src = settings_src("- group:\n  - a: 0.5\n    $float: true\n  - b: 1");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        let SettingValue::Settings(inner) = &items[0].value else {
            panic!("expected a group");
        };
        assert_eq!(inner[0].value, SettingValue::String("0.5".into()));
        assert!(inner[0].float);
        assert_eq!(inner[1].value, SettingValue::Number(1.into()));
        assert!(!inner[1].float);

        let src = settings_src("- group:\n  - a: 0.5\n  $float: true");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        // The nested value fails first, in mapping order, before the group's
        // own `$float` cross-check would.
        assert!(
            err.contains("instance[0].group[0].a") && err.contains("floating-point"),
            "got: {err}"
        );
    }

    #[test]
    fn engine_flattening_stores_a_float_setting_as_text() {
        // A `$float` item is a string setting to the engine: the canonical
        // decimal text, one `Str` per leaf, so the store needs no float type
        // and the mod reads it back with `Wh_GetStringSetting`.
        let src =
            settings_src("- opacity: 0.85\n  $float: true\n- weights: [1, 2.5]\n  $float: true");
        let flat = extract_initial_settings_for_engine(&src, false)
            .unwrap()
            .unwrap();
        assert_eq!(
            flat,
            vec![
                (
                    "opacity".to_owned(),
                    EngineSettingValue::Str("0.85".to_owned())
                ),
                (
                    "weights[0]".to_owned(),
                    EngineSettingValue::Str("1".to_owned())
                ),
                (
                    "weights[1]".to_owned(),
                    EngineSettingValue::Str("2.5".to_owned())
                ),
            ]
        );
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
            "Failed to parse settings: expected a single document in the stream, but found more"
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

    #[test]
    fn object_array_reordered_default_row_is_accepted() {
        // ultimate-custom-tray shape: the annotated template first, a default row
        // with the SAME keys in a different order. Order is irrelevant (the UI
        // looks keys up by name), so this is valid.
        let src = settings_src(
            "- items:\n  - - state: enabled\n    - name: X\n  - - name: Y\n    - state: disabled",
        );
        extract_initial_settings(&src, "en").expect("a reordered default row is valid");
    }

    #[test]
    fn object_array_partial_default_row_is_accepted() {
        // windows-11-start-menu-buttons shape: the full template first, later rows
        // a SUBSET of the keys (overriding only some fields). Missing keys are
        // fine - they fall back to the template default.
        let src =
            settings_src("- buttons:\n  - - preset: custom\n    - name: X\n  - - preset: settings");
        extract_initial_settings(&src, "en").expect("a partial default row is valid");
    }

    #[test]
    fn object_array_extra_key_in_a_later_row_is_rejected() {
        // A later group declares `bogus`, a key the first (schema) group does not
        // - dead in the UI form, mistyped in the store. Rejected.
        let src = settings_src(
            "- buttons:\n  - - preset: custom\n  - - preset: settings\n    - bogus: 1",
        );
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("object array 'buttons'") && err.contains("bogus"),
            "got: {err}"
        );
    }

    #[test]
    fn object_array_type_conflict_in_a_later_row_is_rejected() {
        // `count` is a number in the first group but a string in a later group -
        // the UI schema (from the first) would mistype the store value. Rejected.
        let src = settings_src("- items:\n  - - count: 1\n  - - count: hi");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("object array 'items'") && err.contains("count"),
            "got: {err}"
        );
    }

    #[test]
    fn object_array_nested_object_subset_ok_but_extra_nested_key_rejected() {
        // The subset rule recurses (the note in the reject fn): a later row's
        // nested `sub` group may drop a key...
        let ok = settings_src(
            "- rows:\n  - - sub:\n      - x: 1\n      - y: 2\n  - - sub:\n      - x: 3",
        );
        extract_initial_settings(&ok, "en").expect("a nested subset is valid");

        // ...but may not introduce one the template's `sub` does not declare.
        let bad = settings_src("- rows:\n  - - sub:\n      - x: 1\n  - - sub:\n      - z: 9");
        let err = extract_initial_settings(&bad, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("object array 'rows'"), "got: {err}");
    }

    #[test]
    fn object_array_nested_array_is_governed_by_the_template_not_by_its_own_first_row() {
        // explorer-command-bar shape: the template row declares a nested
        // `subItems` array that itself declares `subItems` (a submenu). A DEFAULT
        // row then fills that array in, with a submenu only on its second entry.
        // The nested rows are data, so the schema for them is the TEMPLATE's
        // nested row - not the default row's own first entry, which happens to
        // have no submenu.
        let src = settings_src(concat!(
            "- items:\n",
            "  - - name: T\n",
            "    - subItems:\n",
            "      - - name: ST\n",
            "        - subItems:\n",
            "          - - name: SST\n",
            "  - - name: A\n",
            "    - subItems:\n",
            "      - - name: B\n",
            "      - - name: C\n",
            "        - subItems:\n",
            "          - - name: D",
        ));
        extract_initial_settings(&src, "en")
            .expect("a nested default row may declare a key its siblings omit");
        // The engine store keys the submenu by path, so the deeper row's values
        // reach the mod exactly where it reads them.
        let flat = extract_initial_settings_for_engine(&src, false)
            .expect("the engine path parses it too")
            .unwrap();
        assert!(
            flat.iter()
                .any(|(k, _)| k == "items[1].subItems[1].subItems[0].name"),
            "got: {flat:?}"
        );
    }

    #[test]
    fn object_array_nested_row_violating_the_template_is_still_rejected() {
        // The same shape, but the nested default row declares `bogus`, which the
        // template's nested row does not - dead in the form either way. The error
        // names the inner array, not the outer one.
        let src = settings_src(concat!(
            "- items:\n",
            "  - - name: T\n",
            "    - subItems:\n",
            "      - - name: ST\n",
            "  - - name: A\n",
            "    - subItems:\n",
            "      - - name: B\n",
            "      - - name: C\n",
            "        - bogus: 1",
        ));
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("object array 'items[1].subItems'") && err.contains("bogus"),
            "got: {err}"
        );
    }

    // The `$showIf` / `$hideIf` conditions. Each accepted block asserts the
    // RESOLVED reference - the absolute declaration path, no subscripts - and
    // the value list a scalar is normalized to.
    fn cond(path: &str, values: Vec<SettingValue>) -> Condition {
        Condition {
            path: path.to_owned(),
            values,
        }
    }

    fn bools(values: &[bool]) -> Vec<SettingValue> {
        values.iter().map(|&b| SettingValue::Bool(b)).collect()
    }

    fn strings(values: &[&str]) -> Vec<SettingValue> {
        values
            .iter()
            .map(|&s| SettingValue::String(s.to_owned()))
            .collect()
    }

    fn numbers(values: &[i64]) -> Vec<SettingValue> {
        values
            .iter()
            .map(|&n| SettingValue::Number(n.into()))
            .collect()
    }

    fn group(item: &SettingItem) -> &[SettingItem] {
        match &item.value {
            SettingValue::Settings(inner) => inner,
            other => panic!("expected a group, got {other:?}"),
        }
    }

    fn rows(item: &SettingItem) -> &[Vec<SettingItem>] {
        match &item.value {
            SettingValue::SettingsArray(rows) => rows,
            other => panic!("expected an object array, got {other:?}"),
        }
    }

    fn settings_error(yaml: &str) -> String {
        extract_initial_settings(&settings_src(yaml), "en")
            .expect_err("expected a parse error")
            .to_string()
    }

    #[test]
    fn a_sibling_boolean_gates_the_items_after_it_on_either_branch() {
        // center-new-windows: a switch, then the setting it enables;
        // taskbar-thumbnail-size: one setting per branch of the switch.
        let src = settings_src(concat!(
            "- fadeTrickEnabled: true\n",
            "- fadeDelayMs: 100\n",
            "  $showIf: {fadeTrickEnabled: true}\n",
            "- size: 150\n",
            "  $showIf: {fadeTrickEnabled: false}\n",
            "- plain: 1",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].show_if, None);
        assert_eq!(
            items[1].show_if,
            Some(vec![cond("fadeTrickEnabled", bools(&[true]))])
        );
        assert_eq!(
            items[2].show_if,
            Some(vec![cond("fadeTrickEnabled", bools(&[false]))])
        );
        assert_eq!(items[3].show_if, None);
        assert!(items.iter().all(|item| item.hide_if.is_none()));
    }

    #[test]
    fn an_option_value_or_a_list_of_them_reveals_a_companion() {
        // lock-keys-notifier: `custom` reveals the file; taskbar-labels: two of
        // four modes. A `$hideIf` names the complement (shell-flyout-positions).
        let src = settings_src(concat!(
            "- soundMode: none\n",
            "  $options:\n",
            "  - none: No sound\n",
            "  - custom: Custom file\n",
            "- soundFile: ''\n",
            "  $showIf: {soundMode: custom}\n",
            "- mode: a\n",
            "  $options:\n",
            "  - a: A\n",
            "  - b: B\n",
            "  - c: C\n",
            "- excluded: ['']\n",
            "  $showIf: {mode: [a, c]}\n",
            "- shift: 0\n",
            "  $hideIf: {mode: b}\n",
            "- both: 0\n",
            "  $showIf: {soundMode: custom}\n",
            "  $hideIf: {mode: [b, c]}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[1].show_if,
            Some(vec![cond("soundMode", strings(&["custom"]))])
        );
        assert_eq!(
            items[3].show_if,
            Some(vec![cond("mode", strings(&["a", "c"]))])
        );
        assert_eq!(items[4].show_if, None);
        assert_eq!(items[4].hide_if, Some(vec![cond("mode", strings(&["b"]))]));
        assert_eq!(
            items[5].show_if,
            Some(vec![cond("soundMode", strings(&["custom"]))])
        );
        assert_eq!(
            items[5].hide_if,
            Some(vec![cond("mode", strings(&["b", "c"]))])
        );
    }

    #[test]
    fn several_entries_in_one_map_are_kept_in_declaration_order() {
        let src = settings_src(concat!(
            "- a: true\n",
            "- b: 1\n",
            "- c: 1\n",
            "  $showIf: {b: [1, 2], a: true}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[2].show_if,
            Some(vec![cond("b", numbers(&[1, 2])), cond("a", bools(&[true]))])
        );
    }

    #[test]
    fn an_integer_or_an_empty_string_may_be_the_gate() {
        // explorer-treeitem-tweaker: a numbered mode; taskbar-auto-hide-per-
        // monitor: a setting read only while another is blank.
        let src = settings_src(concat!(
            "- widthMode: 0\n",
            "- widthForAll: 100\n",
            "  $showIf: {widthMode: 1}\n",
            "- monitorInterfaceName: ''\n",
            "- monitor: 1\n",
            "  $showIf: {monitorInterfaceName: ''}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[1].show_if,
            Some(vec![cond("widthMode", numbers(&[1]))])
        );
        assert_eq!(
            items[3].show_if,
            Some(vec![cond("monitorInterfaceName", strings(&[""]))])
        );
    }

    #[test]
    fn a_reference_resolves_in_the_items_own_list_first_then_outward() {
        // mouse-button-remap: copy-pasted groups, each gating on its own
        // `enabled`; disk-usage-bar-customizer: a nested group naming a gate
        // one level up, and a group gated from another group by a full path.
        let src = settings_src(concat!(
            "- rendering:\n",
            "  - useVisualStyles: false\n",
            "- xbutton1:\n",
            "  - enabled: false\n",
            "  - key: a\n",
            "    $showIf: {enabled: true}\n",
            "- xbutton2:\n",
            "  - enabled: false\n",
            "  - key: b\n",
            "    $showIf: {enabled: true}\n",
            "- customRendering:\n",
            "  - renderBarBorder: true\n",
            "  - lightModeColors:\n",
            "    - barBorderColor: BCBCBC\n",
            "      $showIf: {renderBarBorder: true}\n",
            "  $showIf: {rendering.useVisualStyles: false}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            group(&items[1])[1].show_if,
            Some(vec![cond("xbutton1.enabled", bools(&[true]))])
        );
        assert_eq!(
            group(&items[2])[1].show_if,
            Some(vec![cond("xbutton2.enabled", bools(&[true]))])
        );
        let custom = &items[3];
        assert_eq!(
            custom.show_if,
            Some(vec![cond("rendering.useVisualStyles", bools(&[false]))])
        );
        assert_eq!(
            group(&group(custom)[1])[0].show_if,
            Some(vec![cond(
                "customRendering.renderBarBorder",
                bools(&[true])
            )])
        );
    }

    #[test]
    fn a_nearer_declaration_shadows_a_root_one() {
        // The rule of any scoped language: the first list declaring the name
        // wins, so a group with its own `enabled` gates on that one even when
        // the root declares an `enabled` too.
        let src = settings_src(concat!(
            "- enabled: true\n",
            "- g:\n",
            "  - enabled: false\n",
            "  - a: 1\n",
            "    $showIf: {enabled: true}\n",
            "  - b: 1\n",
            "    $showIf: {g.enabled: true}\n",
            "- h:\n",
            "  - a: 1\n",
            "    $showIf: {enabled: true}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            group(&items[1])[1].show_if,
            Some(vec![cond("g.enabled", bools(&[true]))])
        );
        // The full path from the root reaches the same one.
        assert_eq!(
            group(&items[1])[2].show_if,
            Some(vec![cond("g.enabled", bools(&[true]))])
        );
        // A group without its own reaches the root's.
        assert_eq!(
            group(&items[2])[0].show_if,
            Some(vec![cond("enabled", bools(&[true]))])
        );
    }

    #[test]
    fn a_row_item_names_its_row_siblings_and_resolves_without_subscripts() {
        // keyboard-shortcut-actions: `args` per row, shown for two actions;
        // explorer-force-details-columns: a row's `width` under its own
        // `force_width`. Every row is resolved, and each to the one path.
        let src = settings_src(concat!(
            "- rows:\n",
            "  - - action: nothing\n",
            "      $options:\n",
            "      - nothing: Nothing\n",
            "      - keypress: Key press\n",
            "      - start: Start\n",
            "    - args: ''\n",
            "      $showIf: {action: [keypress, start]}\n",
            "    - sub:\n",
            "      - width: 0\n",
            "        $showIf: {action: start}\n",
            "  - - action: start\n",
            "    - args: x\n",
            "      $showIf: {action: [keypress, start]}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        let rows = rows(&items[0]);
        assert_eq!(
            rows[0][1].show_if,
            Some(vec![cond("rows.action", strings(&["keypress", "start"]))])
        );
        assert_eq!(
            group(&rows[0][2])[0].show_if,
            Some(vec![cond("rows.action", strings(&["start"]))])
        );
        assert_eq!(
            rows[1][1].show_if,
            Some(vec![cond("rows.action", strings(&["keypress", "start"]))])
        );
    }

    #[test]
    fn a_row_item_is_judged_through_the_template_row() {
        // The template row is the schema of every row, so a partial row's
        // reference reaches a sibling only the template declares, and a row
        // redeclaring the gate without its `$options` is held to them.
        let src = settings_src(concat!(
            "- rows:\n",
            "  - - action: nothing\n",
            "      $options:\n",
            "      - nothing: Nothing\n",
            "      - start: Start\n",
            "    - args: ''\n",
            "      $showIf: {action: start}\n",
            "  - - args: x\n",
            "      $showIf: {action: start}\n",
            "  - - action: start\n",
            "    - args: y\n",
            "      $hideIf: {action: nothing}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        let rows = rows(&items[0]);
        assert_eq!(
            rows[1][0].show_if,
            Some(vec![cond("rows.action", strings(&["start"]))])
        );
        assert_eq!(
            rows[2][1].hide_if,
            Some(vec![cond("rows.action", strings(&["nothing"]))])
        );
        let template = "- rows:\n  - - action: a\n      $options:\n      - a: A\n      - b: B\n    - args: ''\n    - g:\n      - x: 1\n";
        let err = settings_error(&format!(
            "{template}  - - action: b\n    - args: y\n      $showIf: {{action: z}}"
        ));
        assert!(
            err.contains("rows[1].args.$showIf") && err.contains("$options"),
            "got: {err}"
        );
        let err = settings_error(&format!(
            "{template}  - - args: y\n      $showIf: {{args: z}}"
        ));
        assert!(
            err.contains("rows[1].args.$showIf refers to itself"),
            "got: {err}"
        );
        let err = settings_error(&format!(
            "{template}  - - g:\n      - x: 2\n      $showIf: {{g.x: 1}}"
        ));
        assert!(
            err.contains("rows[1].g.$showIf refers to a setting under it"),
            "got: {err}"
        );
    }

    #[test]
    fn a_chain_and_a_dependent_declared_before_its_gate_resolve() {
        // mouse-button-remap: enabled -> dblEnabled -> dblKey: custom ->
        // dblCustomKey; taskbar-background-helper: the gate declared after.
        let src = settings_src(concat!(
            "- enabled: false\n",
            "- dblEnabled: false\n",
            "  $showIf: {enabled: true}\n",
            "- dblKey: none\n",
            "  $options:\n",
            "  - none: None\n",
            "  - custom: Custom\n",
            "  $showIf: {dblEnabled: true}\n",
            "- dblCustomKey: ''\n",
            "  $showIf: {dblKey: custom}\n",
            "- color: FFFFFF\n",
            "  $showIf: {accentColor: false}\n",
            "- accentColor: true",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[1].show_if,
            Some(vec![cond("enabled", bools(&[true]))])
        );
        assert_eq!(
            items[2].show_if,
            Some(vec![cond("dblEnabled", bools(&[true]))])
        );
        assert_eq!(
            items[3].show_if,
            Some(vec![cond("dblKey", strings(&["custom"]))])
        );
        assert_eq!(
            items[4].show_if,
            Some(vec![cond("accentColor", bools(&[false]))])
        );
    }

    #[test]
    fn a_dynamic_select_gate_takes_any_string() {
        // Its values are only known at runtime, beside or without a static
        // list.
        let src = settings_src(concat!(
            "- device: ''\n",
            "  $dynamicSelect: true\n",
            "- monitor: primary\n",
            "  $options:\n",
            "  - primary: Primary\n",
            "  - none: None\n",
            "  $dynamicSelect: true\n",
            "- a: 1\n",
            "  $showIf: {device: '{0.0.0.00000000}', monitor: DISPLAY2}",
        ));
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[2].show_if,
            Some(vec![
                cond("device", strings(&["{0.0.0.00000000}"])),
                cond("monitor", strings(&["DISPLAY2"])),
            ])
        );
    }

    #[test]
    fn conditions_do_not_reach_the_engine_flatten() {
        // An editor hint only: the flattened store is the one the same block
        // produces without the annotations.
        let gated = settings_src(
            "- enabled: true\n- delay: 100\n  $showIf: {enabled: true}\n- g:\n  - x: a\n    $hideIf: {enabled: false}",
        );
        let plain = settings_src("- enabled: true\n- delay: 100\n- g:\n  - x: a");
        assert_eq!(
            extract_initial_settings_for_engine(&gated, false).unwrap(),
            extract_initial_settings_for_engine(&plain, false).unwrap()
        );
    }

    #[test]
    fn a_condition_map_must_be_a_non_empty_map_of_references() {
        for (yaml, cause) in [
            (
                "- a: 1\n  $showIf: x",
                "instance[0].$showIf must be a non-empty map",
            ),
            (
                "- a: 1\n  $showIf: [b]",
                "instance[0].$showIf must be a non-empty map",
            ),
            (
                "- a: 1\n  $hideIf: {}",
                "instance[0].$hideIf must be a non-empty map",
            ),
            (
                "- a: 1\n  $showIf: {'b c': 1}",
                "instance[0].$showIf key 'b c' is not a setting reference",
            ),
            (
                "- a: 1\n  $showIf: {'b.': 1}",
                "instance[0].$showIf key 'b.' is not a setting reference",
            ),
            (
                "- a: 1\n  $showIf: {'': 1}",
                "instance[0].$showIf key '' is not a setting reference",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
    }

    #[test]
    fn a_condition_value_is_a_scalar_or_a_list_of_one_kind() {
        const CAUSE: &str = "instance[0].$showIf value for 'b' must be a boolean, an integer, a string, or a list of one of them";
        for value in ["[]", "~", "{c: 1}", "[true, 1]", "[a, 1]", "[[a]]"] {
            assert_eq!(
                settings_error(&format!("- a: 1\n  $showIf: {{b: {value}}}")),
                format!("Failed to parse settings: {CAUSE}"),
                "{value}"
            );
        }
        // A number takes the int32 rule at its own position, listed or not.
        assert_eq!(
            settings_error("- a: 1\n  $showIf: {b: 1.5}"),
            "Failed to parse settings: instance[0].$showIf.b must be an integer; \
             floating-point numbers are not supported"
        );
        assert_eq!(
            settings_error("- a: 1\n  $showIf: {b: [1, 3000000000]}"),
            "Failed to parse settings: instance[0].$showIf.b[1] must be a 32-bit integer; \
             3000000000 is out of range"
        );
    }

    #[test]
    fn a_reference_must_name_a_setting_in_scope() {
        for (yaml, cause) in [
            // Nothing declares it.
            (
                "- a: 1\n  $showIf: {b: 1}",
                "a.$showIf refers to 'b', which is not a setting in scope",
            ),
            // A member of a group is not in the root's scope.
            (
                "- g:\n  - b: 1\n- a: 1\n  $showIf: {b: 1}",
                "a.$showIf refers to 'b', which is not a setting in scope",
            ),
            // A path through a scalar.
            (
                "- b: 1\n- a: 1\n  $hideIf: {b.c: 1}",
                "a.$hideIf refers to 'b.c', which is not a setting in scope",
            ),
            // A path into an array, from outside it.
            (
                "- rows:\n  - - b: 1\n- a: 1\n  $showIf: {rows.b: 1}",
                "a.$showIf refers to 'rows.b', which is inside an array",
            ),
            // The row's own siblings are reached by name, not by path.
            (
                "- rows:\n  - - b: 1\n    - a: 1\n      $showIf: {rows.b: 1}",
                "rows[0].a.$showIf refers to 'rows.b', which is inside an array",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
    }

    #[test]
    fn a_gate_must_hold_one_comparable_value() {
        for (target, reference) in [
            ("g:\n  - b: 1", "g"),
            ("rows:\n  - - b: 1", "rows"),
            ("list: [1, 2]", "list"),
            ("names: [a, b]", "names"),
            ("opacity: 0.5\n  $float: true", "opacity"),
        ] {
            assert_eq!(
                settings_error(&format!(
                    "- {target}\n- a: 1\n  $showIf: {{{reference}: 1}}"
                )),
                format!(
                    "Failed to parse settings: a.$showIf refers to '{reference}', \
                     which must be a boolean, number or string setting"
                ),
                "{target}"
            );
        }
    }

    #[test]
    fn a_condition_value_must_be_of_the_gates_kind() {
        for (gate, value, kind) in [
            ("b: true", "1", "a boolean"),
            ("b: true", "x", "a boolean"),
            ("b: 1", "true", "an integer"),
            ("b: 1", "'1'", "an integer"),
            ("b: x", "1", "a string"),
            ("b: x", "[true, false]", "a string"),
        ] {
            assert_eq!(
                settings_error(&format!("- {gate}\n- a: 1\n  $showIf: {{b: {value}}}")),
                format!("Failed to parse settings: a.$showIf value for 'b' must be {kind}"),
                "{gate} / {value}"
            );
        }
    }

    #[test]
    fn a_condition_value_on_an_options_gate_must_be_one_of_them() {
        // A typo would otherwise hide the setting forever, silently. The
        // values are language-invariant, so the check is against the one list.
        let src = concat!(
            "- b: x\n",
            "  $options:\n",
            "  - x: X\n",
            "  - y: Y\n",
            "  $options:fr:\n",
            "  - y: Y\n",
            "  - x: X\n",
            "- a: 1\n",
            "  $showIf: {b: [x, z]}",
        );
        assert_eq!(
            settings_error(src),
            "Failed to parse settings: a.$showIf value 'z' for 'b' is not one of its $options"
        );
    }

    #[test]
    fn a_reference_to_the_item_itself_or_under_it_is_rejected() {
        for (yaml, cause) in [
            ("- a: 1\n  $showIf: {a: 1}", "a.$showIf refers to itself"),
            (
                "- g:\n  - a: 1\n    $showIf: {g.a: 1}",
                "g.a.$showIf refers to itself",
            ),
            (
                "- g:\n  - b: 1\n  $showIf: {g.b: 1}",
                "g.$showIf refers to a setting under it",
            ),
            (
                "- g:\n  - h:\n    - b: 1\n  $hideIf: {g.h.b: 1}",
                "g.$hideIf refers to a setting under it",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        // The spelling a group-collapse sugar would take is one the rules
        // above reject: the group's own member is not in the group's scope.
        assert_eq!(
            settings_error("- g:\n  - enabled: true\n  - b: 1\n  $showIf: {enabled: true}"),
            "Failed to parse settings: g.$showIf refers to 'enabled', which is not a setting in scope"
        );
    }

    #[test]
    fn one_setting_named_twice_in_a_map_is_rejected() {
        // Two spellings of one setting cannot both hold unless they agree, and
        // the wire carries one entry per setting.
        assert_eq!(
            settings_error("- g:\n  - b: 1\n  - a: 1\n    $showIf: {b: 1, g.b: 2}"),
            "Failed to parse settings: g.a.$showIf names 'g.b' more than once"
        );
    }

    #[test]
    fn a_cycle_among_the_conditions_is_rejected() {
        for (yaml, cause) in [
            // Two items naming each other.
            (
                "- a: 1\n  $showIf: {b: 1}\n- b: 1\n  $showIf: {a: 1}",
                "a.$showIf forms a cycle through 'b'",
            ),
            // Through a `$hideIf`, three long.
            (
                "- a: 1\n  $showIf: {b: 1}\n- b: 1\n  $hideIf: {c: 1}\n- c: 1\n  $showIf: {a: 1}",
                "a.$showIf forms a cycle through 'b'",
            ),
            // Through a parent: a group gated on a setting that is gated on
            // the group's own member, which is visible only while the group
            // is.
            (
                "- g:\n  - m: 1\n  $showIf: {x: 1}\n- x: 1\n  $showIf: {g.m: 1}",
                "g.$showIf forms a cycle through 'x'",
            ),
            // Inside one row of an array.
            (
                "- rows:\n  - - a: 1\n      $showIf: {b: 1}\n    - b: 1\n      $showIf: {a: 1}",
                "rows[0].a.$showIf forms a cycle through 'rows[0].b'",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        // A dependency shared by two items is not a cycle.
        let src = settings_src(
            "- x: true\n- a: 1\n  $showIf: {x: true}\n- b: 1\n  $showIf: {x: true, a: 1}",
        );
        assert!(extract_initial_settings(&src, "en").is_ok());
    }

    /// Ten thousand items, each gated on the one declared after it, so the
    /// walk from the first reaches every other before it closes any. On a
    /// stack a walk that descended a frame per link would run out of: the
    /// nesting bound does not cap a chain.
    #[test]
    fn a_chain_as_long_as_the_block_is_walked() {
        const LEN: usize = 10_000;
        let yaml = (0..LEN)
            .map(|i| {
                if i + 1 < LEN {
                    format!("- a{i}: 1\n  $showIf: {{a{}: 1}}\n", i + 1)
                } else {
                    format!("- a{i}: 1")
                }
            })
            .collect::<String>();
        let src = settings_src(&yaml);
        let items = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || extract_initial_settings(&src, "en"))
            .unwrap()
            .join()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(items.len(), LEN);
        assert_eq!(items[0].show_if, Some(vec![cond("a1", numbers(&[1]))]));
    }

    // The `#!` marker: the spelling of the annotations that Windhawk 1.7.3
    // reads as a comment. A marked block reads exactly as its bare twin, and
    // is also read as 1.7.3 reads it, so a marker anywhere it does not belong
    // is an error.
    #[test]
    fn the_marked_spelling_reads_as_the_bare_one() {
        let bare = settings_src(concat!(
            "- accentColor: \"#3399FF\"\n",
            "  $name: Accent color\n",
            "  $format: colorRgb\n",
            "- opacity: \"0.85\"\n",
            "  $name: Opacity\n",
            "  $float: true\n",
            "  $min: 0\n",
            "  $max: 1\n",
            "- monitor: primary\n",
            "  $name: Monitor\n",
            "  $options:\n",
            "  - primary: Primary monitor\n",
            "  - secondary: Secondary monitor\n",
            "  $dynamicSelect: true\n",
            "- fadeEnabled: true\n",
            "- fadeDelay: 100\n",
            "  $showIf:\n",
            "    fadeEnabled: true\n",
            "- group:\n",
            "  - scale: 1\n",
            "    $float: true\n",
            "- rows:\n",
            "  - - action: a\n",
            "      $options:\n",
            "      - a: A\n",
            "      - b: B\n",
            "    - args: ''\n",
            "      $showIf: {action: [a, b]}",
        ));
        let marked = settings_src(concat!(
            "- accentColor: \"#3399FF\"\n",
            "  $name: Accent color\n",
            "  #! $format: colorRgb\n",
            "- opacity: \"0.85\"\n",
            "  $name: Opacity\n",
            "  #! $float: true\n",
            "  #! $min: 0\n",
            "  #! $max: 1\n",
            "- monitor: primary\n",
            "  $name: Monitor\n",
            "  #! $options:\n",
            "  #! - primary: Primary monitor\n",
            "  #! - secondary: Secondary monitor\n",
            "  #! $dynamicSelect: true\n",
            "- fadeEnabled: true\n",
            "- fadeDelay: 100\n",
            "  #! $showIf:\n",
            "  #!   fadeEnabled: true\n",
            "- group:\n",
            "  - scale: 1\n",
            "    #! $float: true\n",
            "- rows:\n",
            "  - - action: a\n",
            "      $options:\n",
            "      - a: A\n",
            "      - b: B\n",
            "    - args: ''\n",
            "      #! $showIf: {action: [a, b]}",
        ));
        let items = extract_initial_settings(&bare, "en").unwrap().unwrap();
        assert_eq!(
            extract_initial_settings(&marked, "en").unwrap().unwrap(),
            items
        );
        assert_eq!(
            extract_initial_settings_for_engine(&marked, false).unwrap(),
            extract_initial_settings_for_engine(&bare, false).unwrap()
        );
        // The annotations really are there to compare.
        assert_eq!(items[0].format.as_deref(), Some("colorRgb"));
        assert!(items[1].float && items[1].min.is_some() && items[1].max.is_some());
        assert!(items[2].dynamic_select && items[2].options.is_some());
        assert!(items[4].show_if.is_some());
        assert!(group(&items[5])[0].float);
        assert!(rows(&items[6])[0][1].show_if.is_some());
    }

    #[test]
    fn a_marked_continuation_may_carry_its_marker_at_either_indent() {
        let at_item = settings_src("- b: true\n- a: 1\n  #! $showIf:\n  #!   b: true");
        let at_own = settings_src("- b: true\n- a: 1\n  #! $showIf:\n    #! b: true");
        let items = extract_initial_settings(&at_item, "en").unwrap().unwrap();
        assert_eq!(items[1].show_if, Some(vec![cond("b", bools(&[true]))]));
        assert_eq!(
            extract_initial_settings(&at_own, "en").unwrap().unwrap(),
            items
        );
    }

    #[test]
    fn options_may_be_marked_beside_a_marked_dynamic_select_or_left_bare() {
        // Hidden from 1.7.3 the setting is a free text field there, where any
        // runtime value can be typed; left bare, 1.7.3 draws the static
        // dropdown. The current core reads the item the same either way.
        let hidden = settings_src(
            "- monitor: primary\n  #! $options:\n  #! - primary: P\n  #! - none: N\n  #! $dynamicSelect: true",
        );
        let shown = settings_src(
            "- monitor: primary\n  $options:\n  - primary: P\n  - none: N\n  #! $dynamicSelect: true",
        );
        let items = extract_initial_settings(&hidden, "en").unwrap().unwrap();
        assert!(items[0].dynamic_select);
        assert_eq!(items[0].options.as_ref().map(Vec::len), Some(2));
        assert_eq!(
            extract_initial_settings(&shown, "en").unwrap().unwrap(),
            items
        );
        // Beside a BARE `$dynamicSelect` the block no longer installs on
        // 1.7.3, whatever else is marked.
        assert_eq!(
            settings_error(
                "- monitor: primary\n  #! $options:\n  #! - primary: P\n  #! - none: N\n  $dynamicSelect: true"
            ),
            "Failed to parse settings: instance[0].$dynamicSelect is not an allowed property \
             for Windhawk 1.7.3; mark it, or mark nothing in this block"
        );
    }

    #[test]
    fn a_marker_on_a_key_1_7_3_accepts_is_rejected() {
        for (yaml, key) in [
            ("- a: x\n  #! $name: A", "$name"),
            ("- a: x\n  $name: A\n  #! $name:fr: B", "$name:fr"),
            ("- a: x\n  #! $description: D", "$description"),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!(
                    "Failed to parse settings: instance[0].{key} needs no marker: Windhawk 1.7.3 accepts it"
                ),
                "{yaml}"
            );
        }
        // `$options` too, unless the item is a dynamic select.
        for yaml in [
            "- a: x\n  #! $options:\n  #! - x: X\n  #! - y: Y",
            "- a: x\n  #! $options:\n  #! - x: X\n  #! - y: Y\n  #! $dynamicSelect: false",
            "- a: x\n  #! $options:\n  #! - x: X\n  #! - y: Y\n  #! $format: colorRgb",
        ] {
            assert_eq!(
                settings_error(yaml),
                "Failed to parse settings: instance[0].$options may be marked only beside \
                 $dynamicSelect: Windhawk 1.7.3 accepts $options",
                "{yaml}"
            );
        }
        // The language variants of `$options` go together.
        assert_eq!(
            settings_error(
                "- a: x\n  #! $options:\n  #! - x: X\n  #! - y: Y\n  $options:fr:\n  - x: XF\n  - y: YF\n  #! $dynamicSelect: true"
            ),
            "Failed to parse settings: instance[0]: $options and its language variants must be marked together"
        );
    }

    #[test]
    fn a_marked_underscored_key_is_ignored_like_a_bare_one() {
        let src = settings_src("- a: x\n  #! $_new: 1\n  #! $format: colorRgb");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].format.as_deref(), Some("colorRgb"));
        assert_eq!(items[0].name, None);
    }

    #[test]
    fn a_misspelled_key_behind_a_marker_fails_as_bare() {
        // The marker un-comments the line; what it says is then judged by the
        // ordinary grammar, so a typo is loud rather than a silent comment.
        assert_eq!(
            settings_error("- a: x\n  #! $fromat: colorRgb"),
            "Failed to parse settings: instance[0].$fromat is not an allowed property"
        );
    }

    #[test]
    fn a_marker_anywhere_holds_the_whole_block_to_1_7_3() {
        // A bare `$showIf` beside a marked `$format` fails 1.7.3's parse
        // regardless of the marker, so the block is rejected here.
        assert_eq!(
            settings_error("- b: true\n- a: x\n  #! $format: colorRgb\n  $showIf: {b: true}"),
            "Failed to parse settings: instance[1].$showIf is not an allowed property \
             for Windhawk 1.7.3; mark it, or mark nothing in this block"
        );
        // Nested items are held to it too.
        assert_eq!(
            settings_error("- g:\n  - a: 1\n    $min: 0\n- b: x\n  #! $format: colorRgb"),
            "Failed to parse settings: instance[0].g[0].$min is not an allowed property \
             for Windhawk 1.7.3; mark it, or mark nothing in this block"
        );
        assert_eq!(
            settings_error("- rows:\n  - - a: 1\n      $max: 9\n- b: x\n  #! $format: colorRgb"),
            "Failed to parse settings: instance[0].rows[0][0].$max is not an allowed property \
             for Windhawk 1.7.3; mark it, or mark nothing in this block"
        );
    }

    #[test]
    fn a_key_written_both_ways_is_a_duplicate_key() {
        let err = settings_error("- a: x\n  $format: colorRgb\n  #! $format: colorArgb");
        assert!(
            err.starts_with("Failed to parse settings:") && !err.contains(LEGACY_READING),
            "the full reading's loader rejects it: {err}"
        );
    }

    #[test]
    fn a_marker_may_only_add_an_annotation() {
        for (yaml, cause) in [
            // A whole item.
            (
                "- a: 1\n#! - b: 2",
                "instance[1] (b) exists only with the markers; a settings item may not be marked",
            ),
            // A second setting key on an item.
            (
                "- a: 1\n  #! b: 2",
                "instance[0].b exists only with the markers; a settings item may not be marked",
            ),
            // An item of a group.
            (
                "- g:\n  - a: 1\n  #! - b: 2",
                "instance[0].g[1] (b) exists only with the markers; a settings item may not be marked",
            ),
            // A row of an object array.
            (
                "- rows:\n  - - c: 1\n  #! - - c: 2",
                "instance[0].rows has 2 rows with the markers and 1 without",
            ),
            // An element of a value array.
            (
                "- list:\n  - 1\n  #! - 2",
                "instance[0].list reads differently with and without the markers; a marker may only add an annotation",
            ),
            // An entry of a bare `$options` list, with two left for 1.7.3.
            (
                "- a: x\n  $options:\n  - x: X\n  - y: Y\n  #! - z: Z",
                "instance[0].$options reads differently with and without the markers; a marker may only add an annotation",
            ),
            // A marked continuation of a plain scalar.
            (
                "- a: some text\n    #! more text",
                "instance[0].a reads differently with and without the markers; a marker may only add an annotation",
            ),
        ] {
            assert_eq!(
                settings_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        // An entry of a bare `$options` list with one left is 1.7.3's own
        // failure, reported as such.
        assert_eq!(
            settings_error("- a: x\n  $options:\n  - x: X\n  #! - y: Y"),
            "Failed to parse settings: as Windhawk 1.7.3 reads this block: \
             instance[0].$options must have at least two options"
        );
    }

    #[test]
    fn a_marked_line_inside_a_block_scalar_reads_differently() {
        // The pass sees text, not YAML, so it strips the marker there too;
        // the comparison then sees the scalar differ and rejects the block.
        for yaml in [
            "- a: x\n  $description: |\n    line\n    #! more",
            "- a: x\n  $description: >-\n    line\n    #! $format: colorRgb",
        ] {
            assert_eq!(
                settings_error(yaml),
                "Failed to parse settings: instance[0].$description reads differently \
                 with and without the markers; a marker may only add an annotation",
                "{yaml}"
            );
        }
    }

    #[test]
    fn a_marked_float_is_judged_by_1_7_3s_number_rule_too() {
        // With the annotation absent in the 1.7.3 reading, a float literal is
        // the plain number 1.7.3 would truncate, and is rejected as such;
        // a string holding it, or an integer, passes there.
        assert_eq!(
            settings_error("- opacity: 0.85\n  #! $float: true"),
            "Failed to parse settings: as Windhawk 1.7.3 reads this block: \
             instance[0].opacity must be an integer; floating-point numbers are not supported"
        );
        let src = settings_src("- scale: 1\n  #! $float: true");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::String("1".into()));
        assert!(items[0].float);
        let src = settings_src("- opacity: '0.85'\n  #! $float: true\n  #! $min: 0");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::String("0.85".into()));
        // A bare `$float` keeps taking the literal.
        let src = settings_src("- opacity: 0.85\n  $float: true");
        assert!(extract_initial_settings(&src, "en").is_ok());
    }

    #[test]
    fn the_full_readings_errors_come_first() {
        // A block that is simply wrong is reported as such before its
        // compatibility is judged: here the bare `$showIf` would fail the
        // 1.7.3 key check, but the misspelled marked key fails the grammar
        // first.
        assert_eq!(
            settings_error("- b: true\n- a: x\n  $showIf: {b: true}\n  #! $fromat: y"),
            "Failed to parse settings: instance[1].$fromat is not an allowed property"
        );
    }

    #[test]
    fn a_malformed_marker_is_rejected_with_its_line() {
        assert_eq!(
            settings_error("- a: x\n  #!$format: colorRgb"),
            "Failed to parse settings: the '#!' marker on line 2 must be followed by a space"
        );
        // A commented-out annotation, with a space after the `#`, stays the
        // comment it is.
        let src = settings_src("- a: x\n  # $format: colorRgb");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].format, None);
    }
}
