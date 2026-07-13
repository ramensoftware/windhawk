//! `services::compiler`: a port of `services/compiler.ts` - clang++ invocation
//! per architecture with the mod source piped on stdin, the per-target flag
//! construction, randomized DLL naming with a collision check, sequential
//! targets, kill-on-cancel of the compiler's process tree, and the structured
//! `COMPILER_FAILED` error carrying exit code / stdout / stderr. Plus
//! `getCompileFlags`, the clangd flag set for `compile_flags.txt`,
//! single-sourced with the real compile flags here.
//!
//! Split into single-concern submodules, wired by the `orchestrate` compile
//! loop:
//! - `flags`: flag construction (the compile arg builder, the `compilerOptions`
//!   splitter, the per-mod backward-compat includes, the shared `WH_*` tail, the
//!   version->hex transform, and the editor `getCompileFlags` producer).
//! - `invoke`: process spawn + the `COMPILER_FAILED` builder + output logging.
//! - `pch`: the precompiled-header sub-orchestration.
//! - `orchestrate`: the `compile_mod` per-target loop + the compile-only arch
//!   policy + the collision-loop DLL namer.
//!
//! `compile_mod` is called only by `services::install` (the install/compile
//! orchestration), never from dispatch directly; `getCompileFlags`
//! (`flags::get_compile_flags`) is the one dispatch-served command of this
//! module. The arch taxonomy, the LCG, and the DLL-name vocabulary moved to
//! `domain` (`compile_targets` / `dll_name`).

pub mod flags;
mod invoke;
mod orchestrate;
mod pch;

pub use orchestrate::{CompileOutput, compile_mod};
