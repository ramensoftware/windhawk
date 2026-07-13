//! `mod settings get`/`set`: read or write a mod's runtime settings, validated
//! against the types declared in the mod source's settings block.

use std::io::{self, Write};

use serde_json::{Map, Value, json};
use windhawk_core_protocol::{ModIdParams, SetModSettingsParams};

use crate::Environment;
use crate::commands::parse::{parse_mod_source, reject_initial_settings_error};
use crate::commands::render::scalar_to_string;
use crate::commands::{app_settings, language};
use crate::error::CliError;
use crate::output::CommandResult;
use crate::validate::mod_settings::{flatten_setting_key_types, parse_setting_input};

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
    key: &str,
    raw_value: &str,
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

    let key_types = flatten_setting_key_types(&initial_settings);
    let Some(declared_type) = key_types.get(key).copied() else {
        let valid_keys = key_types.keys().cloned().collect::<Vec<_>>().join("\n  ");
        return Err(CliError::usage(format!(
            "Key '{key}' is not a declared setting of mod '{id}'.\nValid keys:\n  {valid_keys}"
        )));
    };

    let new_value = parse_setting_input(key, declared_type, raw_value)?;

    let mut current: Map<String, Value> = env.core.invoke_as(
        "getModSettings",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    let previous_value = current.get(key).cloned().unwrap_or(Value::Null);

    // Write the whole settings map back with the one key changed - setModSettings
    // replaces the section wholesale. No tray notification: matches the
    // extension's setModSettings IPC handler; the engine picks up the change.
    current.insert(key.to_owned(), new_value.clone());
    env.core.invoke(
        "setModSettings",
        &SetModSettingsParams {
            mod_id: id.to_owned(),
            settings: current,
        },
    )?;

    Ok(Box::new(ModSettingsSetResult {
        id: id.to_owned(),
        key: key.to_owned(),
        value: new_value,
        previous_value,
    }))
}

struct ModSettingsSetResult {
    id: String,
    key: String,
    value: Value,
    previous_value: Value,
}

impl CommandResult for ModSettingsSetResult {
    fn json_data(&self) -> Value {
        json!({
            "id": self.id,
            "key": self.key,
            "value": self.value,
            "previousValue": self.previous_value,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "{}: {} -> {}",
            self.key,
            format_setting_value(&self.previous_value),
            format_setting_value(&self.value),
        )
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
            key: "flag".to_owned(),
            value: json!(1),
            previous_value: Value::Null,
        };
        assert_eq!(render_text(&result), "flag: <unset> -> 1\n");
    }
}
