//! `mod install` / `mod update` / `mod compile`: the compile-bearing async
//! pipeline, plus the shared source-fetch, id-reconcile, and install helpers.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{
    AppSettings, CompileInstalledModParams, CompileInstalledModResult, FetchRepoModSourceParams,
    InstallModParams, InstallModResult, ListInstalledModsParams, ListInstalledModsResult,
    ModConfig, ModMetadata, OperationEvent, UpdateSuppression, parse_suppression,
};

use crate::Environment;
use crate::args::{ModInstallArgs, ModUpdateArgs};
use crate::commands::parse::{parse_mod_source, require_metadata};
use crate::commands::{app_settings, check_for_updates, language};
use crate::error::{CliError, arch_label};
use crate::logger::Logger;
use crate::output::CommandResult;

/// Normalize a mod source for storage: drop a leading UTF-8 BOM, then
/// normalize all line endings to CRLF (the TS `/\r\n|\r|\n/g -> '\r\n'`). Repo
/// fetches arrive normalized and BOM-free; `--file`/stdin sources do not, so
/// the source written to disk stays consistent with the GUI install. The
/// parser tolerates a BOM, but nothing downstream benefits from storing one.
/// Idempotent and pure.
fn normalize_source(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            other => out.push(other),
        }
    }
    out
}

/// The source's declared non-empty `@id`, or the shared usage error (exit 2) - a
/// mod with no id is a malformed install, a bad value rather than an internal
/// failure. `origin` names which source is missing it, as everywhere else on
/// this path. Borrows the metadata; the id-reconcile callers (`reconcile_id`,
/// `compile`) own the subsequent compare and their own mismatch message, so the
/// extraction lives here (mods-only callers) rather than in `commands/parse.rs`.
/// DEFENSIVE, not a live diagnostic: `extract_metadata` rejects an empty or
/// malformed `@id` before it will hand back any metadata, so a `ParsedModSource`
/// that carries metadata at all carries a valid id. The check stays because the
/// CLI reads a wire DTO whose `id` is an `Option` and drives a DLL it loads at
/// runtime; it does not get to assume the core validated. It must not become the
/// place a reader learns the rule, hence this note.
#[track_caller]
fn require_source_id<'a>(metadata: &'a ModMetadata, origin: &str) -> Result<&'a str, CliError> {
    match metadata.id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => Ok(id),
        None => Err(CliError::usage(format!(
            "Mod id must be specified in the source code of {origin} (no `// @id`)."
        ))),
    }
}

/// Reconcile the source's declared `@id` against an optional `id_arg` (pure).
/// A missing `@id`, or a mismatch with `id_arg`, is a usage error (exit 2): a
/// malformed/ambiguous install is a bad value, not an internal failure.
#[track_caller]
fn reconcile_id(
    metadata: &ModMetadata,
    origin: &str,
    id_arg: Option<&str>,
) -> Result<String, CliError> {
    let source_mod_id = require_source_id(metadata, origin)?;
    if let Some(id_arg) = id_arg
        && source_mod_id != id_arg
    {
        return Err(CliError::usage(format!(
            "Mod id mismatch: source declares '{source_mod_id}', argument was '{id_arg}'."
        )));
    }
    Ok(source_mod_id.to_owned())
}

/// How a `--file` argument is named in a diagnostic: the quoted path, or
/// `standard input` for the `-` convention (quoting `'-'` names nothing a
/// reader can act on).
fn file_origin(path: &str) -> String {
    if path == "-" {
        "standard input".to_owned()
    } else {
        format!("'{path}'")
    }
}

/// Read a source file from a path, or from stdin when the path is `-`. A missing
/// path is a bad flag value (exit 2), not an unhandled error.
#[track_caller]
fn read_file_or_stdin(path: &str) -> Result<String, CliError> {
    if path == "-" {
        return match std::io::read_to_string(std::io::stdin()) {
            Ok(source) => Ok(source),
            Err(e) => Err(CliError::generic(format!("Failed to read stdin: {e}"))),
        };
    }
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(CliError::usage(format!("--file: '{path}' does not exist")))
        }
        Err(e) => Err(CliError::generic(format!("Failed to read '{path}': {e}"))),
    }
}

/// The header the core looks for inside a precompiled-headers folder; the
/// per-target `windhawk_t_<triple>.pch` it compiles is written beside it.
const PCH_HEADER_NAME: &str = "windhawk_pch.h";

/// Validate a `--pch-folder` value and return it as an absolute path. The core
/// joins [`PCH_HEADER_NAME`] onto the value verbatim and silently skips the
/// whole precompiled-header stage when that header is absent, so a folder that
/// cannot produce one is rejected here (exit 2) rather than compiling without
/// the header the flag asked for - the same reason `--no-precompiled` is
/// rejected with `--file` instead of being ignored. Absolute rather than
/// canonical: `canonicalize` would hand the compiler a `\\?\` path.
#[track_caller]
fn resolve_pch_folder(folder: &str) -> Result<String, CliError> {
    let absolute = std::path::absolute(folder)
        .map_err(|e| CliError::usage(format!("--pch-folder: cannot resolve '{folder}': {e}")))?;
    if !absolute.is_dir() {
        return Err(CliError::usage(format!(
            "--pch-folder: '{folder}' is not an existing directory"
        )));
    }
    if !absolute.join(PCH_HEADER_NAME).is_file() {
        return Err(CliError::usage(format!(
            "--pch-folder: '{folder}' does not contain {PCH_HEADER_NAME}"
        )));
    }
    Ok(absolute.to_string_lossy().into_owned())
}

