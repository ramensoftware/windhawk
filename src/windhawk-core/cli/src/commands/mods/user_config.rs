//! `mod user-config status`/`enable`/`disable`/`delete`: which configuration of
//! a mod the current Windows account uses - its own or the global one - and the
//! commands that switch and delete it. The account's programs and every other
//! `mod` command follow that choice.
//!
//! The account is the one the CLI runs as, so an elevated CLI acts for the
//! account that elevated it; every result names it. Portable mode has no user
//! configurations, so there every command is a usage error.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{
    CoreInfo, ModConfig, ModIdParams, SessionUser, SetModUserScopeParams, UserScope,
};

use crate::Environment;
use crate::error::CliError;
use crate::output::CommandResult;

/// The account the core acts for (`getCoreInfo`'s `user`), or the usage error
/// of a portable install.
fn account(env: &Environment) -> Result<SessionUser, CliError> {
    let info: CoreInfo = env.core.invoke_as("getCoreInfo", &json!({}))?;
    if info.portable {
        return Err(CliError::usage(
            "Mod user configurations are not available in portable mode.",
        ));
    }
    Ok(info.user)
}

fn account_label(account: &SessionUser) -> String {
    if account.name == account.sid {
        account.sid.clone()
    } else {
        format!("{} ({})", account.name, account.sid)
    }
}

// ---------------------------------------------------------------------------
// mod user-config status
// ---------------------------------------------------------------------------

pub(super) fn status(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    let account = account(env)?;
    let config = super::require_config(env, id)?;
    Ok(Box::new(UserConfigResult {
        id: id.to_owned(),
        account,
        user_scope: config.user_scope,
        changed: false,
        action: Action::Status,
    }))
}

// ---------------------------------------------------------------------------
// mod user-config enable / disable
// ---------------------------------------------------------------------------

/// `enable` switches the account's own configuration on, creating it as a copy
/// of the global one when there is none - which, like `mod remove`, needs
/// `--yes`, since from then on the account's programs stop following the global
/// configuration. `disable` switches it off, back to the global one, and keeps
/// it for a later `enable`. Already in the asked-for state is a no-op.
pub(super) fn set_active(
    env: &Environment,
    id: &str,
    active: bool,
) -> Result<Box<dyn CommandResult>, CliError> {
    let account = account(env)?;
    let config = super::require_config(env, id)?;
    let currently_active = config.user_scope == UserScope::Active;
    let action = if active {
        Action::Enable
    } else {
        Action::Disable
    };
    if currently_active == active {
        return Ok(Box::new(UserConfigResult {
            id: id.to_owned(),
            account,
            user_scope: config.user_scope,
            changed: false,
            action,
        }));
    }

    if active && config.user_scope == UserScope::None && !env.yes {
        eprintln!(
            "Would give the Windows account {} its own configuration of mod '{id}': \
             the mod's settings, its enabled state and its advanced options would apply \
             to that account's programs only, starting as a copy of the current \
             configuration. Other accounts keep the global configuration, and changes \
             to it would no longer affect this account. The mod reloads once in the \
             account's running programs. Pass --yes to confirm.",
            account_label(&account)
        );
        return Err(CliError::usage(format!(
            "Refusing to create a user configuration of '{id}' without --yes"
        )));
    }

    let config: ModConfig = env.core.invoke_as(
        "setModUserScope",
        &SetModUserScopeParams {
            mod_id: id.to_owned(),
            active,
        },
    )?;
    Ok(Box::new(UserConfigResult {
        id: id.to_owned(),
        account,
        user_scope: config.user_scope,
        changed: true,
        action,
    }))
}

// ---------------------------------------------------------------------------
// mod user-config delete
// ---------------------------------------------------------------------------

