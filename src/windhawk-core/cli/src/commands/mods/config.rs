//! `mod config get`/`set`: read or edit the settable `ModConfig` fields, with
//! the settable/read-only field tables and the drift guard that keeps them in
//! sync with the struct.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{ModConfig, ModConfigPatch, UpdateModConfigParams};

use crate::Environment;
use crate::commands::render::scalar_to_string;
use crate::error::CliError;
use crate::output::{CommandResult, to_value};

// ---------------------------------------------------------------------------
// mod config get
// ---------------------------------------------------------------------------

pub(super) fn config_get(
    env: &Environment,
    id: &str,
    field: Option<&str>,
) -> Result<Box<dyn CommandResult>, CliError> {
    let config = super::require_config(env, id)?;
    let config_value = to_value(&config);

    let Some(field) = field else {
        return Ok(Box::new(ModConfigAllResult {
            id: id.to_owned(),
            config,
        }));
    };

    let present = config_value
        .as_object()
        .is_some_and(|obj| obj.contains_key(field));
    if !present {
        return Err(CliError::usage(format!(
            "Unknown config field '{field}'. Run 'mod config get {id}' to see all fields."
        )));
    }
    let value = config_value.get(field).cloned().unwrap_or(Value::Null);

    Ok(Box::new(ModConfigFieldResult {
        id: id.to_owned(),
        field: field.to_owned(),
        value,
    }))
}

/// Format a config value (mirror of the TS `formatConfigValue`): an empty array
/// is `<empty list>`, a non-empty array is comma-joined, scalars stringify.
fn format_config_value(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            if items.is_empty() {
                "<empty list>".to_owned()
            } else {
                items
                    .iter()
                    .map(scalar_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        other => scalar_to_string(other),
    }
}

struct ModConfigAllResult {
    id: String,
    config: ModConfig,
}

impl CommandResult for ModConfigAllResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "config": self.config })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let value = to_value(&self.config);
        if let Some(obj) = value.as_object() {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for key in keys {
                let v = obj.get(key).unwrap_or(&Value::Null);
                writeln!(out, "{key}={}", format_config_value(v))?;
            }
        }
        Ok(())
    }
}

struct ModConfigFieldResult {
    id: String,
    field: String,
    value: Value,
}

impl CommandResult for ModConfigFieldResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "field": self.field, "value": self.value })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "{}", format_config_value(&self.value))
    }
}

// ---------------------------------------------------------------------------
// mod config set
// ---------------------------------------------------------------------------

/// The value type of a settable config field, governing how its CLI values are
/// parsed.
#[derive(Clone, Copy)]
enum ConfigFieldType {
    Boolean,
    StringArray,
}

/// Fields the CLI lets the user edit, with their value type (mod config set).
/// Narrower than the full ModConfig shape: include/exclude/architecture/version
/// are metadata-driven (clobbered on every install/compile), `disabled` has its
/// own enable/disable commands, and `libraryFileName` is internal.
const SETTABLE_FIELDS: &[(&str, ConfigFieldType)] = &[
    ("loggingEnabled", ConfigFieldType::Boolean),
    ("debugLoggingEnabled", ConfigFieldType::Boolean),
    ("includeCustom", ConfigFieldType::StringArray),
    ("excludeCustom", ConfigFieldType::StringArray),
    ("includeExcludeCustomOnly", ConfigFieldType::Boolean),
    (
        "patternsMatchCriticalSystemProcesses",
        ConfigFieldType::Boolean,
    ),
];

/// Rejection reasons for read-only ModConfig fields, keyed by field name (the
/// TS `READ_ONLY_FIELD_REASONS`).
const READ_ONLY_FIELD_REASONS: &[(&str, &str)] = &[
    ("disabled", "use 'mod enable <id>' / 'mod disable <id>'"),
    (
        "include",
        "metadata-driven (overwritten on every mod install/compile)",
    ),
    (
        "exclude",
        "metadata-driven (overwritten on every mod install/compile)",
    ),
    (
        "architecture",
        "metadata-driven (overwritten on every mod install/compile)",
    ),
    ("version", "metadata-driven"),
    ("libraryFileName", "internal (managed by the compiler)"),
];

