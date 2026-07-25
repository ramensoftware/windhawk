//! The architecture -> compilation-target taxonomy: the one table that
//! single-sources the arch -> target / subfolder mapping and the
//! empty-architectures default for the compile and cleanup services. It returns
//! DATA - the per-arch target + subfolder + skip-eligibility - in REQUEST
//! order, and applies no compile-only policy (no `mod_targets` skip, no dedup);
//! those stay caller-side.
//!
//! Unknown-arch handling is one `Result`: `targets_for_arch` errors on the first
//! unknown architecture, and the best-effort callers (cleanup, download) go
//! through `subfolders_for_arch`, which discards an unknown (`unwrap_or_default`).
//! Metadata validation rejects any architecture outside the supported set BEFORE
//! install/compile/cleanup, so the unknown path is unreachable for
//! store-installed mods; the compile-side reject is a fail-fast guard, the
//! best-effort callers default to "nothing to do".
//!
//! Output is REQUEST order (the order the architectures were declared, with the
//! empty default expanding to `[x86, x86-64]`): the x86-64 arch under
//! arm64-enabled emits X86_64 THEN Aarch64. All three callers share this one
//! order. Compile-target ORDER is not parity-pinned (the compile parity check
//! sorts), only the per-(mod, version, target) argv is; the download fetch order
//! (observable via its first-failure error) stays request order because that is
//! what this leaf emits.

use serde::Deserialize;

/// The machine-architecture scenario a session compiles for: the CLI `--arch`
/// selector's resolved value and the session-wide arch scope. It fixes which
/// target worlds a mod's declared architectures expand into. `auto` is not a
/// variant - it is resolved to `X64`/`Arm64` from the detected OS native machine
/// before this scope is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompileArch {
    /// An x64 machine: the x86/x64 target worlds, no aarch64.
    X64,
    /// An arm64 machine: x86/x64 plus aarch64, dropping the extra x64 build for a
    /// mod that only injects into common system processes (the all-common skip).
    Arm64,
    /// Every machine scenario's union: x86/x64 plus aarch64, with NO common-process
    /// x64 skip, so the widest set the mod metadata allows is built.
    All,
}

impl CompileArch {
    /// Whether aarch64 is an eligible target world - true for `Arm64` and `All`,
    /// the flag the `targets_for_arch` taxonomy and the cleanup/download subfolder
    /// set are gated on.
    pub fn arm64_enabled(self) -> bool {
        matches!(self, Self::Arm64 | Self::All)
    }

    /// Whether the arm64-machine optimization applies: dropping the skip-eligible
    /// extra x64 build when every mod target is a common system process. Only the
    /// single `Arm64` machine scenario drops it; `All` keeps every scenario's
    /// targets (the union), and `X64` has no skip-eligible target to drop.
    pub fn skips_common_x64(self) -> bool {
        matches!(self, Self::Arm64)
    }
}

/// A clang compilation target (the TS `CompilationTarget`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompilationTarget {
    I686,
    X86_64,
    Aarch64,
}

impl CompilationTarget {
    /// The clang `-target` triple.
    pub fn triple(self) -> &'static str {
        match self {
            Self::I686 => "i686-w64-mingw32",
            Self::X86_64 => "x86_64-w64-mingw32",
            Self::Aarch64 => "aarch64-w64-mingw32",
        }
    }

    /// The per-architecture compiled-DLL subfolder under `Engine\Mods`.
    pub fn subfolder(self) -> &'static str {
        match self {
            Self::I686 => "32",
            Self::X86_64 => "64",
            Self::Aarch64 => "arm64",
        }
    }

    /// The compilation targets always supported, plus aarch64 when arm64 is
    /// enabled (the TS `supportedCompilationTargets`). The ONE home for the
    /// `[I686, X86_64](+Aarch64)` enumeration, used by the DLL-name collision
    /// check, the runtime-library refresh, and the full-uninstall subfolder
    /// sweep.
    pub fn all(arm64_enabled: bool) -> Vec<CompilationTarget> {
        let mut targets = vec![CompilationTarget::I686, CompilationTarget::X86_64];
        if arm64_enabled {
            targets.push(CompilationTarget::Aarch64);
        }
        targets
    }
}

