//! `services::install`: the install/compile use-case orchestration -
//! `compileInstalledMod` (recompile an installed mod's stored source) and
//! `installMod` (the full install/reinstall flow). The staged keyed-`Mod`
//! capture/commit machinery and the pending-artifact set: the slow
//! compile/download runs with no command lock (its DLLs registered in the
//! pending set); the commit takes the exclusive keyed `Mod` lock(s) for the
//! config write, settings migration, source write, old-DLL cleanup, and the
//! read-back.
//!
//! Split into single-concern submodules:
//! - `orchestrate`: the two prepare/body orchestrators + the named commit
//!   section + the rename step + the config writer.
//! - `download`: the precompiled-download arm (the install-side sibling of
//!   `compiler::compile_mod`).
//! - `migrate`: the mod-settings migration.
//! - `cleanup`: the old-DLL removal sweeps.

mod cleanup;
mod download;
mod migrate;
pub mod orchestrate;

pub(crate) use cleanup::delete_mod_files;
pub(crate) use migrate::engine_items_to_map;
