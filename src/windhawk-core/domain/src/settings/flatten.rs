//! The engine flattener: the `extractInitialSettingsForEngine` pass that turns
//! the validated/transformed settings tree into a flat name->value list
//! (`parseSettings` / `parseSettingsValue` of the TS implementation). Runs on
//! the typed `SettingItem` tree, so it has no defensive arms beyond the leaf
//! match.

use crate::model::{EngineSettingValue, SettingItem, SettingValue};

/// `parseSettings(settings, keyPrefix)`: flatten each item under `prefix`. A
/// top-level call passes an empty prefix (the key is the item key); a nested
/// call passes the parent path (the child key is `<prefix>.<key>`).
pub(super) fn flatten_settings(
    items: &[SettingItem],
    prefix: &str,
    out: &mut Vec<(String, EngineSettingValue)>,
) {
    for item in items {
        let key = if prefix.is_empty() {
            item.key.clone()
        } else {
            format!("{prefix}.{}", item.key)
        };
        flatten_value(&item.value, &key, out);
    }
}

/// `parseSettingsValue(value, key)`: scalars are stored at `key`; an array of
/// scalars at `key[i]`; a nested settings array recurses with `key` as the
/// prefix; an array of settings arrays recurses with `key[i]` as the prefix.
fn flatten_value(value: &SettingValue, key: &str, out: &mut Vec<(String, EngineSettingValue)>) {
    match value {
        SettingValue::Bool(b) => out.push((key.to_owned(), EngineSettingValue::Int(i32::from(*b)))),
        SettingValue::Number(n) => out.push((key.to_owned(), number_to_engine(n))),
        SettingValue::String(s) => out.push((key.to_owned(), EngineSettingValue::Str(s.clone()))),
        SettingValue::NumberArray(ns) => {
            for (i, n) in ns.iter().enumerate() {
                out.push((format!("{key}[{i}]"), number_to_engine(n)));
            }
        }
        SettingValue::StringArray(ss) => {
            for (i, s) in ss.iter().enumerate() {
                out.push((format!("{key}[{i}]"), EngineSettingValue::Str(s.clone())));
            }
        }
        SettingValue::Settings(inner) => flatten_settings(inner, key, out),
        SettingValue::SettingsArray(arrays) => {
            for (i, inner) in arrays.iter().enumerate() {
                flatten_settings(inner, &format!("{key}[{i}]"), out);
            }
        }
    }
}

/// A validated settings number is an int32 (see `validate::validate_number`), so
/// the `as_i64` always fits; the fallback is defensive only.
fn number_to_engine(n: &serde_json::Number) -> EngineSettingValue {
    EngineSettingValue::Int(n.as_i64().unwrap_or(0) as i32)
}
