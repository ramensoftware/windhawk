//! `app settings get [<key>]` / `app settings set <key> <value>`: read Windhawk
//! application settings (all of them or a single dotted key), and write a
//! single setting through the preview restart gate and the post-write tray
//! notify. The dotted-key schema, value parsing, and patch builder are the pure
//! `validate::app_settings` helpers; this module owns the I/O (read, preview,
//! apply, notify) and the render.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{
    AppSettings, AppSettingsIntents, AppSettingsPatch, AppSettingsPatchParams, NotifyTrayParams,
    TrayAction,
};

use crate::Environment;
use crate::args::{AppCommand, AppSettingsCommand};
use crate::commands::app_settings;
use crate::commands::render::scalar_to_string;
use crate::error::CliError;
use crate::output::{CommandResult, to_value};
use crate::validate::app_settings as app_settings_schema;

pub fn dispatch(
    env: &Environment,
    command: AppCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    match command {
        AppCommand::Settings { command } => match command {
            AppSettingsCommand::Get { key } => get(env, key),
            AppSettingsCommand::Set {
                key,
                value,
                confirm_app_restart,
            } => set(env, &key, &value, confirm_app_restart),
        },
    }
}

fn get(env: &Environment, key: Option<String>) -> Result<Box<dyn CommandResult>, CliError> {
    let settings = app_settings(env)?;

    let Some(key) = key else {
        return Ok(Box::new(AppSettingsAllResult { settings }));
    };

    if app_settings_schema::field_type(&key).is_none() {
        return Err(unknown_setting(&key));
    }

    let settings_value = to_value(&settings);
    let value = lookup_dotted(&settings_value, &key);
    Ok(Box::new(AppSettingsKeyResult { key, value }))
}

fn set(
    env: &Environment,
    key: &str,
    raw_value: &str,
    confirm_app_restart: bool,
) -> Result<Box<dyn CommandResult>, CliError> {
    let Some(field_type) = app_settings_schema::field_type(key) else {
        return Err(unknown_setting(key));
    };

    let new_value = app_settings_schema::parse_value(key, field_type, raw_value)?;
    // Build the dotted-key patch Value, then decode it into the typed request
    // DTO so a core param rename is a build error; the single-field patch
    // round-trips byte-identically through `skip_serializing_if`. The decode
    // cannot fail for a schema-valid key, so a failure is an internal GENERIC.
    let patch: AppSettingsPatch =
        serde_json::from_value(app_settings_schema::build_patch(key, new_value.clone()))
            .map_err(|e| CliError::generic(format!("internal: invalid app-settings patch: {e}")))?;
    let params = AppSettingsPatchParams { patch };

    let before = app_settings(env)?;
    let before_value = to_value(&before);
    let previous_value = lookup_dotted(&before_value, key);

    // A setting that reads as null is in the schema but not applicable in this
    // installation mode (e.g. disableRunUIScheduledTask in portable mode).
    // Reject it as a usage error (exit 2) rather than letting the core reject
    // the write as a generic failure (exit 1).
    if matches!(previous_value, Some(Value::Null)) {
        return Err(CliError::usage(format!(
            "Setting '{key}' is not available in this Windhawk installation mode."
        )));
    }

    // The refusal gate needs the restart intent BEFORE anything is written;
    // applyAppSettings only reports intents after writing, so use the pure
    // preview command.
    let preview: AppSettingsIntents = env.core.invoke_as("previewAppSettingsEffects", &params)?;
    if preview.requires_restart && !confirm_app_restart {
        return Err(CliError::restart_required(format!(
            "Setting '{key}' requires a Windhawk restart. Pass --confirm-app-restart to proceed."
        )));
    }

    let applied: AppSettingsIntents = env.core.invoke_as("applyAppSettings", &params)?;

    // Tray notification: matches the extension's updateAppSettings handler
    // exactly. This is the one CLI command that spawns the tray program.
    if applied.requires_restart {
        env.core.invoke(
            "notifyTray",
            &NotifyTrayParams {
                action: TrayAction::RestartBg,
            },
        )?;
    } else if applied.requires_notify {
        env.core.invoke(
            "notifyTray",
            &NotifyTrayParams {
                action: TrayAction::AppSettingsChanged,
            },
        )?;
    }

    Ok(Box::new(AppSettingsSetResult {
        key: key.to_owned(),
        value: new_value,
        previous_value,
        restart_requested: applied.requires_restart,
        notify_requested: applied.requires_notify,
    }))
}

