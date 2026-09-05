//! Flag construction: the clang++ argument builder for a compile target, the
//! `compilerOptions` splitter, the per-mod backward-compatibility includes, the
//! shared `WH_*` macro-define tail, the version->hex transform, and the editor
//! `getCompileFlags` producer. `flags` is the single owner of the flag-fragment
//! vocabulary (`STD_CPP`/`WINDOWS_VERSION_DEFINES`/`FP_EXCEPTION_MAYTRAP`); `pch`
//! reads them and `wh_macro_defines` from here (the only `flags`->`pch` edges).

use std::path::Path;

use serde_json::Value;
use windhawk_core_domain::{CompilationTarget, ModId, Version, coerce_version};

use crate::error::CoreError;
use crate::services::wire::to_value_result;

// Flag fragments single-sourced between the real compile and getCompileFlags.
pub(super) const STD_CPP: &str = "-std=c++23";
pub(super) const WINDOWS_VERSION_DEFINES: &[&str] = &[
    "-DWINVER=0x0A00",
    "-D_WIN32_WINNT=0x0A00",
    "-D_WIN32_IE=0x0A00",
    "-DNTDDI_VERSION=0x0A000008",
];

/// Suppress `-Wunneeded-internal-declaration` (on by default in clang): an
/// internal-linkage function or variable that is defined but never referenced.
/// Shared by the real compile and the editor flag set.
pub(super) const NO_UNNEEDED_INTERNAL_DECLARATION: &str = "-Wno-unneeded-internal-declaration";

/// Floating-point operations may observe the FP environment and raise
/// exceptions instead of assuming the default non-trapping mode. A
/// code-generation flag, so it is applied to the PCH build as well as the
/// compile and the editor set: clang rejects an `-include-pch` whose FP
/// exception mode differs from the consuming compile.
pub(super) const FP_EXCEPTION_MAYTRAP: &str = "-ffp-exception-behavior=maytrap";

/// Suppress `-Wdeprecated-declarations` (on by default in clang): a use of an
/// API marked deprecated. Compile-only - the diagnostics of a successful
/// compile become the install result's `warnings`, so the noise reaches whoever
/// installs the mod, while the editor keeps the warning for the author who can
/// act on it.
const NO_DEPRECATED_DECLARATIONS: &str = "-Wno-deprecated-declarations";

/// Control Flow Guard metadata: the load config's guard function table and
/// `IMAGE_DLLCHARACTERISTICS_GUARD_CF` on the image. A process running under
/// strict CFG refuses to load an image without them, so a mod compiled without
/// this one cannot be injected there at all. The `-nochecks` variant leaves out
/// the indirect-call checks themselves, which is what a mod needs: it routinely
/// calls through pointers to targets outside its own guard table - hook
/// trampolines and addresses resolved in other modules - and a check rejects
/// those at runtime. Unlike `FP_EXCEPTION_MAYTRAP` it is code generation ALONE -
/// it is not part of the PCH's language options and defines no macro - so the
/// AST-only PCH build and the clangd flag set both leave it out, and clang
/// accepts an `-include-pch` across the difference.
const CF_GUARD: &str = "-mguard=cf-nochecks";

/// `WH_WINDHAWK_VERSION`: `0x` + the major/minor/patch bytes + `00` (the TS
/// `windhawkVersionHex`); `0x00000000` when the version does not parse.
pub(super) fn windhawk_version_hex(version: Option<&str>) -> String {
    match version.and_then(coerce_version) {
        Some((major, minor, patch)) => format!("0x{major:02x}{minor:02x}{patch:02x}00"),
        None => "0x00000000".to_owned(),
    }
}