/// Guard the settable/read-only field tables against `ModConfig` drift: a field
/// added to the struct must be CONSCIOUSLY classified as settable or read-only,
/// never silently fall into neither table (where `mod config get` would expose
/// it but `mod config set` could neither set nor explain it). Two independent
/// layers:
///
/// 1. The exhaustive destructure names all 12 fields with NO `..` rest-pattern,
///    mirroring the `ModConfigPatch::has_any` precedent
///    (protocol/src/settings.rs): a new `ModConfig` field is a BUILD error here,
///    so the compiler - not the author's memory - keeps the guard in sync with
///    the struct. (A `..` would compile straight through a new field and defeat
///    the guard, so it is deliberately omitted.)
/// 2. The set assertions: the two tables PARTITION `ModConfig`'s serialized keys
///    exactly (union == keys AND no field appears in both), so neither an
///    unclassified new field nor one mistakenly listed as both settable and
///    read-only slips through.
#[cfg(test)]
mod config_table_guard {
    use std::collections::BTreeSet;

    use windhawk_core_protocol::ModConfig;

    use super::{READ_ONLY_FIELD_REASONS, SETTABLE_FIELDS};
    use crate::commands::mods::test_support::config;

    #[test]
    fn settable_and_read_only_tables_partition_mod_config() {
        let config = config(false);

        // Compile-time exhaustiveness: every field named, no `..`, so adding a
        // ModConfig field fails to build until it is listed here (and, by the
        // assertions below, classified into one of the two tables).
        let ModConfig {
            library_file_name: _,
            disabled: _,
            logging_enabled: _,
            debug_logging_enabled: _,
            include: _,
            exclude: _,
            include_custom: _,
            exclude_custom: _,
            include_exclude_custom_only: _,
            patterns_match_critical_system_processes: _,
            architecture: _,
            version: _,
        } = &config;

        // The serialized camelCase key set the two tables must partition.
        let value = serde_json::to_value(&config).unwrap();
        let keys: BTreeSet<&str> = value
            .as_object()
            .expect("ModConfig serializes to an object")
            .keys()
            .map(String::as_str)
            .collect();

        let settable: BTreeSet<&str> = SETTABLE_FIELDS.iter().map(|(f, _)| *f).collect();
        let read_only: BTreeSet<&str> = READ_ONLY_FIELD_REASONS.iter().map(|(f, _)| *f).collect();

        // Disjoint: no field is both settable and read-only.
        let both: Vec<&str> = settable.intersection(&read_only).copied().collect();
        assert!(both.is_empty(), "fields in both tables: {both:?}");

        // Exhaustive: settable + read-only covers exactly the serialized keys.
        let union: BTreeSet<&str> = settable.union(&read_only).copied().collect();
        assert_eq!(
            union, keys,
            "settable + read-only must partition ModConfig's fields"
        );
    }
}

pub(super) fn config_set(
    env: &Environment,
    id: &str,
    field: &str,
    values: &[String],
) -> Result<Box<dyn CommandResult>, CliError> {
    let config = super::require_config(env, id)?;

    if let Some((_, reason)) = READ_ONLY_FIELD_REASONS.iter().find(|(f, _)| *f == field) {
        return Err(CliError::usage(format!(
            "'{field}' is not settable: {reason}"
        )));
    }
    let Some((_, field_type)) = SETTABLE_FIELDS.iter().find(|(f, _)| *f == field) else {
        let settable = SETTABLE_FIELDS
            .iter()
            .map(|(f, _)| *f)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CliError::usage(format!(
            "Unknown config field '{field}'. Settable fields: {settable}."
        )));
    };

    let config_value = to_value(&config);
    let previous_value = config_value.get(field).cloned().unwrap_or(Value::Null);
    let new_value = parse_config_value(field, *field_type, values)?;

    // Build the single-field patch Value, then decode it into the typed request
    // DTO so a core param rename is a build error; the one-field patch
    // round-trips byte-identically through `skip_serializing_if`. The decode
    // cannot fail for a settable field, so a failure is an internal GENERIC.
    let patch: ModConfigPatch = serde_json::from_value(json!({ field: new_value }))
        .map_err(|e| CliError::generic(format!("internal: invalid mod-config patch: {e}")))?;
    env.core.invoke(
        "updateModConfig",
        &UpdateModConfigParams {
            mod_id: id.to_owned(),
            patch,
        },
    )?;

    Ok(Box::new(ModConfigSetResult {
        id: id.to_owned(),
        field: field.to_owned(),
        value: new_value,
        previous_value,
    }))
}

