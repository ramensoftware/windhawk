//! User-data export/import handlers: `exportUserData`, `inspectUserData`,
//! `importUserData`, and `cancelImportUserData`. The core owns the archive format
//! and the transaction; this host owns the file dialogs and the archive file I/O.
//! So a handler that needs a file runs the native Save/Open picker
//! (`crate::file_dialog`) around the core call:
//!
//!  - `exportUserData` calls the core, then saves the returned archive string.
//!  - `inspectUserData` validates an archive via the session-free `inspectUserData`
//!    (like `parseModSource`), echoing the bytes back so a follow-up import needs no
//!    read. The archive is the one the request carries (the user pasted it), or the
//!    one an Open dialog picks and this host reads.
//!  - `importUserData` is async (it compiles): it drives the core import over the
//!    archive the webview holds and forwards per-mod progress as
//!    `importUserDataProgress` events; `cancelImportUserData` signals it.
//!
//! A cancelled dialog is a benign no-op (`canceled: true`, no error); a core failure
//! or a file-IO failure attaches the standard error object the front-end surfaces.

use std::path::Path;

use serde_json::{Value, json};
use windhawk_core_host::{HostError, SessionApiExt, arch_label};
use windhawk_core_protocol::{
    ErrorCode, ExportUserDataParams, ImportAppSettingsProgress, ImportAppSettingsStatus,
    ImportProgressItem, ImportUserDataParams, InspectUserDataParams, MAX_ARCHIVE_BYTES,
    OperationEvent, WireError,
};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::file_dialog::{DialogOutcome, FileDialog};
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, AsyncOp, HostEffect, Outcome, Terminal};
use crate::ipc::reply;
use crate::shape::webview_ipc::{
    ExportUserDataReply, ImportUserDataReply, InspectUserDataReply, InstallerReply, to_wire,
};

/// The default file name the export Save dialog seeds: a local timestamp plus a
/// `windhawk-backup` base (e.g. `2020-12-25-14h30m05-windhawk-backup.json`), so
/// exports are self-describing and sort chronologically in a folder.
fn default_archive_name() -> String {
    // SAFETY: GetLocalTime fills the SYSTEMTIME out-param; it has no failure mode.
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    format_archive_name(
        now.wYear,
        now.wMonth,
        now.wDay,
        now.wHour,
        now.wMinute,
        now.wSecond,
    )
}

/// Format the export default name from local calendar fields, split out from
/// [`default_archive_name`] so the exact pattern is unit-tested without the clock.
fn format_archive_name(
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
) -> String {
    format!("{year:04}-{month:02}-{day:02}-{hour:02}h{minute:02}m{second:02}-windhawk-backup.json")
}

/// `exportUserData`: aggregate the selected reads into an archive (the core), then
/// open a Save dialog and write it. The reply carries `succeeded` and the export
/// `summary` (per-mod warnings) on success, `canceled` on a dismissed dialog, and an
/// attached error on a core or IO failure.
pub fn export_user_data(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: ExportUserDataParams = serde_json::from_value(data.clone())?;
    let reply = match ctx.session.invoke("exportUserData", &params) {
        Ok(value) => finish_export(ctx.file_dialog.as_ref(), value),
        Err(error) => {
            eprintln!("windhawk-ui: exportUserData failed: {error}");
            let mut reply = to_wire(ExportUserDataReply {
                succeeded: false,
                summary: None,
                canceled: None,
            });
            reply::attach_error(&mut reply, &error);
            reply
        }
    };
    Ok(Outcome::Reply(reply))
}

/// The core export produced `{ archive, summary }`; run the Save dialog and write the
/// archive. A user cancel is a benign no-op; a write or dialog failure attaches the
/// error.
fn finish_export(dialog: &dyn FileDialog, value: Value) -> Value {
    let archive = value.get("archive").and_then(Value::as_str).unwrap_or("");
    let summary = value.get("summary").cloned();
    match dialog.save_archive(&default_archive_name()) {
        DialogOutcome::Picked(path) => match std::fs::write(&path, archive) {
            Ok(()) => to_wire(ExportUserDataReply {
                succeeded: true,
                summary,
                canceled: None,
            }),
            Err(e) => {
                eprintln!(
                    "windhawk-ui: exportUserData write to {} failed: {e}",
                    path.display()
                );
                let mut reply = to_wire(ExportUserDataReply {
                    succeeded: false,
                    summary: None,
                    canceled: None,
                });
                reply::attach_error(&mut reply, &io_error(&path, &e));
                reply
            }
        },
        DialogOutcome::Canceled => to_wire(ExportUserDataReply {
            succeeded: false,
            summary: None,
            canceled: Some(true),
        }),
        DialogOutcome::Failed(message) => {
            eprintln!("windhawk-ui: export Save dialog failed: {message}");
            let mut reply = to_wire(ExportUserDataReply {
                succeeded: false,
                summary: None,
                canceled: None,
            });
            reply::attach_error(&mut reply, &dialog_error(&message));
            reply
        }
    }
}

