//! `_diagEmitEvents`: internal diagnostic command (not part of the contract
//! inventory; no compatibility promise). Emits `events` progress events
//! `{"seq": n}` with a cancel-aware wait of `intervalMs` between them, then
//! completes with `{"emitted": n}`. Exists so the ABI suite and the bridge can
//! verify async event ordering, terminal-event-exactly-once, cancellation, and
//! destroy-under-load before the first async contract command is ported.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::runtime::PreparedOp;
use crate::session::SessionInner;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmitEventsParams {
    /// Number of progress events to emit.
    events: u32,
    /// Cancel-aware wait before each event, in milliseconds.
    #[serde(default)]
    interval_ms: u64,
    /// Panic instead of completing (exercises the operation-thread panic
    /// firewall).
    #[serde(default)]
    panic: bool,
}

pub fn prepare_emit_events(
    _session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: EmitEventsParams = decode_params("_diagEmitEvents", params)?;
    Ok(PreparedOp(Box::new(move |ctx| {
        for seq in 0..params.events {
            if params.interval_ms > 0
                && ctx
                    .cancel_token()
                    .wait(Duration::from_millis(params.interval_ms))
            {
                return Err(CoreError::canceled());
            }
            ctx.check_canceled()?;
            ctx.emit_progress(json!({ "seq": seq }));
        }
        if params.panic {
            panic!("_diagEmitEvents requested panic");
        }
        Ok(json!({ "emitted": params.events }))
    })))
}