/// Split a `compilerOptions` string into argv, honoring single/double quotes
/// (the TS `splitargs` with the default whitespace separator); empty/whitespace
/// yields no arguments.
pub(super) fn parse_compiler_options(options: Option<&str>) -> Vec<String> {
    let Some(options) = options else {
        return Vec::new();
    };
    if options.trim().is_empty() {
        return Vec::new();
    }

    let mut single = false;
    let mut double = false;
    let mut token = String::new();
    let mut out = Vec::new();
    for ch in options.chars() {
        if ch == '\'' && !double {
            single = !single;
            continue;
        }
        if ch == '"' && !single {
            double = !double;
            continue;
        }
        if !single && !double && ch.is_whitespace() {
            if !token.is_empty() {
                out.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        out.push(token);
    }
    out
}

/// The per-(modId, version) backward-compatibility `-include` flags some
/// historical mod versions need to compile under the current toolchain (the TS
/// `backwardCompatibilityFlags`). Keyed on the `<id>\n<version>` string.
fn backward_compatibility_flags(key: &str) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if key == "chrome-ui-tweaks\n1.0.0" {
        flags.extend(["-include", "atomic", "-include", "optional"]);
    }
    if key == "sib-plusplus-tweaker\n0.7.1" {
        flags.extend(["-include", "atomic"]);
    }
    if matches!(
        key,
        "classic-explorer-treeview\n1.1.3" | "sysdm-general-tab\n1.1"
    ) {
        flags.extend(["-include", "cmath"]);
    }
    if matches!(
        key,
        "ce-disable-process-button-flashing\n1.0.1" | "windows-7-clock-spacing\n1.0.0"
    ) {
        flags.extend(["-include", "vector"]);
    }
    flags
}

/// Escape a value for embedding in a C string literal: both `\` and `"` take a
/// leading backslash. The backslash has to be escaped along with the quote -
/// escaping the quote alone leaves a value ending in `\` producing `L"1.0\"`,
/// where the trailing backslash escapes the literal's own closing quote and the
/// literal runs on into whatever follows.
fn escape_c_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The contiguous five-line `WH_*` macro-define tail shared VERBATIM by
/// `build_compile_args` (here) and `build_pch_args` (in `pch`) - the one DRY
/// point the split would otherwise entrench across a module boundary. Scoped to
/// EXACTLY this run: the MinGW STDIO define, the `WH_MOD` marker, and the
/// string-escaped `WH_MOD_ID`/`WH_MOD_VERSION`/`WH_WINDHAWK_VERSION`. The
/// `-DUNICODE`/`-D_UNICODE` defines and the conditional `WINDOWS_VERSION_DEFINES`
/// are deliberately NOT folded in - they differ across the two builders, so
/// including them would force an argv reorder the byte-identical contract forbids.
pub(super) fn wh_macro_defines(
    mod_id: &ModId,
    version: &Version,
    version_hex: &str,
) -> Vec<String> {
    vec![
        "-D__USE_MINGW_ANSI_STDIO=0".to_owned(),
        "-DWH_MOD".to_owned(),
        format!("-DWH_MOD_ID=L\"{}\"", escape_c_string(mod_id.as_str())),
        format!(
            "-DWH_MOD_VERSION=L\"{}\"",
            escape_c_string(version.as_str())
        ),
        format!("-DWH_WINDHAWK_VERSION={version_hex}"),
    ]
}

/// The shared compile inputs both arg builders need: the swap-prone
/// `mod_id`/`version` as the newtypes (so a misordered construction is a type
/// error), the windhawk-version hex, and the mod's compiler options - the
/// per-COMPILE constants. The per-builder extras (engine path + dll/pch paths
/// for the compile; header/pch paths for the PCH build) and the per-TARGET
/// `target` are passed separately, so this is constructed once in
/// `orchestrate::compile_mod` and threaded through both builders and their
/// wrappers.
pub(super) struct CompileSpec<'a> {
    pub mod_id: &'a ModId,
    pub version: &'a Version,
    pub version_hex: &'a str,
    pub compiler_options: &'a [String],
}

/// clang's spellings for one argument handed straight to the linker, the flag
/// and its argument in separate options. The single-dash `-for-linker` and the
/// joined `-Xlinker=` are not accepted, so they are not here.
const LINKER_ARG_FLAGS: &[&str] = &["-Xlinker", "--for-linker"];