/// `inspectUserData`: validate an archive and project it to a manifest through the
/// session-free stateless transport (the archive is pure over its string). The
/// webview either hands over the archive text it holds (the user pasted it), or
/// leaves the pick to us: an Open dialog plus a read. The reply echoes the bytes
/// back as `archive` either way, so a follow-up import needs no file read.
pub fn inspect_user_data(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let archive = match data.get("archive").and_then(Value::as_str) {
        Some(pasted) => pasted.to_owned(),
        None => match pick_and_read_archive(ctx) {
            Ok(archive) => archive,
            Err(reply) => return Ok(Outcome::Reply(reply)),
        },
    };
    let params = InspectUserDataParams {
        archive: archive.clone(),
    };
    let reply = match ctx.core.invoke_stateless("inspectUserData", &params) {
        Ok(value) => to_wire(InspectUserDataReply {
            succeeded: true,
            manifest: value.get("manifest").cloned(),
            archive: Some(archive),
            canceled: None,
        }),
        Err(error) => {
            eprintln!("windhawk-ui: inspectUserData failed: {error}");
            let mut reply = failed_inspect();
            reply::attach_error(&mut reply, &error);
            reply
        }
    };
    Ok(Outcome::Reply(reply))
}

/// Run the Open dialog and read the picked archive. The error side is the finished
/// `inspectUserData` reply for the outcome that stopped it: a dismissed dialog (a
/// benign no-op), a dialog failure, or a file this will not read (unreadable, or
/// past the archive cap).
fn pick_and_read_archive(ctx: &BridgeCtx) -> Result<String, Value> {
    let path = match ctx.file_dialog.open_archive() {
        DialogOutcome::Picked(path) => path,
        DialogOutcome::Canceled => return Err(canceled_inspect()),
        DialogOutcome::Failed(message) => {
            eprintln!("windhawk-ui: inspect Open dialog failed: {message}");
            let mut reply = failed_inspect();
            reply::attach_error(&mut reply, &dialog_error(&message));
            return Err(reply);
        }
    };
    read_archive(&path).map_err(|error| {
        eprintln!(
            "windhawk-ui: inspectUserData read {} failed: {error}",
            path.display()
        );
        let mut reply = failed_inspect();
        reply::attach_error(&mut reply, &error);
        reply
    })
}

/// Read an archive file, refusing one past the archive cap by its SIZE first: the
/// read pulls the whole document into memory, so a file that cannot be a valid
/// archive must be rejected before it is read rather than after. The core still
/// enforces the cap over the string it is handed (the pasted-archive path never
/// touches a file).
fn read_archive(path: &Path) -> Result<String, HostError> {
    let size = std::fs::metadata(path)
        .map_err(|e| io_error(path, &e))?
        .len();
    if size > MAX_ARCHIVE_BYTES {
        return Err(too_large_error(path, size));
    }
    std::fs::read_to_string(path).map_err(|e| io_error(path, &e))
}

/// `importUserData`: drive the async core import over the archive the webview holds
/// (from an earlier inspect). Per-mod progress is forwarded as `importUserDataProgress`
/// events; the op's terminal becomes the `{ succeeded, summary }` reply. A synchronous
/// start failure replies inline through the SAME terminal (so the failure shape is
/// single-sourced with the async path).
///
/// The tray notification an app-settings import needs (a background restart, or the
/// lighter app-settings-changed ping) is fired by the CORE the moment it applies the
/// settings - so the restart overlaps the mod loop and survives a mid-import cancel -
/// rather than from a follow-up here. What the host owns is the announcement to its
/// own surfaces: the app-settings progress marker names
/// [`HostEffect::AppSettingsChanged`] (see [`import_effect`]), on which the bridge
/// pushes the imported settings to the front-end and re-themes the native window,
/// with the same timing.
pub fn import_user_data(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: ImportUserDataParams = serde_json::from_value(data.clone())?;
    match ctx.start_async("importUserData", &params) {
        Ok(start) => Ok(Outcome::Async(AsyncOp {
            start,
            kind: AsyncKind {
                terminal: Terminal::Shaped(import_terminal),
                progress: Some(import_progress),
                effect: Some(import_effect),
            },
            context: Value::Null,
        })),
        Err(error) => {
            eprintln!("windhawk-ui: importUserData could not start: {error}");
            let object = reply::error_object(&error);
            let mut reply = import_terminal(Err(error), &Value::Null);
            reply::attach_error_object(&mut reply, object);
            Ok(Outcome::Reply(reply))
        }
    }
}

