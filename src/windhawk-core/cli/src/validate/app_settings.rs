//! The dotted-key AppSettings schema and the `app settings set` value parsing /
//! patch builder (`commands/app.ts`).
//!
//! The schema is the single source of truth for which keys `app settings`
//! accepts and each key's value type. `get` lists and validates against it;
//! `set` additionally parses the raw value to its type and folds it into the
//! `AppSettings` patch shape (`engine.<x>` keys nest under the engine
//! sub-object; everything else is top-level).

use serde_json::{Value, json};

use crate::error::CliError;
use crate::validate::int_range::parse_int32_setting;

/// The value type of an app setting, governing how `set` parses the raw string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldType {
    String,
    Boolean,
    Number,
    StringArray,
}

/// Dotted-key schema for AppSettings: top-level names for the flat settings,
/// `engine.<x>` for fields nested under the engine sub-object. The order is
/// presentational only - `get`'s all-listing sorts.
pub const APP_SETTINGS_SCHEMA: &[(&str, FieldType)] = &[
    ("language", FieldType::String),
    ("disableUpdateCheck", FieldType::Boolean),
    ("disableRunUIScheduledTask", FieldType::Boolean),
    ("devModeOptOut", FieldType::Boolean),
    ("hideTrayIcon", FieldType::Boolean),
    ("alwaysCompileModsLocally", FieldType::Boolean),
    ("dontAutoShowToolkit", FieldType::Boolean),
    ("modTasksDialogDelay", FieldType::Number),
    ("safeMode", FieldType::Boolean),
    ("loggingVerbosity", FieldType::Number),
    ("engine.loggingVerbosity", FieldType::Number),
    ("engine.include", FieldType::StringArray),
    ("engine.exclude", FieldType::StringArray),
    ("engine.injectIntoCriticalProcesses", FieldType::Boolean),
    ("engine.injectIntoIncompatiblePrograms", FieldType::Boolean),
    ("engine.injectIntoGames", FieldType::Boolean),
];

/// The declared type of `key`, or `None` if it is not an app setting.
pub fn field_type(key: &str) -> Option<FieldType> {
    APP_SETTINGS_SCHEMA
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, t)| *t)
}

/// Parse a raw CLI string into the typed JSON value for `key` per its schema
/// type (the TS `parseValue`).
#[track_caller]
pub fn parse_value(key: &str, ty: FieldType, raw: &str) -> Result<Value, CliError> {
    match ty {
        FieldType::String => Ok(Value::String(raw.to_owned())),
        FieldType::Boolean => match raw {
            "true" | "1" => Ok(Value::Bool(true)),
            "false" | "0" => Ok(Value::Bool(false)),
            _ => Err(CliError::usage(format!(
                "Setting '{key}' is boolean; value must be one of true/false/1/0, got '{raw}'."
            ))),
        },
        FieldType::Number => Ok(json!(parse_int32_setting(key, raw)?)),
        // Comma-separated, each item trimmed so the `a, b` form printed by `get`
        // round-trips; an empty string clears the list.
        FieldType::StringArray => Ok(if raw.is_empty() {
            json!([])
        } else {
            Value::Array(
                raw.split(',')
                    .map(|item| Value::String(item.trim().to_owned()))
                    .collect(),
            )
        }),
    }
}

/// Build the `AppSettings` patch object for a single dotted key (the TS
/// `buildPatch`). Only `engine.<x>` nests; everything else is top-level.
pub fn build_patch(key: &str, value: Value) -> Value {
    if let Some(engine_key) = key.strip_prefix("engine.") {
        json!({ "engine": { engine_key: value } })
    } else {
        json!({ key: value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_type_lookup() {
        assert_eq!(field_type("language"), Some(FieldType::String));
        assert_eq!(field_type("safeMode"), Some(FieldType::Boolean));
        assert_eq!(field_type("modTasksDialogDelay"), Some(FieldType::Number));
        assert_eq!(field_type("engine.include"), Some(FieldType::StringArray));
        assert_eq!(
            field_type("engine.injectIntoGames"),
            Some(FieldType::Boolean)
        );
        assert_eq!(field_type("nope"), None);
    }

    #[test]
    fn parses_each_type() {
        assert_eq!(
            parse_value("language", FieldType::String, "fr").unwrap(),
            json!("fr")
        );
        assert_eq!(
            parse_value("safeMode", FieldType::Boolean, "1").unwrap(),
            json!(true)
        );
        assert_eq!(
            parse_value("loggingVerbosity", FieldType::Number, "2").unwrap(),
            json!(2)
        );
        assert_eq!(
            parse_value("engine.include", FieldType::StringArray, "a, b ,c").unwrap(),
            json!(["a", "b", "c"])
        );
        assert_eq!(
            parse_value("engine.include", FieldType::StringArray, "").unwrap(),
            json!([])
        );
    }

    #[test]
    fn rejects_bad_boolean() {
        let err = parse_value("safeMode", FieldType::Boolean, "maybe").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.message().contains("is boolean"));
    }

    #[test]
    fn build_patch_nests_engine_keys_only() {
        assert_eq!(
            build_patch("safeMode", json!(true)),
            json!({ "safeMode": true })
        );
        assert_eq!(
            build_patch("engine.injectIntoGames", json!(false)),
            json!({ "engine": { "injectIntoGames": false } })
        );
    }
}
