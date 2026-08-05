//! `mod install` / `mod update` / `mod compile`: the compile-bearing async
//! pipeline, plus the shared source-fetch, id-reconcile, and install helpers.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{
    CompileInstalledModParams, CompileInstalledModResult, FetchRepoModSourceParams,
    InstallModParams, InstallModResult, ModConfig, ModMetadata, OperationEvent,
};

use crate::Environment;
use crate::args::{ModInstallArgs, ModUpdateArgs};
use crate::commands::parse::{parse_mod_source, require_metadata};
use crate::commands::{app_settings, language};
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

fn extract_and_reconcile(
    env: &Environment,
    source: &str,
    origin: &str,
    id_arg: Option<&str>,
    language: &str,
) -> Result<Reconciled, CliError> {
    let normalized_source = normalize_source(source);
    let parsed = parse_mod_source(env, &normalized_source, language)?;
    // Metadata parsing validates the source; a malformed source being installed
    // is a usage error (exit 2), not an internal one. The `origin` names which
    // source failed - the parse message alone says only that some source did.
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::usage(format!("Failed to parse metadata from {origin}: {message}"))
    })?;
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
        pch_folder: None,
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
    let current_config = super::require_config(env, &args.id)?;
    let previous_version = current_config.version.clone();

    let raw_source = fetch_repo_source(env, &args.id, None)?;
    let settings = app_settings(env)?;
    let reconciled = extract_and_reconcile(
        env,
        &raw_source,
        &format!("repository mod '{}'", args.id),
        Some(&args.id),
        &language(&settings),
    )?;
    let latest_version = reconciled.metadata.version.clone().unwrap_or_default();
    let architectures = reconciled.metadata.architecture.clone().unwrap_or_default();

    // Fast path: latest == installed. No write (the spec'd upToDate exit-0).
    if !latest_version.is_empty() && latest_version == previous_version {
        return Ok(Box::new(ModUpdateResult {
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
        }));
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
        },
    )?;

    Ok(Box::new(ModUpdateResult {
        id: reconciled.mod_id,
        version: result.mod_version,
        metadata: reconciled.metadata,
        config: result.config,
        architectures: result.architecture,
        compiled_locally: result.compiled_locally,
        up_to_date: false,
        previous_version,
        warnings: result.warnings,
    }))
}

pub(super) fn compile(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    super::require_config(env, id)?;
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings))?;

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
        let disabled_marker = if self.config.disabled {
            " [disabled]"
        } else {
            ""
        };
        writeln!(out, "{verb}: {} {}{disabled_marker}", self.id, self.version)?;
        write_install_trailer(out, self.compiled_locally, &self.architectures)
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
        if self.up_to_date {
            return writeln!(out, "Already up to date: {} {}", self.id, self.version);
        }
        let disabled_marker = if self.config.disabled {
            " [disabled]"
        } else {
            ""
        };
        writeln!(
            out,
            "Updated: {} {} -> {}{disabled_marker}",
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
    use crate::commands::mods::test_support::{config, happy_metadata};
    use crate::output::render_text;

    #[test]
    fn mod_install_text_covers_file_repo_and_disabled() {
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
    }

    #[test]
    fn mod_update_text_covers_up_to_date_and_changed() {
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
}
