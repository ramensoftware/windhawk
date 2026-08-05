//! The CLI's single error authority: the [`Category`] exit-class enum, the
//! wire-code -> category map, and the canonical `error.code` string the
//! `--json` envelope reports. The TypeScript front-end splits this across
//! errors.ts / output.ts / dllBackend.ts; consolidating it here keeps the text
//! and `--json` paths from drifting in how they classify a failure.

use std::io;
use std::panic::Location;

use serde_json::Value;
use windhawk_core_host::{HostError, HostErrorKind};
use windhawk_core_protocol::{
    CompileDetails, ErrorCode, OsErrorDetails, SourceLocation, WireError,
};

/// Win32 `ERROR_ACCESS_DENIED`. A registry or filesystem call that comes back
/// with this is the unelevated-process case: a non-portable install keeps its
/// settings under `HKEY_LOCAL_MACHINE` and its files under Program Files, both
/// of which a medium-integrity process may read but not write. Spelled out here
/// rather than pulled from `windows-sys` - this crate is `forbid(unsafe_code)`
/// and holds no Win32 edge.
const ERROR_ACCESS_DENIED: u32 = 5;

/// The single exit-class authority: one variant per `error.code` / exit-code
/// class. Every wire [`ErrorCode`] maps onto one of these (see
/// [`Category::from_wire`]), collapsing the wire spellings that duplicate a CLI
/// class (`APP_ROOT_INVALID` -> `EnvInvalid`, `COMPILER_FAILED` -> `CompileFailed`,
/// `CANCELED` -> `Cancelled`) so the emitted `error.code` is canonical regardless
/// of which layer raised the failure. `Generic` is reachable ONLY from a
/// CLI-side failure (a transport/contract/decode failure with no wire error in
/// hand); the map over `ErrorCode` is total, so no wire code falls through to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Argument/usage error at the CLI boundary. Exit 2.
    Usage,
    /// A CLI-side failure with no structured wire error (a transport, contract,
    /// or result-decode failure). Exit 1.
    Generic,
    /// App-root discovery/validation failed. Exit 3.
    EnvInvalid,
    /// A command references a mod that is not installed locally. Exit 4.
    ModNotInstalled,
    /// A mod id/version is not in the repository. Exit 5.
    ModNotInRepo,
    /// Network/HTTP failure talking to the repository. Exit 6.
    RepoUnreachable,
    /// The compiler returned non-zero for at least one architecture. Exit 7.
    CompileFailed,
    /// An app-settings change needs a restart and `--confirm-app-restart` was not
    /// passed. Exit 8.
    RestartRequired,
    /// The operation was cancelled. Exit 9.
    Cancelled,
    // The five classes below each carry their own `error.code` and their own
    // exit code (10-14), so every wire `ErrorCode` is exit-distinguishable and
    // `Generic` (exit 1) is reserved for a CLI-side failure with no wire error
    // in hand.
    /// An update/install is already in flight; retry later. Exit 10.
    UpdateInProgress,
    /// A filesystem operation failed. Exit 11.
    IoFailed,
    /// A registry operation failed. Exit 12.
    RegistryFailed,
    /// The core rejected the request (unknown command / bad params). Exit 13.
    InvalidRequest,
    /// The core reported an internal invariant violation. Exit 14.
    Internal,
}

impl Category {
    /// Map a wire [`ErrorCode`] onto its exit class 1:1. The three wire spellings
    /// that duplicate a CLI class collapse onto the canonical variant
    /// (`AppRootInvalid` -> `EnvInvalid`, `CompilerFailed` -> `CompileFailed`,
    /// `Canceled` -> `Cancelled`); the other eight map to the like-named variant.
    /// Exhaustive over `ErrorCode`, so a new wire code is a build error here, not
    /// a silent fall-through to `Generic`.
    fn from_wire(code: ErrorCode) -> Category {
        match code {
            ErrorCode::AppRootInvalid => Category::EnvInvalid,
            ErrorCode::ModNotInstalled => Category::ModNotInstalled,
            ErrorCode::ModNotInRepo => Category::ModNotInRepo,
            ErrorCode::RepoUnreachable => Category::RepoUnreachable,
            ErrorCode::CompilerFailed => Category::CompileFailed,
            // Missing development tools is an environment problem (exit 3), like an
            // invalid app root; the core's message names the fix (install them).
            ErrorCode::DevToolsMissing => Category::EnvInvalid,
            // The core's up-front restart gate (importUserData without
            // confirmAppRestart) is the same class as the CLI's own
            // restart-required errors (exit 8).
            ErrorCode::RestartRequired => Category::RestartRequired,
            ErrorCode::Canceled => Category::Cancelled,
            ErrorCode::UpdateInProgress => Category::UpdateInProgress,
            ErrorCode::IoFailed => Category::IoFailed,
            ErrorCode::RegistryFailed => Category::RegistryFailed,
            ErrorCode::InvalidRequest => Category::InvalidRequest,
            ErrorCode::Internal => Category::Internal,
        }
    }