/// Fetch a mod's source from the repository (network, async; no progress
/// events). Emits a stderr status line unless `--quiet`. 404 -> exit 5; other
/// network failures -> exit 6.
fn fetch_repo_source(
    env: &Environment,
    mod_id: &str,
    version: Option<&str>,
) -> Result<String, CliError> {
    let suffix = version.map(|v| format!(" version {v}")).unwrap_or_default();
    env.logger
        .info(&format!("Fetching {mod_id}{suffix} from repository..."));
    let source: String = env.core.invoke_async_as(
        "fetchRepoModSource",
        &FetchRepoModSourceParams {
            mod_id: mod_id.to_owned(),
            version: version.map(str::to_owned),
        },
        |_| {},
    )?;
    Ok(source)
}

/// Normalize line endings, parse, and reconcile the source's id. Returns the
/// normalized source (writable to disk) plus the resolved id and parsed
/// metadata.
struct Reconciled {
    mod_id: String,
    normalized_source: String,
    metadata: ModMetadata,
}

/// `strict` holds the source to `parseModSource`'s strict rules and rejects a
/// settings block that fails them, where otherwise the settings are left to
/// the core's install.
fn extract_and_reconcile(
    env: &Environment,
    source: &str,
    origin: &str,
    id_arg: Option<&str>,
    language: &str,
    strict: bool,
) -> Result<Reconciled, CliError> {
    let normalized_source = normalize_source(source);
    let parsed = parse_mod_source(env, &normalized_source, language, strict)?;
    // Metadata parsing validates the source; a malformed source being installed
    // is a usage error (exit 2), not an internal one. The `origin` names which
    // source failed - the parse message alone says only that some source did.
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::usage(format!("Failed to parse metadata from {origin}: {message}"))
    })?;
    if strict && let Some(message) = parsed.errors.initial_settings {
        return Err(CliError::usage(format!("{message} (in {origin})")));
    }
    let mod_id = reconcile_id(&metadata, origin, id_arg)?;
    Ok(Reconciled {
        mod_id,
        normalized_source,
        metadata,
    })
}

struct InstallOpts {
    /// `true` sets disabled on install; `false` omits the field, preserving the
    /// existing state on a reinstall and defaulting a fresh install to enabled.
    disabled: bool,
    force_local_compile: bool,
    /// `alwaysCompileModsLocally`, threaded in from the command's single
    /// AppSettings fetch (the GUI uses its own cache).
    always_compile_locally: bool,
    /// The validated absolute `--pch-folder`, or `None` for a compile with no
    /// precompiled header.
    pch_folder: Option<String>,
}

struct PipelineResult {
    mod_version: String,
    architecture: Vec<String>,
    compiled_locally: bool,
    config: ModConfig,
    /// Clang warnings from a successful local compile (empty on a clean compile
    /// or a precompiled download).
    warnings: String,
}

/// Shared tail of `mod install` and `mod update`: decide compile-vs-download and
/// run the core `installMod` operation (settings migration, compile-or-download,
/// persist, cleanup), rendering its per-target `Compiling for <arch>...` progress
/// as it compiles. The operation is async and tracked, so Ctrl+C cancels an
/// in-flight compile; while compiling it emits one progress event per ACTUAL
/// target (`report_compile_progress`). No tray notification.
fn run_install_pipeline(
    env: &Environment,
    mod_id: &str,
    source: &str,
    metadata: &ModMetadata,
    opts: InstallOpts,
) -> Result<PipelineResult, CliError> {
    let mod_version = metadata.version.clone().unwrap_or_default();
    let architecture = metadata.architecture.clone().unwrap_or_default();

    let compile_locally = opts.always_compile_locally || opts.force_local_compile;

    let params = InstallModParams {
        storage_id: mod_id.to_owned(),
        source: source.to_owned(),
        metadata: metadata.clone(),
        // Absent `disabled` preserves the existing state (a fresh install then
        // defaults to enabled in the core); only an explicit --disabled sets it
        // (`skip_serializing_if` omits the `None`, byte-identical to the old Map).
        disabled: if opts.disabled { Some(true) } else { None },
        logging_enabled: None,
        compile_locally,
        // local@ mods (file installs) stay out of the user profile.
        track_in_profile: !mod_id.starts_with("local@"),
        pch_folder: opts.pch_folder,
        rename_from_storage_id: None,
    };

    let logger = env.logger;
    let result: InstallModResult = env.core.invoke_async_as("installMod", &params, |event| {
        report_compile_progress(logger, event)
    })?;
    report_compile_warnings(logger, &result.warnings);

    Ok(PipelineResult {
        mod_version,
        architecture,
        compiled_locally: compile_locally,
        config: result.config,
        warnings: result.warnings,
    })
}

