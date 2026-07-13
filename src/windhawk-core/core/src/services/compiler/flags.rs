//! Flag construction: the clang++ argument builder for a compile target, the
//! `compilerOptions` splitter, the per-mod backward-compatibility includes, the
//! shared `WH_*` macro-define tail, the version->hex transform, and the editor
//! `getCompileFlags` producer. `flags` is the single owner of the flag-fragment
//! vocabulary (`STD_CPP`/`WINDOWS_VERSION_DEFINES`); `pch` reads them and
//! `wh_macro_defines` from here (the only `flags`->`pch` edges).

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

/// The contiguous five-line `WH_*` macro-define tail shared VERBATIM by
/// `build_compile_args` (here) and `build_pch_args` (in `pch`) - the one DRY
/// point the split would otherwise entrench across a module boundary. Scoped to
/// EXACTLY this run: the MinGW STDIO define, the `WH_MOD` marker, and the
/// quote-escaped `WH_MOD_ID`/`WH_MOD_VERSION`/`WH_WINDHAWK_VERSION`. The
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
        format!("-DWH_MOD_ID=L\"{}\"", mod_id.as_str().replace('"', "\\\"")),
        format!(
            "-DWH_MOD_VERSION=L\"{}\"",
            version.as_str().replace('"', "\\\"")
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

/// The clang++ argument vector for one target (the TS `compileModInternal`):
/// the optimized shared-library build, the Unicode and Windows-version
/// defines (the latter skipped for the one historical incompatible mod), the
/// `WH_*` macros, the engine import library, the source-on-stdin marker, the
/// per-target `-o`, then the mod's own compiler options and the per-mod
/// backward-compatibility includes. Pure, so the special cases are unit-tested
/// without spawning clang.
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
        "-shared".to_owned(),
        "-DUNICODE".to_owned(),
        "-D_UNICODE".to_owned(),
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
    args.extend(spec.compiler_options.iter().cloned());
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
}
