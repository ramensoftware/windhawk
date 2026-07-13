//! `editMod`: open an installed mod for editing. Read its stored source, parse
//! the `@id`, and locate the workspace already editing it (edit-reuse). If
//! found, re-seed its editor-mode settings and open VSCodium - which focuses
//! the already-open window for that folder. If not, allocate and initialize a
//! fresh workspace from the installed source, then open it.

use serde_json::Value;
use windhawk_core_protocol::ModIdParams;

use super::{DevError, compile_flags, get_mod_source, parse_bare_id};
use crate::editor::workspace::WorkspaceInit;
use crate::ipc::bridge::BridgeCtx;

/// The `editMod` entry point: run the launch flow, returning its outcome for the
/// caller ([`super::handle`]) to shape into the reply.
pub(super) fn run(ctx: &BridgeCtx, data: &Value) -> Result<(), DevError> {
    let editor = &ctx.editor;
    let req: ModIdParams = serde_json::from_value(data.clone())?;

    let source = get_mod_source(ctx, &req.mod_id)?;
    let mod_id = parse_bare_id(ctx, &source).ok_or(DevError::MissingId)?;

    let located = editor
        .workspaces()
        .locate(&mod_id, |candidate| parse_bare_id(ctx, candidate))?;

    let workspace = match located {
        Some(workspace) => {
            // Reuse the existing `mod.wh.cpp` (it may hold unsaved edits), but
            // re-seed the editor-mode settings: a workspace found via the `@id`
            // fallback may have had `editedModId` cleared on a prior exit, and
            // without rewriting it the extension would enter browse mode.
            // Required, not optional.
            editor
                .workspaces()
                .reseed_editor_mode(workspace.path(), &mod_id)?;
            workspace
        }
        None => {
            let flags = compile_flags(ctx)?;
            editor
                .workspaces()
                .allocate_and_initialize(&WorkspaceInit {
                    mod_source: &source,
                    mod_id: &mod_id,
                    compile_flags: &flags,
                })?
        }
    };

    editor.launcher().open_workspace(workspace.path())?;
    Ok(())
}