/// Render a core compile-progress event as a `Compiling for <arch>...` line on
/// stderr (via `logger.info`, so `--quiet` suppresses it). The core emits one
/// `Progress { compileTarget: <triple> }` per ACTUAL compile target, right before
/// it compiles that target, so this reflects the real deduped/skip-filtered set
/// (e.g. an extra `arm64` on an ARM64 machine or under `--arch arm64`/`all`), not
/// the mod's declared architecture list. `arch_label` maps the clang triple to
/// the friendly label,
/// shared with the `[compile:<arch>]` failure diagnostics. Other event variants
/// (`Installing`, terminal) are not reachable here for a compile op.
fn report_compile_progress(logger: Logger, event: &OperationEvent) {
    if let OperationEvent::Progress { payload } = event
        && let Some(triple) = payload.get("compileTarget").and_then(Value::as_str)
    {
        logger.info(&format!("Compiling for {}...", arch_label(triple)));
    }
}

/// Surface a successful compile's clang warnings on stderr, each target's block
/// already tagged with its triple by the core. Emitted via `logger.warn`, so
/// `--quiet` keeps them (they ARE warnings). Empty - a clean compile or a
/// precompiled download - prints nothing. This is the text-mode channel; the
/// warnings also ride back on the result for `--json`.
fn report_compile_warnings(logger: Logger, warnings: &str) {
    if !warnings.is_empty() {
        logger.warn(&format!("Compiler warnings:\n{warnings}"));
    }
}

pub(super) fn install(
    env: &Environment,
    args: ModInstallArgs,
) -> Result<Box<dyn CommandResult>, CliError> {
    let file_mode = args.file.is_some();

    if !file_mode && args.id.is_none() {
        return Err(CliError::usage(
            "mod install: provide <id> or --file <path>",
        ));
    }
    if file_mode && args.version.is_some() {
        return Err(CliError::usage(
            "mod install: [version] is not valid with --file",
        ));
    }
    if file_mode && args.no_precompiled {
        // --file always compiles locally, so --no-precompiled is a no-op; reject
        // it rather than silently ignore (matching the [version] rule).
        return Err(CliError::usage(
            "mod install: --no-precompiled has no effect with --file (it always compiles locally)",
        ));
    }
    if !file_mode && args.pch_folder.is_some() {
        // A repo install compiles from fetched source with no folder of the
        // user's to cache a header in, so the flag has nothing to act on.
        return Err(CliError::usage("mod install: --pch-folder requires --file"));
    }
    if !file_mode && args.strict {
        return Err(CliError::usage("mod install: --strict requires --file"));
    }
    // Before any file read, stdin drain, or network fetch, so a bad folder fails
    // without side effects.
    let pch_folder = args
        .pch_folder
        .as_deref()
        .map(resolve_pch_folder)
        .transpose()?;

    // Safe: validated above that a non-file install has an id.
    let repo_id = args.id.as_deref().unwrap_or_default();
    let (raw_source, origin) = match &args.file {
        Some(file) => (read_file_or_stdin(file)?, file_origin(file)),
        None => (
            fetch_repo_source(env, repo_id, args.version.as_deref())?,
            format!("repository mod '{repo_id}'"),
        ),
    };

    let settings = app_settings(env)?;
    let reconciled = extract_and_reconcile(
        env,
        &raw_source,
        &origin,
        args.id.as_deref(),
        &language(&settings),
        args.strict,
    )?;

    // A --file install is a locally-authored mod: stored under `local@<id>`,
    // always compiled locally, and kept out of the user profile.
    let install_id = if file_mode {
        format!("local@{}", reconciled.mod_id)
    } else {
        reconciled.mod_id.clone()
    };

    let result = run_install_pipeline(
        env,
        &install_id,
        &reconciled.normalized_source,
        &reconciled.metadata,
        InstallOpts {
            disabled: args.disabled,
            // --file always compiles locally (the supplied source is
            // authoritative); a repo install honors --no-precompiled.
            force_local_compile: file_mode || args.no_precompiled,
            always_compile_locally: settings.always_compile_mods_locally,
            pch_folder,
        },
    )?;

    Ok(Box::new(ModInstallResult {
        id: install_id,
        file_mode,
        version: result.mod_version,
        metadata: reconciled.metadata,
        config: result.config,
        architectures: result.architecture,
        compiled_locally: result.compiled_locally,
        warnings: result.warnings,
    }))
}

pub(super) fn update(
    env: &Environment,
    args: ModUpdateArgs,
) -> Result<Box<dyn CommandResult>, CliError> {
    if args.all {
        return update_all(env, &args);
    }
    let Some(id) = args.id.as_deref() else {
        return Err(CliError::usage(
            "A mod ID is required unless --all is used.",
        ));
    };
    let current_config = super::require_config(env, id)?;
    let settings = app_settings(env)?;
    let reconciled = fetch_latest(env, id, &settings)?;
    Ok(Box::new(apply_update(
        env,
        current_config,
        reconciled,
        &settings,
        &args,
    )?))
}

