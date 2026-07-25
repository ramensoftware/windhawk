//! The clap argv tree and the shared global options, modeling the full command
//! surface: the sync reads/writes plus the async path (`mod
//! install`/`update`/`compile`, the `repo` group, `update run`).
//!
//! `--yes` (destructive-op confirmation) is a global option: it gates `mod
//! remove` and `update run`.

use clap::{Args, Parser, Subcommand, ValueEnum};

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

    /// Which architectures a compile targets, overriding the core's OS
    /// detection: `auto` (detect the native machine), `x64`, `arm64`, or `all`.
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "arch",
        default_value = "auto"
    )]
    pub arch: ArchArg,
}

/// The `--arch` selector: which machine the core compiles as. Maps to the
/// session config's `compileArch`; `auto` forwards nothing and lets the core
/// detect the OS native machine.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ArchArg {
    /// Detect the OS native machine (the default).
    Auto,
    /// Compile as an x64 machine: x86/x64 targets, no arm64.
    X64,
    /// Compile as an arm64 machine: x86/x64 plus arm64, dropping the extra x64
    /// build for a mod that only injects into common system processes.
    Arm64,
    /// Compile every machine scenario's union: x86/x64 plus arm64, with no
    /// common-process x64 skip.
    All,
}

impl ArchArg {
    /// The `compileArch` config value the core reads, or `None` for `auto` (the
    /// core detects the machine).
    pub fn as_config(self) -> Option<&'static str> {
        match self {
            ArchArg::Auto => None,
            ArchArg::X64 => Some("x64"),
            ArchArg::Arm64 => Some("arm64"),
            ArchArg::All => Some("all"),
        }
    }
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
    /// Export and inspect Windhawk user data (app settings and mods).
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum DataCommand {
    /// Export app settings and mods to a user-data archive.
    Export(DataExportArgs),
    /// Print the manifest of a user-data archive.
    Inspect(DataInspectArgs),
    /// Import app settings and mods from a user-data archive.
    Import(DataImportArgs),
}

#[derive(Args, Debug)]
pub struct DataExportArgs {
    /// Write the archive to a file, or '-' for stdout (the default).
    #[arg(long, value_name = "path|-", default_value = "-")]
    pub out: String,
    /// Overwrite the --out file if it already exists.
    #[arg(long)]
    pub force: bool,
    /// Include Windhawk application settings (the default).
    #[arg(long = "app-settings")]
    pub app_settings: bool,
    /// Exclude Windhawk application settings.
    #[arg(long = "no-app-settings")]
    pub no_app_settings: bool,
    /// Mod scope: all | all-except-local | none | <id,id,...>. Default: all.
    #[arg(long, value_name = "scope", default_value = "all")]
    pub mods: String,
    /// Include each mod's runtime settings (the default).
    #[arg(long)]
    pub settings: bool,
    /// Exclude each mod's runtime settings.
    #[arg(long = "no-settings")]
    pub no_settings: bool,
    /// Include each mod's configuration (the default).
    #[arg(long)]
    pub config: bool,
    /// Exclude each mod's configuration.
    #[arg(long = "no-config")]
    pub no_config: bool,
    /// Turn settings OFF for these in-scope mods (comma-separated ids).
    #[arg(long = "skip-settings", value_name = "id,...")]
    pub skip_settings: Option<String>,
    /// Turn config OFF for these in-scope mods (comma-separated ids).
    #[arg(long = "skip-config", value_name = "id,...")]
    pub skip_config: Option<String>,
    /// Turn settings ON for these in-scope mods (comma-separated ids).
    #[arg(long = "with-settings", value_name = "id,...")]
    pub with_settings: Option<String>,
    /// Turn config ON for these in-scope mods (comma-separated ids).
    #[arg(long = "with-config", value_name = "id,...")]
    pub with_config: Option<String>,
    /// Embed repository mod source too, so the archive restores offline.
    #[arg(long)]
    pub offline: bool,
}

#[derive(Args, Debug)]
pub struct DataInspectArgs {
    /// Path to a user-data archive, or '-' for stdin.
    #[arg(value_name = "path|-")]
    pub path: String,
}

#[derive(Args, Debug)]
pub struct DataImportArgs {
    /// Path to a user-data archive, or '-' for stdin.
    #[arg(value_name = "path|-")]
    pub path: String,
    /// Import Windhawk application settings (the default).
    #[arg(long = "app-settings")]
    pub app_settings: bool,
    /// Do not import Windhawk application settings.
    #[arg(long = "no-app-settings")]
    pub no_app_settings: bool,
    /// Mod scope: all | all-except-local | none | <id,id,...>. Default: all.
    #[arg(long, value_name = "scope", default_value = "all")]
    pub mods: String,
    /// Import each mod's runtime settings (the default).
    #[arg(long)]
    pub settings: bool,
    /// Do not import each mod's runtime settings.
    #[arg(long = "no-settings")]
    pub no_settings: bool,
    /// Import each mod's configuration (the default).
    #[arg(long)]
    pub config: bool,
    /// Do not import each mod's configuration.
    #[arg(long = "no-config")]
    pub no_config: bool,
    /// Turn settings OFF for these in-scope mods (comma-separated ids).
    #[arg(long = "skip-settings", value_name = "id,...")]
    pub skip_settings: Option<String>,
    /// Turn config OFF for these in-scope mods (comma-separated ids).
    #[arg(long = "skip-config", value_name = "id,...")]
    pub skip_config: Option<String>,
    /// Turn settings ON for these in-scope mods (comma-separated ids).
    #[arg(long = "with-settings", value_name = "id,...")]
    pub with_settings: Option<String>,
    /// Turn config ON for these in-scope mods (comma-separated ids).
    #[arg(long = "with-config", value_name = "id,...")]
    pub with_config: Option<String>,
    /// How to treat an already-installed mod: overwrite (default) or skip.
    #[arg(long = "on-conflict", value_enum, default_value = "overwrite")]
    pub on_conflict: ConflictArg,
    /// Force a local compile (may still fetch a reference-only mod's source).
    #[arg(long = "no-precompiled")]
    pub no_precompiled: bool,
    /// Network-free restore: force local compile AND require embedded source.
    #[arg(long)]
    pub offline: bool,
    /// Proceed even when the imported app settings require a Windhawk restart.
    #[arg(long = "confirm-app-restart")]
    pub confirm_app_restart: bool,
}

/// The `--on-conflict` selector, mapping onto the core `ConflictPolicy`.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ConflictArg {
    /// Reinstall an already-installed mod, applying the archive over a clean
    /// baseline.
    Overwrite,
    /// Leave an already-installed mod untouched.
    Skip,
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
    /// Set one or more runtime settings. Each `key=value` pair's key and value
    /// type are validated against the mod's declared initial settings; every
    /// pair is checked before any write, so a batch applies atomically or not
    /// at all.
    Set {
        /// Mod ID.
        #[arg(value_name = "id")]
        id: String,
        /// One or more `key=value` pairs. The key is the flat-storage form
        /// (e.g. myMod.options[0].name); the value is split on the first `=`.
        #[arg(value_name = "key=value", required = true)]
        pairs: Vec<String>,
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
