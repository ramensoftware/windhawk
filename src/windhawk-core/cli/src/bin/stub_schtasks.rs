//! A stub `schtasks.exe` for the CLI's registry-mode integration tests,
//! the analogue of `stub_compiler`. The core toggles a Windows scheduled task
//! via `schtasks.exe /change /tn <task> /enable|/disable` when `app settings
//! set disableRunUIScheduledTask` is applied in registry mode; the
//! `WINDHAWK_DEBUG_SCHTASKS_PATH` override points the core at this stub so the
//! side effect is observable without touching a real scheduled task.
//!
//! It records its argv (one per line) to the file named by
//! `WINDHAWK_TEST_SCHTASKS_LOG`, so the test can assert the exact `/change /tn ...`
//! invocation, then exits 0 (a nonzero exit is only a warning in the core, so the
//! settings write would still succeed - exit 0 keeps the test's intent clear).

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(log) = std::env::var_os("WINDHAWK_TEST_SCHTASKS_LOG") {
        // Append so a test that toggles more than one task records every call.
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            let _ = writeln!(file, "{}", args.join(" "));
        }
    }
}