/// The repository's current source of an installed mod, parsed and reconciled
/// against its id: what an update would install, and the version it would
/// bring.
fn fetch_latest(
    env: &Environment,
    id: &str,
    settings: &AppSettings,
) -> Result<Reconciled, CliError> {
    let raw_source = fetch_repo_source(env, id, None)?;
    extract_and_reconcile(
        env,
        &raw_source,
        &format!("repository mod '{id}'"),
        Some(id),
        &language(settings),
        false,
    )
}

/// Install what [`fetch_latest`] fetched over the installed mod, or report it
/// up to date when it is the installed version.
fn apply_update(
    env: &Environment,
    current_config: ModConfig,
    reconciled: Reconciled,
    settings: &AppSettings,
    args: &ModUpdateArgs,
) -> Result<ModUpdateResult, CliError> {
    let previous_version = current_config.version.clone();
    let latest_version = reconciled.metadata.version.clone().unwrap_or_default();
    let architectures = reconciled.metadata.architecture.clone().unwrap_or_default();

    // Fast path: latest == installed. No write (the upToDate exit-0).
    if !latest_version.is_empty() && latest_version == previous_version {
        return Ok(ModUpdateResult {
            id: reconciled.mod_id,
            version: latest_version,
            metadata: reconciled.metadata,
            config: current_config,
            architectures,
            compiled_locally: false,
            up_to_date: true,
            previous_version,
            // No compile ran on the up-to-date fast path.
            warnings: String::new(),
        });
    }

    let result = run_install_pipeline(
        env,
        &reconciled.mod_id,
        &reconciled.normalized_source,
        &reconciled.metadata,
        InstallOpts {
            // Preserve the current disabled state unless --disabled is passed.
            disabled: args.disabled,
            force_local_compile: args.no_precompiled,
            always_compile_locally: settings.always_compile_mods_locally,
            // A repo update has no local folder to cache a header in.
            pch_folder: None,
        },
    )?;

    Ok(ModUpdateResult {
        id: reconciled.mod_id,
        version: result.mod_version,
        metadata: reconciled.metadata,
        config: result.config,
        architectures: result.architecture,
        compiled_locally: result.compiled_locally,
        up_to_date: false,
        previous_version,
        warnings: result.warnings,
    })
}

/// What `mod update --all` did with one installed mod.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateAllOutcome {
    Updated {
        from: String,
        to: String,
    },
    UpToDate,
    /// Left alone: a local mod, one never compiled, or one whose updates were
    /// turned off for the version on offer (or for every version).
    Skipped(&'static str),
    Failed(String),
}

/// Whether `mod update --all` leaves a mod alone before asking the repository:
/// a local mod has no repository version, a mod with no config was never
/// compiled, and `*` turns down every version. A pin on one version is only
/// known to apply once the version on offer is, so it is checked after the
/// fetch.
fn skip_before_fetch(id: &str, config: Option<&ModConfig>) -> Option<&'static str> {
    if id.starts_with("local@") {
        return Some("local mod");
    }
    let Some(config) = config else {
        return Some("not compiled");
    };
    if matches!(
        parse_suppression(&config.updates_disabled_for_version),
        Some(UpdateSuppression::All)
    ) {
        return Some("updates turned off");
    }
    None
}

/// One installed mod through `mod update --all`: checked against the
/// repository now, left alone when the version on offer was turned down, and
/// installed otherwise.
fn update_one_of_all(
    env: &Environment,
    id: &str,
    config: ModConfig,
    settings: &AppSettings,
    args: &ModUpdateArgs,
) -> Result<UpdateAllOutcome, CliError> {
    let reconciled = fetch_latest(env, id, settings)?;
    let latest = reconciled.metadata.version.clone().unwrap_or_default();
    if latest == config.version {
        return Ok(UpdateAllOutcome::UpToDate);
    }
    if config.suppresses_update_offer(&latest) {
        return Ok(UpdateAllOutcome::Skipped("this version was turned down"));
    }
    let from = config.version.clone();
    let result = apply_update(env, config, reconciled, settings, args)?;
    Ok(if result.up_to_date {
        UpdateAllOutcome::UpToDate
    } else {
        UpdateAllOutcome::Updated {
            from,
            to: result.version,
        }
    })
}

/// `mod update --all`: every installed repository mod with a newer version,
/// one at a time, each checked against the repository now rather than against
/// the cached result of the last update check. A failure is recorded and the
/// next mod is tried. With `--auto` (the scheduled task), nothing happens
/// unless the autoUpdateMods setting is on and update checks are enabled.
fn update_all(env: &Environment, args: &ModUpdateArgs) -> Result<Box<dyn CommandResult>, CliError> {
    let settings = app_settings(env)?;
    if args.auto && !(settings.auto_update_mods && check_for_updates(&settings)) {
        return Ok(Box::new(ModUpdateAllResult {
            disabled_by_setting: true,
            rows: Vec::new(),
        }));
    }

    let listed: ListInstalledModsResult = env.core.invoke_as(
        "listInstalledMods",
        &ListInstalledModsParams {
            language: language(&settings),
            check_for_updates: check_for_updates(&settings),
            sync_profile: false,
        },
    )?;

    let mut rows = Vec::new();
    for (id, entry) in listed.mods {
        if let Some(reason) = skip_before_fetch(&id, entry.config.as_ref()) {
            rows.push((id, UpdateAllOutcome::Skipped(reason)));
            continue;
        }
        let Some(config) = entry.config else {
            continue;
        };
        let outcome = update_one_of_all(env, &id, config, &settings, args)
            .unwrap_or_else(|e| UpdateAllOutcome::Failed(e.message().to_owned()));
        if let UpdateAllOutcome::Failed(message) = &outcome {
            env.logger
                .info(&format!("Failed to update {id}: {message}"));
        }
        rows.push((id, outcome));
    }

    Ok(Box::new(ModUpdateAllResult {
        disabled_by_setting: false,
        rows,
    }))
}

