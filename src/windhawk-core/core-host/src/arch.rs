//! The user-facing architecture label for a clang compile-target triple, shared
//! by the CLI (its `Compiling ... for <arch>...` progress and `[compile:<arch>]`
//! failure diagnostics) and the UI host (its import-progress forwarding), so
//! both front-ends name a target the same way from one table.

/// Map a clang target triple to the short architecture label Windows users
/// recognize - the vocabulary of Microsoft's download pages, the Settings
/// "System type" ("x64-based processor"), and Task Manager - rather than the raw
/// triple. An unrecognized triple passes through verbatim.
pub fn arch_label(triple: &str) -> &str {
    match triple {
        "i686-w64-mingw32" => "x86",
        "x86_64-w64-mingw32" => "x64",
        "aarch64-w64-mingw32" => "ARM64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_known_triples_and_passes_unknown_through() {
        assert_eq!(arch_label("i686-w64-mingw32"), "x86");
        assert_eq!(arch_label("x86_64-w64-mingw32"), "x64");
        assert_eq!(arch_label("aarch64-w64-mingw32"), "ARM64");
        // An unrecognized triple is surfaced verbatim rather than hidden.
        assert_eq!(arch_label("sparc-unknown"), "sparc-unknown");
    }
}
