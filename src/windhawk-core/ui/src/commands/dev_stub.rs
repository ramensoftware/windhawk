//! The development-command stubs. The launch entry points (`createNewMod` /
//! `editMod` / `forkMod`) are handled by `commands::dev`; the rest of the
//! development surface is the sidebar/editor-mode commands, which the native UI
//! never sends (it hosts no sidebar), so this stub is defense in depth. A
//! `reply`-type command returns the typed `UNSUPPORTED` rejection as its reply;
//! a `message`-type one has no `messageId`, so it only logs. The reply-vs-log
//! split is decided by the envelope (whether the front-end correlated it), not
//! duplicated as a per-command kind here.

use serde_json::Value;

use crate::ipc::outcome::Outcome;
use crate::ipc::reply;

/// The development commands. `forkModFromSource` is NOT a distinct command: the
/// front-end sends `forkMod` with an optional `modSource`
/// (`webviewIPCMessages.ts` `ForkModData`), so only `forkMod` appears here.
const DEV_COMMANDS: &[&str] = &[
    // Launch entry points: handled by commands::dev before reaching the stub, and
    // listed here only so is_development_command still classifies them as development
    // commands (they never actually route to the stub).
    "createNewMod",
    "editMod",
    "forkMod",
    // Sidebar / editor-mode message-type commands: stubbed (the native UI never sends
    // them).
    "getInitialSidebarParams",
    "stopCompileEditedMod",
    "previewEditedMod",
    // reply-type (editor mode, never entered)
    "enableEditedMod",
    "enableEditedModLogging",
    "compileEditedMod",
    "exitEditorMode",
];

/// Whether `command` is an out-of-scope development command.
pub fn is_development_command(command: &str) -> bool {
    DEV_COMMANDS.contains(&command)
}

/// Stub a development command with a typed `UNSUPPORTED` rejection. `expects_reply`
/// is the envelope's correlation: a `messageWithReply` gets the error as its reply;
/// a fire-and-forget `message` only logs.
pub fn handle(command: &str, expects_reply: bool) -> Outcome {
    if expects_reply {
        Outcome::Reply(unsupported_payload(command))
    } else {
        eprintln!(
            "windhawk-ui: development command '{command}' is unsupported in the non-development UI"
        );
        Outcome::Done
    }
}

/// The typed `UNSUPPORTED` error payload for a development command.
pub fn unsupported_payload(command: &str) -> Value {
    reply::ui_error_payload(
        "UNSUPPORTED",
        &format!("'{command}' is a development command, unsupported in the non-development UI"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_development_commands() {
        assert!(is_development_command("createNewMod"));
        assert!(is_development_command("compileEditedMod"));
        // forkModFromSource is folded into forkMod; not a distinct command.
        assert!(!is_development_command("forkModFromSource"));
        // Non-development and read commands are not stubbed.
        assert!(!is_development_command("getInstalledMods"));
        assert!(!is_development_command("deleteMod"));
    }

    #[test]
    fn reply_form_returns_unsupported_payload() {
        let Outcome::Reply(data) = handle("enableEditedMod", true) else {
            panic!("expected a reply outcome");
        };
        assert_eq!(data["error"]["code"], json!("UNSUPPORTED"));
    }

    #[test]
    fn message_form_is_fire_and_forget() {
        // A stubbed message-type command (the launch ones are handled elsewhere).
        assert!(matches!(handle("previewEditedMod", false), Outcome::Done));
    }
}