/// Whether a LINKER argument names the output file. `pending_path` carries the
/// separate spelling across calls: its path is simply the next argument to
/// reach the linker, which the mod may spell in a different option than the one
/// carrying the flag (`-Xlinker -o -Wl,out.dll`).
fn is_linker_output(arg: &str, pending_path: &mut bool) -> bool {
    if *pending_path {
        *pending_path = false;
        return true;
    }
    if arg == "-o" || arg == "--output" {
        *pending_path = true;
        return true;
    }
    // Prefix-matched as at the driver level, which also covers the single-dash
    // long option: ld has no `-output`, so it reads `-output=x` as an `-o`
    // joined to the path `utput=x` and writes the image there.
    (arg.starts_with("-o") && arg.len() > 2) || arg.starts_with("--output=")
}

/// The mod's compiler options with any output-file redirect dropped. The
/// compile's own `-o` precedes them and clang honors the LAST one, so an `-o`
/// here would put the shared library somewhere the install never looks: clang
/// exits 0 with nothing to say, and the mod is registered under a
/// `LibraryFileName` that was never written. An `-o` passed through to the
/// LINKER does the same and needs its own handling: clang renders its own `-o`
/// ahead of the arguments a mod forwards, so the forwarded one is the last the
/// linker sees. Every other option a mod passes still overrides the defaults
/// ahead of it.
///
/// This keeps a mod from MOVING the compiled DLL. It is not a sandbox around
/// clang's option surface, which can write and run other things besides.
fn without_output_redirect(options: &[String]) -> Vec<String> {
    let mut kept = Vec::with_capacity(options.len());
    let mut rest = options.iter();
    let mut pending_path = false;
    while let Some(arg) = rest.next() {
        // The separate spelling carries its path in the next argument, which
        // would otherwise be left behind as an input file.
        if arg == "-o" || arg == "--output" {
            rest.next();
            continue;
        }
        // The joined spellings. `-o` is matched by prefix: no mod flag
        // legitimately starts with it, and a miss here is the redirect getting
        // through.
        if (arg.starts_with("-o") && arg.len() > 2) || arg.starts_with("--output=") {
            continue;
        }
        // A comma list of linker arguments: take the redirect out of the list
        // and keep the rest, so the `-Wl,--export-all-symbols` published mods
        // pass still reaches the linker. Dropped empty rather than passing a
        // bare `-Wl,`.
        if let Some(list) = arg.strip_prefix("-Wl,") {
            let elements: Vec<&str> = list
                .split(',')
                .filter(|element| !is_linker_output(element, &mut pending_path))
                .collect();
            if !elements.is_empty() {
                kept.push(format!("-Wl,{}", elements.join(",")));
            }
            continue;
        }
        // One linker argument, joined onto the flag.
        if let Some(value) = arg.strip_prefix("--for-linker=") {
            if !is_linker_output(value, &mut pending_path) {
                kept.push(arg.clone());
            }
            continue;
        }
        // One linker argument in the next option; the two travel together, so
        // the redirect takes both.
        if LINKER_ARG_FLAGS.contains(&arg.as_str()) {
            match rest.next() {
                Some(value) if is_linker_output(value, &mut pending_path) => {}
                Some(value) => {
                    kept.push(arg.clone());
                    kept.push(value.clone());
                }
                None => kept.push(arg.clone()),
            }
            continue;
        }
        kept.push(arg.clone());
    }
    kept
}

