//! Key validation for `mod settings set` (the `commands/mod.ts` settings half).
//!
//! A key is a flat storage key in the engine's convention (scalar = `key`,
//! nested object = `parent.child`, scalar array = `parent[i]`, object array =
//! `parent[i].child`). [`resolve_setting_key_type`] is the authority: it walks a
//! mod's declared initial settings to type-check ONE key, accepting ANY array
//! index because Windhawk arrays are dynamic (the source declares a template;
//! the runtime array grows unboundedly). An object array's schema is its FIRST
//! declared group - the UI reads keys from that element alone, and the domain
//! rejects later groups that are not subsets of it, so the first element is the
//! authoritative shape at every index. [`flatten_setting_key_types`] enumerates
//! that first-group template for the "valid keys" hint shown when a key does not
//! resolve. Both keep the boolean-vs-number distinction the engine's own
//! flattening drops, so `set` can type-check input before it writes.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use windhawk_core_protocol::{InitialSettingItem, InitialSettings, InitialSettingsValue};

use crate::error::CliError;
use crate::validate::int_range::parse_int32_setting;

/// A flattened setting leaf's declared scalar type. The distinction drives input
/// validation in [`parse_setting_input`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingLeafType {
    Boolean,
    Number,
    String,
}

/// Flatten a mod's declared initial settings into a `{ flat-key -> leaf type }`
/// map (the TS `flattenSettingKeyTypes`). A `BTreeMap` so the valid-keys error
/// list comes out sorted.
pub fn flatten_setting_key_types(settings: &InitialSettings) -> BTreeMap<String, SettingLeafType> {
    let mut out = BTreeMap::new();
    flatten_items(settings, "", &mut out);
    out
}

fn flatten_items(
    settings: &[InitialSettingItem],
    prefix: &str,
    out: &mut BTreeMap<String, SettingLeafType>,
) {
    for item in settings {
        let key = if prefix.is_empty() {
            item.key.clone()
        } else {
            format!("{prefix}.{}", item.key)
        };
        flatten_value(&item.value, &key, out);
    }
}

fn flatten_value(
    value: &InitialSettingsValue,
    key: &str,
    out: &mut BTreeMap<String, SettingLeafType>,
) {
    match value {
        InitialSettingsValue::Bool(_) => {
            out.insert(key.to_owned(), SettingLeafType::Boolean);
        }
        InitialSettingsValue::Number(_) => {
            out.insert(key.to_owned(), SettingLeafType::Number);
        }
        InitialSettingsValue::String(_) => {
            out.insert(key.to_owned(), SettingLeafType::String);
        }
        // Scalar arrays: each index is a leaf of the element type (an empty array
        // carries no type info, so it contributes no keys).
        InitialSettingsValue::NumberArray(items) => {
            for i in 0..items.len() {
                out.insert(format!("{key}[{i}]"), SettingLeafType::Number);
            }
        }
        InitialSettingsValue::StringArray(items) => {
            for i in 0..items.len() {
                out.insert(format!("{key}[{i}]"), SettingLeafType::String);
            }
        }
        // A nested object: its leaves live at the current key's namespace.
        InitialSettingsValue::Settings(items) => {
            flatten_items(items, key, out);
        }
        // An object array: the schema is the FIRST declared group (later groups
        // are subset default rows), so enumerate that template once at `key[0]`.
        InitialSettingsValue::SettingsArray(groups) => {
            if let Some(first) = groups.first() {
                flatten_items(first, &format!("{key}[0]"), out);
            }
        }
    }
}

