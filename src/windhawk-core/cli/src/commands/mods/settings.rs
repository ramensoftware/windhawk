//! `mod settings get`/`set`/`write`: read a mod's runtime settings, change some
//! of them, or replace them all, validated against the types declared in the
//! mod source's settings block; `mod settings defaults`: the settings a fresh
//! install writes; `mod settings options`: the options the mod itself wrote at
//! runtime for its `$dynamicSelect` settings.

use std::collections::BTreeSet;
use std::io::{self, Write};

use serde_json::{Map, Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    DynamicSelectOption, InitialSettings, ModIdParams, SetModSettingsParams,
};

use crate::Environment;
use crate::commands::parse::{parse_mod_source, reject_initial_settings_error};
use crate::commands::render::scalar_to_string;
use crate::commands::{app_settings, language};
use crate::error::CliError;
use crate::output::CommandResult;
use crate::validate::mod_settings::{
    flatten_setting_key_types, parse_setting_input, resolve_setting_key_type,
};
use crate::validate::mod_settings_map::validate_settings_map;

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
    let parsed = parse_mod_source(env, &source, &language(&settings), false)?;

    // A malformed settings block in the stored source is a generic failure
    // (exit 1): the source was valid when installed, so a parse failure now is
    // an internal problem, not a usage error.
    reject_initial_settings_error(
        &format!("installed mod '{id}'"),
        parsed.errors.initial_settings,
    )?;
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
/// `items[0].child`), each with its type and, for a bounded number, its range;
/// a trailing note spells out that the `[0]` is only a template so a reader
/// does not read the list as "only index 0 is allowed".
#[track_caller]
fn unknown_key_error(id: &str, key: &str, initial_settings: &InitialSettings) -> CliError {
    let key_types = flatten_setting_key_types(initial_settings);
    let valid_keys = key_types
        .iter()
        .map(|(key, ty)| format!("{key} ({})", ty.describe()))
        .collect::<Vec<_>>()
        .join("\n  ");
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
#[track_caller]
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

/// One key's before/after transition; a null on either side is an absent key.
struct SettingChange {
    key: String,
    value: Value,
    previous_value: Value,
}

fn changes_json(changes: &[SettingChange]) -> Vec<Value> {
    changes
        .iter()
        .map(|c| {
            json!({
                "key": c.key,
                "value": c.value,
                "previousValue": c.previous_value,
            })
        })
        .collect()
}

/// One `<key>: <previous> -> <new>` line per change.
fn write_changes(out: &mut dyn Write, changes: &[SettingChange]) -> io::Result<()> {
    for c in changes {
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

struct ModSettingsSetResult {
    id: String,
    changes: Vec<SettingChange>,
}

impl CommandResult for ModSettingsSetResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "changes": changes_json(&self.changes) })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        write_changes(out, &self.changes)
    }
}

// ---------------------------------------------------------------------------
// mod settings write
// ---------------------------------------------------------------------------

pub(super) fn settings_write(
    env: &Environment,
    id: &str,
    path: &str,
    dry_run: bool,
) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;
    let input = read_settings_input(path)?;
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings), false)?;
    reject_initial_settings_error(
        &format!("installed mod '{id}'"),
        parsed.errors.initial_settings,
    )?;
    // A mod that declares no settings takes `{}`, which clears whatever the
    // store holds.
    let initial_settings = parsed.initial_settings.unwrap_or_default();

    let stored: Map<String, Value> = env.core.invoke_as(
        "getModSettings",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    let new_settings = validate_settings_map(id, &initial_settings, &stored, &input)?;
    let changes = diff_settings(&stored, &new_settings);

    // One setModSettings call replaces the section wholesale, the way the
    // settings editor saves. It is skipped when nothing changed, which leaves
    // the mod's settings change time alone.
    let written = !dry_run && !changes.is_empty();
    if written {
        env.core.invoke(
            "setModSettings",
            &SetModSettingsParams {
                mod_id: id.to_owned(),
                settings: new_settings,
            },
        )?;
    }

    Ok(Box::new(ModSettingsWriteResult {
        id: id.to_owned(),
        changes,
        written,
        dry_run,
    }))
}