/// The clang++ argument vector for one target (the TS `compileModInternal`):
/// the optimized, CFG-marked shared-library build, the Unicode and
/// Windows-version defines (the latter skipped for the one historical
/// incompatible mod), the `WH_*` macros, the engine import library, the
/// source-on-stdin marker, the per-target `-o`, then the mod's own compiler
/// options (bar an output redirect) and the per-mod backward-compatibility
/// includes. Pure, so the special cases are unit-tested without spawning clang.
pub(super) fn build_compile_args(
    spec: &CompileSpec,
    target: CompilationTarget,
    engine_path: &str,
    dll_path: &Path,
    pch_path: Option<&Path>,
) -> Vec<String> {
    let engine_lib = Path::new(engine_path)
        .join(target.subfolder())
        .join("windhawk.lib");
    let key = format!("{}\n{}", spec.mod_id, spec.version);

    let mut args: Vec<String> = vec![
        STD_CPP.to_owned(),
        "-O2".to_owned(),
        FP_EXCEPTION_MAYTRAP.to_owned(),
        CF_GUARD.to_owned(),
        "-shared".to_owned(),
        "-DUNICODE".to_owned(),
        "-D_UNICODE".to_owned(),
        NO_UNNEEDED_INTERNAL_DECLARATION.to_owned(),
        NO_DEPRECATED_DECLARATIONS.to_owned(),
    ];
    // One historical mod is incompatible with the modern Windows-version
    // defines (the TS classic-taskdlg-fix special case).
    if key != "classic-taskdlg-fix\n1.1.0" {
        args.extend(WINDOWS_VERSION_DEFINES.iter().map(|s| (*s).to_owned()));
    }
    args.extend(wh_macro_defines(
        spec.mod_id,
        spec.version,
        spec.version_hex,
    ));
    args.push(engine_lib.to_string_lossy().into_owned());
    args.push("-x".to_owned());
    args.push("c++".to_owned());
    args.push("-".to_owned());
    args.push("-include".to_owned());
    args.push("windhawk_api.h".to_owned());
    args.push("-target".to_owned());
    args.push(target.triple().to_owned());
    args.push("-Wl,--export-all-symbols".to_owned());
    args.push("-o".to_owned());
    args.push(dll_path.to_string_lossy().into_owned());
    // Consume the cached precompiled header when one was prepared (the TS
    // `pchPath ? ['-include-pch', pchPath] : []`), before the mod's own options.
    if let Some(pch) = pch_path {
        args.push("-include-pch".to_owned());
        args.push(pch.to_string_lossy().into_owned());
    }
    args.extend(without_output_redirect(spec.compiler_options));
    args.extend(
        backward_compatibility_flags(&key)
            .iter()
            .map(|s| (*s).to_owned()),
    );
    args
}

/// `getCompileFlags`: the fixed clangd flag set written to `compile_flags.txt`
/// (the TS `editorWorkspaceUtils` `compileFlags`), single-sourced with the real
/// compile flags above. A pure, session-free handler (`Handler::Stateless`):
/// takes no params; returns the flag array.
pub fn get_compile_flags(_params: Value) -> Result<Value, CoreError> {
    to_value_result("getCompileFlags", &editor_compile_flags())
}