/// Resolve a flat storage key to its declared leaf type, tolerating ARBITRARY
/// array indices. Windhawk array settings are dynamic: a mod's source declares a
/// template, but the runtime array can hold any number of elements - the engine
/// stores/reads `key[0]`, `key[1]`, ... unboundedly. An object array's schema is
/// its FIRST declared group (the UI's schema; the domain guarantees later groups
/// are subsets of it), so every index resolves against that first group. So
/// `items[7].icon` is a valid settable key even when the source declares only
/// `items[0]`. Type-checking against the fixed set that
/// [`flatten_setting_key_types`] enumerates would wrongly reject every index
/// past the template; this walks the declared structure instead, accepting any
/// index at an array node and taking the leaf type from the first group.
///
/// Returns `None` when the key names no declared setting: an unknown base name,
/// an index into a non-array (or a missing index into an array), or a path that
/// stops above a scalar leaf.
pub fn resolve_setting_key_type(settings: &InitialSettings, key: &str) -> Option<SettingLeafType> {
    // `group` is the settings list the next segment resolves within; it narrows
    // as we descend into nested groups and array elements.
    let mut group = settings;
    let mut segments = key.split('.').peekable();
    while let Some(segment) = segments.next() {
        let (name, index) = parse_segment(segment)?;
        let item = group.iter().find(|it| it.key == name)?;
        let is_last = segments.peek().is_none();
        match &item.value {
            // Scalars: a leaf. Valid only as the final segment and without an
            // index (an index into a scalar is not a real key).
            InitialSettingsValue::Bool(_) => {
                return (is_last && index.is_none()).then_some(SettingLeafType::Boolean);
            }
            InitialSettingsValue::Number(_) => {
                return (is_last && index.is_none()).then_some(SettingLeafType::Number);
            }
            InitialSettingsValue::String(_) => {
                return (is_last && index.is_none()).then_some(SettingLeafType::String);
            }
            // Scalar arrays: `key[i]` is a leaf of the element type for any `i`.
            InitialSettingsValue::NumberArray(_) => {
                return (is_last && index.is_some()).then_some(SettingLeafType::Number);
            }
            InitialSettingsValue::StringArray(_) => {
                return (is_last && index.is_some()).then_some(SettingLeafType::String);
            }
            // Nested object: no index; descend into its group for the rest.
            InitialSettingsValue::Settings(inner) => {
                if is_last || index.is_some() {
                    return None;
                }
                group = inner;
            }
            // Object array: `key[i]` selects the schema for any `i` from the
            // FIRST declared group (the UI schema; later groups are subsets of
            // it); descend into that group for the rest of the path.
            InitialSettingsValue::SettingsArray(groups) => {
                let template = groups.first()?;
                if index.is_none() || is_last {
                    return None;
                }
                group = template;
            }
        }
    }
    // Every group/array segment above continues the loop, so reaching here means
    // the key named a non-leaf node (or was empty): not settable.
    None
}

/// Split one dotted key segment into its base name and optional array index:
/// `icon` -> (`icon`, None), `items[3]` -> (`items`, Some(3)). Returns None for a
/// malformed segment (empty name, missing/extra brackets, or a non-numeric
/// index), which the caller treats as an unknown key.
fn parse_segment(segment: &str) -> Option<(&str, Option<usize>)> {
    match segment.split_once('[') {
        None => (!segment.is_empty()).then_some((segment, None)),
        Some((name, rest)) => {
            let digits = rest.strip_suffix(']')?;
            if name.is_empty() || digits.is_empty() {
                return None;
            }
            Some((name, Some(digits.parse::<usize>().ok()?)))
        }
    }
}

