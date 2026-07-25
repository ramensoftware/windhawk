//! `mod settings get`/`set`: read or write a mod's runtime settings, validated
//! against the types declared in the mod source's settings block.

use std::collections::BTreeSet;
use std::io::{self, Write};

use serde_json::{Map, Value, json};
use windhawk_core_protocol::{InitialSettings, ModIdParams, SetModSettingsParams};

use crate::Environment;
use crate::commands::parse::{parse_mod_source, reject_initial_settings_error};
use crate::commands::render::scalar_to_string;
use crate::commands::{app_settings, language};
use crate::error::CliError;
use crate::output::CommandResult;
use crate::validate::mod_settings::{
    flatten_setting_key_types, parse_setting_input, resolve_setting_key_type,
};

// ---------------------------------------------------------------------------
// mod settings get
// ---------------------------------------------------------------------------

pub(super) fn settings_get(
    env: &Environment,
    id: &str,
    key: Option<&str>,
) -> Result<Box<dyn CommandResult>, CliError> {
    // Existence check first (exit 4 if not installed), then the runtime settings.
    super::require_config(env, id)?;
    let settings: Map<String, Value> = env.core.invoke_as(
        "getModSettings",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;

    let Some(key) = key else {
        return Ok(Box::new(ModSettingsAllResult {
            id: id.to_owned(),
            settings,
        }));
    };

    // Absent key reads as null (the TS hasOwnProperty fallback).
    let value = settings.get(key).cloned().unwrap_or(Value::Null);
    Ok(Box::new(ModSettingsKeyResult {
        id: id.to_owned(),
        key: key.to_owned(),
        value,
    }))
}

/// Format a runtime setting value (mirror of the TS `formatSettingValue`): a
/// null (absent) is `<unset>`, scalars stringify.
fn format_setting_value(value: &Value) -> String {
    match value {
        Value::Null => "<unset>".to_owned(),
        other => scalar_to_string(other),
    }
}

struct ModSettingsAllResult {
    id: String,
    settings: Map<String, Value>,
}

impl CommandResult for ModSettingsAllResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "settings": self.settings })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let mut keys: Vec<&String> = self.settings.keys().collect();
        keys.sort();
        for key in keys {
            let v = self.settings.get(key).unwrap_or(&Value::Null);
            writeln!(out, "{key}={}", format_setting_value(v))?;
        }
        Ok(())
    }
}

struct ModSettingsKeyResult {
    id: String,
    key: String,
    value: Value,
}

impl CommandResult for ModSettingsKeyResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "key": self.key, "value": self.value })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.value.is_null() {
            // Blank line: scripts detect absence by empty stdout or by --json.
            writeln!(out)
        } else {
            writeln!(out, "{}", format_setting_value(&self.value))
        }
    }
}

// ---------------------------------------------------------------------------
// mod settings set
// ---------------------------------------------------------------------------

pub(super) fn settings_set(
    env: &Environment,
    id: &str,
    pairs: &[String],
) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings))?;

    // A malformed settings block in the stored source is a generic failure
    // (exit 1): the source was valid when installed, so a parse failure now is
    // an internal problem, not a usage error.
    reject_initial_settings_error(parsed.errors.initial_settings)?;
    let initial_settings = parsed.initial_settings.unwrap_or_default();
    if initial_settings.is_empty() {
        return Err(CliError::usage(format!(
            "Mod '{id}' declares no settings; there is nothing to set."
        )));
    }

    // Parse and type-check every pair BEFORE touching the store: a bad token,
    // an unknown key, a duplicate, or a type mismatch aborts the whole batch,
    // so a partial write can never leave some keys applied and others rejected.
    let mut typed: Vec<(String, Value)> = Vec::with_capacity(pairs.len());
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for pair in pairs {
        let (key, raw_value) = split_pair(pair)?;
        if !seen.insert(key) {
            return Err(CliError::usage(format!(
                "Key '{key}' is set more than once in this command."
            )));
        }
        let Some(declared_type) = resolve_setting_key_type(&initial_settings, key) else {
            return Err(unknown_key_error(id, key, &initial_settings));
        };
        typed.push((
            key.to_owned(),
            parse_setting_input(key, declared_type, raw_value)?,
        ));
    }

    let mut current: Map<String, Value> = env.core.invoke_as(
        "getModSettings",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;

    // Apply every pair into the map, recording each key's prior value for the
    // report, then write the whole map back in ONE call - setModSettings
    // replaces the section wholesale, so the batch lands atomically. No tray
    // notification: matches the extension's setModSettings IPC handler; the
    // engine picks up the change.
    let mut changes = Vec::with_capacity(typed.len());
    for (key, new_value) in typed {
        let previous_value = current.get(&key).cloned().unwrap_or(Value::Null);
        current.insert(key.clone(), new_value.clone());
        changes.push(SettingChange {
            key,
            value: new_value,
            previous_value,
        });
    }
    env.core.invoke(
        "setModSettings",
        &SetModSettingsParams {
            mod_id: id.to_owned(),
            settings: current,
        },
    )?;

    Ok(Box::new(ModSettingsSetResult {
        id: id.to_owned(),
        changes,
    }))
}