/// `cancelImportUserData`: signal the in-flight `importUserData` op (the extension's
/// `currentImportOp?.cancel()`). `succeeded` is whether an op was found; its own
/// CANCELED terminal still produces the import reply. At most one import runs at a
/// time, so finding it by command is unambiguous.
pub fn cancel_import_user_data(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let succeeded = ctx.ops.cancel_by_command("importUserData");
    Ok(Outcome::Reply(to_wire(InstallerReply {
        succeeded,
        error: None,
    })))
}

/// The import terminal reply: `{ succeeded: true, summary }` from the completed
/// operation's `summary`, or `{ succeeded: false }` on failure (the pump attaches the
/// error object). A pure success/failure projection.
fn import_terminal(outcome: Result<Value, HostError>, _ctx: &Value) -> Value {
    match outcome {
        Ok(result) => to_wire(ImportUserDataReply {
            succeeded: true,
            summary: result.get("summary").cloned(),
        }),
        Err(_) => to_wire(ImportUserDataReply {
            succeeded: false,
            summary: None,
        }),
    }
}

/// Forward an import progress event to `importUserDataProgress`. The payload is either
/// a per-mod marker (forwarded verbatim) or a stamped install sub-event whose
/// `compileTarget` is mapped from the raw clang triple to the user-facing arch label
/// the GUI shows (via [`label_compile_target`]). A terminal event never reaches here,
/// so it cannot make a reply.
fn import_progress(event: &OperationEvent) -> Vec<Envelope> {
    match event {
        OperationEvent::Progress { payload } => {
            vec![Envelope::event(
                "importUserDataProgress",
                label_compile_target(payload),
            )]
        }
        OperationEvent::Installing
        | OperationEvent::Completed { .. }
        | OperationEvent::Failed { .. } => Vec::new(),
    }
}

/// The host effect an import's progress names: the app-settings step reporting
/// `applied` means the archive's app settings are on disk, so the front-end's
/// `appUISettings` - the language and the theme it renders with - and the native
/// window/editor theme are stale until the bridge re-announces them. Named at the
/// marker rather than at the import's terminal because the settings are applied
/// BEFORE the mod loop, which can run for minutes; the user sees the imported
/// language and theme take effect as they land. A per-mod marker names nothing.
fn import_effect(event: &OperationEvent) -> Option<HostEffect> {
    let OperationEvent::Progress { payload } = event else {
        return None;
    };
    let marker: ImportAppSettingsProgress = serde_json::from_value(payload.clone()).ok()?;
    match (marker.item, marker.status) {
        (ImportProgressItem::AppSettings, ImportAppSettingsStatus::Applied) => {
            Some(HostEffect::AppSettingsChanged)
        }
        _ => None,
    }
}

/// Rewrite a forwarded progress payload's `compileTarget` from the raw clang triple
/// to the user-facing arch label (x86 / x64 / ARM64), so the GUI's
/// `Compiling {{modId}} for {{target}}...` line names a target the way the CLI's
/// `Compiling ... for <arch>...` does. A per-mod marker (no `compileTarget`) is
/// returned unchanged.
fn label_compile_target(payload: &Value) -> Value {
    let mut payload = payload.clone();
    if let Some(triple) = payload.get("compileTarget").and_then(Value::as_str) {
        let label = arch_label(triple).to_owned();
        if let Some(object) = payload.as_object_mut() {
            object.insert("compileTarget".to_owned(), Value::String(label));
        }
    }
    payload
}

/// The base `inspectUserData` failure reply (no manifest, no archive), before an error
/// object is attached.
fn failed_inspect() -> Value {
    to_wire(InspectUserDataReply {
        succeeded: false,
        manifest: None,
        archive: None,
        canceled: None,
    })
}

/// The `inspectUserData` reply for a dismissed Open dialog: a benign no-op.
fn canceled_inspect() -> Value {
    to_wire(InspectUserDataReply {
        succeeded: false,
        manifest: None,
        archive: None,
        canceled: Some(true),
    })
}

/// A file-IO failure as the standard wire error, carrying the failing `path` (the
/// most useful locus, surfaced by the front-end's unified error notification).
fn io_error(path: &Path, e: &std::io::Error) -> HostError {
    HostError::wire(WireError::with_details(
        ErrorCode::IoFailed,
        e.to_string(),
        json!({ "path": path.display().to_string() }),
    ))
}

