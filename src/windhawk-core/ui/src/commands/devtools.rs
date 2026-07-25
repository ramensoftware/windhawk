//! Dev-tools install handlers: `startInstallDevTools` (download the Windhawk
//! installer and run it with the reinstall + `/DEVTOOLS` flags to add the optional
//! development tools) and `cancelInstallDevTools`. This is the update flow's twin:
//! it reuses the shared installer terminal shaper (`{ succeeded, error? }`,
//! [`crate::commands::update::installer_terminal`]) and the core's single-flight
//! `Update` lock. Only the progress event names differ from `startUpdate`
//! (`devToolsInstallDownloadProgress` / `devToolsInstalling`), so the front-end's
//! install-dev-tools modal listens on its own events.

use serde_json::{Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::OperationEvent;

use crate::commands::update::installer_terminal;
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, AsyncOp, Outcome, Terminal};
use crate::shape::webview_ipc::{InstallerReply, to_wire};

/// `startInstallDevTools`: start the dev-tools install download/launch op. The reply
/// is `{ succeeded: true }` on completion, `{ succeeded: false, error }` on failure
/// (including a synchronous `UPDATE_IN_PROGRESS` start rejection). Progress events
/// drive `devToolsInstallDownloadProgress`/`devToolsInstalling`.
pub fn start_install_dev_tools(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    match ctx.session.invoke_async("startInstallDevTools", &json!({})) {
        Ok(op_id) => Ok(Outcome::Async(AsyncOp {
            op_id,
            kind: AsyncKind {
                terminal: Terminal::Shaped(installer_terminal),
                progress: Some(install_progress),
                effect: None,
            },
            context: Value::Null,
        })),
        Err(error) => Ok(Outcome::Reply(installer_terminal(Err(error), &Value::Null))),
    }
}

/// `cancelInstallDevTools`: signal the in-flight `startInstallDevTools` op. There is
/// at most one in flight (the core's single-flight lock rejects a concurrent one), so
/// finding it by command is unambiguous.
pub fn cancel_install_dev_tools(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let succeeded = ctx.ops.cancel_by_command("startInstallDevTools");
    let reply = InstallerReply {
        succeeded,
        error: None,
    };
    Ok(Outcome::Reply(to_wire(reply)))
}

/// Map a `startInstallDevTools` progress event to its front-end event envelope. The
/// download `progress` payload forwards verbatim as `devToolsInstallDownloadProgress`;
/// the one-shot `installing` becomes `devToolsInstalling`. It is never handed a
/// terminal event, so it cannot produce a reply.
fn install_progress(event: &OperationEvent) -> Vec<Envelope> {
    match event {
        OperationEvent::Progress { payload } => {
            vec![Envelope::event(
                "devToolsInstallDownloadProgress",
                payload.clone(),
            )]
        }
        OperationEvent::Installing => vec![Envelope::event("devToolsInstalling", json!({}))],
        OperationEvent::Completed { .. } | OperationEvent::Failed { .. } => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_maps_to_the_devtools_events() {
        let download = OperationEvent::Progress {
            payload: json!({ "progress": 42 }),
        };
        let envelopes = install_progress(&download);
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].command, "devToolsInstallDownloadProgress");
        assert_eq!(envelopes[0].data, json!({ "progress": 42 }));

        let installing = install_progress(&OperationEvent::Installing);
        assert_eq!(installing.len(), 1);
        assert_eq!(installing[0].command, "devToolsInstalling");
        assert_eq!(installing[0].data, json!({}));

        // A terminal event yields no progress envelope (it never reaches here).
        assert!(
            install_progress(&OperationEvent::Completed {
                result: Value::Null
            })
            .is_empty()
        );
    }
}