struct ModUpdateAllResult {
    /// `--auto` with the setting off: nothing was looked at.
    disabled_by_setting: bool,
    rows: Vec<(String, UpdateAllOutcome)>,
}

impl ModUpdateAllResult {
    fn count(&self, pick: fn(&UpdateAllOutcome) -> bool) -> usize {
        self.rows.iter().filter(|(_, o)| pick(o)).count()
    }
}

fn is_updated(outcome: &UpdateAllOutcome) -> bool {
    matches!(outcome, UpdateAllOutcome::Updated { .. })
}

fn is_failed(outcome: &UpdateAllOutcome) -> bool {
    matches!(outcome, UpdateAllOutcome::Failed(_))
}

impl CommandResult for ModUpdateAllResult {
    fn json_data(&self) -> Value {
        let mods: Vec<Value> = self
            .rows
            .iter()
            .map(|(id, outcome)| match outcome {
                UpdateAllOutcome::Updated { from, to } => {
                    json!({ "id": id, "status": "updated", "from": from, "to": to })
                }
                UpdateAllOutcome::UpToDate => json!({ "id": id, "status": "upToDate" }),
                UpdateAllOutcome::Skipped(reason) => {
                    json!({ "id": id, "status": "skipped", "reason": reason })
                }
                UpdateAllOutcome::Failed(error) => {
                    json!({ "id": id, "status": "failed", "error": error })
                }
            })
            .collect();
        json!({
            "disabledBySetting": self.disabled_by_setting,
            "updated": self.count(is_updated),
            "failed": self.count(is_failed),
            "mods": mods,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.disabled_by_setting {
            return writeln!(
                out,
                "Automatic mod updates are off (autoUpdateMods, or update checks disabled)."
            );
        }
        for (id, outcome) in &self.rows {
            match outcome {
                UpdateAllOutcome::Updated { from, to } => {
                    writeln!(out, "Updated: {id} {from} -> {to}")?
                }
                UpdateAllOutcome::UpToDate => writeln!(out, "Up to date: {id}")?,
                UpdateAllOutcome::Skipped(reason) => writeln!(out, "Skipped: {id} ({reason})")?,
                UpdateAllOutcome::Failed(error) => writeln!(out, "Failed: {id}: {error}")?,
            }
        }
        writeln!(
            out,
            "{} updated, {} failed.",
            self.count(is_updated),
            self.count(is_failed)
        )
    }
}

pub(super) fn compile(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings), false)?;

    // As in extract_and_reconcile: a malformed stored source is a usage error,
    // named by the mod it was read from.
    let origin = format!("installed mod '{id}'");
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::usage(format!("Failed to parse metadata from {origin}: {message}"))
    })?;
    let source_mod_id = require_source_id(&metadata, &origin)?;
    // Local mods are stored under `local@<id>` but the source declares the bare
    // `<id>`; strip the prefix before comparing.
    let expected = id.strip_prefix("local@").unwrap_or(id);
    if source_mod_id != expected {
        return Err(CliError::usage(format!(
            "Mod id mismatch: source declares '{source_mod_id}', config has '{id}'."
        )));
    }

    let mod_version = metadata.version.clone().unwrap_or_default();
    let architecture = metadata.architecture.clone().unwrap_or_default();

    let params = CompileInstalledModParams {
        storage_id: id.to_owned(),
        source,
        metadata: metadata.clone(),
    };
    let logger = env.logger;
    let result: CompileInstalledModResult =
        env.core
            .invoke_async_as("compileInstalledMod", &params, |event| {
                report_compile_progress(logger, event)
            })?;
    report_compile_warnings(logger, &result.warnings);

    Ok(Box::new(ModCompileResult {
        id: id.to_owned(),
        version: mod_version,
        metadata,
        config: result.config,
        architectures: architecture,
        warnings: result.warnings,
    }))
}

/// The shared `Method:`/`Architectures:` trailer of the install/update text
/// output.
fn write_install_trailer(
    out: &mut dyn Write,
    compiled_locally: bool,
    architectures: &[String],
) -> io::Result<()> {
    writeln!(
        out,
        "Method:       {}",
        if compiled_locally {
            "compiled locally"
        } else {
            "downloaded precompiled"
        }
    )?;
    if !architectures.is_empty() {
        writeln!(out, "Architectures: {}", architectures.join(", "))?;
    }
    Ok(())
}

