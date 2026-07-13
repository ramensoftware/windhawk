//! Thin entry point: collect argv, run the library, exit with the mapped code.
//! All front-end logic lives in the library crate; this returns `ExitCode` so
//! the runtime flushes stdout/stderr on exit.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let code = windhawk_cli::run(std::env::args().collect());
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