    /// The canonical `error.code` string for the class, emitted regardless of
    /// which wire spelling produced it. Replaces the retired
    /// `code_str(ErrorCode)` stringify, whose
    /// `to_value(...).unwrap_or("INTERNAL")` masked a stringify failure as
    /// `INTERNAL`; this exhaustive map has no fallback.
    fn code_str(self) -> &'static str {
        match self {
            Category::Usage => "USAGE",
            Category::Generic => "GENERIC",
            Category::EnvInvalid => "ENV_INVALID",
            Category::ModNotInstalled => "MOD_NOT_INSTALLED",
            Category::ModNotInRepo => "MOD_NOT_IN_REPO",
            Category::RepoUnreachable => "REPO_UNREACHABLE",
            Category::CompileFailed => "COMPILE_FAILED",
            Category::RestartRequired => "RESTART_REQUIRED",
            Category::Cancelled => "CANCELLED",
            Category::UpdateInProgress => "UPDATE_IN_PROGRESS",
            Category::IoFailed => "IO_FAILED",
            Category::RegistryFailed => "REGISTRY_FAILED",
            Category::InvalidRequest => "INVALID_REQUEST",
            Category::Internal => "INTERNAL",
        }
    }

    /// The process exit code for the class.
    fn exit_code(self) -> i32 {
        match self {
            Category::Usage => 2,
            Category::Generic => 1,
            Category::EnvInvalid => 3,
            Category::ModNotInstalled => 4,
            Category::ModNotInRepo => 5,
            Category::RepoUnreachable => 6,
            Category::CompileFailed => 7,
            Category::RestartRequired => 8,
            Category::Cancelled => 9,
            // The operationally-actionable conditions (retry, fix permissions)
            // take 10-12; the two "should-not-happen" contract/internal
            // failures take 13-14. Each wire code is exit-distinguishable, so
            // `Generic` (exit 1) means "no structured wire error in hand".
            Category::UpdateInProgress => 10,
            Category::IoFailed => 11,
            Category::RegistryFailed => 12,
            Category::InvalidRequest => 13,
            Category::Internal => 14,
        }
    }
}

/// A CLI-layer error: the exit [`Category`], the human `message`, and the wire
/// error's structured `details`. `details` stays an `Option<Value>` (not typed
/// to the compile shape) because `from_wire` is code-agnostic - any wire code
/// may carry a per-code `details` payload; only the interpreting render reads it
/// through a typed view ([`CompileDetails`]). Today only a `COMPILER_FAILED`
/// carries it, for the `[compile:<arch>]` diagnostics; `None` otherwise.
#[derive(Debug)]
pub struct CliError {
    pub(crate) category: Category,
    message: String,
    details: Option<Value>,
    /// The source origin (DIAGNOSTIC): a wire error carries the core's, a
    /// CLI-side constructor captures its `#[track_caller]` call site. Rendered
    /// only in text-mode stderr; the `--json` envelope stays clean. Boxed so the
    /// origin does not bloat `Result<_, CliError>` on every command's return
    /// (clippy `result_large_err`).
    ///
    /// TWO RULES keep this pointing at the command rather than at plumbing, the
    /// pair `services::wire::WireResultExt` enforces on the core side:
    ///
    /// 1. A shared helper that BUILDS an error carries `#[track_caller]`, so the
    ///    origin names the command that called it, not the helper's own line.
    ///    Command entry points do NOT - their line is the useful one.
    /// 2. Inside such a helper the constructor is called DIRECTLY, in a `match`
    ///    arm or an `if`. A closure (`ok_or_else(|| ...)`, `map_err(|e| ...)`)
    ///    or a fn-item (`map_err(CliError::usage)`) breaks the chain: a closure
    ///    is not `#[track_caller]`, so the captured site collapses to the
    ///    closure body, and a fn-item collapses it to libcore's dispatch.
    location: Option<Box<SourceLocation>>,
}

