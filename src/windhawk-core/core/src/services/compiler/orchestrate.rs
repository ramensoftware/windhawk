//! The compile orchestration: `compile_mod` drives the per-target loop - pick
//! the targets, name a unique DLL, then for each target optionally rebuild the
//! PCH, compile, and handle cancel/failure. Holds the compile-only arch policy
//! (`compilation_targets`) and the collision-loop DLL namer (`unique_dll_name`,
//! which touches the `Files` port, so its loop stays here rather than in the
//! pure `domain` leaf).

use std::path::Path;

use windhawk_core_domain::{
    CompilationTarget, ModId, Version, compiled_dll_name, lcg_next_six, lcg_seed, targets_for_arch,
};
use windhawk_core_ports::Files;
use windhawk_core_protocol::ModMetadata;

use super::flags::{CompileSpec, parse_compiler_options, windhawk_version_hex};
use super::invoke::{compile_one, compiler_failed, log_compiler_output};
use super::pch::maybe_make_pch;
use crate::error::CoreError;
use crate::pending::PendingHandle;
use crate::runtime::OpContext;
use crate::services::wire::WireResultExt;
use crate::session::SessionInner;

/// The DISTINCT targets to compile for a mod's declared architectures (the TS
/// `compilationTargetsFromArchitecture`): the shared `domain::targets_for_arch`
/// taxonomy, with the COMPILE-only policy applied here - REJECT an unknown arch
/// (the best-effort `subfolders_for_arch` skips it), drop the SKIP-ELIGIBLE
/// extra x64 build when every named target is a common system process, then
/// dedup to distinct targets.
///
/// The skip filters on `skip_eligible`, NOT the `X86_64` class: `X86_64` is also
/// emitted unconditionally by the `amd64` arch (never skip-eligible), so a class
/// filter would silently drop an `amd64`-requested x64 build on the reachable
/// `["amd64","x86-64"]` input. The dedup runs AFTER the skip (eligibility is
/// decided on the pre-dedup list), so a mod listing both `amd64` and `x86-64`
/// compiles x64 once - a deliberate divergence from the TS multiset, dropping
/// the redundant compile and its duplicate compiler-output `LogFn` line.
fn compilation_targets(
    arm64_enabled: bool,
    architectures: &[String],
    mod_targets: &[String],
) -> Result<Vec<CompilationTarget>, CoreError> {
    // Keep in lowercase (matched case-insensitively against mod targets).
    const COMMON_SYSTEM_MOD_TARGETS: &[&str] = &[
        "startmenuexperiencehost.exe",
        "searchhost.exe",
        "explorer.exe",
        "shellexperiencehost.exe",
        "shellhost.exe",
        "dwm.exe",
        "notepad.exe",
        "regedit.exe",
    ];

    // Compile REJECTS an unknown architecture (the best-effort callers skip it).
    let arch_targets = targets_for_arch(architectures, arm64_enabled)
        .map_err(|arch| CoreError::internal(format!("Unsupported architecture: {arch}")))?;

    // Skip the extra x64 build (only the skip-eligible x86-64-arm one) when
    // every named target is a common system process (the TS `.every(...)`).
    let all_common = !mod_targets.is_empty()
        && mod_targets
            .iter()
            .all(|t| COMMON_SYSTEM_MOD_TARGETS.contains(&t.to_lowercase().as_str()));

    // Skip first (on the pre-dedup list), then dedup to distinct targets.
    let mut targets: Vec<CompilationTarget> = Vec::with_capacity(arch_targets.len());
    for at in arch_targets {
        if all_common && at.skip_eligible() {
            continue;
        }
        let target = at.target();
        if !targets.contains(&target) {
            targets.push(target);
        }
    }

    if targets.is_empty() {
        return Err(CoreError::internal(
            "The current architecture is not supported",
        ));
    }
    Ok(targets)
}

/// A DLL name `<modId>_<version>_<6 digits>.dll` not colliding with any
/// currently-present compiled DLL across the supported targets (the TS
/// randomized-name + `doesCompiledModExist` loop). The "random" component is
/// derived from the `Clock` port so it is unpredictable enough in production
/// and deterministic under the test clock; the collision check guarantees
/// uniqueness regardless.
fn unique_dll_name(
    files: &dyn Files,
    engine_mods_dir: &Path,
    mod_id: &ModId,
    version: &Version,
    supported: &[CompilationTarget],
    seed_ms: i64,
) -> String {
    // Seed the domain LCG ONCE, then take a fresh step per collision-check
    // iteration so each candidate name differs (the loop touches the `Files`
    // port, so it stays here, not in the pure `domain` leaf).
    let mut state = lcg_seed(seed_ms);
    loop {
        let rand6 = lcg_next_six(&mut state);
        let name = compiled_dll_name(mod_id.as_str(), version.as_str(), rand6);
        if supported
            .iter()
            .all(|t| !files.exists(&engine_mods_dir.join(t.subfolder()).join(&name)))
        {
            return name;
        }
    }
}

