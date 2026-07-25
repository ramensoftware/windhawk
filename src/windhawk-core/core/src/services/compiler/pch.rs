//! The editor flow's precompiled-header sub-orchestration: regenerate a stale
//! per-target `.pch` before the compile that consumes it. Its own spawn/cancel
//! workflow, so it earns its own module. `build_pch_args` reads `flags`'s
//! shared flag-fragment consts and the `wh_macro_defines` tail; the cancel/exit
//! handling calls `invoke::compiler_failed`.

use std::path::{Path, PathBuf};

use windhawk_core_domain::CompilationTarget;
use windhawk_core_ports::{Files, ProcessRequest, Processes};

use super::flags::{
    CompileSpec, FP_EXCEPTION_MAYTRAP, STD_CPP, WINDOWS_VERSION_DEFINES, wh_macro_defines,
};
use super::invoke::compiler_failed;
use crate::callbacks::LogLevel;
use crate::error::CoreError;
use crate::pending::PendingHandle;
use crate::runtime::OpContext;
use crate::services::wire::WireResultExt;
use crate::session::SessionInner;

/// The editor flow's per-target precompiled-header step (the TS
/// `makePrecompiledHeaders`, gated by `compileMod`): when the folder holds a
/// `windhawk_pch.h`, regenerate the cached `windhawk_t_<triple>.pch` if it is
/// missing or older than the header, then return its path so the compile uses
/// `-include-pch`. Returns `None` when there is no header to precompile. A
/// cancel observed after the PCH compile unlinks the prior targets' pending
/// DLLs and ends the operation; a nonzero exit is `COMPILER_FAILED` (the TS
/// throws `CompilerError`, leaving any artifacts for the next sweep).
#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_make_pch(
    session: &SessionInner,
    processes: &dyn Processes,
    files: &dyn Files,
    pch_folder: &str,
    spec: &CompileSpec,
    target: CompilationTarget,
    pending: &PendingHandle,
    ctx: &OpContext,
) -> Result<Option<PathBuf>, CoreError> {
    let header_path = Path::new(pch_folder).join("windhawk_pch.h");
    // No header to precompile (the TS `if (fs.existsSync(pchHeaderPath))`).
    if !files.exists(&header_path) {
        return Ok(None);
    }
    let pch_path = Path::new(pch_folder).join(format!("windhawk_t_{}.pch", target.triple()));

    // Regenerate when the .pch is missing or older than its header (the TS
    // `!exists(pchPath) || mtime(pchPath) < mtime(pchHeaderPath)`).
    let needs_rebuild = if !files.exists(&pch_path) {
        true
    } else {
        let pch_mtime = files.modified_ms(&pch_path).wire()?;
        let header_mtime = files.modified_ms(&header_path).wire()?;
        pch_mtime < header_mtime
    };

    if needs_rebuild {
        let clang = Path::new(session.storage().info().compiler_path.as_str())
            .join("bin")
            .join("clang++.exe");
        let request = ProcessRequest {
            program: clang.to_string_lossy().into_owned(),
            args: build_pch_args(spec, target, &header_path, &pch_path),
            cwd: Some(session.storage().info().compiler_path.clone()),
            stdin: None,
        };
        let output = processes
            .run_capture(&request, ctx.cancel_token())
            .map_err(|e| CoreError::internal(format!("Failed to run the compiler (PCH): {e}")))?;

        // A kill (cancel) takes priority over the exit code (the TS
        // `wasCanceled` check, here before the DLL is even registered).
        if ctx.cancel_token().is_canceled() {
            pending.unlink_all(files);
            return Err(CoreError::canceled());
        }
        if output.exit_code != 0 {
            return Err(compiler_failed(
                target,
                output.exit_code,
                output.stdout,
                output.stderr,
            ));
        }
        log_pch_output(session, target, &output.stdout, &output.stderr);
    }

    Ok(Some(pch_path))
}

/// The clang++ argument vector for a precompiled-header build (the TS
/// `makePrecompiledHeaders`): the optimized Unicode + fixed Windows-version
/// defines (no per-mod special cases here), the `WH_*` macros, `-x c++-header`
/// over the header file, the per-target `-o`, and only the `-D` subset of the
/// mod's compiler options.
fn build_pch_args(
    spec: &CompileSpec,
    target: CompilationTarget,
    header_path: &Path,
    pch_path: &Path,
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        STD_CPP.to_owned(),
        "-O2".to_owned(),
        FP_EXCEPTION_MAYTRAP.to_owned(),
        "-DUNICODE".to_owned(),
        "-D_UNICODE".to_owned(),
    ];
    args.extend(WINDOWS_VERSION_DEFINES.iter().map(|s| (*s).to_owned()));
    args.extend(wh_macro_defines(
        spec.mod_id,
        spec.version,
        spec.version_hex,
    ));
    args.push("-x".to_owned());
    args.push("c++-header".to_owned());
    args.push(header_path.to_string_lossy().into_owned());
    args.push("-target".to_owned());
    args.push(target.triple().to_owned());
    args.push("-o".to_owned());
    args.push(pch_path.to_string_lossy().into_owned());
    // Only the `-D` flags of the mod's compiler options (the TS
    // `extraArgs.filter(arg => arg.startsWith('-D'))`).
    args.extend(
        spec.compiler_options
            .iter()
            .filter(|a| a.starts_with("-D"))
            .cloned(),
    );
    args
}

fn log_pch_output(session: &SessionInner, target: CompilationTarget, stdout: &str, stderr: &str) {
    if !stdout.is_empty() {
        session.log(
            LogLevel::Warn,
            format!(
                "Precompiled headers stdout for target {}:\n{stdout}",
                target.triple()
            ),
        );
    }
    if !stderr.is_empty() {
        session.log(
            LogLevel::Warn,
            format!(
                "Precompiled headers stderr for target {}:\n{stderr}",
                target.triple()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windhawk_core_domain::{ModId, Version};

    #[test]
    fn pch_args_use_c_plus_plus_header_and_only_d_options() {
        let mod_id = ModId::from("test-mod");
        let version = Version::from("1.0");
        let opts: Vec<String> = vec!["-DFOO=1".to_owned(), "-lcomctl32".to_owned()];
        let spec = CompileSpec {
            mod_id: &mod_id,
            version: &version,
            version_hex: "0x01060100",
            compiler_options: &opts,
        };
        let args = build_pch_args(
            &spec,
            CompilationTarget::I686,
            Path::new("C:\\pch\\windhawk_pch.h"),
            Path::new("C:\\pch\\windhawk_t_i686-w64-mingw32.pch"),
        );
        // The header input, the header-output mode, and the per-target output.
        assert!(args.contains(&"c++-header".to_owned()));
        assert!(args.contains(&"C:\\pch\\windhawk_pch.h".to_owned()));
        assert!(args.contains(&"C:\\pch\\windhawk_t_i686-w64-mingw32.pch".to_owned()));
        assert!(args.contains(&"-DWH_WINDHAWK_VERSION=0x01060100".to_owned()));
        // The FP-exception mode matches the compile that consumes this PCH, so
        // clang does not reject the `-include-pch`.
        assert!(args.contains(&FP_EXCEPTION_MAYTRAP.to_owned()));
        // Only the -D subset of the mod options is forwarded (no -l flags), and
        // the build is not a shared library.
        assert!(args.contains(&"-DFOO=1".to_owned()));
        assert!(!args.contains(&"-lcomctl32".to_owned()));
        assert!(!args.contains(&"-shared".to_owned()));
    }
}