/// Build the usage error for a key that resolves to no declared setting. The
/// "valid keys" hint lists the declared template keys (an object array shows
/// `items[0].child`); a trailing note spells out that the `[0]` is only a
/// template so a reader does not read the list as "only index 0 is allowed".
fn unknown_key_error(id: &str, key: &str, initial_settings: &InitialSettings) -> CliError {
    let key_types = flatten_setting_key_types(initial_settings);
    let valid_keys = key_types.keys().cloned().collect::<Vec<_>>().join("\n  ");
    let note = if key_types.keys().any(|k| k.contains('[')) {
        "\nArray keys accept any index; [0] shows the declared template."
    } else {
        ""
    };
    CliError::usage(format!(
        "Key '{key}' is not a declared setting of mod '{id}'.\nValid keys:\n  {valid_keys}{note}"
    ))
}

/// Split a `key=value` token on the FIRST `=`: a flat key never contains `=`,
/// but a string value can. An absent `=` or an empty key is a usage error.
fn split_pair(pair: &str) -> Result<(&str, &str), CliError> {
    match pair.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key, value)),
        Some(_) => Err(CliError::usage(format!(
            "Invalid setting '{pair}': the key before '=' is empty."
        ))),
        None => Err(CliError::usage(format!(
            "Invalid setting '{pair}': expected key=value."
        ))),
    }
}

/// One key's before/after transition, as applied by a `mod settings set`.
struct SettingChange {
    key: String,
    value: Value,
    previous_value: Value,
}

struct ModSettingsSetResult {
    id: String,
    changes: Vec<SettingChange>,
}

impl CommandResult for ModSettingsSetResult {
    fn json_data(&self) -> Value {
        let changes: Vec<Value> = self
            .changes
            .iter()
            .map(|c| {
                json!({
                    "key": c.key,
                    "value": c.value,
                    "previousValue": c.previous_value,
                })
            })
            .collect();
        json!({ "id": self.id, "changes": changes })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        for c in &self.changes {
            writeln!(
                out,
                "{}: {} -> {}",
                c.key,
                format_setting_value(&c.previous_value),
                format_setting_value(&c.value),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn mod_settings_get_formats_unset_and_present() {
        let mut settings = Map::new();
        settings.insert("flag".to_owned(), json!("1"));
        settings.insert("count".to_owned(), json!(7));
        let all = ModSettingsAllResult {
            id: "m".to_owned(),
            settings,
        };
        // Sorted keys.
        assert_eq!(render_text(&all), "count=7\nflag=1\n");

        // An absent key renders a blank line in text and value:null in JSON.
        let absent = ModSettingsKeyResult {
            id: "m".to_owned(),
            key: "missing".to_owned(),
            value: Value::Null,
        };
        assert_eq!(render_text(&absent), "\n");
        assert_eq!(
            absent.json_data(),
            json!({ "id": "m", "key": "missing", "value": null })
        );

        let present = ModSettingsKeyResult {
            id: "m".to_owned(),
            key: "count".to_owned(),
            value: json!(7),
        };
        assert_eq!(render_text(&present), "7\n");
    }

    #[test]
    fn mod_settings_set_shows_unset_to_value() {
        let result = ModSettingsSetResult {
            id: "m".to_owned(),
            changes: vec![SettingChange {
                key: "flag".to_owned(),
                value: json!(1),
                previous_value: Value::Null,
            }],
        };
        assert_eq!(render_text(&result), "flag: <unset> -> 1\n");
    }

    #[test]
    fn mod_settings_set_renders_every_change_in_order() {
        let result = ModSettingsSetResult {
            id: "m".to_owned(),
            changes: vec![
                SettingChange {
                    key: "flag".to_owned(),
                    value: json!(1),
                    previous_value: Value::Null,
                },
                SettingChange {
                    key: "count".to_owned(),
                    value: json!(7),
                    previous_value: json!(3),
                },
            ],
        };
        // One transition line per change, in argv order (not sorted).
        assert_eq!(render_text(&result), "flag: <unset> -> 1\ncount: 3 -> 7\n");
        assert_eq!(
            result.json_data(),
            json!({
                "id": "m",
                "changes": [
                    { "key": "flag", "value": 1, "previousValue": null },
                    { "key": "count", "value": 7, "previousValue": 3 },
                ],
            })
        );
    }

    #[test]
    fn split_pair_splits_on_first_equals_and_rejects_bad_tokens() {
        assert_eq!(split_pair("flag=true").unwrap(), ("flag", "true"));
        // A value may contain '='; only the first splits.
        assert_eq!(split_pair("label=a=b").unwrap(), ("label", "a=b"));
        // An empty value is a valid token (type-checking rejects it later).
        assert_eq!(split_pair("label=").unwrap(), ("label", ""));

        assert_eq!(split_pair("flag").unwrap_err().exit_code(), 2);
        assert_eq!(split_pair("=1").unwrap_err().exit_code(), 2);
    }
}