fn editor_compile_flags() -> Vec<String> {
    let mut flags = vec![
        "-x".to_owned(),
        "c++".to_owned(),
        STD_CPP.to_owned(),
        "-target".to_owned(),
        CompilationTarget::X86_64.triple().to_owned(),
        FP_EXCEPTION_MAYTRAP.to_owned(),
        "-DUNICODE".to_owned(),
        "-D_UNICODE".to_owned(),
    ];
    flags.extend(WINDOWS_VERSION_DEFINES.iter().map(|s| (*s).to_owned()));
    flags.extend(
        [
            "-D__USE_MINGW_ANSI_STDIO=0",
            "-DWH_MOD",
            "-DWH_EDITING",
            "-include",
            "windhawk_api.h",
            "-Wall",
            "-Wextra",
            "-Wno-unused-parameter",
            "-Wno-missing-field-initializers",
            "-Wno-cast-function-type-mismatch",
            NO_UNNEEDED_INTERNAL_DECLARATION,
        ]
        .iter()
        .map(|s| (*s).to_owned()),
    );
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn version_hex_matches_the_ts_format() {
        assert_eq!(windhawk_version_hex(Some("1.6.1")), "0x01060100");
        assert_eq!(windhawk_version_hex(Some("v1.8.0-beta")), "0x01080000");
        assert_eq!(windhawk_version_hex(None), "0x00000000");
        assert_eq!(windhawk_version_hex(Some("not-a-version")), "0x00000000");
    }

    #[test]
    fn compiler_options_split_on_whitespace_and_quotes() {
        assert_eq!(parse_compiler_options(None), Vec::<String>::new());
        assert_eq!(parse_compiler_options(Some("   ")), Vec::<String>::new());
        assert_eq!(
            parse_compiler_options(Some("-lcomctl32 -lgdi32")),
            vec!["-lcomctl32", "-lgdi32"]
        );
        assert_eq!(
            parse_compiler_options(Some("-DFOO=\"a b\" -lc")),
            vec!["-DFOO=a b", "-lc"]
        );
    }

    #[test]
    fn editor_flags_are_single_sourced_with_the_compile_defines() {
        let flags = editor_compile_flags();
        assert!(flags.contains(&STD_CPP.to_owned()));
        for define in WINDOWS_VERSION_DEFINES {
            assert!(flags.contains(&(*define).to_owned()));
        }
        assert!(flags.contains(&"-DWH_EDITING".to_owned()));
        // The FP-exception and unneeded-internal-declaration flags are shared
        // with the real compile.
        assert!(flags.contains(&FP_EXCEPTION_MAYTRAP.to_owned()));
        assert!(flags.contains(&NO_UNNEEDED_INTERNAL_DECLARATION.to_owned()));
        // The CFG metadata and the deprecation suppression are compile-only.
        assert!(!flags.contains(&CF_GUARD.to_owned()));
        assert!(!flags.contains(&NO_DEPRECATED_DECLARATIONS.to_owned()));
    }

    #[test]
    fn backward_compat_flags_match_the_per_mod_special_cases() {
        assert_eq!(
            backward_compatibility_flags("chrome-ui-tweaks\n1.0.0"),
            ["-include", "atomic", "-include", "optional"]
        );
        assert_eq!(
            backward_compatibility_flags("sib-plusplus-tweaker\n0.7.1"),
            ["-include", "atomic"]
        );
        assert_eq!(
            backward_compatibility_flags("classic-explorer-treeview\n1.1.3"),
            ["-include", "cmath"]
        );
        assert_eq!(
            backward_compatibility_flags("sysdm-general-tab\n1.1"),
            ["-include", "cmath"]
        );
        assert_eq!(
            backward_compatibility_flags("windows-7-clock-spacing\n1.0.0"),
            ["-include", "vector"]
        );
        // A version that does not match a special case keeps the flags off,
        // including a same-id different-version mod.
        assert!(backward_compatibility_flags("chrome-ui-tweaks\n2.0.0").is_empty());
        assert!(backward_compatibility_flags("test-mod\n1.0").is_empty());
    }

    /// Build a compile argv from owned inputs, constructing the `CompileSpec`
    /// internally (it borrows, so the temporaries must outlive it).
    #[allow(clippy::too_many_arguments)]
    fn compile_args(
        mod_id: &str,
        version: &str,
        version_hex: &str,
        opts: &[&str],
        target: CompilationTarget,
        engine: &str,
        dll: &str,
        pch: Option<&str>,
    ) -> Vec<String> {
        let mod_id = ModId::from(mod_id);
        let version = Version::from(version);
        let opts: Vec<String> = opts.iter().map(|s| (*s).to_owned()).collect();
        let spec = CompileSpec {
            mod_id: &mod_id,
            version: &version,
            version_hex,
            compiler_options: &opts,
        };
        build_compile_args(&spec, target, engine, Path::new(dll), pch.map(Path::new))
    }

    #[test]
    fn build_args_skip_windows_defines_only_for_the_incompatible_mod() {
        let normal = compile_args(
            "test-mod",
            "1.0",
            "0x01060100",
            &["-lcomctl32"],
            CompilationTarget::X86_64,
            "C:\\Engine",
            "C:\\out\\m.dll",
            None,
        );
        assert!(normal.contains(&"-DWINVER=0x0A00".to_owned()));
        assert!(normal.contains(&"-DWH_WINDHAWK_VERSION=0x01060100".to_owned()));
        assert!(normal.contains(&"-DWH_MOD_ID=L\"test-mod\"".to_owned()));
        // The FP-exception, CFG and unneeded-internal-declaration flags precede
        // the mod's own options, so a mod can still override them.
        assert!(normal.contains(&FP_EXCEPTION_MAYTRAP.to_owned()));
        assert!(normal.contains(&CF_GUARD.to_owned()));
        assert!(normal.contains(&NO_UNNEEDED_INTERNAL_DECLARATION.to_owned()));
        assert!(normal.contains(&NO_DEPRECATED_DECLARATIONS.to_owned()));
        let fp_idx = normal
            .iter()
            .position(|a| a == FP_EXCEPTION_MAYTRAP)
            .unwrap();
        let guard_idx = normal.iter().position(|a| a == CF_GUARD).unwrap();
        let opt_idx = normal.iter().position(|a| a == "-lcomctl32").unwrap();
        assert!(fp_idx < opt_idx);
        assert!(guard_idx < opt_idx);
        // The mod's own options and the engine import library are present.
        assert!(normal.contains(&"-lcomctl32".to_owned()));
        assert!(normal.iter().any(|a| a.ends_with("64\\windhawk.lib")));

        // classic-taskdlg-fix 1.1.0 omits the Windows-version defines.
        let special = compile_args(
            "classic-taskdlg-fix",
            "1.1.0",
            "0x0",
            &[],
            CompilationTarget::X86_64,
            "C:\\Engine",
            "C:\\out\\m.dll",
            None,
        );
        assert!(!special.contains(&"-DWINVER=0x0A00".to_owned()));
        // chrome-ui-tweaks 1.0.0 appends its backward-compatibility includes.
        let compat = compile_args(
            "chrome-ui-tweaks",
            "1.0.0",
            "0x0",
            &[],
            CompilationTarget::X86_64,
            "C:\\Engine",
            "C:\\out\\m.dll",
            None,
        );
        assert!(compat.contains(&"optional".to_owned()));
    }

    #[test]
    fn build_args_escape_quotes_in_the_mod_id_and_version() {
        let args = compile_args(
            "weird\"id",
            "1\"0",
            "0x0",
            &[],
            CompilationTarget::I686,
            "E",
            "o",
            None,
        );
        assert!(args.contains(&"-DWH_MOD_ID=L\"weird\\\"id\"".to_owned()));
        assert!(args.contains(&"-DWH_MOD_VERSION=L\"1\\\"0\"".to_owned()));
    }

    #[test]
    fn build_args_escape_backslashes_so_the_literal_stays_terminated() {
        // A version ending in a backslash: escaping the quote alone would emit
        // L"1.0\" and swallow the closing quote.
        let args = compile_args(
            "m",
            "1.0\\",
            "0x0",
            &[],
            CompilationTarget::I686,
            "E",
            "o",
            None,
        );
        assert!(args.contains(&"-DWH_MOD_VERSION=L\"1.0\\\\\"".to_owned()));

        // A backslash-quote pair escapes to backslash-backslash then
        // backslash-quote, so neither character can terminate the literal.
        let args = compile_args(
            "m",
            "1.0\\\"",
            "0x0",
            &[],
            CompilationTarget::I686,
            "E",
            "o",
            None,
        );
        assert!(args.contains(&"-DWH_MOD_VERSION=L\"1.0\\\\\\\"\"".to_owned()));
    }

    #[test]
    fn build_args_consume_a_precompiled_header_when_present() {
        // No PCH: no -include-pch (the compileInstalledMod path).
        let none = compile_args(
            "m",
            "1.0",
            "0x0",
            &[],
            CompilationTarget::X86_64,
            "E",
            "o.dll",
            None,
        );
        assert!(!none.contains(&"-include-pch".to_owned()));

        // With a PCH: -include-pch <path> sits after -o and before the options.
        let with_pch = compile_args(
            "m",
            "1.0",
            "0x0",
            &["-lc"],
            CompilationTarget::X86_64,
            "E",
            "o.dll",
            Some("C:\\pch\\windhawk_t_x86_64-w64-mingw32.pch"),
        );
        let pch_idx = with_pch
            .iter()
            .position(|a| a == "-include-pch")
            .expect("-include-pch present");
        assert_eq!(
            with_pch[pch_idx + 1],
            "C:\\pch\\windhawk_t_x86_64-w64-mingw32.pch"
        );
        let o_idx = with_pch.iter().position(|a| a == "-o").unwrap();
        let opt_idx = with_pch.iter().position(|a| a == "-lc").unwrap();
        assert!(o_idx < pch_idx && pch_idx < opt_idx);
    }

    #[test]
    fn build_args_drop_a_mod_option_that_redirects_the_output() {
        let args = compile_args(
            "m",
            "1.0",
            "0x0",
            &[
                "-o",
                "C:\\evil1.dll",
                "-lcomctl32",
                "-oC:\\evil2.dll",
                "--output",
                "C:\\evil3.dll",
                "--output=C:\\evil4.dll",
            ],
            CompilationTarget::X86_64,
            "E",
            "C:\\out\\m.dll",
            None,
        );
        // Neither the redirect nor the path it carries reaches clang, so the
        // compile's own -o stays the last (and only) one.
        assert!(!args.iter().any(|a| a.contains("evil")));
        assert_eq!(args.iter().filter(|a| *a == "-o").count(), 1);
        let o_idx = args.iter().position(|a| a == "-o").unwrap();
        assert_eq!(args[o_idx + 1], "C:\\out\\m.dll");
        // Everything else the mod asked for survives.
        assert!(args.contains(&"-lcomctl32".to_owned()));
    }

    #[test]
    fn build_args_drop_an_output_redirect_aimed_at_the_linker() {
        // Every spelling the shipped clang accepts for handing `-o` down to the
        // linker, where it beats the driver's own: the comma list (joined,
        // separate, and split across two options), `-Xlinker`, and both
        // `--for-linker` forms. A miss lands the DLL at the mod's path with the
        // compile still exiting 0.
        let args = compile_args(
            "m",
            "1.0",
            "0x0",
            &[
                "-Wl,-o,C:\\evil1.dll",
                "-Wl,-oC:\\evil2.dll",
                "-Wl,-o",
                "-Wl,C:\\evil3.dll",
                "-Wl,-output=C:\\evil4.dll",
                "-Xlinker",
                "-o",
                "-Xlinker",
                "C:\\evil5.dll",
                "--for-linker",
                "--output",
                "--for-linker",
                "C:\\evil6.dll",
                "--for-linker=-o",
                "--for-linker=C:\\evil7.dll",
                "-lcomctl32",
            ],
            CompilationTarget::X86_64,
            "E",
            "C:\\out\\m.dll",
            None,
        );
        assert!(!args.iter().any(|a| a.contains("evil")));
        // No orphaned flag is left to consume the compile's own output path,
        // and no bare `-Wl,` is passed on.
        assert!(!args.iter().any(|a| a == "-Xlinker" || a == "--for-linker"));
        assert!(!args.iter().any(|a| a == "-Wl,"));
        // The compile's own `-o` is the ONLY one in the argv, at the driver
        // level and inside a `-Wl,` list: a path-less `-o` that survived would
        // leave nothing for the `evil` check above to catch, and the linker
        // would read the next argument it saw as the output name.
        assert_eq!(args.iter().filter(|a| *a == "-o").count(), 1);
        assert!(
            !args
                .iter()
                .filter_map(|a| a.strip_prefix("-Wl,"))
                .any(|list| list.split(',').any(|element| element == "-o"))
        );
        let o_idx = args.iter().position(|a| a == "-o").unwrap();
        assert_eq!(args[o_idx + 1], "C:\\out\\m.dll");
        assert!(args.contains(&"-lcomctl32".to_owned()));
    }

    #[test]
    fn linker_options_that_are_not_a_redirect_pass_through_untouched() {
        // The one linker option published mods actually pass, alone and sharing
        // a comma list with a redirect - the list is rebuilt without the
        // redirect rather than dropped whole.
        let args = compile_args(
            "m",
            "1.0",
            "0x0",
            &[
                "-Wl,--export-all-symbols",
                "-Wl,--allow-multiple-definition,-o,C:\\evil.dll",
                "-Xlinker",
                "--dynamicbase",
                "--for-linker=--no-insert-timestamp",
            ],
            CompilationTarget::X86_64,
            "E",
            "C:\\out\\m.dll",
            None,
        );
        assert!(!args.iter().any(|a| a.contains("evil")));
        assert!(args.contains(&"-Wl,--export-all-symbols".to_owned()));
        assert!(args.contains(&"-Wl,--allow-multiple-definition".to_owned()));
        assert!(args.contains(&"--for-linker=--no-insert-timestamp".to_owned()));
        let x_idx = args.iter().position(|a| a == "-Xlinker").unwrap();
        assert_eq!(args[x_idx + 1], "--dynamicbase");
    }
}
