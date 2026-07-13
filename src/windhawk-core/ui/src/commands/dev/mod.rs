//! The four launch entry-point handlers: the native half of authoring - resolve
//! a source and an id, prepare a per-mod workspace, and open VSCodium on it.
//! They mirror the extension's `createNewMod` / `editMod` / `forkMod` step for
//! step, with the workspace op swapped for the multi-workspace model
//! ([`crate::editor::workspace`]) and the spawn behind the launch seam
//! ([`crate::editor::launch::LaunchEditor`]).
//!
//! All three are `messageWithReply`s: the handler answers the front-end with a
//! small reply so it can react. Success is an empty object; a missing editor
//! (the development tools are an optional install component, so the resolved UI
//! path can be empty) is `{ uiMissing: true }`, which the front-end turns into
//! the "install development tools" modal; any other failure is the standard
//! `error` payload, auto-surfaced like any command error.
//!
//! The core interop is only already-dispatched, synchronous/stateless commands:
//! `getModSource` / `doesModExist` on the session, and the stateless
//! `parseModSource` / `appendToModIdAndName` / `getCompileFlags` off the
//! [`GatedCore`](windhawk_core_host::GatedCore). The native side adds no id
//! logic: it only chooses the collision suffixes and lets the core apply them.

mod edit;
mod fork;
mod new;

use std::fmt;
use std::io;

use serde_json::{Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    AppendToModIdAndNameParams, ModIdParams, ParseModSourceParams, ParsedModSource,
};

use crate::editor::launch::LaunchError;
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::outcome::Outcome;
use crate::ipc::reply;

/// Route one launch entry point to its handler and shape its outcome into the
/// reply. Dispatch forwards only the three launch commands here; any other
/// command is a caller bug. The editor availability is checked FIRST, before
/// any workspace work, so a missing-editor install replies `uiMissing` without
/// allocating an orphan workspace.
pub fn handle(ctx: &BridgeCtx, command: &str, data: &Value) -> Result<Outcome, HostError> {
    Ok(Outcome::Reply(dev_reply(run_command(ctx, command, data))))
}

fn run_command(ctx: &BridgeCtx, command: &str, data: &Value) -> Result<(), DevError> {
    if !ctx.editor.launcher().is_available() {
        return Err(DevError::UiMissing);
    }
    match command {
        "createNewMod" => new::run(ctx),
        "editMod" => edit::run(ctx, data),
        "forkMod" => fork::run(ctx, data),
        other => {
            eprintln!("windhawk-ui: dev::handle reached with an unexpected command '{other}'");
            Ok(())
        }
    }
}

/// Shape a launch entry point's outcome into its reply. Success is an empty
/// object. A missing editor is `{ uiMissing: true }` - a distinct signal the
/// front-end turns into the "install development tools" modal, deliberately NOT
/// the standard `error` object, so the IPC layer does not auto-surface it as a
/// generic error notification. Any other failure is the standard `{ error: {
/// code, message, .. } }` payload, which the front-end auto-surfaces like any
/// command error.
fn dev_reply(result: Result<(), DevError>) -> Value {
    match result {
        Ok(()) => json!({}),
        Err(DevError::UiMissing) => json!({ "uiMissing": true }),
        Err(DevError::Host(error)) => reply::host_error_payload(&error),
        Err(error) => reply::ui_error_payload(error.code(), &error.to_string()),
    }
}

/// Garbage-collect abandoned workspaces, run at native-UI startup and after a
/// `deleteMod`. Best-effort. The keep/reclaim decision reads `doesModExist` per
/// workspace; a core failure there defaults to keep, so a transient error never
/// reclaims a real mod's workspace. On a fresh install (no container yet) it is
/// a no-op.
pub(crate) fn sweep_abandoned_workspaces(ctx: &BridgeCtx) {
    let result = ctx.editor.workspaces().sweep(
        |storage_id| does_mod_exist(ctx, storage_id).unwrap_or(true),
        |source| parse_bare_id(ctx, source),
    );
    match result {
        Ok(report) => {
            if !report.reclaimed.is_empty() || !report.in_use.is_empty() {
                eprintln!(
                    "windhawk-ui: editor workspace sweep - kept {:?}, reclaimed {:?}, in use {:?}",
                    report.kept, report.reclaimed, report.in_use
                );
            }
        }
        Err(error) => eprintln!("windhawk-ui: editor workspace sweep failed: {error}"),
    }
}

/// A launch-flow failure, shaped into the handler's reply by [`dev_reply`].
#[derive(Debug)]
enum DevError {
    /// The development tools are not installed (the resolved UI path is empty), so
    /// there is no editor to launch. Replied as `{ uiMissing: true }`.
    UiMissing,
    /// A core call failed (source read, parse, collision check, id transform, flags).
    Host(HostError),
    /// Preparing or seeding the workspace directory failed.
    Io(io::Error),
    /// Opening VSCodium failed (editor exe not found, or a spawn I/O error).
    Launch(LaunchError),
    /// The source carried no parseable `@id`, so the mod cannot be named or located.
    MissingId,
    /// A fork's source `@id` did not match the mod being forked.
    IdMismatch { expected: String, actual: String },
}

