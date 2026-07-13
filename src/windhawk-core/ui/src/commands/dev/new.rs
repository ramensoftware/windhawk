//! `createNewMod`: start a fresh mod from the vendored template. Parse the
//! template's `@id`, suffix `-N` / ` (N)` until the `local@<id>` is free,
//! allocate a new workspace, initialize it from the final source, and open
//! VSCodium.

use super::{DevError, append_id_and_name, compile_flags, find_free_suffix, parse_bare_id};
use crate::editor::template::MOD_TEMPLATE;
use crate::editor::workspace::WorkspaceInit;
use crate::ipc::bridge::BridgeCtx;

/// The `createNewMod` entry point: run the launch flow, returning its outcome for
/// the caller ([`super::handle`]) to shape into the reply.
pub(super) fn run(ctx: &BridgeCtx) -> Result<(), DevError> {
    let editor = &ctx.editor;

    let base_id = parse_bare_id(ctx, MOD_TEMPLATE).ok_or(DevError::MissingId)?;
    // Attempt 0 is the bare template id; a collision advances to `-2` / ` (2)`,
    // `-3` / ` (3)`, and so on. The extension's counter starts at 2 (`-1` is
    // skipped), so the native side matches it: `n + 1` maps attempt 1 to `-2`
    // (extension.ts createNewMod, the behavioral oracle).
    let (id_suffix, name_suffix) = find_free_suffix(ctx, &base_id, 0, |n| {
        if n == 0 {
            (String::new(), String::new())
        } else {
            let counter = n + 1;
            (format!("-{counter}"), format!(" ({counter})"))
        }
    })?;

    let source = if id_suffix.is_empty() {
        MOD_TEMPLATE.to_owned()
    } else {
        append_id_and_name(ctx, MOD_TEMPLATE, &id_suffix, &name_suffix)?
    };
    let mod_id = format!("{base_id}{id_suffix}");

    let flags = compile_flags(ctx)?;
    let workspace = editor
        .workspaces()
        .allocate_and_initialize(&WorkspaceInit {
            mod_source: &source,
            mod_id: &mod_id,
            compile_flags: &flags,
        })?;

    editor.launcher().open_workspace(workspace.path())?;
    Ok(())
}
