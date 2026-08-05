//! `forkMod`: copy an existing mod under a new `-fork` id. The source is
//! `data.modSource` when present (forking a repo mod's fetched source) or the
//! installed mod's stored source otherwise; the same handler serves both, keyed
//! on whether `modSource` is present (there is no separate `forkModFromSource`
//! command). Validate the source `@id` against the mod being forked, suffix
//! `-fork` / `-forkN` until the `local@<id>` is free, allocate, initialize, and
//! open VSCodium.

use serde::Deserialize;
use serde_json::Value;

use super::{
    DevError, append_id_and_name, find_free_suffix, get_mod_source, open_editor, parse_bare_id,
};
use crate::ipc::bridge::BridgeCtx;

/// The `forkMod` envelope `data` (`{ modId, modSource? }`, the front-end's
/// `ForkModData`). `modSource` present selects the fork-from-source variant.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkRequest {
    mod_id: String,
    #[serde(default)]
    mod_source: Option<String>,
}

/// The `forkMod` entry point: run the launch flow, returning its outcome for the
/// caller ([`super::handle`]) to shape into the reply.
pub(super) fn run(ctx: &BridgeCtx, data: &Value) -> Result<(), DevError> {
    let req: ForkRequest = serde_json::from_value(data.clone())?;

    let source = match req.mod_source {
        Some(source) => source,
        None => get_mod_source(ctx, &req.mod_id)?,
    };
    let base_id = parse_bare_id(ctx, &source).ok_or(DevError::MissingId)?;

    // The source's `@id` must equal the mod being forked (the storage id minus the
    // `local@` scope), matching the extension's guard.
    let expected = req.mod_id.strip_prefix("local@").unwrap_or(&req.mod_id);
    if base_id != expected {
        return Err(DevError::IdMismatch {
            expected: expected.to_owned(),
            actual: base_id,
        });
    }

    // A fork is always suffixed: `-fork` / ` - Fork` first, then `-forkN` /
    // ` - Fork (N)` for N from 2.
    let (id_suffix, name_suffix) = find_free_suffix(ctx, &base_id, 1, |n| {
        if n == 1 {
            ("-fork".to_owned(), " - Fork".to_owned())
        } else {
            (format!("-fork{n}"), format!(" - Fork ({n})"))
        }
    })?;

    let forked = append_id_and_name(ctx, &source, &id_suffix, &name_suffix)?;
    let mod_id = format!("{base_id}{id_suffix}");

    // A fork's id is new by construction, so there is never a workspace to reuse.
    open_editor(ctx, &mod_id, forked, false)
}
