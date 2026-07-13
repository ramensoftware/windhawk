//! App-update handlers: `startUpdate` (download the installer, launch it) and
//! `cancelUpdate`. `startUpdate` is the ONE async command with progress events
//! the front-end consumes - `updateDownloadProgress` then `updateInstalling` -
//! so its [`AsyncKind`] carries a progress mapper alongside the terminal
//! shaper. `cancelUpdate` is synchronous: it signals the in-flight
//! `startUpdate` op's `CancelToken` through the registry (the extension's
//! `currentUpdateOp.cancel()`).

use serde_json::{Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::OperationEvent;

use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, AsyncOp, Outcome, Terminal};
use crate::ipc::reply;

/// `startUpdate`: start the update download/launch op. The reply is
/// `{ succeeded: true }` on completion, `{ succeeded: false, error }` on failure
/// (including a synchronous `UPDATE_IN_PROGRESS` start rejection, which the
/// extension surfaces as a rejected result rather than a throw). Progress events
/// drive `updateDownloadProgress`/`updateInstalling`.
pub fn start_update(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    match ctx.session.invoke_async("startUpdate", &json!({})) {
        Ok(op_id) => Ok(Outcome::Async(AsyncOp {
            op_id,
            kind: AsyncKind {
                terminal: Terminal::Shaped(installer_terminal),
                progress: Some(start_update_progress),
            },
            context: Value::Null,
        })),
        Err(error) => Ok(Outcome::Reply(installer_terminal(Err(error), &Value::Null))),
    }
}

/// `cancelUpdate`: signal the in-flight `startUpdate` op. `succeeded` is whether an
/// op was found and signaled (the extension's `currentUpdateOp?.cancel()`); the
/// op's own CANCELED terminal still produces the `startUpdate` reply. There is at
/// most one `startUpdate` in flight (the core rejects a concurrent one), so
/// finding it by command is unambiguous.
pub fn cancel_update(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let succeeded = ctx.ops.cancel_by_command("startUpdate");
    Ok(Outcome::Reply(json!({ "succeeded": succeeded })))
}

/// The shared installer terminal reply: `{ succeeded, error? }`. The `error` is the
/// failure's message string (the extension's `e.message`), not the standard error
/// payload. Shared by `startUpdate` and `startInstallDevTools` (both run the same
/// installer op and reply in the same shape; see [`crate::commands::devtools`]).
pub(crate) fn installer_terminal(outcome: Result<Value, HostError>, _ctx: &Value) -> Value {
    match outcome {
        Ok(_) => json!({ "succeeded": true }),
        Err(error) => json!({ "succeeded": false, "error": reply::error_message(&error) }),
    }
}

/// Map a `startUpdate` progress event to its front-end event envelope. The
/// download `progress` payload (`{ progress }`) forwards verbatim as
/// `updateDownloadProgress`; the one-shot `installing` becomes `updateInstalling`.
/// It is never handed a terminal event, so it cannot produce a reply.
fn start_update_progress(event: &OperationEvent) -> Vec<Envelope> {
    match event {
        OperationEvent::Progress { payload } => {
            vec![Envelope::event("updateDownloadProgress", payload.clone())]
        }
        OperationEvent::Installing => vec![Envelope::event("updateInstalling", json!({}))],
        OperationEvent::Completed { .. } | OperationEvent::Failed { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windhawk_core_protocol::{ErrorCode, WireError};

    #[test]
    fn terminal_maps_success_and_failure() {
        assert_eq!(
            installer_terminal(Ok(Value::Null), &Value::Null),
            json!({ "succeeded": true })
        );
        let err = HostError::wire(WireError::new(
            ErrorCode::UpdateInProgress,
            "already updating",
        ));
        assert_eq!(
            installer_terminal(Err(err), &Value::Null),
            json!({ "succeeded": false, "error": "already updating" })
        );
    }

    #[test]
    fn progress_maps_download_and_installing() {
        let download = OperationEvent::Progress {
            payload: json!({ "progress": 73 }),
        };
        let envelopes = start_update_progress(&download);
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].command, "updateDownloadProgress");
        assert_eq!(envelopes[0].data, json!({ "progress": 73 }));

        let installing = start_update_progress(&OperationEvent::Installing);
        assert_eq!(installing.len(), 1);
        assert_eq!(installing[0].command, "updateInstalling");
        assert_eq!(installing[0].data, json!({}));

        // A terminal event yields no progress envelope (it never reaches here).
        assert!(
            start_update_progress(&OperationEvent::Completed {
                result: Value::Null
            })
            .is_empty()
        );
    }
}