/// Add a `warnings` field to a result's `--json` `data` when the compile emitted
/// any, mirroring the protocol DTOs' skip-when-empty - a clean compile's JSON
/// keeps its pre-warnings shape.
fn insert_warnings(data: &mut Value, warnings: &str) {
    if !warnings.is_empty() {
        data["warnings"] = json!(warnings);
    }
}

struct ModInstallResult {
    id: String,
    file_mode: bool,
    version: String,
    metadata: ModMetadata,
    config: ModConfig,
    architectures: Vec<String>,
    compiled_locally: bool,
    /// Clang warnings from a successful local compile; empty on a clean compile
    /// or a precompiled download. Surfaced on stderr during the operation
    /// (`report_compile_warnings`) and, when present, in `--json` output.
    warnings: String,
}

impl CommandResult for ModInstallResult {
    fn json_data(&self) -> Value {
        let mut data = json!({
            "id": self.id,
            "version": self.version,
            "metadata": self.metadata,
            "config": self.config,
            "architectures": self.architectures,
            "compiledLocally": self.compiled_locally,
        });
        insert_warnings(&mut data, &self.warnings);
        data
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let verb = if self.file_mode {
            "Installed from file"
        } else {
            "Installed"
        };
        // Reflect the actual persisted state: a reinstall without --disabled
        // preserves an existing disabled mod (matches `mod update`).
        let marker = state_marker(&self.config);
        writeln!(out, "{verb}: {} {}{marker}", self.id, self.version)?;
        write_install_trailer(out, self.compiled_locally, &self.architectures)
    }
}

/// The marker ending the first line of an install or update: the reason of a
/// mod the catalog nuked, which does not run whatever its switch says, else
/// whether the mod is switched off.
fn state_marker(config: &ModConfig) -> String {
    if !config.nuked.is_empty() {
        format!(" [nuked: {}]", config.nuked)
    } else if config.disabled {
        " [disabled]".to_owned()
    } else {
        String::new()
    }
}

struct ModUpdateResult {
    id: String,
    version: String,
    metadata: ModMetadata,
    config: ModConfig,
    architectures: Vec<String>,
    compiled_locally: bool,
    up_to_date: bool,
    previous_version: String,
    /// Clang warnings from a successful local recompile; empty on a download or
    /// the up-to-date fast path. See [`ModInstallResult::warnings`].
    warnings: String,
}

impl CommandResult for ModUpdateResult {
    fn json_data(&self) -> Value {
        let mut data = json!({
            "id": self.id,
            "version": self.version,
            "metadata": self.metadata,
            "config": self.config,
            "architectures": self.architectures,
            "compiledLocally": self.compiled_locally,
            "upToDate": self.up_to_date,
            "previousVersion": self.previous_version,
        });
        insert_warnings(&mut data, &self.warnings);
        data
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        let marker = state_marker(&self.config);
        if self.up_to_date {
            return writeln!(
                out,
                "Already up to date: {} {}{marker}",
                self.id, self.version
            );
        }
        writeln!(
            out,
            "Updated: {} {} -> {}{marker}",
            self.id, self.previous_version, self.version
        )?;
        write_install_trailer(out, self.compiled_locally, &self.architectures)
    }
}

struct ModCompileResult {
    id: String,
    version: String,
    metadata: ModMetadata,
    config: ModConfig,
    architectures: Vec<String>,
    /// Clang warnings from the successful recompile; empty on a clean compile.
    /// See [`ModInstallResult::warnings`].
    warnings: String,
}

