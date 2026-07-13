//! The `mod` group: `mod list`, `mod show`, `mod config get`/`set`, `mod
//! settings get`/`set`, `mod enable`/`disable`, `mod remove` (sync), plus the
//! compile-bearing async commands `mod install`, `mod update`, and `mod
//! compile`.

mod config;
mod install;
mod lifecycle;
mod list;
mod settings;
mod show;

#[cfg(test)]
mod test_support;

use windhawk_core_host::HostErrorKind;
use windhawk_core_protocol::{ErrorCode, ModConfig, ModIdParams};

use crate::Environment;
use crate::args::{ModCommand, ModConfigCommand, ModSettingsCommand};
use crate::error::CliError;
use crate::output::CommandResult;

pub fn dispatch(
    env: &Environment,
    command: ModCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    match command {
        ModCommand::List(args) => list::list(env, args),
        ModCommand::Show { id } => show::show(env, &id),
        ModCommand::Enable { id } => lifecycle::set_enabled(env, &id, true),
        ModCommand::Disable { id } => lifecycle::set_enabled(env, &id, false),
        ModCommand::Remove { id } => lifecycle::remove(env, &id),
        ModCommand::Config { command } => match command {
            ModConfigCommand::Get { id, field } => config::config_get(env, &id, field.as_deref()),
            ModConfigCommand::Set { id, field, values } => {
                config::config_set(env, &id, &field, &values)
            }
        },
        ModCommand::Settings { command } => match command {
            ModSettingsCommand::Get { id, key } => settings::settings_get(env, &id, key.as_deref()),
            ModSettingsCommand::Set { id, key, value } => {
                settings::settings_set(env, &id, &key, &value)
            }
        },
        ModCommand::Install(args) => install::install(env, args),
        ModCommand::Update(args) => install::update(env, args),
        ModCommand::Compile { id } => install::compile(env, &id),
    }
}

// ---------------------------------------------------------------------------
// shared
// ---------------------------------------------------------------------------

/// `getModConfig`, mapping the not-installed `null` to a `MOD_NOT_INSTALLED`
/// error (exit 4).
fn require_config(env: &Environment, id: &str) -> Result<ModConfig, CliError> {
    let config: Option<ModConfig> = env.core.invoke_as(
        "getModConfig",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    config.ok_or_else(|| CliError::mod_not_installed(id))
}

/// `getModSource`, mapping the DLL's `MOD_NOT_INSTALLED` (a registered config
/// whose source file is missing) to the same exit-4 error with a clearer
/// message - the TS `getModSourceOrThrow`.
fn require_source(env: &Environment, id: &str) -> Result<String, CliError> {
    match env.core.invoke_as::<String, _>(
        "getModSource",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    ) {
        Ok(source) => Ok(source),
        // Discriminate on the WIRE truth, not the post-canonicalized exit class:
        // the core reports a registered config whose source file is missing as
        // MOD_NOT_INSTALLED, which the CLI re-messages at the same exit class.
        Err(error) => {
            if matches!(error.kind(), HostErrorKind::Wire(wire) if wire.code == ErrorCode::ModNotInstalled)
            {
                Err(CliError::mod_not_installed_with(format!(
                    "Mod '{id}' is registered in config but its source file is missing"
                )))
            } else {
                Err(error.into())
            }
        }
    }
}