/// Remove the account's own configuration, leaving the global one in use.
/// Requires `--yes`, like `mod remove`: what the account set there is lost. An
/// account with none is a no-op.
pub(super) fn delete(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    let account = account(env)?;
    let config = super::require_config(env, id)?;
    if config.user_scope == UserScope::None {
        return Ok(Box::new(UserConfigResult {
            id: id.to_owned(),
            account,
            user_scope: config.user_scope,
            changed: false,
            action: Action::Delete,
        }));
    }

    if !env.yes {
        eprintln!(
            "Would delete the configuration of mod '{id}' that belongs to the Windows \
             account {}, and what that account set there; the account would use the \
             global configuration. Pass --yes to confirm.",
            account_label(&account)
        );
        return Err(CliError::usage(format!(
            "Refusing to delete the user configuration of '{id}' without --yes"
        )));
    }

    let config: ModConfig = env.core.invoke_as(
        "deleteModUserConfig",
        &ModIdParams {
            mod_id: id.to_owned(),
        },
    )?;
    Ok(Box::new(UserConfigResult {
        id: id.to_owned(),
        account,
        user_scope: config.user_scope,
        changed: true,
        action: Action::Delete,
    }))
}

// ---------------------------------------------------------------------------
// output
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Action {
    Status,
    Enable,
    Disable,
    Delete,
}

struct UserConfigResult {
    id: String,
    account: SessionUser,
    user_scope: UserScope,
    changed: bool,
    action: Action,
}

impl CommandResult for UserConfigResult {
    fn json_data(&self) -> Value {
        let mut data = json!({
            "id": self.id,
            "userScope": self.user_scope,
            "account": { "sid": self.account.sid, "name": self.account.name },
        });
        if !matches!(self.action, Action::Status) {
            data["changed"] = Value::Bool(self.changed);
        }
        data
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let id = &self.id;
        let account = account_label(&self.account);
        let line = match (self.action, self.changed, self.user_scope) {
            (Action::Status, _, UserScope::None) => {
                format!("{id}: none - {account} uses the global configuration")
            }
            (Action::Status, _, UserScope::Inactive) => format!(
                "{id}: inactive - {account} has its own configuration, switched off, and \
                 uses the global one"
            ),
            (Action::Status, _, UserScope::Active) => {
                format!("{id}: active - {account} uses its own configuration")
            }
            (Action::Enable, true, _) => {
                format!("Using the configuration of {account}: {id}")
            }
            (Action::Enable, false, _) => {
                format!("Already using the configuration of {account}: {id}")
            }
            (Action::Disable, true, _) => format!(
                "Using the global configuration: {id} (the configuration of {account} \
                 is kept for a later enable)"
            ),
            (Action::Disable, false, _) => {
                format!("Already using the global configuration: {id}")
            }
            (Action::Delete, true, _) => {
                format!("Deleted the configuration of {account}: {id}")
            }
            (Action::Delete, false, _) => {
                format!("No configuration of {account} to delete: {id}")
            }
        };
        writeln!(out, "{line}")
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    fn result(action: Action, changed: bool, user_scope: UserScope) -> UserConfigResult {
        UserConfigResult {
            id: "m".to_owned(),
            account: SessionUser {
                sid: "S-1-5-21-1-2-3-1001".to_owned(),
                name: "HOST\\alice".to_owned(),
            },
            user_scope,
            changed,
            action,
        }
    }

    #[test]
    fn status_names_the_scope_and_the_account() {
        let text = render_text(&result(Action::Status, false, UserScope::Inactive));
        assert!(
            text.starts_with("m: inactive - HOST\\alice (S-1-5-21-1-2-3-1001)"),
            "{text}"
        );
        assert_eq!(
            result(Action::Status, false, UserScope::Active).json_data(),
            json!({
                "id": "m",
                "userScope": "active",
                "account": { "sid": "S-1-5-21-1-2-3-1001", "name": "HOST\\alice" },
            })
        );
    }

    #[test]
    fn an_account_without_a_name_is_shown_by_its_sid_once() {
        let mut unnamed = result(Action::Delete, true, UserScope::None);
        unnamed.account.name = unnamed.account.sid.clone();
        assert_eq!(
            render_text(&unnamed),
            "Deleted the configuration of S-1-5-21-1-2-3-1001: m\n"
        );
    }

    #[test]
    fn the_writes_report_whether_they_changed_anything() {
        assert_eq!(
            render_text(&result(Action::Enable, false, UserScope::Active)),
            "Already using the configuration of HOST\\alice (S-1-5-21-1-2-3-1001): m\n"
        );
        assert_eq!(
            result(Action::Disable, true, UserScope::Inactive).json_data()["changed"],
            json!(true)
        );
    }
}
