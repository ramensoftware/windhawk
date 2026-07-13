//! A stub clang++ for the CLI's compile-bearing integration tests. It stands in
//! for the real compiler at `<CompilerPath>/bin/clang++.exe` so `mod install` /
//! `mod compile` / `mod update` can be driven without a toolchain, and doubles
//! as the `update run` installer payload.
//!
//! Compiler mode (a `-o <path>` argument is present): drain the mod source the
//! core pipes to stdin (so the core's write never blocks), then either write a
//! placeholder DLL to `<path>` and exit 0, or - when
//! `WINDHAWK_TEST_STUB_COMPILER_FAIL` is set - print a diagnostic to stderr and
//! exit with `WINDHAWK_TEST_STUB_COMPILER_EXIT` (default 1), exercising the
//! `COMPILER_FAILED` -> exit 7 path. Installer mode (no `-o`): exit 0 without
//! touching stdin, so a detached `update run` launch returns cleanly.

use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .iter()
        .position(|a| a == "-o")
        .and_then(|i| args.get(i + 1))
        .cloned();

    // Compiler mode only: drain the piped source. Installer mode has no stdin
    // pipe, so reading would risk blocking on an inherited handle.
    if out.is_some() {
        let mut source = String::new();
        let _ = std::io::stdin().read_to_string(&mut source);
    }

    if std::env::var_os("WINDHAWK_TEST_STUB_COMPILER_FAIL").is_some() {
        eprintln!("stub clang++: simulated compile failure");
        let code = std::env::var("WINDHAWK_TEST_STUB_COMPILER_EXIT")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(1);
        std::process::exit(code);
    }

    if let Some(out) = out {
        // The core does not validate the PE; it only needs the file to exist.
        let _ = std::fs::write(&out, b"stub-dll");
    }
}
