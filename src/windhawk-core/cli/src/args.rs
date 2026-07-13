//! The clap argv tree and the shared global options, modeling the full command
//! surface: the sync reads/writes plus the async path (`mod
//! install`/`update`/`compile`, the `repo` group, `update run`).
//!
//! `--yes` (destructive-op confirmation) is a global option: it gates `mod
//! remove` and `update run`.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "windhawk-cli",
    about = "Command-line interface for Windhawk",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: TopCommand,
}

/// Global options threaded into every subcommand (clap `global = true`).
#[derive(Args, Debug)]
pub struct GlobalArgs {
    /// Override Windhawk app root (the directory containing windhawk.ini).
    #[arg(long, global = true, value_name = "path")]
    pub app_root: Option<String>,

    /// Emit JSON output on stdout instead of human-readable text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Skip confirmation for destructive operations (mod remove).
    #[arg(long, global = true)]
    pub yes: bool,

    /// Suppress non-essential stderr output (errors and warnings still print).
    #[arg(long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand, Debug)]
pub enum TopCommand {
    /// Install, list, compile, and configure mods.
    Mod {
        #[command(subcommand)]
        command: ModCommand,
    },
    /// Read and modify Windhawk application-level settings.
    App {
        #[command(subcommand)]
        command: AppCommand,
    },
    /// Operate on .wh.cpp mod source files directly.
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },
    /// Query the Windhawk mod repository.
    Repo {
        #[command(subcommand)]
        command: RepoCommand,
    },
    /// Query and install Windhawk updates.
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModCommand {
    /// List installed mods.
    List(ModListArgs),
    /// Show metadata, README, and initial settings for an installed mod.
    Show {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
    /// Enable an installed mod.
    Enable {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
    /// Disable an installed mod.
    Disable {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
    /// Uninstall a mod: removes config, source, DLLs, and profile entry.
    Remove {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
    /// Read and modify a mod's configuration.
    Config {
        #[command(subcommand)]
        command: ModConfigCommand,
    },
    /// Read and modify a mod's runtime settings.
    Settings {
        #[command(subcommand)]
        command: ModSettingsCommand,
    },
    /// Install or reinstall a mod from the repository or a local source file.
    Install(ModInstallArgs),
    /// Update an installed mod to its latest repository version.
    Update(ModUpdateArgs),
    /// Recompile an already-installed mod from its stored source.
    Compile {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
}

#[derive(Args, Debug)]
pub struct ModInstallArgs {
    /// Mod ID. Required unless --file is used; with --file, an optional
    /// sanity-check against the source's @id.
    #[arg(value_name = "id")]
    pub id: Option<String>,
    /// Mod version. Default is latest. Not valid with --file.
    #[arg(value_name = "version")]
    pub version: Option<String>,
    /// Read mod source from a local file. Use '-' for stdin.
    #[arg(long, value_name = "path")]
    pub file: Option<String>,
    /// Install in disabled state. Default is enabled.
    #[arg(long)]
    pub disabled: bool,
    /// Force local compilation even if alwaysCompileModsLocally is false.
    /// Repository installs only; --file always compiles locally.
    #[arg(long = "no-precompiled")]
    pub no_precompiled: bool,
}

#[derive(Args, Debug)]
pub struct ModUpdateArgs {
    /// Mod ID.
    #[arg(value_name = "id")]
    pub id: String,
    /// Install in disabled state. Without this flag, the current state is
    /// preserved.
    #[arg(long)]
    pub disabled: bool,
    /// Force local compilation even if alwaysCompileModsLocally is false.
    #[arg(long = "no-precompiled")]
    pub no_precompiled: bool,
}

#[derive(Args, Debug)]
pub struct ModListArgs {
    /// Show only enabled mods.
    #[arg(long)]
    pub enabled: bool,
    /// Show only disabled mods.
    #[arg(long)]
    pub disabled: bool,
    /// Show only mods with an update available.
    #[arg(long = "update-available")]
    pub update_available: bool,
}

#[derive(Subcommand, Debug)]
pub enum ModConfigCommand {
    /// Print a mod's config.
    Get {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// Single config field to print; omit to print all.
        #[arg(value_name = "field")]
        field: Option<String>,
    },
    /// Set a config field. Variadic: one value for scalars, zero-or-more for
    /// arrays.
    Set {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// Config field to modify.
        #[arg(value_name = "field")]
        field: String,
        /// Value(s). One value for scalars; zero-or-more for arrays.
        #[arg(value_name = "values")]
        values: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModSettingsCommand {
    /// Print a mod's runtime settings.
    Get {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// Single setting key to print; omit to print all.
        #[arg(value_name = "key")]
        key: Option<String>,
    },
    /// Set a runtime setting. Validates key and value type against the mod's
    /// declared initial settings.
    Set {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// Setting key (flat-storage form, e.g. myMod.options[0].name).
        #[arg(value_name = "key")]
        key: String,
        /// New value.
        #[arg(value_name = "value")]
        value: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AppCommand {
    /// Application settings.
    Settings {
        #[command(subcommand)]
        command: AppSettingsCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum AppSettingsCommand {
    /// Print Windhawk application settings.
    Get {
        /// Single setting key (dotted for nested, e.g. engine.injectIntoGames);
        /// omit to print all.
        #[arg(value_name = "key")]
        key: Option<String>,
    },
    /// Set a Windhawk application setting.
    Set {
        /// Setting key (dotted for nested).
        #[arg(value_name = "key")]
        key: String,
        /// New value. List settings take a comma-separated value; pass an empty
        /// string ("") to clear the list.
        #[arg(value_name = "value")]
        value: String,
        /// Confirm that the CLI may ask Windhawk to restart if the setting
        /// demands it.
        #[arg(long = "confirm-app-restart")]
        confirm_app_restart: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SourceCommand {
    /// Extract metadata from a .wh.cpp file.
    Meta {
        /// Path to a .wh.cpp mod source file.
        #[arg(value_name = "file")]
        file: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum RepoCommand {
    /// List all mods in the repository.
    List(RepoListArgs),
    /// List all published versions of a mod.
    Versions {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
    },
    /// Show repository metadata, README, and initial settings for a mod.
    Show {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// Specific version to fetch. Default is latest.
        #[arg(value_name = "version")]
        version: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct RepoListArgs {
    /// Also include installed-state data per mod.
    #[arg(long = "with-installed")]
    pub with_installed: bool,
}

#[derive(Subcommand, Debug)]
pub enum UpdateCommand {
    /// Show the cached latest Windhawk version and compare with the installed
    /// version.
    Status,
    /// Download and launch the Windhawk installer. Requires --yes.
    Run,
}