/// Parse a raw CLI string into the JSON value to store for a setting of the
/// declared `ty` (the TS `parseSettingInput`). A boolean is normalized to the
/// number `1`/`0` the engine stores; a number is range-checked; a string is
/// stored verbatim.
pub fn parse_setting_input(key: &str, ty: SettingLeafType, raw: &str) -> Result<Value, CliError> {
    match ty {
        SettingLeafType::Boolean => match raw {
            "true" | "1" => Ok(json!(1)),
            "false" | "0" => Ok(json!(0)),
            _ => Err(CliError::usage(format!(
                "Setting '{key}' is declared as boolean; value must be one of true/false/1/0, got '{raw}'."
            ))),
        },
        SettingLeafType::Number => Ok(json!(parse_int32_setting(key, raw)?)),
        SettingLeafType::String => Ok(Value::String(raw.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> InitialSettings {
        serde_json::from_str(source).expect("parse initial settings")
    }

    #[test]
    fn flattens_scalars_and_nested_objects_and_arrays() {
        // A scalar boolean, a scalar number, a scalar string, a string array, a
        // nested object, and an array of objects.
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "count", "value": 3},
                {"key": "label", "value": "hi"},
                {"key": "names", "value": ["a", "b"]},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "items", "value": [[{"key": "name", "value": "x"}]]}
            ]"#,
        );
        let flat = flatten_setting_key_types(&settings);

        assert_eq!(flat["flag"], SettingLeafType::Boolean);
        assert_eq!(flat["count"], SettingLeafType::Number);
        assert_eq!(flat["label"], SettingLeafType::String);
        assert_eq!(flat["names[0]"], SettingLeafType::String);
        assert_eq!(flat["names[1]"], SettingLeafType::String);
        assert_eq!(flat["group.inner"], SettingLeafType::Number);
        assert_eq!(flat["items[0].name"], SettingLeafType::String);
    }

    #[test]
    fn empty_arrays_contribute_no_keys() {
        let settings = parse(r#"[{"key": "empty", "value": []}]"#);
        assert!(flatten_setting_key_types(&settings).is_empty());
    }

    #[test]
    fn resolves_scalars_nested_and_arbitrary_array_indices() {
        // A scalar, a nested group, a scalar array, and an object array declared
        // with a single template element (the common shape, e.g. tray `items`).
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "names", "value": ["a"]},
                {"key": "items", "value": [[
                    {"key": "action", "value": 0},
                    {"key": "icon", "value": ""},
                    {"key": "label", "value": "x"}
                ]]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);

        assert_eq!(r("flag"), Some(SettingLeafType::Boolean));
        assert_eq!(r("group.inner"), Some(SettingLeafType::Number));

        // A scalar array accepts any index past the one declared element.
        assert_eq!(r("names[0]"), Some(SettingLeafType::String));
        assert_eq!(r("names[9]"), Some(SettingLeafType::String));

        // The reported bug: an object-array child at an index the template does
        // not literally declare still resolves to the template's leaf type.
        assert_eq!(r("items[0].icon"), Some(SettingLeafType::String));
        assert_eq!(r("items[1].icon"), Some(SettingLeafType::String));
        assert_eq!(r("items[42].action"), Some(SettingLeafType::Number));
    }

    #[test]
    fn object_array_resolves_against_the_first_group_at_any_index() {
        // A multi-group object array (the first group is the full template, the
        // second a subset default row). Every index - and every template key,
        // even one absent from the subset row - resolves against the first group.
        let settings = parse(
            r#"[
                {"key": "buttons", "value": [
                    [{"key": "preset", "value": "custom"}, {"key": "name", "value": ""}],
                    [{"key": "preset", "value": "settings"}]
                ]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);
        assert_eq!(r("buttons[0].preset"), Some(SettingLeafType::String));
        assert_eq!(r("buttons[1].name"), Some(SettingLeafType::String));
        assert_eq!(r("buttons[9].name"), Some(SettingLeafType::String));
    }

    #[test]
    fn rejects_non_leaf_and_malformed_keys() {
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "names", "value": ["a"]},
                {"key": "items", "value": [[{"key": "icon", "value": ""}]]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);

        assert_eq!(r("nope"), None); // unknown base name
        assert_eq!(r("flag[0]"), None); // index into a scalar
        assert_eq!(r("group"), None); // stops above a group
        assert_eq!(r("names"), None); // scalar array needs an index
        assert_eq!(r("items"), None); // object array needs an index
        assert_eq!(r("items[0]"), None); // object-array element is not a leaf
        assert_eq!(r("items[0].nope"), None); // unknown child of the template
        assert_eq!(r("items[].icon"), None); // empty index
        assert_eq!(r("items[x].icon"), None); // non-numeric index
    }

    #[test]
    fn parses_typed_input() {
        assert_eq!(
            parse_setting_input("k", SettingLeafType::Boolean, "true").unwrap(),
            json!(1)
        );
        assert_eq!(
            parse_setting_input("k", SettingLeafType::Boolean, "0").unwrap(),
            json!(0)
        );
        assert_eq!(
            parse_setting_input("k", SettingLeafType::Number, "42").unwrap(),
            json!(42)
        );
        assert_eq!(
            parse_setting_input("k", SettingLeafType::String, "raw").unwrap(),
            json!("raw")
        );
    }

    #[test]
    fn rejects_bad_typed_input() {
        let err = parse_setting_input("k", SettingLeafType::Boolean, "yes").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.message().contains("declared as boolean"));

        let err = parse_setting_input("k", SettingLeafType::Number, "1.5").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }
}