/// The result of a successful compile: the freshly written library file name
/// and the pending-artifact registration that protects its DLLs until the
/// commit section drops it.
pub struct CompileOutput {
    pub target_dll_name: String,
    pub pending: PendingHandle,
}

/// Compile a mod's source into per-architecture DLLs (the TS
/// `Compiler.compileMod`). Runs in the operation's slow phase (no command
/// lock); the returned `CompileOutput` carries the pending registration the
/// caller's commit section consumes. On cancel the partial DLLs are unlinked
/// and `CANCELED` returned; on a nonzero clang exit, `COMPILER_FAILED`.
///
/// `pch_folder` is the editor `installMod` flow's precompiled-headers folder
/// (the TS `precompiledHeadersFolder`): when set and it holds a
/// `windhawk_pch.h`, a stale per-target `.pch` is regenerated before the
/// compile that consumes it via `-include-pch`. `compileInstalledMod` passes
/// `None`.
pub fn compile_mod(
    session: &SessionInner,
    storage_id: &str,
    metadata: &ModMetadata,
    source: &str,
    pch_folder: Option<&str>,
    ctx: &OpContext,
) -> Result<CompileOutput, CoreError> {
    let storage = session.storage();
    let info = storage.info();
    let arm64_enabled = session.config().arm64_enabled;
    let engine_mods_dir = storage.engine_mods_dir();
    let compiler_path = info.compiler_path.clone();
    let engine_path = info.engine_path.clone();
    // Wrap point: the id and version travel together into the compile, so they
    // become newtypes here (the id from storage_id, the version from the nested
    // metadata), making a swap a type error across the builders below.
    let mod_id = ModId::from(storage_id);
    let version = Version::from(metadata.version.clone().unwrap_or_default());
    let version_hex = windhawk_version_hex(session.config().windhawk_version.as_deref());

    let files = session.deps().files.clone();
    let processes = session.deps().processes.clone();

    let supported = CompilationTarget::all(arm64_enabled);

    let target_dll_name = unique_dll_name(
        files.as_ref(),
        &engine_mods_dir,
        &mod_id,
        &version,
        &supported,
        session.deps().clock.now_ms(),
    );

    let compiler_options = parse_compiler_options(metadata.compiler_options.as_deref());
    let architectures = metadata.architecture.clone().unwrap_or_default();
    let mod_targets = metadata.include.clone().unwrap_or_default();
    let targets = compilation_targets(arm64_enabled, &architectures, &mod_targets)?;

    // The per-COMPILE constant bundle (CompileSpec), built once and threaded
    // through both arg builders; `target` varies per loop iteration and is
    // passed separately.
    let spec = CompileSpec {
        mod_id: &mod_id,
        version: &version,
        version_hex: &version_hex,
        compiler_options: &compiler_options,
    };

    let mut pending = PendingHandle::new(session.pending());

    for target in targets {
        // Editor flow: regenerate the cached per-target precompiled header if it
        // is stale (before the compile that consumes it). Runs before the DLL is
        // registered in the pending set, so a PCH cancel only has to unlink the
        // prior targets' DLLs.
        let pch_path = match pch_folder {
            Some(folder) => maybe_make_pch(
                session,
                processes.as_ref(),
                files.as_ref(),
                folder,
                &spec,
                target,
                &pending,
                ctx,
            )?,
            None => None,
        };

        let subfolder_dir = engine_mods_dir.join(target.subfolder());
        let dll_path = subfolder_dir.join(&target_dll_name);
        files.create_dirs(&subfolder_dir).wire()?;
        pending.add(dll_path.clone());

        let output = compile_one(
            processes.as_ref(),
            &compiler_path,
            &engine_path,
            &spec,
            target,
            source,
            &dll_path,
            pch_path.as_deref(),
            ctx,
        )?;

        // A kill (cancel) takes priority over the exit code (the TS
        // `wasCanceled` check): unlink the partial DLLs and stop.
        if ctx.cancel_token().is_canceled() {
            pending.unlink_all(files.as_ref());
            return Err(CoreError::canceled());
        }
        if output.exit_code != 0 {
            // On a compile error the DLLs are left for the next sweep (the TS
            // does not unlink here); dropping `pending` only deregisters them.
            return Err(compiler_failed(
                target,
                output.exit_code,
                output.stdout,
                output.stderr,
            ));
        }
        // A clean compile's stdout/stderr (clang warnings) is diagnostic noise
        // by default; surface it as `Warn` records only when the operator opts
        // in with WINDHAWK_LOG_COMPILER_WARNINGS=1 (threaded in as the config
        // flag, since the core never reads the environment).
        if session.config().log_compiler_warnings {
            log_compiler_output(session, target, &output.stdout, &output.stderr);
        }
    }

    Ok(CompileOutput {
        target_dll_name,
        pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_selection_follows_the_architecture_rules() {
        // No arm64: x86-64 -> x64 only.
        let t = compilation_targets(false, &["x86-64".into()], &[]).unwrap();
        assert_eq!(t, vec![CompilationTarget::X86_64]);

        // arm64 enabled: x86-64 -> x64 + aarch64, in REQUEST order (the shared
        // taxonomy emits x64 before aarch64; compile-target order is not
        // parity-pinned - the corpus compile self-diff sorts).
        let t = compilation_targets(true, &["x86-64".into()], &[]).unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );

        // arm64 enabled, all targets common system processes -> aarch64 only
        // (the skip-eligible x64 is dropped).
        let t = compilation_targets(true, &["x86-64".into()], &["explorer.exe".into()]).unwrap();
        assert_eq!(t, vec![CompilationTarget::Aarch64]);

        // Empty architectures default to x86 + x86-64.
        let t = compilation_targets(false, &[], &[]).unwrap();
        assert_eq!(t, vec![CompilationTarget::I686, CompilationTarget::X86_64]);

        // Unknown architecture: the compile path REJECTS it (maps the shared
        // taxonomy's `Err` to an internal error). This is an INTENTIONAL
        // divergence from the best-effort cleanup/download path
        // (`subfolders_for_arch`), which SKIPS an unknown architecture - one
        // shared taxonomy, two callers' policies (compile `?`, best-effort
        // `unwrap_or_default`). The paired check is
        // install::cleanup::tests::cleanup_subfolders_expand_and_dedup.
        assert!(compilation_targets(false, &["sparc".into()], &[]).is_err());
    }

    // Multi-architecture inputs pin two behaviors. (1) The all-common SKIP rule:
    // `X86_64` is produced by TWO arches - the `x86-64` arm (where the all-common
    // skip lives) AND the `amd64` arm (unconditional, NOT skip-eligible) - so the
    // skip must drop only the skip-eligible x86-64-arm x64, never the X86_64
    // *class*, or the amd64-requested x64 build silently dies. (2) DEDUP: because
    // both map to X86_64, the final list is deduped to distinct targets, so a mod
    // listing both compiles (and logs) x64 once, not twice - the deliberate
    // optimization. `architecture` may be any combination of
    // [x86, x86-64, amd64, arm64] (domain::metadata SUPPORTED_ARCHITECTURE), so
    // `["amd64", "x86-64"]` is reachable input; the single-arch
    // `target_selection_follows_the_architecture_rules` above covers neither.
    #[test]
    fn amd64_x64_build_survives_the_x86_64_arm_skip_rule() {
        // arm64 on, all targets common system processes: the x86-64 arm skips
        // its x64 build, but the amd64 arm's x64 build is NOT skip-eligible and
        // must remain. Dropping the X86_64 class here is the regression.
        let t = compilation_targets(
            true,
            &["amd64".into(), "x86-64".into()],
            &["explorer.exe".into()],
        )
        .unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64],
            "the amd64-requested x64 build must survive the x86-64 arm's all-common skip"
        );

        // arm64 on, not all-common: both arms emit x64, but each DISTINCT target
        // is built once, so the second x64 is deduped away (first-build order
        // preserved: amd64's x64, then x86-64's aarch64). This is the deliberate
        // optimization that drops the redundant x64 compile and its duplicate
        // compiler-output log line.
        let t = compilation_targets(true, &["amd64".into(), "x86-64".into()], &[]).unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );
    }

    #[test]
    fn empty_architectures_default_expansion_feeds_the_skip_rule() {
        // The empty-architectures default ([x86, x86-64]) expands BEFORE the
        // per-arch mapping (inside targets_for_arch), so the all-common skip
        // applies to the defaulted x86-64 too: arm64 on + all-common drops the
        // defaulted x64 build, leaving [I686, Aarch64].
        let t = compilation_targets(true, &[], &["explorer.exe".into()]).unwrap();
        assert_eq!(t, vec![CompilationTarget::I686, CompilationTarget::Aarch64]);
    }
}