/// One requested architecture's resolved compile target, plus whether it is the
/// SKIP-ELIGIBLE extra x64 build - the x86-64 arch's x64 under arm64-enabled,
/// which the compile caller drops when every mod target is a common system
/// process. The `amd64` arch's x64 is NOT skip-eligible (it is an unconditional
/// request), so the skip filters on this flag, never the `X86_64` class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArchTarget {
    target: CompilationTarget,
    skip_eligible: bool,
}

impl ArchTarget {
    pub fn target(self) -> CompilationTarget {
        self.target
    }

    pub fn subfolder(self) -> &'static str {
        self.target.subfolder()
    }

    pub fn skip_eligible(self) -> bool {
        self.skip_eligible
    }
}

/// The empty-architectures default ([x86, x86-64]); one home for the literal.
const DEFAULT_ARCHITECTURES: [&str; 2] = ["x86", "x86-64"];

/// Map a mod's declared architectures to compile targets (the shared half of
/// the TS `compilationTargetsFromArchitecture` / `subfoldersFromArchitectures`).
/// The empty-architectures default ([x86, x86-64]) is applied here, ONCE, before
/// the per-arch mapping, so every caller gets it without re-implementing it.
/// Returns the resolved targets in REQUEST order (NOT deduped - the compile
/// caller dedups to distinct targets, the cleanup/download callers dedup
/// subfolders via `subfolders_for_arch`). Applies no compile-only policy (no
/// `mod_targets` skip); that stays caller-side.
///
/// `Err(arch)` names the first UNSUPPORTED architecture: the compile caller maps
/// it to a reject error, while the best-effort `subfolders_for_arch` discards
/// it. Metadata validation rejects unsupported architectures before any install,
/// so this is the unreachable fail-fast path.
pub fn targets_for_arch(
    architectures: &[String],
    arm64_enabled: bool,
) -> Result<Vec<ArchTarget>, String> {
    let defaults;
    let architectures = if architectures.is_empty() {
        defaults = DEFAULT_ARCHITECTURES.map(String::from);
        &defaults[..]
    } else {
        architectures
    };

    let mut targets = Vec::new();
    let push = |target, skip_eligible, targets: &mut Vec<ArchTarget>| {
        targets.push(ArchTarget {
            target,
            skip_eligible,
        });
    };
    for architecture in architectures {
        match architecture.as_str() {
            "x86" => push(CompilationTarget::I686, false, &mut targets),
            "x86-64" => {
                if arm64_enabled {
                    // The extra x64 build is skip-eligible: the compile caller
                    // drops it when every mod target is a common system process.
                    push(CompilationTarget::X86_64, true, &mut targets);
                    push(CompilationTarget::Aarch64, false, &mut targets);
                } else {
                    push(CompilationTarget::X86_64, false, &mut targets);
                }
            }
            "amd64" => push(CompilationTarget::X86_64, false, &mut targets),
            "arm64" => {
                if arm64_enabled {
                    push(CompilationTarget::Aarch64, false, &mut targets);
                }
            }
            other => return Err(other.to_owned()),
        }
    }
    Ok(targets)
}