impl CliError {
    /// A CLI-side error with no `details`, tagged with `location` (the
    /// `#[track_caller]` call site of the public constructor).
    fn classified(category: Category, message: String, location: SourceLocation) -> CliError {
        CliError {
            category,
            message,
            details: None,
            location: Some(Box::new(location)),
        }
    }

    /// Override the origin (used by `From<HostError>` to adopt the host's deeper
    /// origin - the DLL-load site, the core's wire origin - over the conversion
    /// site).
    fn at(mut self, location: Option<SourceLocation>) -> CliError {
        self.location = location.map(Box::new);
        self
    }

    #[track_caller]
    pub fn usage(message: impl Into<String>) -> CliError {
        CliError::classified(
            Category::Usage,
            message.into(),
            SourceLocation::from(Location::caller()),
        )
    }

    /// App-root discovery failed (no `--app-root`, no `WINDHAWK_UI_PATH`, no
    /// `windhawk.ini` in the CLI exe's directory). Exit 3, mirroring the TS
    /// `EnvInvalidError`.
    #[track_caller]
    pub fn env_invalid(message: impl Into<String>) -> CliError {
        CliError::classified(
            Category::EnvInvalid,
            message.into(),
            SourceLocation::from(Location::caller()),
        )
    }

    /// A command references a mod that is not installed locally. Exit 4,
    /// mirroring the TS `ModNotInstalledError`.
    #[track_caller]
    pub fn mod_not_installed(mod_id: &str) -> CliError {
        CliError::classified(
            Category::ModNotInstalled,
            format!("Mod not installed: {mod_id}"),
            SourceLocation::from(Location::caller()),
        )
    }

    /// `MOD_NOT_INSTALLED` (exit 4) with a caller-supplied message - used when
    /// the config exists but the source file is missing.
    #[track_caller]
    pub fn mod_not_installed_with(message: impl Into<String>) -> CliError {
        CliError::classified(
            Category::ModNotInstalled,
            message.into(),
            SourceLocation::from(Location::caller()),
        )
    }

    /// An app-settings change requires a Windhawk restart and the user did not
    /// pass `--confirm-app-restart`; the CLI refuses to write. Exit 8, mirroring
    /// the TS `RestartRequiredError`.
    #[track_caller]
    pub fn restart_required(message: impl Into<String>) -> CliError {
        CliError::classified(
            Category::RestartRequired,
            message.into(),
            SourceLocation::from(Location::caller()),
        )
    }

    #[track_caller]
    pub fn generic(message: impl Into<String>) -> CliError {
        CliError::classified(
            Category::Generic,
            message.into(),
            SourceLocation::from(Location::caller()),
        )
    }

    /// Map a DLL error envelope's typed `WireError` to a CLI error, mapping its
    /// wire code to the canonical [`Category`] (so the exit-code map classifies
    /// it identically to the same condition raised CLI-side) and preserving its
    /// `details` (so a `COMPILER_FAILED` keeps the target/exitCode/stdout/stderr
    /// the `[compile:<arch>]` diagnostics render).
    pub fn from_wire(error: WireError) -> CliError {
        CliError {
            category: Category::from_wire(error.code),
            // The core's origin rides the wire error; surface it as the CLI
            // error's origin rather than a CLI-side site.
            location: error.location.map(Box::new),
            message: error.message,
            details: error.details,
        }
    }

