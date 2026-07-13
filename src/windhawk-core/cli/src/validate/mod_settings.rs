//! Flat-key validation for `mod settings set` (the `commands/mod.ts` settings
//! half).
//!
//! A mod's declared initial settings are flattened into a typed flat key space
//! matching the engine's storage-key convention (scalar = `key`, nested object
//! = `parent.child`, scalar array = `parent[0]`, object array =
//! `parent[0].child`). Unlike the engine's own flattening this keeps the
//! boolean-vs-number distinction so `set` can type-check input before it
//! writes.

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
        // An array of objects: each top-level index is a separate grouped
        // object; recurse into each at `key[i]`.
        InitialSettingsValue::SettingsArray(groups) => {
            for (i, group) in groups.iter().enumerate() {
                flatten_items(group, &format!("{key}[{i}]"), out);
            }
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
