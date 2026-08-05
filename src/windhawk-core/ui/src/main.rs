//! Thin entry point: build and run the Tauri app, or serve one runtime-broker
//! channel. All protocol-adapter policy lives in the library crate. The `windows`
//! subsystem suppresses the console window for the GUI in every build.
//!
//! The mode is chosen here, before anything else runs, so the broker never
//! touches the single-instance plugin, the detect mutex, the startup watchdog, or
//! Tauri: it has no window and no webview, which is the whole point of splitting
//! it out of the process that does.

#![forbid(unsafe_code)]
#![windows_subsystem = "windows"]

use std::process::ExitCode;

/// The mode switch and its one argument. The channel is a name the UI issued and
/// is already listening on; the broker connects out to it and to nothing else.
const BROKER_FLAG: &str = "--runtime-broker";
const CHANNEL_FLAG: &str = "--channel";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(BROKER_FLAG) {
        windhawk_ui::run();
        return ExitCode::SUCCESS;
    }

    let channel = match (args.get(1).map(String::as_str), args.get(2)) {
        (Some(CHANNEL_FLAG), Some(channel)) => channel.clone(),
        _ => String::new(),
    };
    // The exit code is how the rung that holds a process handle learns WHICH
    // pre-handshake failure happened; the broker has no console to say it in.
    ExitCode::from(windhawk_ui::broker::run_broker(&channel))
}
