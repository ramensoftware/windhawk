//! The compile orchestration: `compile_mod` drives the per-target loop - pick
//! the targets, name a unique DLL, then for each target optionally rebuild the
//! PCH, compile, and handle cancel/failure. Holds the compile-only arch policy
//! (`compilation_targets`) and the collision-loop DLL namer (`unique_dll_name`,
//! which touches the `Files` port, so its loop stays here rather than in the
//! pure `domain` leaf).

use std::path::{Path, PathBuf};

use serde_json::json;
use windhawk_core_domain::{
    CompilationTarget, CompileArch, ModId, Version, compiled_dll_name, lcg_next_six, lcg_seed,
    targets_for_arch,
};
use windhawk_core_ports::Files;
use windhawk_core_protocol::ModMetadata;

use super::flags::{CompileSpec, parse_compiler_options, windhawk_version_hex};
use super::invoke::{
    compile_one, compiler_failed, compiler_wrote_no_output, format_compiler_warnings,
};
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
///
/// The all-common skip is gated on `arch.skips_common_x64()`: only the single
/// `Arm64` machine scenario drops the extra x64 build. `All` builds every
/// scenario's union, so it KEEPS the skip-eligible x64 even for a common-process
/// mod (`x64` has no skip-eligible target either way).
fn compilation_targets(
    arch: CompileArch,
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
    let arch_targets = targets_for_arch(architectures, arch.arm64_enabled())
        .map_err(|arch| CoreError::internal(format!("Unsupported architecture: {arch}")))?;

    // Skip the extra x64 build (only the skip-eligible x86-64-arm one) when
    // every named target is a common system process (the TS `.every(...)`), and
    // only in the arm64-machine scenario - `all` keeps the union.
    let all_common = arch.skips_common_x64()
        && !mod_targets.is_empty()
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

/// Refuse the compile up front when `engine_mods_dir` will not take a write:
/// create it if it is missing, then probe it. Both halves say the same thing -
/// the DLLs have nowhere to go - so both map onto one error.
///
/// The message names the folder's ROLE and the OS cause; the folder itself rides
/// in the `path` detail and the raw code in `osError`, which is what lets a
/// caller tell a lack of rights from a full disk and is what drives the CLI's
/// `hint: run this command as administrator`.
///
/// The per-architecture subfolders the DLLs actually land in are created UNDER
/// this folder, so its answer settles theirs; a subfolder already present and
/// separately locked down is a broken install rather than the missing-rights
/// case, and still reaches the user through the compiler.
fn ensure_mods_dir_writable(files: &dyn Files, engine_mods_dir: &Path) -> Result<(), CoreError> {
    match files
        .create_dirs(engine_mods_dir)
        .and_then(|()| files.probe_writable(engine_mods_dir))
    {
        Ok(()) => Ok(()),
        Err(e) => Err(CoreError::io_failed(
            format!(
                "The folder for compiled mods is not writable: {}",
                e.message()
            ),
            e.path,
            e.os.os_error,
        )),
    }
}

/// A DLL name `<modId>_<version>_<6 digits>.dll` colliding with neither a
/// currently-present compiled DLL nor another in-flight operation's
/// reservation, taken for this operation in `pending` (the TS randomized-name +
/// `doesCompiledModExist` loop). The "random" component is derived from the
/// `Clock` port so it is unpredictable enough in production and deterministic
/// under the test clock; the collision checks guarantee uniqueness regardless.
///
/// The pending half answers what the filesystem cannot: two operations on one
/// mod run their slow phase unlocked and can read the same millisecond, so both
/// derive the same suffix while neither has written a file yet. The reservation
/// covers every supported target, so the name is this operation's alone
/// whichever subset of them it goes on to build.
fn unique_dll_name(
    files: &dyn Files,
    pending: &mut PendingHandle,
    engine_mods_dir: &Path,
    mod_id: &ModId,
    version: &Version,
    supported: &[CompilationTarget],
    seed_ms: i64,
) -> String {
    // Seed the domain LCG ONCE, then take a fresh step per collision-check
    // iteration so each candidate name differs (the loop touches the `Files`
    // port and the session's pending set, so it stays here, not in the pure
    // `domain` leaf).
    let mut state = lcg_seed(seed_ms);
    loop {
        let rand6 = lcg_next_six(&mut state);
        let name = compiled_dll_name(mod_id.as_str(), version.as_str(), rand6);
        let paths: Vec<PathBuf> = supported
            .iter()
            .map(|t| engine_mods_dir.join(t.subfolder()).join(&name))
            .collect();
        if !paths.iter().any(|p| files.exists(p)) && pending.claim_all(paths) {
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
    /// Per-target clang diagnostics of the successful compile (warnings), each a
    /// triple-tagged block joined by a blank line; empty when the compile was
    /// clean. The download path leaves it empty (no compiler ran). Carried up to
    /// the install/recompile result for the front-end's compiler-output channel.
    pub warnings: String,
}

/// Compile a mod's source into per-architecture DLLs (the TS
/// `Compiler.compileMod`). Runs in the operation's slow phase (no command
/// lock); the returned `CompileOutput` carries the pending registration the
/// caller's commit section consumes. On cancel the partial DLLs are unlinked
/// and `CANCELED` returned; on a nonzero clang exit - or a zero exit that left
/// no DLL behind - `COMPILER_FAILED`.
///
/// `pch_folder` is the precompiled-headers folder (the TS
/// `precompiledHeadersFolder`): when set and it holds a `windhawk_pch.h`, a
/// stale per-target `.pch` is regenerated before the compile that consumes it
/// via `-include-pch`. `None` compiles without one.
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
    let arm64_enabled = session.arm64_enabled();
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

    // Refuse before any clang runs when the destination will not take the DLLs.
    // A system install keeps it under `%ProgramData%`, which a process without
    // administrator rights may read but not write - and clang reports that
    // refusal as an ordinary exit-1 failure, so left to the compiler the user is
    // told their mod does not build. A portable copy keeps it inside the install
    // tree, which whoever runs the copy can already write, so there is nothing
    // there for a probe to find.
    if !storage.portable() {
        ensure_mods_dir_writable(files.as_ref(), &engine_mods_dir)?;
    }

    let supported = CompilationTarget::all(arm64_enabled);

    // Name and reserve in one step: the reservation is what makes the DLLs
    // operation-private, and it is dropped by the caller's commit section.
    let mut pending = PendingHandle::new(session.pending());
    let target_dll_name = unique_dll_name(
        files.as_ref(),
        &mut pending,
        &engine_mods_dir,
        &mod_id,
        &version,
        &supported,
        session.deps().clock.now_ms(),
    );

    let compiler_options = parse_compiler_options(metadata.compiler_options.as_deref());
    let architectures = metadata.architecture.clone().unwrap_or_default();
    let mod_targets = metadata.include.clone().unwrap_or_default();
    let targets = compilation_targets(session.compile_arch(), &architectures, &mod_targets)?;

    // The per-COMPILE constant bundle (CompileSpec), built once and threaded
    // through both arg builders; `target` varies per loop iteration and is
    // passed separately.
    let spec = CompileSpec {
        mod_id: &mod_id,
        version: &version,
        version_hex: &version_hex,
        compiler_options: &compiler_options,
    };

    let mut warning_blocks: Vec<String> = Vec::new();

    for target in targets {
        // Report the ACTUAL target about to be compiled, before any clang work
        // for it (the per-target PCH rebuild below, then the compile). This is
        // the single source of the CLI's `Compiling for <arch>...` progress: the
        // deduped, skip-filtered target set, not the mod's declared architecture
        // list. The clang triple travels on the wire (as in the COMPILER_FAILED
        // `details.target`); the consumer maps it to the friendly arch label.
        ctx.emit_progress(json!({ "compileTarget": target.triple() }));

        // Regenerate the cached per-target precompiled header if it is stale,
        // before the compile that consumes it.
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
        // A zero exit is not proof the library exists. A mod's `@compilerOptions`
        // can suppress the output outright (`-fsyntax-only`), name it elsewhere
        // in a spelling `without_output_redirect` does not know, or an antivirus
        // can take it back off disk. Trusting the exit code registers a
        // `LibraryFileName` that was never written: the user is told the mod
        // installed and it silently never loads. Asking the filesystem settles
        // it without a view on how clang can be talked into it, so it also
        // backstops the flag filter.
        if !files.exists(&dll_path) {
            return Err(compiler_wrote_no_output(
                target,
                output.stdout,
                output.stderr,
            ));
        }
        // Carry a clean compile's clang diagnostics (warnings) up to the result
        // so the front-end can show them in its compiler-output channel. A
        // FAILING compile instead carries its output in the `COMPILER_FAILED`
        // error, handled above.
        if let Some(block) = format_compiler_warnings(target, &output.stdout, &output.stderr) {
            warning_blocks.push(block);
        }
    }

    Ok(CompileOutput {
        target_dll_name,
        pending,
        warnings: warning_blocks.join("\n\n"),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use windhawk_core_ports::{FileError, FileErrorKind};
    use windhawk_core_protocol::ErrorCode;
    use windhawk_core_testkit::FakeFiles;

    use super::*;
    use crate::pending::PendingArtifacts;

    const MODS_DIR: &str = "C:\\fixture\\AppData\\Engine\\Mods";

    #[test]
    fn a_mods_folder_that_refuses_a_write_fails_the_compile_before_it_starts() {
        let files = FakeFiles::new();
        files.set_probe_fault(FileError::new(
            "probe_writable",
            MODS_DIR,
            FileErrorKind::Other,
            5, // ERROR_ACCESS_DENIED, the unelevated case
            "Access is denied.",
        ));

        let wire = ensure_mods_dir_writable(&files, Path::new(MODS_DIR))
            .expect_err("a folder that refuses a write is not compilable into")
            .to_wire();

        assert_eq!(wire.code, ErrorCode::IoFailed);
        // The message names the folder's ROLE and the OS cause; it must not be
        // the compiler's exit-1 "might require a newer Windhawk version", which
        // is what the user is told when clang is left to discover this.
        assert_eq!(
            wire.message,
            "The folder for compiled mods is not writable: Access is denied."
        );
        let details = wire.details.expect("details");
        assert_eq!(details["path"], MODS_DIR);
        // The raw code is what lets the CLI offer the elevation hint and a
        // front-end tell a lack of rights from a full disk.
        assert_eq!(details["osError"], 5);
    }

    #[test]
    fn a_missing_mods_folder_that_cannot_be_created_reports_the_same_thing() {
        // The two halves of the check are one answer: whether the refusal comes
        // from creating the folder or from writing into it, the DLLs have
        // nowhere to go and the reader is owed the same sentence.
        let files = FakeFiles::new();
        files.set_create_dirs_fault(FileError::new(
            "create_dirs",
            MODS_DIR,
            FileErrorKind::Other,
            5,
            "Access is denied.",
        ));

        let wire = ensure_mods_dir_writable(&files, Path::new(MODS_DIR))
            .expect_err("a folder that cannot be created is not compilable into")
            .to_wire();

        assert_eq!(wire.code, ErrorCode::IoFailed);
        assert_eq!(
            wire.message,
            "The folder for compiled mods is not writable: Access is denied."
        );
        assert_eq!(wire.details.expect("details")["osError"], 5);
    }

    #[test]
    fn a_writable_mods_folder_passes_the_check() {
        assert!(ensure_mods_dir_writable(&FakeFiles::new(), Path::new(MODS_DIR)).is_ok());
    }

    #[test]
    fn a_name_another_operation_reserved_is_not_handed_out_again() {
        // Two compiles of one mod at one version run their slow phase unlocked,
        // so both can read the same clock millisecond and derive the same first
        // candidate. Neither has written a DLL yet, so the filesystem calls both
        // names free - the reservation is what separates them.
        const SEED_MS: i64 = 1_700_000_000_000;
        let files = FakeFiles::new();
        let set = Arc::new(PendingArtifacts::new());
        let mod_id = ModId::from("test-mod");
        let version = Version::from("1.0");
        let supported = CompilationTarget::all(true);
        let dir = Path::new(MODS_DIR);

        let mut first = PendingHandle::new(set.clone());
        let a = unique_dll_name(
            &files, &mut first, dir, &mod_id, &version, &supported, SEED_MS,
        );
        let mut second = PendingHandle::new(set.clone());
        let b = unique_dll_name(
            &files,
            &mut second,
            dir,
            &mod_id,
            &version,
            &supported,
            SEED_MS,
        );
        assert_ne!(a, b, "one clock millisecond must not yield one DLL name");

        // A name is reserved across EVERY supported target, so it stays the
        // operation's own whichever subset of them it goes on to build.
        for target in &supported {
            assert!(set.contains(&dir.join(target.subfolder()).join(&a)));
            assert!(set.contains(&dir.join(target.subfolder()).join(&b)));
        }

        // Committing the first operation releases only its own paths.
        drop(first);
        assert!(!set.contains(&dir.join("64").join(&a)));
        assert!(set.contains(&dir.join("64").join(&b)));
    }

    #[test]
    fn target_selection_follows_the_architecture_rules() {
        // x64 machine: x86-64 -> x64 only.
        let t = compilation_targets(CompileArch::X64, &["x86-64".into()], &[]).unwrap();
        assert_eq!(t, vec![CompilationTarget::X86_64]);

        // arm64 machine: x86-64 -> x64 + aarch64, in REQUEST order (the shared
        // taxonomy emits x64 before aarch64; compile-target order is not
        // parity-pinned - the compile parity check sorts).
        let t = compilation_targets(CompileArch::Arm64, &["x86-64".into()], &[]).unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );

        // arm64 machine, all targets common system processes -> aarch64 only
        // (the skip-eligible x64 is dropped).
        let t = compilation_targets(
            CompileArch::Arm64,
            &["x86-64".into()],
            &["explorer.exe".into()],
        )
        .unwrap();
        assert_eq!(t, vec![CompilationTarget::Aarch64]);

        // Empty architectures default to x86 + x86-64.
        let t = compilation_targets(CompileArch::X64, &[], &[]).unwrap();
        assert_eq!(t, vec![CompilationTarget::I686, CompilationTarget::X86_64]);

        // Unknown architecture: the compile path REJECTS it (maps the shared
        // taxonomy's `Err` to an internal error). This is an INTENTIONAL
        // divergence from the best-effort cleanup/download path
        // (`subfolders_for_arch`), which SKIPS an unknown architecture - one
        // shared taxonomy, two callers' policies (compile `?`, best-effort
        // `unwrap_or_default`). The paired check is
        // install::cleanup::tests::cleanup_subfolders_expand_and_dedup.
        assert!(compilation_targets(CompileArch::X64, &["sparc".into()], &[]).is_err());
    }

    #[test]
    fn all_scope_builds_the_union_without_the_common_process_skip() {
        // `all` (the union across machine scenarios) keeps the skip-eligible x64
        // even when every target is a common system process: the same x86-64 mod
        // that `arm64` reduces to aarch64-only stays x64 + aarch64 under `all`.
        let t = compilation_targets(
            CompileArch::All,
            &["x86-64".into()],
            &["explorer.exe".into()],
        )
        .unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );

        // The empty-arch default under `all` + all-common likewise keeps the
        // defaulted x64 (contrast `empty_architectures_default_expansion_feeds_the_skip_rule`,
        // where `arm64` drops it to [I686, Aarch64]).
        let t = compilation_targets(CompileArch::All, &[], &["explorer.exe".into()]).unwrap();
        assert_eq!(
            t,
            vec![
                CompilationTarget::I686,
                CompilationTarget::X86_64,
                CompilationTarget::Aarch64
            ]
        );
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
        // arm64 machine, all targets common system processes: the x86-64 arm
        // skips its x64 build, but the amd64 arm's x64 build is NOT skip-eligible
        // and must remain. Dropping the X86_64 class here is the regression.
        let t = compilation_targets(
            CompileArch::Arm64,
            &["amd64".into(), "x86-64".into()],
            &["explorer.exe".into()],
        )
        .unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64],
            "the amd64-requested x64 build must survive the x86-64 arm's all-common skip"
        );

        // arm64 machine, not all-common: both arms emit x64, but each DISTINCT
        // target is built once, so the second x64 is deduped away (first-build
        // order preserved: amd64's x64, then x86-64's aarch64). This is the
        // deliberate optimization that drops the redundant x64 compile and its
        // duplicate compiler-output log line.
        let t = compilation_targets(CompileArch::Arm64, &["amd64".into(), "x86-64".into()], &[])
            .unwrap();
        assert_eq!(
            t,
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );
    }

    #[test]
    fn empty_architectures_default_expansion_feeds_the_skip_rule() {
        // The empty-architectures default ([x86, x86-64]) expands BEFORE the
        // per-arch mapping (inside targets_for_arch), so the all-common skip
        // applies to the defaulted x86-64 too: arm64 machine + all-common drops
        // the defaulted x64 build, leaving [I686, Aarch64].
        let t = compilation_targets(CompileArch::Arm64, &[], &["explorer.exe".into()]).unwrap();
        assert_eq!(t, vec![CompilationTarget::I686, CompilationTarget::Aarch64]);
    }
}