/// The shared unknown-key usage error for `get` and `set` (exit 2).
fn unknown_setting(key: &str) -> CliError {
    CliError::usage(format!(
        "Unknown app setting '{key}'. Run 'app settings get' to see all settings."
    ))
}

/// Traverse a dotted key through nested objects, mirroring the TS
/// `lookupByDottedKey`. `None` is the "undefined" of a traversal that hit a
/// non-object; `Some(Value::Null)` is a present `null` (e.g.
/// `disableRunUIScheduledTask` in portable mode).
fn lookup_dotted(value: &Value, key: &str) -> Option<Value> {
    let mut cursor = value;
    for part in key.split('.') {
        match cursor {
            Value::Object(map) => cursor = map.get(part)?,
            _ => return None,
        }
    }
    Some(cursor.clone())
}

/// Format an app setting value (mirror of the TS `formatValue`): a missing key
/// is `<unset>`, a present `null` is `<null>`, an empty list is `<empty list>`.
fn format_value(value: &Option<Value>) -> String {
    match value {
        None => "<unset>".to_owned(),
        Some(Value::Null) => "<null>".to_owned(),
        Some(Value::Array(items)) => {
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
        Some(other) => scalar_to_string(other),
    }
}

struct AppSettingsAllResult {
    settings: AppSettings,
}

impl CommandResult for AppSettingsAllResult {
    fn json_data(&self) -> Value {
        json!({ "settings": self.settings })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        // Iterate the schema keys (sorted), not the raw settings object: the
        // text form lists exactly the user-facing dotted-key namespace.
        let value = to_value(&self.settings);
        let mut keys: Vec<&str> = app_settings_schema::APP_SETTINGS_SCHEMA
            .iter()
            .map(|(k, _)| *k)
            .collect();
        keys.sort_unstable();
        for key in keys {
            let v = lookup_dotted(&value, key);
            writeln!(out, "{key}={}", format_value(&v))?;
        }
        Ok(())
    }
}

struct AppSettingsKeyResult {
    key: String,
    value: Option<Value>,
}

impl CommandResult for AppSettingsKeyResult {
    fn json_data(&self) -> Value {
        // `value` is always present for a valid key (an absent traversal is
        // unreachable once the key is schema-validated), so emit it always; an
        // absent value serializes to explicit `null`.
        json!({ "key": self.key, "value": self.value })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "{}", format_value(&self.value))
    }
}

struct AppSettingsSetResult {
    key: String,
    value: Value,
    previous_value: Option<Value>,
    restart_requested: bool,
    notify_requested: bool,
}

impl CommandResult for AppSettingsSetResult {
    fn json_data(&self) -> Value {
        // `previousValue` is always present for a valid key (the present-`null`
        // case is rejected at exit 2 before a result is built), so emit it
        // always; an absent value serializes to explicit `null`.
        json!({
            "key": self.key,
            "value": self.value,
            "previousValue": self.previous_value,
            "restartRequested": self.restart_requested,
            "notifyRequested": self.notify_requested,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "{}: {} -> {}",
            self.key,
            format_value(&self.previous_value),
            format_value(&Some(self.value.clone())),
        )?;
        if self.restart_requested {
            writeln!(out, "Windhawk restart requested.")?;
        } else if self.notify_requested {
            writeln!(out, "Tray notified; engine will pick up the change.")?;
        }
        Ok(())
    }
}

/// Golden (snapshot) tests of the compute-then-render seam for the `app
/// settings` results: the full sorted all-listing block, the
/// `<unset>`/`<null>`/`<empty list>` value tokens, and the three set outcomes
/// (restart / notify / silent), with no DLL or session.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    /// A default `AppSettings`; `portable` controls whether
    /// `disableRunUIScheduledTask` reads as a present `null` (portable) or a
    /// boolean (registry mode), matching the core's `get`.
    fn app_settings(portable: bool) -> AppSettings {
        serde_json::from_value(json!({
            "language": "en",
            "disableUpdateCheck": false,
            "disableRunUIScheduledTask": if portable { Value::Null } else { json!(false) },
            "devModeOptOut": false,
            "hideTrayIcon": false,
            "alwaysCompileModsLocally": false,
            "dontAutoShowToolkit": false,
            "modTasksDialogDelay": 2000,
            "safeMode": false,
            "loggingVerbosity": 0,
            "engine": {
                "loggingVerbosity": 0,
                "include": [],
                "exclude": [],
                "injectIntoCriticalProcesses": false,
                "injectIntoIncompatiblePrograms": false,
                "injectIntoGames": false,
            },
        }))
        .unwrap()
    }

