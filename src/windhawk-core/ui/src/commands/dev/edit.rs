//! `editMod`: open an installed mod for editing. Read its stored source and parse
//! the `@id`, then hand both over to be opened with edit-reuse: the workspace
//! already editing that mod is located and re-seeded when there is one, and a
//! fresh one is prepared from the installed source when there is not. VSCodium's
//! single-instance mechanism focuses an already-open window for the same folder.

use serde_json::Value;
use windhawk_core_protocol::ModIdParams;

use super::{DevError, get_mod_source, open_editor, parse_bare_id};
use crate::ipc::bridge::BridgeCtx;

/// The `editMod` entry point: run the launch flow, returning its outcome for the
/// caller ([`super::handle`]) to shape into the reply.
pub(super) fn run(ctx: &BridgeCtx, data: &Value) -> Result<(), DevError> {
    let req: ModIdParams = serde_json::from_value(data.clone())?;

    let source = get_mod_source(ctx, &req.mod_id)?;
    let mod_id = parse_bare_id(ctx, &source).ok_or(DevError::MissingId)?;

    open_editor(ctx, &mod_id, source, true)
}
