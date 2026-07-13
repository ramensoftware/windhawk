//! The `update` group: `update status` shows the cached "latest available
//! version" (the bleeding-edge cached value, never a live probe) and compares
//! it against the CLI's own installed version; `update run` downloads and
//! launches the Windhawk installer (the C ABI async path, with download
//! progress streamed to stderr).

use std::io::{self, Write};

use serde::Deserialize;
use serde_json::{Value, json};
use windhawk_core_protocol::{AppUpdateStatus, OperationEvent};

use crate::Environment;
use crate::args::UpdateCommand;
use crate::environment;
use crate::error::CliError;
use crate::output::CommandResult;

pub fn dispatch(
    env: &Environment,
    command: UpdateCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    match command {
        UpdateCommand::Status => status(env),
        UpdateCommand::Run => run(env),
    }
}

fn status(env: &Environment) -> Result<Box<dyn CommandResult>, CliError> {
    // The installed version is the CLI's own build-embedded product version -
    // the same value the session was created with. Always present for a native
    // build, so the unknown -> null path is preserved-but-unreachable.
    let installed_version = Some(environment::product_version().to_owned());

    // The bleeding-edge value is the raw cached latest version; latestVersion is
    // the grace-period-filtered value used for the GUI badge and has no place in
    // an on-demand CLI read.
    let status: AppUpdateStatus = env.core.invoke_as("getAppUpdateStatus", &json!({}))?;

    Ok(Box::new(UpdateStatusResult {
        installed_version,
        latest_version: status.latest_version_bleeding_edge,
        update_available: status.update_available_bleeding_edge,
    }))
}

struct UpdateStatusResult {
    installed_version: Option<String>,
    latest_version: Option<String>,
    update_available: bool,
}

impl CommandResult for UpdateStatusResult {
    fn json_data(&self) -> Value {
        json!({
            "installedVersion": self.installed_version,
            "latestVersion": self.latest_version,
            "updateAvailable": self.update_available,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "Installed:        {}",
            self.installed_version.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            out,
            "Latest:           {}",
            self.latest_version.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            out,
            "Update available: {}",
            if self.update_available { "yes" } else { "no" }
        )
    }
}

// ---------------------------------------------------------------------------
// update run
// ---------------------------------------------------------------------------

fn run(env: &Environment) -> Result<Box<dyn CommandResult>, CliError> {
    if !env.yes {
        // update run: --yes is required. Without it, print the planned action
        // to stderr and exit 2 (the `mod remove` pattern).
        eprintln!("Would download and launch the Windhawk installer. Pass --yes to confirm.");
        return Err(CliError::usage(
            "Refusing to run the installer without --yes",
        ));
    }

    // The download de-duplicates progress in the core; this guard is a safety
    // net for future API changes. Progress lives on stderr in both text and JSON
    // modes so stdout stays clean for the single completion object.
    let mut last_progress = -1i64;
    let result: StartUpdateResult =
        env.core
            .invoke_async_as("startUpdate", &json!({}), |event| match event {
                OperationEvent::Progress { payload } => {
                    let progress = payload.get("progress").and_then(Value::as_i64).unwrap_or(0);
                    if progress != last_progress {
                        last_progress = progress;
                        env.logger.info(&format!("Downloading: {progress}%"));
                    }
                }
                OperationEvent::Installing => env.logger.info("Launching installer..."),
                // The terminal events never reach this callback (the invoke consumes
                // them); the catch-all keeps the match exhaustive.
                OperationEvent::Completed { .. } | OperationEvent::Failed { .. } => {}
            })?;

    // startUpdate reports the version it pinned, so the run line cites exactly
    // what was pulled; `None` (the `latest` fallback or a debug URL) renders as
    // the bare "Installer launched".
    Ok(Box::new(UpdateRunResult {
        version: result.version.unwrap_or_default(),
    }))
}

/// The `startUpdate` completion result: the release version the installer pinned,
/// absent for the `latest` fallback (no cached version) or a debug URL override.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartUpdateResult {
    version: Option<String>,
}

struct UpdateRunResult {
    version: String,
}

impl CommandResult for UpdateRunResult {
    fn json_data(&self) -> Value {
        json!({ "version": self.version, "installerLaunched": true })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.version.is_empty() {
            writeln!(out, "Installer launched")
        } else {
            writeln!(out, "Installer launched: {}", self.version)
        }
    }
}

/// Golden (snapshot) tests of the compute-then-render seam for the `update`
/// results: the status block (known vs unknown latest) and the run completion
/// line (with vs without a reported version).
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn update_status_known_latest() {
        let result = UpdateStatusResult {
            installed_version: Some("1.7.3".to_owned()),
            latest_version: Some("1.8.0".to_owned()),
            update_available: true,
        };
        assert_eq!(
            render_text(&result),
            "Installed:        1.7.3\nLatest:           1.8.0\nUpdate available: yes\n"
        );
        assert_eq!(
            result.json_data(),
            json!({
                "installedVersion": "1.7.3",
                "latestVersion": "1.8.0",
                "updateAvailable": true,
            })
        );
    }

    #[test]
    fn update_status_unknown_latest() {
        // No cached latest: the Latest line reads `unknown`, no update.
        let result = UpdateStatusResult {
            installed_version: Some("1.7.3".to_owned()),
            latest_version: None,
            update_available: false,
        };
        assert_eq!(
            render_text(&result),
            "Installed:        1.7.3\nLatest:           unknown\nUpdate available: no\n"
        );
        assert_eq!(result.json_data()["latestVersion"], json!(null));
    }

    #[test]
    fn update_run_with_and_without_version() {
        let known = UpdateRunResult {
            version: "1.8.0".to_owned(),
        };
        assert_eq!(render_text(&known), "Installer launched: 1.8.0\n");
        assert_eq!(known.json_data()["installerLaunched"], json!(true));

        let unknown = UpdateRunResult {
            version: String::new(),
        };
        assert_eq!(render_text(&unknown), "Installer launched\n");
    }
}