    /// The `[compile:<arch>]`-prefixed compiler diagnostics for a compile
    /// failure carrying `details` (`{target, exitCode, stdout, stderr}`),
    /// mirroring the TS `writeCompilerDiagnostics`. `None` for any non-compile
    /// error or a compile error with no details. The caller streams it to
    /// stderr (both text and `--json` modes) before the error envelope, so
    /// stdout stays clean.
    pub fn compiler_diagnostics(&self) -> Option<String> {
        if self.category != Category::CompileFailed {
            return None;
        }
        let details = self.details.as_ref()?;
        let d: CompileDetails = serde_json::from_value(details.clone()).unwrap_or_default();
        let arch = arch_label(&d.target);
        let stdout = d.stdout.trim();
        let stderr = d.stderr.trim();

        // Fall back to the raw exit code when there is no captured output or
        // the failure is not the usual exit-code-1 compile error.
        let mut blocks: Vec<String> = Vec::new();
        if (stdout.is_empty() && stderr.is_empty()) || d.exit_code != Some(1) {
            let exit_str = match d.exit_code {
                // Conventional unsigned 32-bit hex: a negative NTSTATUS like
                // STATUS_DLL_NOT_FOUND renders 0xC0000135, and the fixed width
                // gives a uniform 8-digit column (0x2 -> 0x00000002).
                Some(code) => format!("0x{:08X}", code as u32),
                None => "unknown".to_owned(),
            };
            blocks.push(format!("Exit code: {exit_str}"));
        }
        if !stdout.is_empty() {
            blocks.push(stdout.to_owned());
        }
        if !stderr.is_empty() {
            blocks.push(stderr.to_owned());
        }

        let prefixed = blocks
            .join("\n")
            .split('\n')
            .map(|line| format!("[compile:{arch}] {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        Some(prefixed)
    }

    /// The raw OS code an `IO_FAILED` / `REGISTRY_FAILED` carries in its wire
    /// `details`. `None` for every other code (whose `details` shape has no
    /// `osError`), for a failure that did not come from an OS call, and for a
    /// CLI-side error (which carries no `details` at all).
    fn os_error(&self) -> Option<u32> {
        let details: OsErrorDetails = serde_json::from_value(self.details.clone()?).ok()?;
        details.os_error
    }

    /// The OS code the failure carries paired with the system's own text for it
    /// (`5` -> "Access is denied."), rendered as an `os error <n>: <text>` line
    /// under the `error:` line in TEXT mode only - a bare `(os error 32)` names
    /// the code but not what it means. `None` when the failure carries no OS
    /// code, and when the `message` already spells the text out: a failure the
    /// adapter built from a `std::io::Error` carries the same wording inline, so
    /// a second copy would only stutter.
    ///
    /// The text is whatever `FormatMessage` returns for the code - `std`'s
    /// `Display` for a raw OS error IS that call - so the system's wording (and
    /// its locale) reaches the user without a Win32 edge in this
    /// `forbid(unsafe_code)` crate. Like [`CliError::hint`] this stays out of
    /// the `--json` envelope: a machine consumer reads the raw code from
    /// `error.details.osError` and renders it however it likes.
    pub fn os_error_message(&self) -> Option<(u32, String)> {
        let code = self.os_error()?;
        let rendered = io::Error::from_raw_os_error(code as i32).to_string();
        // `std` renders `<text> (os error <n>)`; drop the suffix, since the line
        // names the code itself. Built from the same `i32` `std` formats, so the
        // strip cannot miss; `unwrap_or` keeps the whole rendering rather than
        // dropping the line if it ever does.
        let text = rendered
            .strip_suffix(&format!(" (os error {})", code as i32))
            .unwrap_or(&rendered);
        (!self.message.contains(text)).then(|| (code, text.to_owned()))
    }

    /// The actionable follow-up for a failure the user can fix themselves,
    /// rendered as a `hint:` line under the `error:` line in TEXT mode only. A
    /// `--json` consumer classifies structurally off `error.details` instead
    /// (`osError`), so the prose stays out of the machine envelope.
    ///
    /// The one case today is elevation: a storage write that failed with
    /// `ERROR_ACCESS_DENIED`. The prose is the REMEDY alone - the failing call
    /// is in the `error:` line above it and what the code means is in the
    /// `os error 5:` line between them.
    pub fn hint(&self) -> Option<&'static str> {
        (self.os_error() == Some(ERROR_ACCESS_DENIED))
            .then_some("run this command as administrator")
    }

    /// The canonical machine-readable code surfaced as the `--json` envelope's
    /// `error.code`; one string per exit class, never the wire spelling.
    pub fn code(&self) -> &'static str {
        self.category.code_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// The source origin (DIAGNOSTIC), rendered only in text-mode stderr; the
    /// `--json` envelope omits it so the machine contract stays unchanged.
    pub fn location(&self) -> Option<&SourceLocation> {
        self.location.as_deref()
    }

    /// The structured wire `details`: surfaced VERBATIM as the `--json`
    /// envelope's `error.details` when present, so a machine consumer gets the
    /// compile target / exit code / output structured rather than scraping the
    /// `[compile:<arch>]` stderr text. The raw `Value`, not the typed
    /// [`CompileDetails`] view - any wire code's per-code shape forwards
    /// unchanged. Only a compile failure carries it today; `None` otherwise.
    pub fn details(&self) -> Option<&Value> {
        self.details.as_ref()
    }

    pub fn exit_code(&self) -> i32 {
        self.category.exit_code()
    }
}

/// The one seam from the shared host's flat failure to the CLI's exit-class
/// model. A structured `Wire` maps through [`CliError::from_wire`]
/// (canonicalizing its wire code to the spec exit class and preserving
/// `details`, so a `COMPILER_FAILED` keeps its diagnostics); every no-wire arm
/// (`Load`/`Gate`/`Transport`/`Decode`) collapses to `GENERIC` (exit 1)
/// carrying the host-owned wording, so every `?` on a host invoke keeps working
/// and the emitted text stays byte-identical to the pre-extraction CLI.
impl From<HostError> for CliError {
    fn from(error: HostError) -> CliError {
        let location = error.location().cloned();
        match error.kind() {
            HostErrorKind::Wire(wire) => CliError::from_wire((**wire).clone()),
            // The no-wire arms collapse to GENERIC but keep the host's origin
            // (the DLL-load site, the gate, ...) over this conversion site.
            _ => CliError::generic(error.to_string()).at(location),
        }
    }
}

/// Friendly architecture label for a clang target triple, matching the
/// `Compiling for <arch>...` progress lines and reused for the `[compile:<arch>]`
/// diagnostics prefix. The mapping lives in the host so the CLI and the UI host
/// name a target identically; re-exported here for the local call sites.
pub(crate) use windhawk_core_host::arch_label;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_codes_map_to_canonical_code_and_exit() {
        // Every wire ErrorCode maps to its canonical error.code string and exit
        // code. The three dual-spelling wire codes collapse onto the CLI class
        // name: APP_ROOT_INVALID -> ENV_INVALID, COMPILER_FAILED ->
        // COMPILE_FAILED, CANCELED -> CANCELLED.
        let cases = [
            (ErrorCode::AppRootInvalid, "ENV_INVALID", 3),
            (ErrorCode::ModNotInstalled, "MOD_NOT_INSTALLED", 4),
            (ErrorCode::ModNotInRepo, "MOD_NOT_IN_REPO", 5),
            (ErrorCode::RepoUnreachable, "REPO_UNREACHABLE", 6),
            (ErrorCode::CompilerFailed, "COMPILE_FAILED", 7),
            (ErrorCode::Canceled, "CANCELLED", 9),
            // Each of the five remaining wire codes has its own exit class, so
            // every wire ErrorCode is exit-distinguishable and exit 1 (GENERIC)
            // is reserved for a CLI-side failure with no wire error.
            (ErrorCode::UpdateInProgress, "UPDATE_IN_PROGRESS", 10),
            (ErrorCode::IoFailed, "IO_FAILED", 11),
            (ErrorCode::RegistryFailed, "REGISTRY_FAILED", 12),
            (ErrorCode::InvalidRequest, "INVALID_REQUEST", 13),
            (ErrorCode::Internal, "INTERNAL", 14),
        ];
        for (code, expected_code, expected_exit) in cases {
            let err = CliError::from_wire(WireError::new(code, "x"));
            assert_eq!(err.code(), expected_code, "{code:?}");
            assert_eq!(err.exit_code(), expected_exit, "{code:?}");
        }
    }