/// An over-the-cap archive file as the standard wire error, carrying the failing
/// `path` like [`io_error`]. Coded and worded like the core's own rejection of an
/// oversized archive, so which layer caught it does not change what the
/// front-end shows.
fn too_large_error(path: &Path, size: u64) -> HostError {
    HostError::wire(WireError::with_details(
        ErrorCode::InvalidRequest,
        format!("archive is too large ({size} bytes; the maximum is {MAX_ARCHIVE_BYTES})"),
        json!({ "path": path.display().to_string() }),
    ))
}

/// A file-dialog (COM/shell) failure as an internal wire error.
fn dialog_error(message: &str) -> HostError {
    HostError::wire(WireError::new(
        ErrorCode::Internal,
        format!("file dialog: {message}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_archive_name_zero_pads_and_reads_as_a_timestamp() {
        assert_eq!(
            format_archive_name(2020, 12, 25, 14, 30, 5),
            "2020-12-25-14h30m05-windhawk-backup.json"
        );
        // Single-digit month/day/hour are zero-padded so names sort chronologically.
        assert_eq!(
            format_archive_name(2026, 1, 2, 3, 4, 9),
            "2026-01-02-03h04m09-windhawk-backup.json"
        );
    }

    /// A completed `importUserData` result carrying the app-settings intents.
    fn completed_with_intents(requires_restart: bool, requires_notify: bool) -> Value {
        json!({
            "summary": {
                "mods": [],
                "appSettings": {
                    "requiresRestart": requires_restart,
                    "requiresNotify": requires_notify,
                },
            },
        })
    }

    #[test]
    fn import_terminal_shapes_success_and_failure() {
        let completed = completed_with_intents(true, false);
        let reply = import_terminal(Ok(completed), &Value::Null);
        assert_eq!(reply["succeeded"], json!(true));
        assert_eq!(
            reply["summary"]["appSettings"]["requiresRestart"],
            json!(true)
        );

        let reply = import_terminal(Err(HostError::decode("x".to_owned())), &Value::Null);
        assert_eq!(reply, json!({ "succeeded": false }));
    }

    #[test]
    fn only_the_applied_app_settings_marker_names_the_announce_effect() {
        let applied = json!({ "item": "appSettings", "status": "applied" });
        assert_eq!(
            import_effect(&OperationEvent::Progress { payload: applied }),
            Some(HostEffect::AppSettingsChanged)
        );

        // The start of the step names nothing: the settings are not on disk yet.
        let applying = json!({ "item": "appSettings", "status": "applying" });
        assert_eq!(
            import_effect(&OperationEvent::Progress { payload: applying }),
            None
        );

        // A per-mod marker changes no app-level state, whatever its status.
        let installed = json!({
            "item": "mod", "modId": "m", "index": 0, "total": 1, "status": "installed"
        });
        assert_eq!(
            import_effect(&OperationEvent::Progress { payload: installed }),
            None
        );

        // Only progress events are offered; a terminal cannot name an effect.
        assert_eq!(import_effect(&OperationEvent::Installing), None);
        assert_eq!(
            import_effect(&OperationEvent::Completed {
                result: Value::Null
            }),
            None
        );
    }

    #[test]
    fn import_progress_forwards_progress_payloads_only() {
        let payload = json!({ "modId": "m", "index": 0, "total": 3, "status": "installing" });
        let envelopes = import_progress(&OperationEvent::Progress {
            payload: payload.clone(),
        });
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].command, "importUserDataProgress");
        // A per-mod marker (no compileTarget) forwards verbatim.
        assert_eq!(envelopes[0].data, payload);

        // A compile sub-event's raw triple is mapped to the arch label the GUI shows;
        // the rest of the payload (the mod dimension) is left untouched.
        let compile =
            json!({ "modId": "m", "index": 1, "total": 3, "compileTarget": "i686-w64-mingw32" });
        let envelopes = import_progress(&OperationEvent::Progress { payload: compile });
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].data["compileTarget"], json!("x86"));
        assert_eq!(envelopes[0].data["modId"], json!("m"));
        assert_eq!(envelopes[0].data["index"], json!(1));

        // A non-progress event yields nothing (it cannot make a reply from the mapper).
        assert!(import_progress(&OperationEvent::Installing).is_empty());
        assert!(
            import_progress(&OperationEvent::Completed {
                result: Value::Null
            })
            .is_empty()
        );
    }
}
