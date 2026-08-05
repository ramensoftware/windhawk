//! Total inbound dispatch: a statically-typed match from the envelope's
//! `command` to a handler. The synchronous reads/writes, the async commands,
//! and the log-window commands are all wired here. The three launch entry
//! points (`createNewMod` / `editMod` / `forkMod`) route to the real
//! `commands::dev` handlers (development is always on for the native build);
//! the remaining sidebar/editor-mode development commands route to the
//! `UNSUPPORTED` stub, defense in depth for a sidebar-less window that never
//! sends them. An unknown command is a typed `INVALID_REQUEST`. Dispatch is
//! total: it never panics on an unknown command.

use serde_json::Value;
use windhawk_core_host::HostError;

use crate::commands;
use crate::commands::dev_stub;
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::outcome::Outcome;
use crate::ipc::reply;

/// Route one command to its handler. `expects_reply` is whether the front-end
/// correlated the request (a `messageWithReply`); it decides whether an
/// out-of-scope / unknown command emits an error `reply` or only logs.
pub fn dispatch(
    ctx: &BridgeCtx,
    command: &str,
    data: &Value,
    expects_reply: bool,
) -> Result<Outcome, HostError> {
    match command {
        "getInitialAppSettings" => commands::app::get_initial_app_settings(ctx, data),
        "getAppSettings" => commands::app::get_app_settings(ctx, data),
        "updateAppSettings" => commands::app::update_app_settings(ctx, data),
        "getInstalledMods" => commands::mods::get_installed_mods(ctx, data),
        "getModConfig" => commands::mods::get_mod_config(ctx, data),
        "getModSettings" => commands::mods::get_mod_settings(ctx, data),
        "getModSourceData" => commands::mods::get_mod_source_data(ctx, data),
        "updateModConfig" => commands::mods::update_mod_config(ctx, data),
        "setModSettings" => commands::mods::set_mod_settings(ctx, data),
        "enableMod" => commands::mods::enable_mod(ctx, data),
        "deleteMod" => commands::mods::delete_mod(ctx, data),
        "updateModRating" => commands::mods::update_mod_rating(ctx, data),
        "installMod" => commands::mods::install_mod(ctx, data),
        "cancelInstallMod" => commands::mods::cancel_install_mod(ctx, data),
        "compileMod" => commands::mods::compile_mod(ctx, data),
        "cancelCompileMod" => commands::mods::cancel_compile_mod(ctx, data),
        "getFeaturedMods" => commands::repo::get_featured_mods(ctx, data),
        "getRepositoryMods" => commands::repo::get_repository_mods(ctx, data),
        "getRepositoryModSourceData" => commands::repo::get_repository_mod_source_data(ctx, data),
        "getModVersions" => commands::repo::get_mod_versions(ctx, data),
        "startUpdate" => commands::update::start_update(ctx, data),
        "cancelUpdate" => commands::update::cancel_update(ctx, data),
        "exportUserData" => commands::userdata::export_user_data(ctx, data),
        "inspectUserData" => commands::userdata::inspect_user_data(ctx, data),
        "importUserData" => commands::userdata::import_user_data(ctx, data),
        "cancelImportUserData" => commands::userdata::cancel_import_user_data(ctx, data),
        "startInstallDevTools" => commands::devtools::start_install_dev_tools(ctx, data),
        "cancelInstallDevTools" => commands::devtools::cancel_install_dev_tools(ctx, data),
        "showLogOutput" | "showAdvancedDebugLogOutput" => commands::logwindow::show(ctx, data),
        // The launch entry points route to the real handlers (development is
        // always on for the native build).
        "createNewMod" | "editMod" | "forkMod" => commands::dev::handle(ctx, command, data),
        other => Ok(out_of_scope(other, expects_reply)),
    }
}

/// Handle a command with no handler arm: a development command (the sidebar/editor-mode
/// ones, which have no native handler) routes to the `UNSUPPORTED` stub; anything else
/// is a typed `INVALID_REQUEST`. A fire-and-forget `message` only logs (no reply
/// channel); a `messageWithReply` gets the typed error as its reply.
fn out_of_scope(command: &str, expects_reply: bool) -> Outcome {
    if dev_stub::is_development_command(command) {
        return dev_stub::handle(command, expects_reply);
    }
    if expects_reply {
        Outcome::Reply(invalid_request_payload(command))
    } else {
        eprintln!("windhawk-ui: ignoring unknown command '{command}'");
        Outcome::Done
    }
}

/// The typed `INVALID_REQUEST` payload for an unknown (or not-yet-implemented)
/// command, kept distinct from the development `UNSUPPORTED` so the two stay
/// separable in logs and on the wire.
fn invalid_request_payload(command: &str) -> Value {
    reply::ui_error_payload("INVALID_REQUEST", &format!("unknown command '{command}'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_reply_command_is_invalid_request() {
        let Outcome::Reply(data) = invalid_request_outcome("noSuchCommand", true) else {
            panic!("expected a reply");
        };
        assert_eq!(data["error"]["code"], json!("INVALID_REQUEST"));
    }

    #[test]
    fn unknown_message_command_only_logs() {
        assert!(matches!(
            invalid_request_outcome("noSuchCommand", false),
            Outcome::Done
        ));
    }

    /// The unknown branch in isolation (no `BridgeCtx`): a non-development command
    /// with no handler arm. The development branch is exercised by `dev_stub`'s own
    /// tests; the read-command arms are integration-covered (they touch a session).
    fn invalid_request_outcome(command: &str, expects_reply: bool) -> Outcome {
        // Mirrors `out_of_scope`'s non-development path; `dev_stub::is_development_command`
        // is false here, so this is the INVALID_REQUEST half.
        assert!(!dev_stub::is_development_command(command));
        if expects_reply {
            Outcome::Reply(invalid_request_payload(command))
        } else {
            Outcome::Done
        }
    }
}