    #[test]
    fn cli_side_constructors_match_the_spec_table() {
        for (err, code, exit) in [
            (CliError::usage("x"), "USAGE", 2),
            (CliError::generic("x"), "GENERIC", 1),
            (CliError::env_invalid("x"), "ENV_INVALID", 3),
            (CliError::mod_not_installed("m"), "MOD_NOT_INSTALLED", 4),
            (CliError::restart_required("x"), "RESTART_REQUIRED", 8),
        ] {
            assert_eq!(err.code(), code);
            assert_eq!(err.exit_code(), exit);
        }
    }

    #[test]
    fn from_wire_preserves_code_and_message() {
        let err = CliError::from_wire(WireError::new(ErrorCode::ModNotInRepo, "nope"));
        assert_eq!(err.code(), "MOD_NOT_IN_REPO");
        assert_eq!(err.message(), "nope");
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn a_canceled_operation_maps_to_exit_9() {
        // The async drain (client.rs) maps a `failed(CANCELED)` operation event
        // through `from_wire`; that is the exit-9 outcome a Ctrl+C cancellation
        // produces. The end-to-end Ctrl+C DELIVERY to a subprocess is a manual
        // smoke item - deterministic console signal delivery is not achievable
        // in the parallel test harness without disrupting sibling tests or a
        // production-only signal change - but the resulting exit code is pinned
        // here. The emitted code is the canonical CANCELLED, not the wire
        // CANCELED.
        let err = CliError::from_wire(WireError::new(ErrorCode::Canceled, "canceled"));
        assert_eq!(err.code(), "CANCELLED");
        assert_eq!(err.exit_code(), 9);
    }

    #[test]
    fn host_error_wire_canonicalizes_and_no_wire_arms_are_generic() {
        // The From<HostError> seam: a structured Wire canonicalizes its code to
        // the spec exit class (APP_ROOT_INVALID -> ENV_INVALID) and keeps its
        // message; every no-wire arm collapses to GENERIC carrying the host
        // wording verbatim.
        let wire: CliError =
            HostError::wire(WireError::new(ErrorCode::AppRootInvalid, "no ini")).into();
        assert_eq!(wire.code(), "ENV_INVALID");
        assert_eq!(wire.exit_code(), 3);
        assert_eq!(wire.message(), "no ini");

        for host in [
            HostError::load("load boom".to_owned()),
            HostError::gate("contract boom".to_owned()),
            HostError::transport("transport boom".to_owned()),
            HostError::decode("decode boom".to_owned()),
        ] {
            let message = host.to_string();
            let err: CliError = host.into();
            assert_eq!(err.code(), "GENERIC");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.message(), message);
        }
    }