/// Parse the variadic config-set values per the field type (the TS
/// `parseFieldValue`): a boolean takes exactly one of true/false/1/0; a
/// string-array takes any count (zero clears the array).
#[track_caller]
fn parse_config_value(
    field: &str,
    field_type: ConfigFieldType,
    values: &[String],
) -> Result<Value, CliError> {
    match field_type {
        ConfigFieldType::Boolean => {
            if values.len() != 1 {
                return Err(CliError::usage(format!(
                    "Boolean field '{field}' requires exactly one value; got {}. \
                     Accepted: true, false, 1, 0.",
                    values.len()
                )));
            }
            match values[0].as_str() {
                "true" | "1" => Ok(Value::Bool(true)),
                "false" | "0" => Ok(Value::Bool(false)),
                other => Err(CliError::usage(format!(
                    "Boolean field '{field}' value must be one of true/false/1/0; got '{other}'."
                ))),
            }
        }
        ConfigFieldType::StringArray => Ok(Value::Array(
            values.iter().map(|v| Value::String(v.clone())).collect(),
        )),
    }
}

struct ModConfigSetResult {
    id: String,
    field: String,
    value: Value,
    previous_value: Value,
}

impl CommandResult for ModConfigSetResult {
    fn json_data(&self) -> Value {
        json!({
            "id": self.id,
            "field": self.field,
            "value": self.value,
            "previousValue": self.previous_value,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "{}: {} -> {}",
            self.field,
            format_config_value(&self.previous_value),
            format_config_value(&self.value),
        )
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::commands::mods::test_support::config;
    use crate::output::render_text;

    #[test]
    fn mod_config_get_formats_arrays_and_scalars() {
        let all = ModConfigAllResult {
            id: "m".to_owned(),
            config: config(false),
        };
        let text = render_text(&all);
        // Keys are sorted; an empty array is `<empty list>`, a bool stringifies.
        assert!(text.contains("include=<empty list>\n"), "{text}");
        assert!(text.contains("loggingEnabled=true\n"), "{text}");
        assert!(text.contains("architecture=x86-64\n"), "{text}");

        let empty_array = ModConfigFieldResult {
            id: "m".to_owned(),
            field: "includeCustom".to_owned(),
            value: json!([]),
        };
        assert_eq!(render_text(&empty_array), "<empty list>\n");

        let filled_array = ModConfigFieldResult {
            id: "m".to_owned(),
            field: "includeCustom".to_owned(),
            value: json!(["a.exe", "b.exe"]),
        };
        assert_eq!(render_text(&filled_array), "a.exe, b.exe\n");
    }

    #[test]
    fn mod_config_set_shows_the_transition() {
        let result = ModConfigSetResult {
            id: "m".to_owned(),
            field: "excludeCustom".to_owned(),
            value: json!([]),
            previous_value: json!(["x.exe"]),
        };
        assert_eq!(
            render_text(&result),
            "excludeCustom: x.exe -> <empty list>\n"
        );
        assert_eq!(
            result.json_data(),
            json!({
                "id": "m",
                "field": "excludeCustom",
                "value": [],
                "previousValue": ["x.exe"],
            })
        );
    }
}