/// The per-architecture DLL subfolders in REQUEST order, deduped (preserving
/// first-seen order), with an unknown architecture skipped (best effort). The
/// one home for the cleanup and download subfolder set: cleanup deletes each
/// subfolder independently (order invisible) and download fetches them
/// sequentially (order observable via its first-failure error), but both want
/// the same request-order deduped set, so they share this helper rather than
/// re-deriving the arch mapping. An unknown architecture is discarded (metadata
/// validation gates it upstream).
pub fn subfolders_for_arch(architectures: &[String], arm64_enabled: bool) -> Vec<&'static str> {
    let targets = targets_for_arch(architectures, arm64_enabled).unwrap_or_default();
    let mut out: Vec<&'static str> = Vec::new();
    for at in targets {
        let subfolder = at.subfolder();
        if !out.contains(&subfolder) {
            out.push(subfolder);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_arch_derives_the_eligibility_and_skip_flags() {
        // aarch64 is a target world for arm64 and all, never for x64.
        assert!(!CompileArch::X64.arm64_enabled());
        assert!(CompileArch::Arm64.arm64_enabled());
        assert!(CompileArch::All.arm64_enabled());

        // Only the single arm64-machine scenario drops the skip-eligible x64; the
        // union (`all`) keeps it, and x64 has none to drop.
        assert!(!CompileArch::X64.skips_common_x64());
        assert!(CompileArch::Arm64.skips_common_x64());
        assert!(!CompileArch::All.skips_common_x64());
    }

    #[test]
    fn compile_arch_deserializes_from_the_lowercase_selector() {
        assert_eq!(
            serde_json::from_str::<CompileArch>("\"x64\"").unwrap(),
            CompileArch::X64
        );
        assert_eq!(
            serde_json::from_str::<CompileArch>("\"arm64\"").unwrap(),
            CompileArch::Arm64
        );
        assert_eq!(
            serde_json::from_str::<CompileArch>("\"all\"").unwrap(),
            CompileArch::All
        );
        // `auto` is resolved before this scope is set, so it is not a variant.
        assert!(serde_json::from_str::<CompileArch>("\"auto\"").is_err());
    }

    fn targets(architectures: &[&str], arm64_enabled: bool) -> Vec<CompilationTarget> {
        targets_for_arch(
            &architectures
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            arm64_enabled,
        )
        .unwrap()
        .into_iter()
        .map(ArchTarget::target)
        .collect()
    }

    #[test]
    fn targets_are_request_order_and_mark_the_skip_eligible_x64() {
        // No arm64: x86-64 -> x64 only.
        assert_eq!(targets(&["x86-64"], false), vec![CompilationTarget::X86_64]);

        // arm64 enabled: x86-64 -> x64 (skip-eligible) THEN aarch64, in REQUEST
        // order.
        let resolved = targets_for_arch(&["x86-64".into()], true).unwrap();
        assert_eq!(
            resolved.iter().map(|t| t.target()).collect::<Vec<_>>(),
            vec![CompilationTarget::X86_64, CompilationTarget::Aarch64]
        );
        // Only the x86-64 arm x64 is skip-eligible.
        assert_eq!(
            resolved
                .iter()
                .map(|t| t.skip_eligible())
                .collect::<Vec<_>>(),
            vec![true, false]
        );

        // Empty architectures default to x86 + x86-64.
        assert_eq!(
            targets(&[], false),
            vec![CompilationTarget::I686, CompilationTarget::X86_64]
        );

        // amd64's x64 is NOT skip-eligible (an unconditional request).
        let resolved = targets_for_arch(&["amd64".into()], true).unwrap();
        assert_eq!(resolved[0].target(), CompilationTarget::X86_64);
        assert!(!resolved[0].skip_eligible());
    }

    #[test]
    fn unknown_architecture_is_an_error() {
        // The shared taxonomy errors on the first unknown arch; compile maps it
        // to a reject, while subfolders_for_arch discards it.
        assert_eq!(
            targets_for_arch(&["sparc".into()], false),
            Err("sparc".to_owned())
        );
    }

    #[test]
    fn subfolders_are_request_order_deduped_and_skip_unknown() {
        assert_eq!(subfolders_for_arch(&[], false), vec!["32", "64"]);
        // arm64-on x86-64: request order ["64", "arm64"] (shared by cleanup and
        // download; cleanup's per-folder deletion order is invisible, download's
        // sequential fetch order is observable and stays request order).
        assert_eq!(
            subfolders_for_arch(&["x86-64".into()], true),
            vec!["64", "arm64"]
        );
        // amd64 + x86-64 both map to "64", deduped to one.
        assert_eq!(
            subfolders_for_arch(&["x86-64".into(), "amd64".into()], false),
            vec!["64"]
        );
        // arm64 arch with arm64 disabled contributes nothing.
        assert_eq!(
            subfolders_for_arch(&["arm64".into()], false),
            Vec::<&str>::new()
        );
        // An unknown architecture is skipped (best effort), leaving an empty set.
        assert_eq!(
            subfolders_for_arch(&["sparc".into()], false),
            Vec::<&str>::new()
        );
    }
}