    fn compiler_failure(exit_code: serde_json::Value, stdout: &str, stderr: &str) -> CliError {
        CliError::from_wire(WireError::with_details(
            ErrorCode::CompilerFailed,
            "Compilation failed",
            serde_json::json!({
                "target": "x86_64-w64-mingw32",
                "exitCode": exit_code,
                "stdout": stdout,
                "stderr": stderr,
            }),
        ))
    }

    #[test]
    fn compiler_diagnostics_prefixes_output_with_the_arch_label() {
        // exit code 1 with captured output: no "Exit code:" line, the arch
        // label maps from the triple, each line is prefixed.
        let err = compiler_failure(serde_json::json!(1), "error: boom\nmore", "");
        let diag = err.compiler_diagnostics().expect("diagnostics");
        assert_eq!(diag, "[compile:x64] error: boom\n[compile:x64] more");
        assert_eq!(err.exit_code(), 7);
    }

    #[test]
    fn compiler_diagnostics_falls_back_to_exit_code_for_nonstandard_failures() {
        // A non-1 exit code with no output renders only the hex exit-code line,
        // zero-padded to the uniform 8-digit column.
        let err = compiler_failure(serde_json::json!(2), "", "");
        assert_eq!(
            err.compiler_diagnostics().unwrap(),
            "[compile:x64] Exit code: 0x00000002"
        );
        // A null exit code is "unknown".
        let err = compiler_failure(serde_json::Value::Null, "", "");
        assert_eq!(
            err.compiler_diagnostics().unwrap(),
            "[compile:x64] Exit code: unknown"
        );
    }

    #[test]
    fn compiler_diagnostics_renders_a_negative_exit_code_as_unsigned_hex() {
        // STATUS_DLL_NOT_FOUND arrives as the signed i32 -1073741515; the
        // unsigned 32-bit bit pattern is the conventional 0xC0000135, not the
        // JS toString(16) `0x-3ffffecb` the TS reference produced.
        let err = compiler_failure(serde_json::json!(-1073741515i64), "", "");
        assert_eq!(
            err.compiler_diagnostics().unwrap(),
            "[compile:x64] Exit code: 0xC0000135"
        );
    }