/// Read the JSON object of settings from the file at `path`, or from stdin
/// when it is `-`. A leading UTF-8 BOM, which Windows editors may write, is
/// skipped. Input that cannot be read, is not JSON, or is not an object is a
/// usage error (exit 2).
#[track_caller]
fn read_settings_input(path: &str) -> Result<Map<String, Value>, CliError> {
    let (read, origin) = if path == "-" {
        (io::read_to_string(io::stdin()), "standard input".to_owned())
    } else {
        (std::fs::read_to_string(path), format!("'{path}'"))
    };
    let text = match read {
        Ok(text) => text,
        Err(e) => return Err(CliError::usage(format!("Failed to read {origin}: {e}"))),
    };
    match serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(CliError::usage(format!(
            "The settings in {origin} must be a JSON object of keys to values."
        ))),
        Err(e) => Err(CliError::usage(format!(
            "The settings in {origin} are not valid JSON: {e}"
        ))),
    }
}

/// The changes writing `new` over `stored` makes: each added or changed key in
/// `new`'s order, then each removed key in `stored`'s order.
fn diff_settings(stored: &Map<String, Value>, new: &Map<String, Value>) -> Vec<SettingChange> {
    let mut changes = Vec::new();
    for (key, value) in new {
        let previous = stored.get(key);
        if previous.is_some_and(|previous| stores_same(previous, value)) {
            continue;
        }
        changes.push(SettingChange {
            key: key.clone(),
            value: value.clone(),
            previous_value: previous.cloned().unwrap_or(Value::Null),
        });
    }
    for (key, previous) in stored {
        if !new.contains_key(key) {
            changes.push(SettingChange {
                key: key.clone(),
                value: Value::Null,
                previous_value: previous.clone(),
            });
        }
    }
    changes
}

/// Whether writing `new` over `stored` leaves the store as it is: the same
/// value, or a number over its own decimal text, the form the portable store
/// reads every value back in.
fn stores_same(stored: &Value, new: &Value) -> bool {
    match (stored, new) {
        (Value::String(text), Value::Number(n)) => *text == n.to_string(),
        _ => stored == new,
    }
}

struct ModSettingsWriteResult {
    id: String,
    changes: Vec<SettingChange>,
    written: bool,
    dry_run: bool,
}

