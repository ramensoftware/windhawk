//! `mod enable`/`disable` and `mod remove`: the lifecycle state changes, each
//! guarded by the existence check and (for remove) the `--yes` confirmation.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{ModIdParams, SetModEnabledParams};

use crate::Environment;
use crate::error::CliError;
use crate::output::CommandResult;

// ---------------------------------------------------------------------------
// mod enable / mod disable
// ---------------------------------------------------------------------------

/// CLI half of enable/disable: the existence check and the already-in-state
/// no-op are CLI-only; the state change is the shared `setModEnabled` core
/// command (which also mirrors the user profile for non-local mods).
pub(super) fn set_enabled(
    env: &Environment,
    id: &str,
    enable: bool,
) -> Result<Box<dyn CommandResult>, CliError> {
    let config = super::require_config(env, id)?;
    let currently_enabled = !config.disabled;

    let changed = if currently_enabled == enable {
        false
    } else {
        // No tray notification: matches the extension's enableMod IPC handler,
        // which writes and lets the engine pick up the change on its own.
        env.core.invoke(
            "setModEnabled",
            &SetModEnabledParams {
                mod_id: id.to_owned(),
                enable,
            },
        )?;
        true
    };

    Ok(Box::new(ModEnableResult {
        id: id.to_owned(),
        enabled: enable,
        changed,
    }))
}

struct ModEnableResult {
    id: String,
    enabled: bool,
    changed: bool,
}

impl CommandResult for ModEnableResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "enabled": self.enabled, "changed": self.changed })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let line = match (self.enabled, self.changed) {
            (true, true) => format!("Enabled: {}", self.id),
            (true, false) => format!("Already enabled: {}", self.id),
            (false, true) => format!("Disabled: {}", self.id),
            (false, false) => format!("Already disabled: {}", self.id),
        };
        writeln!(out, "{line}")
    }
}

// ---------------------------------------------------------------------------
// mod remove
// ---------------------------------------------------------------------------

pub(super) fn remove(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;

    if !env.yes {
        // mod remove: --yes is required. Without it, print the planned action
        // to stderr and exit 2.
        eprintln!(
            "Would remove mod '{id}' (config, source, DLLs, profile entry). \
             Pass --yes to confirm."
        );
        return Err(CliError::usage(format!(
            "Refusing to remove '{id}' without --yes"
        )));
    }

    // No tray notification: matches the extension's deleteMod IPC handler. The
    // editor-draft cleanup for local@ mods is editor-mode-only and stays in the
    // extension; the CLI has no workspace concept.
    env.core.invoke(
        "removeMod",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;

    Ok(Box::new(ModRemoveResult { id: id.to_owned() }))
}

struct ModRemoveResult {
    id: String,
}

impl CommandResult for ModRemoveResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "removed": true })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "Removed: {}", self.id)
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn mod_enable_text_covers_all_four_transitions() {
        let cases = [
            (true, true, "Enabled: m\n"),
            (true, false, "Already enabled: m\n"),
            (false, true, "Disabled: m\n"),
            (false, false, "Already disabled: m\n"),
        ];
        for (enabled, changed, expected) in cases {
            let result = ModEnableResult {
                id: "m".to_owned(),
                enabled,
                changed,
            };
            assert_eq!(render_text(&result), expected);
            assert_eq!(
                result.json_data(),
                json!({ "id": "m", "enabled": enabled, "changed": changed })
            );
        }
    }

    #[test]
    fn mod_remove_renders_id_and_removed_flag() {
        let result = ModRemoveResult { id: "m".to_owned() };
        assert_eq!(render_text(&result), "Removed: m\n");
        assert_eq!(result.json_data(), json!({ "id": "m", "removed": true }));
    }
}