    #[test]
    fn an_access_denied_storage_failure_hints_at_elevation() {
        // The unelevated `mod settings set` case: the core's REGISTRY_FAILED
        // carries os error 5 in its details, so the text render can name the
        // fix the raw "(os error 5)" message does not.
        for code in [ErrorCode::RegistryFailed, ErrorCode::IoFailed] {
            let err = CliError::from_wire(WireError::with_details(
                code,
                "remove_tree failed: RegOpenKeyEx",
                serde_json::json!({ "key": "SOFTWARE\\Windhawk", "osError": 5 }),
            ));
            assert_eq!(
                err.hint(),
                Some("run this command as administrator"),
                "{code:?}"
            );
        }
    }

    #[test]
    fn an_os_code_renders_the_system_text_for_the_code() {
        // The registry backend names the failing call ("RegOpenKeyEx") and
        // carries the raw code in `details`, but neither says what the code
        // means; the text render pairs it with the system's own wording
        // ("Access is denied."). Asserted against `std`'s rendering rather than
        // the English string, so the test holds on a localized Windows.
        let err = CliError::from_wire(WireError::with_details(
            ErrorCode::RegistryFailed,
            "remove_tree failed: RegOpenKeyEx",
            serde_json::json!({ "key": "SOFTWARE\\Windhawk", "osError": 5 }),
        ));
        let (code, text) = err.os_error_message().expect("os error text");
        assert_eq!(code, 5);
        // The `(os error 5)` suffix `std` appends is stripped: the line names
        // the code itself.
        assert!(!text.contains("os error"), "{text}");
        assert_eq!(
            io::Error::from_raw_os_error(5).to_string(),
            format!("{text} (os error 5)")
        );
    }

    #[test]
    fn a_message_that_already_carries_the_system_text_gets_no_second_copy() {
        // An IO failure the adapter built from a `std::io::Error` carries that
        // wording inline, so there is nothing to spell out. The core trims the
        // trailing `(os error 32)` off such a message - the code is a `details`
        // field - so the text alone is what reaches here.
        let rendered = io::Error::from_raw_os_error(32).to_string();
        let text = rendered.replace(" (os error 32)", "");
        let err = CliError::from_wire(WireError::with_details(
            ErrorCode::IoFailed,
            format!("set failed: {text}"),
            serde_json::json!({ "path": "settings.ini", "osError": 32 }),
        ));
        assert_eq!(err.os_error_message(), None);
    }

    #[test]
    fn errors_without_an_os_code_render_no_os_error_line() {
        // A `null` osError (the failure was not from an OS call), and a
        // CLI-side error, which carries no details at all.
        let no_os_call = CliError::from_wire(WireError::with_details(
            ErrorCode::RegistryFailed,
            "guard tripped",
            serde_json::json!({ "key": "Settings", "osError": serde_json::Value::Null }),
        ));
        assert_eq!(no_os_call.os_error_message(), None);
        assert_eq!(CliError::usage("x").os_error_message(), None);
    }

    #[test]
    fn other_storage_failures_and_codeless_errors_have_no_hint() {
        // A different OS code is a different problem (a sharing violation is
        // not fixed by elevating), a `null` osError means the failure was not
        // from an OS call, and a CLI-side error carries no details at all.
        let sharing = CliError::from_wire(WireError::with_details(
            ErrorCode::IoFailed,
            "set failed: sharing violation",
            serde_json::json!({ "path": "settings.ini", "osError": 32 }),
        ));
        assert_eq!(sharing.hint(), None);

        let no_os_call = CliError::from_wire(WireError::with_details(
            ErrorCode::RegistryFailed,
            "guard tripped",
            serde_json::json!({ "key": "Settings", "osError": serde_json::Value::Null }),
        ));
        assert_eq!(no_os_call.hint(), None);

        assert_eq!(CliError::mod_not_installed("m").hint(), None);
        assert_eq!(CliError::usage("x").hint(), None);
    }

    #[test]
    fn non_compile_errors_have_no_diagnostics() {
        assert!(
            CliError::mod_not_installed("m")
                .compiler_diagnostics()
                .is_none()
        );
        assert!(CliError::usage("x").compiler_diagnostics().is_none());
    }
}