impl CommandResult for ModCompileResult {
    fn json_data(&self) -> Value {
        let mut data = json!({
            "id": self.id,
            "version": self.version,
            "metadata": self.metadata,
            "config": self.config,
            "architectures": self.architectures,
        });
        insert_warnings(&mut data, &self.warnings);
        data
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "Compiled: {} {}", self.id, self.version)?;
        if !self.architectures.is_empty() {
            writeln!(out, "Architectures: {}", self.architectures.join(", "))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn normalize_source_unifies_all_line_endings() {
        assert_eq!(normalize_source("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
        // Idempotent on already-CRLF input.
        assert_eq!(normalize_source("a\r\nb\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn normalize_source_drops_a_leading_bom_only() {
        assert_eq!(normalize_source("\u{feff}// @id x\n"), "// @id x\r\n");
        // Idempotent, and a U+FEFF anywhere else is source text.
        assert_eq!(normalize_source("// @id x\r\n"), "// @id x\r\n");
        assert_eq!(normalize_source("a\u{feff}b"), "a\u{feff}b");
    }

    #[test]
    fn file_origin_names_a_path_or_standard_input() {
        assert_eq!(file_origin("mod.wh.cpp"), "'mod.wh.cpp'");
        assert_eq!(file_origin("-"), "standard input");
    }

    #[test]
    fn resolve_pch_folder_accepts_a_folder_holding_the_header() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PCH_HEADER_NAME), b"#pragma once\n").unwrap();

        let resolved = resolve_pch_folder(&dir.path().to_string_lossy()).unwrap();
        let resolved = std::path::Path::new(&resolved);
        assert!(resolved.is_absolute(), "{}", resolved.display());
        assert!(resolved.join(PCH_HEADER_NAME).is_file());
        // Not canonicalized: a `\\?\` path would reach the compiler.
        assert!(
            !resolved.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            resolved.display()
        );
    }

    #[test]
    fn resolve_pch_folder_rejects_a_folder_that_cannot_precompile() {
        let dir = tempfile::tempdir().unwrap();

        // Present but with no header: the core would silently skip the whole
        // PCH stage, so the flag would be a no-op.
        let no_header = resolve_pch_folder(&dir.path().to_string_lossy()).unwrap_err();
        assert_eq!(no_header.exit_code(), 2);
        assert!(
            no_header.message().contains(PCH_HEADER_NAME),
            "{}",
            no_header.message()
        );

        let missing = resolve_pch_folder(&dir.path().join("nope").to_string_lossy()).unwrap_err();
        assert_eq!(missing.exit_code(), 2);
        assert!(
            missing.message().contains("not an existing directory"),
            "{}",
            missing.message()
        );

        // A file where a directory was named fails the same way.
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let not_a_dir = resolve_pch_folder(&file.to_string_lossy()).unwrap_err();
        assert_eq!(not_a_dir.exit_code(), 2);
    }

    #[test]
    fn reconcile_id_requires_an_id_and_matches_the_argument() {
        let with_id = ModMetadata {
            id: Some("my-mod".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            reconcile_id(&with_id, "'m.wh.cpp'", None).unwrap(),
            "my-mod"
        );
        assert_eq!(
            reconcile_id(&with_id, "'m.wh.cpp'", Some("my-mod")).unwrap(),
            "my-mod"
        );

        // Mismatch and missing id are both usage errors (exit 2).
        assert_eq!(
            reconcile_id(&with_id, "'m.wh.cpp'", Some("other"))
                .unwrap_err()
                .exit_code(),
            2
        );
        let no_id = ModMetadata::default();
        let err = reconcile_id(&no_id, "'m.wh.cpp'", None).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        // The missing-id error names which source lacks the `@id`.
        assert!(err.message().contains("'m.wh.cpp'"), "{}", err.message());
    }
}

/// Golden (snapshot) tests of the compute-then-render seam: construct each
/// install-group result struct directly and assert its exact text form and
/// `--json` `data` shape, with no DLL or session.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::commands::mods::test_support::{config, happy_metadata, nuked_config};
    use crate::output::render_text;

    #[test]
    fn mod_install_text_covers_file_repo_disabled_and_nuked() {
        let file = ModInstallResult {
            id: "local@file-mod".to_owned(),
            file_mode: true,
            version: "1.0.0".to_owned(),
            metadata: happy_metadata(),
            config: config(false),
            architectures: vec!["x86-64".to_owned()],
            compiled_locally: true,
            warnings: String::new(),
        };
        assert_eq!(
            render_text(&file),
            "Installed from file: local@file-mod 1.0.0\n\
             Method:       compiled locally\n\
             Architectures: x86-64\n"
        );

        let repo_disabled = ModInstallResult {
            id: "repo-mod".to_owned(),
            file_mode: false,
            version: "1.0.0".to_owned(),
            metadata: happy_metadata(),
            config: config(true),
            architectures: vec!["x86-64".to_owned()],
            compiled_locally: false,
            warnings: String::new(),
        };
        assert_eq!(
            render_text(&repo_disabled),
            "Installed: repo-mod 1.0.0 [disabled]\n\
             Method:       downloaded precompiled\n\
             Architectures: x86-64\n"
        );
        assert_eq!(repo_disabled.json_data()["compiledLocally"], json!(false));

        // A mod that stays nuked reads so, whatever its switch says.
        let nuked = ModInstallResult {
            config: ModConfig {
                disabled: true,
                ..nuked_config("Malware found")
            },
            ..repo_disabled
        };
        assert_eq!(
            render_text(&nuked),
            "Installed: repo-mod 1.0.0 [nuked: Malware found]\n\
             Method:       downloaded precompiled\n\
             Architectures: x86-64\n"
        );
    }

    #[test]
    fn mod_update_text_covers_up_to_date_changed_and_nuked() {
        let up_to_date = ModUpdateResult {
            id: "m".to_owned(),
            version: "1.2.3".to_owned(),
            metadata: happy_metadata(),
            config: config(false),
            architectures: vec!["x86-64".to_owned()],
            compiled_locally: false,
            up_to_date: true,
            previous_version: "1.2.3".to_owned(),
            warnings: String::new(),
        };
        assert_eq!(render_text(&up_to_date), "Already up to date: m 1.2.3\n");
        assert_eq!(up_to_date.json_data()["upToDate"], json!(true));

        let changed = ModUpdateResult {
            id: "m".to_owned(),
            version: "2.0.0".to_owned(),
            metadata: happy_metadata(),
            config: config(true),
            architectures: vec!["x86-64".to_owned()],
            compiled_locally: true,
            up_to_date: false,
            previous_version: "1.2.3".to_owned(),
            warnings: String::new(),
        };
        assert_eq!(
            render_text(&changed),
            "Updated: m 1.2.3 -> 2.0.0 [disabled]\n\
             Method:       compiled locally\n\
             Architectures: x86-64\n"
        );
        assert_eq!(changed.json_data()["previousVersion"], json!("1.2.3"));

        let nuked_up_to_date = ModUpdateResult {
            config: nuked_config("Malware found"),
            ..up_to_date
        };
        assert_eq!(
            render_text(&nuked_up_to_date),
            "Already up to date: m 1.2.3 [nuked: Malware found]\n"
        );
        let nuked_changed = ModUpdateResult {
            config: nuked_config("Malware found"),
            ..changed
        };
        assert_eq!(
            render_text(&nuked_changed),
            "Updated: m 1.2.3 -> 2.0.0 [nuked: Malware found]\n\
             Method:       compiled locally\n\
             Architectures: x86-64\n"
        );
    }

    #[test]
    fn mod_compile_renders_version_and_architectures() {
        let result = ModCompileResult {
            id: "m".to_owned(),
            version: "1.2.3".to_owned(),
            metadata: happy_metadata(),
            config: config(false),
            architectures: vec!["x86".to_owned(), "x86-64".to_owned()],
            warnings: String::new(),
        };
        assert_eq!(
            render_text(&result),
            "Compiled: m 1.2.3\nArchitectures: x86, x86-64\n"
        );
        // The compile result, unlike install, has no compiledLocally field.
        assert_eq!(result.json_data().get("compiledLocally"), None);
    }

    #[test]
    fn compile_warnings_ride_json_only_when_present_and_never_the_text() {
        let base = || ModCompileResult {
            id: "m".to_owned(),
            version: "1.0".to_owned(),
            metadata: happy_metadata(),
            config: config(false),
            architectures: vec!["x86-64".to_owned()],
            warnings: String::new(),
        };
        const TEXT: &str = "Compiled: m 1.0\nArchitectures: x86-64\n";

        // A clean compile omits `warnings` from the JSON (the skip-when-empty
        // shape) and prints only the text result.
        let clean = base();
        assert_eq!(clean.json_data().get("warnings"), None);
        assert_eq!(render_text(&clean), TEXT);

        // A warned compile carries the tagged block in JSON but keeps it OUT of
        // the stdout text - it rides stderr instead (report_compile_warnings).
        let warned = ModCompileResult {
            warnings: "x86_64-w64-mingw32:\nmod.wh.cpp:3:9: warning: unused variable 'x'"
                .to_owned(),
            ..base()
        };
        assert_eq!(
            warned.json_data()["warnings"],
            json!("x86_64-w64-mingw32:\nmod.wh.cpp:3:9: warning: unused variable 'x'")
        );
        assert_eq!(render_text(&warned), TEXT);
    }

    #[test]
    fn update_all_skips_local_uncompiled_and_held_mods_before_fetching() {
        let held = ModConfig {
            updates_disabled_for_version: "*".to_owned(),
            ..config(false)
        };
        let pinned = ModConfig {
            updates_disabled_for_version: "=2.0".to_owned(),
            ..config(false)
        };
        assert_eq!(
            skip_before_fetch("local@mine", Some(&config(false))),
            Some("local mod")
        );
        assert_eq!(skip_before_fetch("happy-mod", None), Some("not compiled"));
        assert_eq!(
            skip_before_fetch("happy-mod", Some(&held)),
            Some("updates turned off")
        );
        // A pin on one version is only known to apply once the offered version
        // is, so it does not skip before the fetch.
        assert_eq!(skip_before_fetch("happy-mod", Some(&pinned)), None);
        assert_eq!(skip_before_fetch("happy-mod", Some(&config(false))), None);
    }

    #[test]
    fn update_all_reports_every_mod_and_the_totals() {
        let result = ModUpdateAllResult {
            disabled_by_setting: false,
            rows: vec![
                (
                    "a".to_owned(),
                    UpdateAllOutcome::Updated {
                        from: "1.0".to_owned(),
                        to: "1.1".to_owned(),
                    },
                ),
                ("b".to_owned(), UpdateAllOutcome::UpToDate),
                ("local@c".to_owned(), UpdateAllOutcome::Skipped("local mod")),
                (
                    "d".to_owned(),
                    UpdateAllOutcome::Failed("network".to_owned()),
                ),
            ],
        };
        assert_eq!(
            render_text(&result),
            "Updated: a 1.0 -> 1.1\n\
             Up to date: b\n\
             Skipped: local@c (local mod)\n\
             Failed: d: network\n\
             1 updated, 1 failed.\n"
        );
        let data = result.json_data();
        assert_eq!(data["updated"], json!(1));
        assert_eq!(data["failed"], json!(1));
        assert_eq!(
            data["mods"][3],
            json!({ "id": "d", "status": "failed", "error": "network" })
        );

        let off = ModUpdateAllResult {
            disabled_by_setting: true,
            rows: Vec::new(),
        };
        assert_eq!(
            render_text(&off),
            "Automatic mod updates are off (autoUpdateMods, or update checks disabled).\n"
        );
        assert_eq!(off.json_data()["disabledBySetting"], json!(true));
    }
}
