//! Process invocation + diagnostics: spawning clang++ for one target with the
//! source on stdin, the structured `COMPILER_FAILED` builder, and the
//! compiler-output logging. `compiler_failed` is called cross-submodule by both
//! `orchestrate` (compile) and `pch` (the deliberate orchestrate/pch -> invoke
//! edge).

use std::path::Path;

use windhawk_core_domain::CompilationTarget;
use windhawk_core_ports::{ProcessOutput, ProcessRequest, Processes};

use super::flags::{CompileSpec, build_compile_args};
use crate::callbacks::LogLevel;
use crate::error::CoreError;
use crate::runtime::OpContext;
use crate::session::SessionInner;

/// `0xC0000135` (STATUS_DLL_NOT_FOUND) as the `i32` `ExitStatus::code` returns
/// it, the "some files are missing" case of the TS `CompilerError`.
const MISSING_DEPENDENCY_EXIT_CODE: i32 = 0xC000_0135u32 as i32;

/// Run clang++ for one target with the source on stdin (the TS
/// `compileModInternal`). Returns the captured output; a spawn failure is
/// `INTERNAL` (the TS `ps.on('error')` reject).
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_one(
    processes: &dyn Processes,
    compiler_path: &str,
    engine_path: &str,
    spec: &CompileSpec,
    target: CompilationTarget,
    source: &str,
    dll_path: &Path,
    pch_path: Option<&Path>,
    ctx: &OpContext,
) -> Result<ProcessOutput, CoreError> {
    let clang = Path::new(compiler_path).join("bin").join("clang++.exe");
    let args = build_compile_args(spec, target, engine_path, dll_path, pch_path);

    let request = ProcessRequest {
        program: clang.to_string_lossy().into_owned(),
        args,
        cwd: Some(compiler_path.to_owned()),
        stdin: Some(source.as_bytes().to_vec()),
    };
    processes
        .run_capture(&request, ctx.cancel_token())
        .map_err(|e| CoreError::internal(format!("Failed to run the compiler: {e}")))
}

/// Build the human message of a `COMPILER_FAILED`, matching the TS
/// `CompilerError` text (exit-code and target specific).
pub(super) fn compiler_failed(
    target: CompilationTarget,
    exit_code: i32,
    stdout: String,
    stderr: String,
) -> CoreError {
    let mut message = String::from("Compilation failed");
    if exit_code == 1 {
        message.push_str(", the mod might require a newer Windhawk version");
        if target == CompilationTarget::Aarch64 {
            message.push_str(", or perhaps the mod isn't compatible with ARM64 yet");
        }
    } else if exit_code == MISSING_DEPENDENCY_EXIT_CODE {
        message.push_str(
            ", some files are missing, please reinstall Windhawk and make sure files aren't being removed by an antivirus",
        );
    } else {
        message.push_str(&format!(
            ", error code: 0x{:x}, please reinstall Windhawk and make sure files aren't being removed by an antivirus",
            exit_code as u32
        ));
    }
    CoreError::compiler_failed(
        message,
        target.triple().to_owned(),
        exit_code,
        stdout,
        stderr,
    )
}

pub(super) fn log_compiler_output(
    session: &SessionInner,
    target: CompilationTarget,
    stdout: &str,
    stderr: &str,
) {
    if !stdout.is_empty() {
        session.log(
            LogLevel::Warn,
            format!("Compiler stdout for target {}:\n{stdout}", target.triple()),
        );
    }
    if !stderr.is_empty() {
        session.log(
            LogLevel::Warn,
            format!("Compiler stderr for target {}:\n{stderr}", target.triple()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_failed_messages_match_the_ts() {
        let e = compiler_failed(CompilationTarget::I686, 1, String::new(), String::new());
        assert_eq!(
            e.to_string(),
            "Compilation failed, the mod might require a newer Windhawk version"
        );
        let e = compiler_failed(CompilationTarget::Aarch64, 1, String::new(), String::new());
        assert!(e.to_string().contains("ARM64"));
        let e = compiler_failed(
            CompilationTarget::X86_64,
            MISSING_DEPENDENCY_EXIT_CODE,
            String::new(),
            String::new(),
        );
        assert!(e.to_string().contains("some files are missing"));
        let e = compiler_failed(CompilationTarget::X86_64, 2, String::new(), String::new());
        assert!(e.to_string().contains("error code: 0x2"));
    }

    #[test]
    fn compiler_failed_carries_the_target_in_its_details() {
        let e = compiler_failed(CompilationTarget::Aarch64, 1, "o".into(), "e".into());
        let wire = e.to_wire();
        let details = wire.details.expect("details");
        assert_eq!(details["target"], "aarch64-w64-mingw32");
        assert_eq!(details["exitCode"], 1);
        assert_eq!(details["stdout"], "o");
        assert_eq!(details["stderr"], "e");
    }
}
