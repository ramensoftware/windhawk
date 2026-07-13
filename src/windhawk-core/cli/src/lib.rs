//! The native Windhawk CLI library: the testable front-end over the C ABI,
//! driven through the shared `core-client` transport, with `main.rs` a thin
//! shell over it. `run` parses argv, resolves the environment, dispatches to a
//! command handler, and renders the result; the pure logic (app-root discovery,
//! error/exit mapping, output rendering) lives in unit-tested modules.

#![forbid(unsafe_code)]

mod app_root;
mod args;
mod cancel;
mod client;
mod commands;
mod environment;
mod error;
mod logger;
mod output;
mod validate;

use std::path::PathBuf;

use args::{Cli, GlobalArgs, ModCommand, SourceCommand, TopCommand};
use client::Core;
use error::CliError;
use logger::Logger;
use output::CommandResult;

use clap::Parser;
use windhawk_core_host::{GatedCore, SessionConfig};

/// The Windhawk environment a session-bearing command runs against: the live
/// core session, the stderr logger, and the destructive-op confirmation flag
/// (`--yes`) the gated commands (`mod remove`) consult.
pub(crate) struct Environment {
    pub(crate) core: Core,
    pub(crate) logger: Logger,
    pub(crate) yes: bool,
}

/// Parse argv, run the selected command, and return the process exit code.
/// `main` is a thin shell that forwards this code.
pub fn run(args: Vec<String>) -> i32 {
    // Install the Ctrl+C handler once, before any command runs: it cancels an
    // in-flight async operation (compile/update) or, with none tracked, exits 9.
    cancel::install_handler();
    match Cli::try_parse_from(args.iter()) {
        Ok(cli) => execute(cli),
        Err(err) => handle_clap_error(&err, &args),
    }
}

fn execute(cli: Cli) -> i32 {
    let Cli { global, command } = cli;
    match run_command(&global, command) {
        Ok(result) => {
            // A broken stdout (e.g. a closed pipe) does not change the success
            // exit code; the work completed.
            let _ = output::emit_result(global.json, result.as_ref());
            0
        }
        Err(err) => output::emit_error(global.json, &err),
    }
}

fn run_command(
    global: &GlobalArgs,
    command: TopCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    match command {
        TopCommand::Source { command } => match command {
            // `source meta` is session-free: it loads the DLL only for the
            // stateless transport, with no app-root resolution.
            SourceCommand::Meta { file } => {
                let gated_core = GatedCore::load(&windhawk_core_host::resolve_dll_path())?;
                commands::source::meta(&gated_core, &file)
            }
        },
        TopCommand::Mod { command } => dispatch_mod(global, command),
        TopCommand::App { command } => commands::app::dispatch(&load_env(global)?, command),
        TopCommand::Repo { command } => commands::repo::dispatch(&load_env(global)?, command),
        TopCommand::Update { command } => commands::update::dispatch(&load_env(global)?, command),
    }
}

fn dispatch_mod(
    global: &GlobalArgs,
    command: ModCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    commands::mods::dispatch(&load_env(global)?, command)
}

/// Resolve the app root, load the DLL, and create the core session. Used by
/// every command except the session-free `source meta`.
fn load_env(global: &GlobalArgs) -> Result<Environment, CliError> {
    let ui_path = std::env::var("WINDHAWK_UI_PATH").ok();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let app_root =
        app_root::resolve_app_root(global.app_root.as_deref(), ui_path.as_deref(), &cwd)?;

    let logger = Logger::new(global.quiet);
    let gated_core = GatedCore::load(&windhawk_core_host::resolve_dll_path())?;
    let config = SessionConfig::resolve(app_root, "windhawk-cli", environment::product_version());
    let core = Core::create(&gated_core, &config, logger)?;
    Ok(Environment {
        core,
        logger,
        yes: global.yes,
    })
}

/// Map a clap parse outcome to an exit code. `--help` / `--version` print to
/// stdout and succeed (exit 0); a usage error prints its human message to
/// stderr and, in `--json` mode, also emits the structured envelope on stdout
/// (exit 2) - the commander-parity behavior.
fn handle_clap_error(err: &clap::Error, args: &[String]) -> i32 {
    use clap::error::ErrorKind;
    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = err.print();
            0
        }
        _ => {
            let _ = err.print();
            if args.iter().any(|a| a == "--json") {
                output::emit_error(true, &CliError::usage(clap_message(err)))
            } else {
                2
            }
        }
    }
}

/// Extract a concise single-line message from a clap error for the `--json`
/// usage envelope (clap renders without color here, so the text is plain). Real
/// parse errors carry an `error:` line; an incomplete-command help dump (a
/// missing subcommand) has none, so it gets a generic message rather than the
/// command's about-line.
fn clap_message(err: &clap::Error) -> String {
    let rendered = err.render().to_string();
    for line in rendered.lines() {
        if let Some(rest) = line.trim().strip_prefix("error:") {
            return rest.trim().to_owned();
        }
    }
    "missing or incomplete command".to_owned()
}