    #[test]
    fn app_settings_get_all_renders_the_full_sorted_block() {
        let result = AppSettingsAllResult {
            settings: app_settings(true),
        };
        // The schema keys, sorted, each as key=value; portable mode shows the
        // scheduled-task field as <null>, empty engine arrays as <empty list>.
        assert_eq!(
            render_text(&result),
            "alwaysCompileModsLocally=false\n\
             devModeOptOut=false\n\
             disableRunUIScheduledTask=<null>\n\
             disableUpdateCheck=false\n\
             dontAutoShowToolkit=false\n\
             engine.exclude=<empty list>\n\
             engine.include=<empty list>\n\
             engine.injectIntoCriticalProcesses=false\n\
             engine.injectIntoGames=false\n\
             engine.injectIntoIncompatiblePrograms=false\n\
             engine.loggingVerbosity=0\n\
             hideTrayIcon=false\n\
             language=en\n\
             loggingVerbosity=0\n\
             modTasksDialogDelay=2000\n\
             safeMode=false\n"
        );
    }

    #[test]
    fn app_settings_get_key_formats_each_value_state() {
        // A present scalar.
        let present = AppSettingsKeyResult {
            key: "language".to_owned(),
            value: Some(json!("en")),
        };
        assert_eq!(render_text(&present), "en\n");
        assert_eq!(
            present.json_data(),
            json!({ "key": "language", "value": "en" })
        );

        // A present null (portable disableRunUIScheduledTask): <null>, kept in JSON.
        let null = AppSettingsKeyResult {
            key: "disableRunUIScheduledTask".to_owned(),
            value: Some(Value::Null),
        };
        assert_eq!(render_text(&null), "<null>\n");
        assert_eq!(
            null.json_data(),
            json!({ "key": "disableRunUIScheduledTask", "value": null })
        );

        // An absent value (traversal hit nothing, unreachable for a valid key):
        // <unset> in text, and explicit `null` in JSON (the field is always
        // present).
        let unset = AppSettingsKeyResult {
            key: "language".to_owned(),
            value: None,
        };
        assert_eq!(render_text(&unset), "<unset>\n");
        assert_eq!(
            unset.json_data(),
            json!({ "key": "language", "value": null })
        );

        // An empty list renders <empty list>.
        let list = AppSettingsKeyResult {
            key: "engine.include".to_owned(),
            value: Some(json!([])),
        };
        assert_eq!(render_text(&list), "<empty list>\n");
    }

    #[test]
    fn app_settings_set_renders_restart_notify_and_silent() {
        let restart = AppSettingsSetResult {
            key: "safeMode".to_owned(),
            value: json!(true),
            previous_value: Some(json!(false)),
            restart_requested: true,
            notify_requested: false,
        };
        assert_eq!(
            render_text(&restart),
            "safeMode: false -> true\nWindhawk restart requested.\n"
        );
        assert_eq!(
            restart.json_data(),
            json!({
                "key": "safeMode",
                "value": true,
                "previousValue": false,
                "restartRequested": true,
                "notifyRequested": false,
            })
        );

        let notify = AppSettingsSetResult {
            key: "hideTrayIcon".to_owned(),
            value: json!(true),
            previous_value: Some(json!(false)),
            restart_requested: false,
            notify_requested: true,
        };
        assert_eq!(
            render_text(&notify),
            "hideTrayIcon: false -> true\nTray notified; engine will pick up the change.\n"
        );

        // Silent: no trailing intent line.
        let silent = AppSettingsSetResult {
            key: "devModeOptOut".to_owned(),
            value: json!(true),
            previous_value: Some(json!(false)),
            restart_requested: false,
            notify_requested: false,
        };
        assert_eq!(render_text(&silent), "devModeOptOut: false -> true\n");
    }
}