impl DevError {
    /// The wire error `code` for the failures shaped through [`reply::ui_error_payload`]
    /// (the non-`Host`, non-`UiMissing` arms, which carry no [`HostError`] to source a
    /// code from). `UiMissing`/`Host` never reach here - they are shaped separately.
    fn code(&self) -> &'static str {
        match self {
            DevError::Io(_) | DevError::Launch(_) => "IO_FAILED",
            DevError::MissingId | DevError::IdMismatch { .. } => "INVALID_REQUEST",
            DevError::UiMissing | DevError::Host(_) => "INTERNAL",
        }
    }
}

impl fmt::Display for DevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevError::UiMissing => write!(f, "the development tools are not installed"),
            DevError::Host(error) => write!(f, "{error}"),
            DevError::Io(error) => write!(f, "{error}"),
            DevError::Launch(error) => write!(f, "{error}"),
            DevError::MissingId => write!(f, "the mod source has no parseable @id"),
            DevError::IdMismatch { expected, actual } => write!(
                f,
                "the source @id '{actual}' does not match the mod being forked ('{expected}')"
            ),
        }
    }
}

impl From<HostError> for DevError {
    fn from(error: HostError) -> Self {
        DevError::Host(error)
    }
}

impl From<io::Error> for DevError {
    fn from(error: io::Error) -> Self {
        DevError::Io(error)
    }
}

impl From<LaunchError> for DevError {
    fn from(error: LaunchError) -> Self {
        DevError::Launch(error)
    }
}

impl From<serde_json::Error> for DevError {
    fn from(error: serde_json::Error) -> Self {
        // A malformed `data` decodes through the host's `Decode` wording, then logs
        // like any other core failure (fire-and-forget, so never a reply).
        DevError::Host(error.into())
    }
}

/// The stored source of an installed mod (`getModSource`). ENOENT surfaces as the
/// core's `MOD_NOT_INSTALLED`, which the handler logs and aborts on.
fn get_mod_source(ctx: &BridgeCtx, mod_id: &str) -> Result<String, DevError> {
    Ok(ctx.session.invoke_as::<String, _>(
        "getModSource",
        &ModIdParams {
            mod_id: mod_id.to_owned(),
        },
    )?)
}

/// Whether a storage id is occupied (`doesModExist`, `local@<id>` scope already
/// applied by the caller). The collision check the `-N` / `-fork` loops drive.
fn does_mod_exist(ctx: &BridgeCtx, storage_id: &str) -> Result<bool, DevError> {
    Ok(ctx.session.invoke_as::<bool, _>(
        "doesModExist",
        &ModIdParams {
            mod_id: storage_id.to_owned(),
        },
    )?)
}

/// The bare `@id` from a source (`parseModSource`, stateless), or `None` when the
/// source has no parseable metadata id. The language is irrelevant to the id, so it
/// is fixed to `en`. Also the `parse_id` fallback the workspace manager's
/// locate/sweep injects.
fn parse_bare_id(ctx: &BridgeCtx, source: &str) -> Option<String> {
    ctx.core
        .invoke_stateless_as::<ParsedModSource, _>(
            "parseModSource",
            &ParseModSourceParams {
                source: source.to_owned(),
                language: "en".to_owned(),
            },
        )
        .ok()
        .and_then(|parsed| parsed.metadata)
        .and_then(|metadata| metadata.id)
}

/// Append a suffix to a source's `@id` and every `@name[:lang]`
/// (`appendToModIdAndName`, stateless). The core owns the transform; the native side
/// only picks the suffixes.
fn append_id_and_name(
    ctx: &BridgeCtx,
    source: &str,
    id_suffix: &str,
    name_suffix: &str,
) -> Result<String, DevError> {
    Ok(ctx.core.invoke_stateless_as::<String, _>(
        "appendToModIdAndName",
        &AppendToModIdAndNameParams {
            source: source.to_owned(),
            append_to_id: Some(id_suffix.to_owned()),
            append_to_name: Some(name_suffix.to_owned()),
        },
    )?)
}

/// The clangd flag set for `compile_flags.txt` (`getCompileFlags`, stateless).
/// The canonical list, written by the workspace initializer verbatim.
fn compile_flags(ctx: &BridgeCtx) -> Result<Vec<String>, DevError> {
    Ok(ctx
        .core
        .invoke_stateless_as::<Vec<String>, _>("getCompileFlags", &json!({}))?)
}

/// Find the first free id/name suffix for a new/fork mod: for each attempt `n` from
/// `start`, form `local@<base_id><id_suffix>` and return the suffixes of the first
/// that `doesModExist` says is free. `suffix(n)` yields the `(id_suffix, name_suffix)`
/// pair per attempt; `createNewMod` starts at 0 (attempt 0 = no suffix, the bare id)
/// and `forkMod` at 1 (a fork is always suffixed). The resulting id is exactly
/// `base_id + id_suffix` because `appendToModIdAndName` concatenates the suffix onto
/// the `@id`, so the check computes it directly rather than re-parsing each attempt.
fn find_free_suffix(
    ctx: &BridgeCtx,
    base_id: &str,
    start: u32,
    suffix: impl Fn(u32) -> (String, String),
) -> Result<(String, String), DevError> {
    let mut n = start;
    loop {
        let (id_suffix, name_suffix) = suffix(n);
        let candidate = format!("local@{base_id}{id_suffix}");
        if !does_mod_exist(ctx, &candidate)? {
            return Ok((id_suffix, name_suffix));
        }
        n += 1;
    }
}