impl CommandResult for ModSettingsWriteResult {
    fn json_data(&self) -> Value {
        json!({
            "id": self.id,
            "changes": changes_json(&self.changes),
            "written": self.written,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        write_changes(out, &self.changes)?;
        if self.dry_run {
            writeln!(out, "(dry run, nothing written)")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// mod settings defaults
// ---------------------------------------------------------------------------

pub(super) fn settings_defaults(
    env: &Environment,
    id: &str,
) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;
    // The core reports a settings block that fails to parse as INTERNAL (exit
    // 14); parsing the source here first makes it exit 1, as for `set`/`write`.
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings), false)?;
    reject_initial_settings_error(
        &format!("installed mod '{id}'"),
        parsed.errors.initial_settings,
    )?;

    let defaults: Map<String, Value> = env.core.invoke_as(
        "getModDefaultSettings",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    Ok(Box::new(ModSettingsAllResult {
        id: id.to_owned(),
        settings: defaults,
    }))
}

// ---------------------------------------------------------------------------
// mod settings options
// ---------------------------------------------------------------------------

pub(super) fn settings_options(
    env: &Environment,
    id: &str,
) -> Result<Box<dyn CommandResult>, CliError> {
    // Existence check first (exit 4 if not installed): a mod that is not
    // installed has no local storage to read, and an empty answer would read
    // as "no options" rather than "no such mod".
    super::require_config(env, id)?;
    let result: Map<String, Value> = env.core.invoke_as(
        "getModDynamicSelectOptions",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    // The result is keyed by setting path, so the typed decode is per entry
    // array; the map keeps the core's order (the order the mod wrote).
    let mut options = Vec::with_capacity(result.len());
    for (path, entries) in result {
        let entries: Vec<DynamicSelectOption> =
            serde_json::from_value(entries).map_err(HostError::from)?;
        options.push((path, entries));
    }
    Ok(Box::new(ModSettingsOptionsResult {
        id: id.to_owned(),
        options,
    }))
}

struct ModSettingsOptionsResult {
    id: String,
    options: Vec<(String, Vec<DynamicSelectOption>)>,
}

impl CommandResult for ModSettingsOptionsResult {
    fn json_data(&self) -> Value {
        let options: Map<String, Value> = self
            .options
            .iter()
            .map(|(path, entries)| (path.clone(), crate::output::to_value(entries)))
            .collect();
        json!({ "id": self.id, "options": options })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        for (path, entries) in &self.options {
            writeln!(out, "{path}")?;
            for entry in entries {
                writeln!(out, "  {}: {}", entry.value, entry.label)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn mod_settings_options_renders_paths_with_indented_entries_in_order() {
        let option = |value: &str, label: &str| DynamicSelectOption {
            value: value.to_owned(),
            label: label.to_owned(),
        };
        let result = ModSettingsOptionsResult {
            id: "m".to_owned(),
            options: vec![
                (
                    "outputDevice".to_owned(),
                    vec![
                        option("speakers", "Speakers"),
                        option("usb::1", "USB Headset"),
                    ],
                ),
                ("monitor".to_owned(), vec![option("2", "2")]),
            ],
        };
        // Paths and entries in the order given (the order the mod wrote), one
        // path per line with its entries indented beneath it.
        assert_eq!(
            render_text(&result),
            "outputDevice\n  speakers: Speakers\n  usb::1: USB Headset\nmonitor\n  2: 2\n"
        );
        assert_eq!(
            result.json_data(),
            json!({
                "id": "m",
                "options": {
                    "outputDevice": [
                        { "value": "speakers", "label": "Speakers" },
                        { "value": "usb::1", "label": "USB Headset" },
                    ],
                    "monitor": [{ "value": "2", "label": "2" }],
                },
            })
        );

        // A mod that wrote nothing prints nothing; the JSON carries `{}`.
        let empty = ModSettingsOptionsResult {
            id: "m".to_owned(),
            options: Vec::new(),
        };
        assert_eq!(render_text(&empty), "");
        assert_eq!(empty.json_data(), json!({ "id": "m", "options": {} }));
    }

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

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn settings_diff_lists_added_and_changed_keys_then_removed_ones() {
        let stored = map(json!({ "a": 1, "gone1": "x", "b": "same", "gone2": 2 }));
        let new = map(json!({ "c": "added", "b": "same", "a": 5 }));
        let result = ModSettingsWriteResult {
            id: "m".to_owned(),
            changes: diff_settings(&stored, &new),
            written: true,
            dry_run: false,
        };
        // New keys in their own order (the unchanged `b` left out), then the
        // removed keys in stored order.
        assert_eq!(
            render_text(&result),
            "c: <unset> -> added\na: 1 -> 5\ngone1: x -> <unset>\ngone2: 2 -> <unset>\n"
        );
        assert_eq!(
            result.json_data(),
            json!({
                "id": "m",
                "changes": [
                    { "key": "c", "value": "added", "previousValue": null },
                    { "key": "a", "value": 5, "previousValue": 1 },
                    { "key": "gone1", "value": null, "previousValue": "x" },
                    { "key": "gone2", "value": null, "previousValue": 2 },
                ],
                "written": true,
            })
        );
    }

    #[test]
    fn settings_diff_reads_the_portable_text_of_a_number_as_unchanged() {
        // The portable store reads `1` and `-7` back as text.
        let stored = map(json!({ "flag": "1", "count": "-7", "label": "7" }));
        let new = map(json!({ "flag": 1, "count": -7, "label": "7" }));
        assert!(diff_settings(&stored, &new).is_empty());

        // Other text, or a string over a number, is a change.
        let stored = map(json!({ "count": "07", "label": 7 }));
        let new = map(json!({ "count": 7, "label": "7" }));
        assert_eq!(diff_settings(&stored, &new).len(), 2);

        // Nothing changed: no lines, and the JSON says nothing was written.
        let unchanged = ModSettingsWriteResult {
            id: "m".to_owned(),
            changes: Vec::new(),
            written: false,
            dry_run: false,
        };
        assert_eq!(render_text(&unchanged), "");
        assert_eq!(
            unchanged.json_data(),
            json!({ "id": "m", "changes": [], "written": false })
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
