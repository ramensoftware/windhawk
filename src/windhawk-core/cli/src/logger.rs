//! The stderr logger, wired to the core's log callback and used directly by
//! command handlers for status messages.
//!
//! Routes (matching the TS `createStderrLogger`):
//!   error -> always printed, prefixed `error: `
//!   warn  -> always printed, prefixed `warning: `
//!   info  -> printed unless `--quiet`
//!
//! All output goes to stderr so stdout stays clean for the command result.
//! `Copy` so it can be cloned into the core log callback (a `Send` closure on a
//! core-owned thread) as well as held by the environment.

#[derive(Clone, Copy)]
pub struct Logger {
    quiet: bool,
}

impl Logger {
    pub fn new(quiet: bool) -> Logger {
        Logger { quiet }
    }

    pub fn error(&self, message: &str) {
        eprintln!("error: {message}");
    }

    pub fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }

    pub fn info(&self, message: &str) {
        if !self.quiet {
            eprintln!("{message}");
        }
    }

    /// Route a core log callback `(level, message)` (0 = error, 1 = warn, else
    /// info), prefixing `windhawk-core: ` exactly like the TS `dllBackend` log
    /// shim.
    pub fn core_log(&self, level: i32, message: &str) {
        let line = format!("windhawk-core: {message}");
        match level {
            0 => self.error(&line),
            1 => self.warn(&line),
            _ => self.info(&line),
        }
    }
}
