//! `data export` / `data inspect` / `data import`: export Windhawk user data to
//! an archive, print an archive's manifest, and import an archive back. The core
//! owns the archive format and the transaction (`exportUserData` /
//! `inspectUserData` / `importUserData`); this module maps the CLI selection
//! flags onto the shared selection shape, moves the archive bytes to/from the
//! file (or stdout/stdin), drives the gates, and renders the result.
//!
//! `export`/`import` need a session (they read - and, for import, write -
//! installed state); `inspect` is pure over the archive string, so it runs
//! session-free through the stateless transport, like `source meta`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};

use serde_json::{Value, json};
use windhawk_core_host::GatedCore;
use windhawk_core_protocol::{
    ConflictPolicy, ExportOptions, ExportUserDataParams, ExportUserDataResult, ExportWarning,
    FacetToggles, ImportModStatus, ImportOptions, ImportProgress, ImportProgressStatus,
    ImportSummary, ImportUserDataParams, ImportUserDataResult, InspectUserDataParams,
    InspectUserDataResult, ListInstalledModsParams, ListInstalledModsResult, MAX_ARCHIVE_BYTES,
    ModScope, ModScopeKeyword, OperationEvent, PerModToggles, UserDataManifest, UserDataSelection,
};

use crate::Environment;
use crate::args::{ConflictArg, DataExportArgs, DataImportArgs};
use crate::error::{CliError, arch_label};
use crate::logger::Logger;
use crate::output::CommandResult;

/// `data export`: build the selection from the flags, call `exportUserData`,
/// surface any per-mod warnings on stderr, and write the archive to the
/// destination.
pub fn export(env: &Environment, args: DataExportArgs) -> Result<Box<dyn CommandResult>, CliError> {
    let selection = build_selection(&SelectionFlags::from_export(&args))?;
    validate_export_per_mod_scope(env, &selection)?;
    let params = ExportUserDataParams {
        selection,
        options: ExportOptions {
            offline: args.offline,
        },
    };
    let result: ExportUserDataResult = env.core.invoke_as("exportUserData", &params)?;

    // Best-effort export: per-mod warnings go to stderr (they also ride in the
    // `--json` summary), matching `data export` in the plan.
    for warning in &result.summary.warnings {
        env.logger
            .warn(&format!("{}: {}", warning.mod_id, warning.message));
    }

    let to_stdout = args.out == "-";
    if !to_stdout {
        write_archive(&args.out, &result.archive, args.force)?;
    }

    Ok(Box::new(ExportResult {
        archive_to_stdout: to_stdout,
        archive: result.archive,
        out_path: (!to_stdout).then(|| args.out.clone()),
        warnings: result.summary.warnings,
    }))
}

/// `data inspect`: read the archive (file or stdin) and print its manifest.
/// Session-free.
pub fn inspect(core: &GatedCore, path: &str) -> Result<Box<dyn CommandResult>, CliError> {
    let archive = read_path_or_stdin(path)?;
    let result: InspectUserDataResult =
        core.invoke_stateless_as("inspectUserData", &InspectUserDataParams { archive })?;
    Ok(Box::new(InspectResult {
        manifest: result.manifest,
    }))
}

/// `data import`: read the archive, validate it (via `inspectUserData`), build
/// the selection from the flags, run the `--yes` and restart gates, then drive
/// the async `importUserData` transaction, rendering per-mod progress on stderr
/// and the summary at the end. A partial import (a failed mod) still prints the
/// full summary but exits nonzero (`ImportResult::exit_code`).
pub fn import(env: &Environment, args: DataImportArgs) -> Result<Box<dyn CommandResult>, CliError> {
    let archive = read_path_or_stdin(&args.path)?;

    // Validate the archive up front and get its manifest (the plan print, the
    // scope check, and the restart gate all read it). A non-archive is exit 13,
    // like `data inspect`.
    let inspect: InspectUserDataResult = env.core.invoke_as(
        "inspectUserData",
        &InspectUserDataParams {
            archive: archive.clone(),
        },
    )?;
    let manifest = inspect.manifest;

    let selection = build_selection(&SelectionFlags::from_import(&args))?;
    validate_import_per_mod_scope(&manifest, &selection)?;

    // Import is destructive (it overwrites settings and can reinstall mods), so
    // it requires --yes; without it, print the planned actions and exit 2.
    if !env.yes {
        print_import_plan(env, &manifest, &selection);
        return Err(CliError::usage(
            "data import is destructive (it reinstalls mods and overwrites settings); pass --yes to proceed",
        ));
    }

    // The restart gate is the core's: it refuses (RESTART_REQUIRED, exit 8),
    // before any change, an unconfirmed import whose archived app settings
    // would actually change a restart-requiring value. Only the core can judge
    // that - it diffs the archive against the target's current settings - so
    // there is no up-front CLI gate here, unlike `app settings set`, whose
    // patch the CLI itself builds and previews.
    let options = ImportOptions {
        offline: args.offline,
        no_precompiled: args.no_precompiled,
        on_conflict: conflict_policy(args.on_conflict),
        confirm_app_restart: args.confirm_app_restart,
    };
    let params = ImportUserDataParams {
        archive,
        selection,
        options,
    };

    let logger = env.logger;
    let result: ImportUserDataResult =
        env.core
            .invoke_async_as("importUserData", &params, |event| {
                report_import_progress(logger, event)
            })?;
    let summary = result.summary;

    // The tray notification (restart or app-settings-changed) is fired by the
    // core itself, the moment the app settings are applied - so it starts a
    // restart in parallel with the mod loop and survives a mid-import cancel.
    // The summary's intents drive only the informational banner rendered below.
    Ok(Box::new(ImportResult { summary }))
}

/// Map the CLI `--on-conflict` selector onto the core `ConflictPolicy`.
fn conflict_policy(arg: ConflictArg) -> ConflictPolicy {
    match arg {
        ConflictArg::Overwrite => ConflictPolicy::Overwrite,
        ConflictArg::Skip => ConflictPolicy::Skip,
    }
}

/// Reject a `--skip-*` / `--with-*` id that is not in the import scope over the
/// archive's mods (a usage error, exit 2), mirroring the export-side check but
/// resolving the scope against the archive manifest rather than the installed
/// set.
#[track_caller]
fn validate_import_per_mod_scope(
    manifest: &UserDataManifest,
    selection: &UserDataSelection,
) -> Result<(), CliError> {
    if selection.per_mod.is_empty() {
        return Ok(());
    }
    let in_scope = import_scope_ids(manifest, &selection.mods);
    for id in selection.per_mod.keys() {
        if !in_scope.contains(id) {
            return Err(CliError::usage(format!(
                "mod '{id}' is named by a --skip/--with flag but is not in the --mods import scope"
            )));
        }
    }
    Ok(())
}

/// Whether a storage id is a `local@` id. The CLI cannot reach the `domain`
/// predicate (no `cli -> domain` edge), so the prefix rule is restated once
/// here for both scope resolvers.
fn is_local_id(id: &str) -> bool {
    id.starts_with("local@")
}

/// The archive mod ids the scope selects: an explicit list is itself the scope; a
/// keyword scope resolves against the archive's mods (`none` selects nothing).
fn import_scope_ids(manifest: &UserDataManifest, scope: &ModScope) -> BTreeSet<String> {
    let all = || manifest.mods.iter().map(|m| m.mod_id.clone());
    match scope {
        ModScope::Ids { ids } => ids.iter().cloned().collect(),
        ModScope::Keyword(ModScopeKeyword::None) => BTreeSet::new(),
        ModScope::Keyword(ModScopeKeyword::AllExceptLocal) => {
            all().filter(|id| !is_local_id(id)).collect()
        }
        ModScope::Keyword(ModScopeKeyword::All) => all().collect(),
    }
}

/// Print the planned import actions to stderr, so a run without `--yes` shows
/// what it would do before refusing (exit 2).
fn print_import_plan(
    env: &Environment,
    manifest: &UserDataManifest,
    selection: &UserDataSelection,
) {
    let scope = import_scope_ids(manifest, &selection.mods);
    env.logger.warn("data import would (with --yes):");
    if selection.app_settings && manifest.has_app_settings {
        env.logger
            .warn("  - apply the archived Windhawk app settings");
    }
    env.logger
        .warn(&format!("  - import {} mod(s):", scope.len()));
    for m in &manifest.mods {
        if scope.contains(&m.mod_id) {
            env.logger
                .warn(&format!("      {} {}", m.mod_id, m.version));
        }
    }
}

/// Render a core import-progress event on stderr (via the logger, so `--quiet`
/// suppresses the informational lines but keeps failure warnings). A stamped
/// `compileTarget` event names the mod being compiled; the per-mod
/// `ImportProgress` markers report the start and terminal outcome of each mod.
fn report_import_progress(logger: Logger, event: &OperationEvent) {
    let OperationEvent::Progress { payload } = event else {
        return;
    };
    // A driven install's compile progress, stamped with the mod id.
    if let Some(triple) = payload.get("compileTarget").and_then(Value::as_str) {
        let id = payload.get("modId").and_then(Value::as_str).unwrap_or("");
        logger.info(&format!("Compiling {id} for {}...", arch_label(triple)));
        return;
    }
    let Ok(progress) = serde_json::from_value::<ImportProgress>(payload.clone()) else {
        return;
    };
    let pos = format!("[{}/{}]", progress.index + 1, progress.total);
    let id = &progress.mod_id;
    match progress.status {
        ImportProgressStatus::Installing => logger.info(&format!("{pos} Importing {id}...")),
        ImportProgressStatus::Installed => logger.info(&format!("{pos} Installed {id}")),
        ImportProgressStatus::Skipped => logger.info(&format!("{pos} Skipped {id}")),
        ImportProgressStatus::Failed => {
            let reason = progress.message.as_deref().unwrap_or("import failed");
            logger.warn(&format!("{pos} Failed {id}: {reason}"));
        }
    }
}

/// The selection flag values shared by `data export` and `data import` (their
/// clap arg structs carry an identical set), borrowed into one view so the
/// mapping onto the shared selection shape lives in one place.
struct SelectionFlags<'a> {
    app_settings: bool,
    no_app_settings: bool,
    mods: &'a str,
    settings: bool,
    no_settings: bool,
    config: bool,
    no_config: bool,
    skip_settings: Option<&'a str>,
    with_settings: Option<&'a str>,
    skip_config: Option<&'a str>,
    with_config: Option<&'a str>,
}

impl<'a> SelectionFlags<'a> {
    fn from_export(args: &'a DataExportArgs) -> Self {
        Self {
            app_settings: args.app_settings,
            no_app_settings: args.no_app_settings,
            mods: &args.mods,
            settings: args.settings,
            no_settings: args.no_settings,
            config: args.config,
            no_config: args.no_config,
            skip_settings: args.skip_settings.as_deref(),
            with_settings: args.with_settings.as_deref(),
            skip_config: args.skip_config.as_deref(),
            with_config: args.with_config.as_deref(),
        }
    }

    fn from_import(args: &'a DataImportArgs) -> Self {
        Self {
            app_settings: args.app_settings,
            no_app_settings: args.no_app_settings,
            mods: &args.mods,
            settings: args.settings,
            no_settings: args.no_settings,
            config: args.config,
            no_config: args.no_config,
            skip_settings: args.skip_settings.as_deref(),
            with_settings: args.with_settings.as_deref(),
            skip_config: args.skip_config.as_deref(),
            with_config: args.with_config.as_deref(),
        }
    }
}

/// Map the selection flags onto the shared selection shape. The scope, the global
/// facet defaults, and the per-mod overrides are three separate flag groups, so
/// selecting and configuring never share a flag (OQ4).
fn build_selection(flags: &SelectionFlags) -> Result<UserDataSelection, CliError> {
    let app_settings = resolve_toggle(flags.app_settings, flags.no_app_settings, "app-settings")?;
    let settings = resolve_toggle(flags.settings, flags.no_settings, "settings")?;
    let config = resolve_toggle(flags.config, flags.no_config, "config")?;

    Ok(UserDataSelection {
        app_settings,
        mods: parse_scope(flags.mods),
        defaults: FacetToggles { settings, config },
        per_mod: build_per_mod(flags)?,
    })
}

/// Reject a `--skip-*` / `--with-*` id that is not in the export scope (a usage
/// error, exit 2, per CLI_SPEC). Only runs when there are per-mod overrides; a
/// keyword scope reads the installed set to resolve which ids it selects, while
/// an explicit `--mods` list is the scope itself.
#[track_caller]
fn validate_export_per_mod_scope(
    env: &Environment,
    selection: &UserDataSelection,
) -> Result<(), CliError> {
    if selection.per_mod.is_empty() {
        return Ok(());
    }
    let in_scope = scope_ids(env, &selection.mods)?;
    for id in selection.per_mod.keys() {
        if !in_scope.contains(id) {
            return Err(CliError::usage(format!(
                "mod '{id}' is named by a --skip/--with flag but is not in the --mods export scope"
            )));
        }
    }
    Ok(())
}

/// The storage ids the scope selects. An explicit list is itself the scope; a
/// keyword scope resolves against the installed set (`none` selects nothing).
fn scope_ids(env: &Environment, scope: &ModScope) -> Result<BTreeSet<String>, CliError> {
    Ok(match scope {
        ModScope::Ids { ids } => ids.iter().cloned().collect(),
        ModScope::Keyword(ModScopeKeyword::None) => BTreeSet::new(),
        ModScope::Keyword(keyword) => {
            let installed = installed_ids(env)?;
            if matches!(keyword, ModScopeKeyword::AllExceptLocal) {
                installed
                    .into_iter()
                    .filter(|id| !is_local_id(id))
                    .collect()
            } else {
                installed
            }
        }
    })
}

/// The storage ids of every installed mod (a pure read). The id set does not
/// depend on the language, so the default is fine.
fn installed_ids(env: &Environment) -> Result<BTreeSet<String>, CliError> {
    let result: ListInstalledModsResult = env.core.invoke_as(
        "listInstalledMods",
        &ListInstalledModsParams {
            language: "en".to_owned(),
            check_for_updates: false,
            sync_profile: false,
        },
    )?;
    Ok(result.mods.into_keys().collect())
}

/// Resolve a default-ON toggle from its `--x` / `--no-x` pair: `--no-x` turns it
/// off, `--x` (or neither) leaves it on; both together is a usage error.
#[track_caller]
fn resolve_toggle(positive: bool, negative: bool, name: &str) -> Result<bool, CliError> {
    if positive && negative {
        return Err(CliError::usage(format!(
            "--{name} and --no-{name} are mutually exclusive"
        )));
    }
    Ok(!negative)
}

/// Parse the `--mods` scope: a keyword, or a comma-separated id list.
fn parse_scope(raw: &str) -> ModScope {
    match raw {
        "all" => ModScope::Keyword(ModScopeKeyword::All),
        "all-except-local" => ModScope::Keyword(ModScopeKeyword::AllExceptLocal),
        "none" => ModScope::Keyword(ModScopeKeyword::None),
        list => ModScope::Ids {
            ids: split_ids(Some(list)),
        },
    }
}

/// Build the per-mod override map from the `--skip-*` / `--with-*` id lists.
/// Naming one id in both the skip and the with list for a single facet is a
/// usage error.
fn build_per_mod(flags: &SelectionFlags) -> Result<BTreeMap<String, PerModToggles>, CliError> {
    let mut per_mod: BTreeMap<String, PerModToggles> = BTreeMap::new();
    set_facet(&mut per_mod, flags.skip_settings, Facet::Settings, false)?;
    set_facet(&mut per_mod, flags.with_settings, Facet::Settings, true)?;
    set_facet(&mut per_mod, flags.skip_config, Facet::Config, false)?;
    set_facet(&mut per_mod, flags.with_config, Facet::Config, true)?;
    Ok(per_mod)
}

#[derive(Clone, Copy)]
enum Facet {
    Settings,
    Config,
}

impl Facet {
    fn name(self) -> &'static str {
        match self {
            Facet::Settings => "settings",
            Facet::Config => "config",
        }
    }
}

/// Pin one facet to `value` for each id in `ids`, erroring if the facet was
/// already pinned for that id (a mod named in both the skip and the with list).
#[track_caller]
fn set_facet(
    per_mod: &mut BTreeMap<String, PerModToggles>,
    ids: Option<&str>,
    facet: Facet,
    value: bool,
) -> Result<(), CliError> {
    for id in split_ids(ids) {
        let entry = per_mod.entry(id.clone()).or_default();
        let slot = match facet {
            Facet::Settings => &mut entry.settings,
            Facet::Config => &mut entry.config,
        };
        if slot.is_some() {
            return Err(CliError::usage(format!(
                "mod '{id}' appears in more than one --*-{} list",
                facet.name()
            )));
        }
        *slot = Some(value);
    }
    Ok(())
}

/// Split a comma-separated id list, dropping empty entries and trimming
/// whitespace.
fn split_ids(raw: Option<&str>) -> Vec<String> {
    raw.into_iter()
        .flat_map(|s| s.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Write the archive to `out`. Without `force` the create is exclusive, so an
/// existing file is refused rather than clobbered: the destination is typically
/// a backup the caller wants to keep, and an export that lands on it has no
/// undo. The exclusive create is also the check - a stat-then-write would leave
/// a window in which the file appears between the two.
#[track_caller]
fn write_archive(out: &str, archive: &str, force: bool) -> Result<(), CliError> {
    let opened = if force {
        std::fs::File::create(out)
    } else {
        std::fs::File::create_new(out)
    };
    match opened.and_then(|mut file| file.write_all(archive.as_bytes())) {
        Ok(()) => Ok(()),
        // AlreadyExists is reachable only without `force`, which asks for the
        // truncating create. A refusal to clobber is the caller pointing the
        // command at the wrong destination, so it is a usage error (exit 2), not
        // the write failure (exit 1) the other kinds are.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(CliError::usage(format!(
            "'{out}' already exists; pass --force to overwrite it"
        ))),
        Err(e) => Err(CliError::generic(format!("Failed to write '{out}': {e}"))),
    }
}

/// Read a `<path>` argument as UTF-8 text, or stdin when it is `-` (the `mod
/// install --file -` convention). Both arms are bounded by the archive cap
/// BEFORE the whole document lands in memory - the file by its size, stdin by a
/// limited read - so an oversized input costs a stat, or one byte past the cap,
/// rather than its full length.
#[track_caller]
fn read_path_or_stdin(path: &str) -> Result<String, CliError> {
    if path == "-" {
        // One byte past the cap is enough to know the stream is over it.
        let text = match io::read_to_string(io::stdin().take(MAX_ARCHIVE_BYTES + 1)) {
            Ok(text) => text,
            Err(e) => return Err(CliError::generic(format!("Failed to read stdin: {e}"))),
        };
        let size = text.len() as u64;
        return if size > MAX_ARCHIVE_BYTES {
            Err(too_large("the archive on stdin", size))
        } else {
            Ok(text)
        };
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.len() > MAX_ARCHIVE_BYTES => {
            Err(too_large(&format!("'{path}'"), metadata.len()))
        }
        Ok(_) => match std::fs::read_to_string(path) {
            Ok(text) => Ok(text),
            Err(e) => Err(CliError::generic(format!("Failed to read '{path}': {e}"))),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(CliError::usage(format!("'{path}' does not exist")))
        }
        Err(e) => Err(CliError::generic(format!("Failed to read '{path}': {e}"))),
    }
}

/// The over-the-cap rejection, named after where the archive was coming from.
/// A usage error (exit 2): the caller pointed the command at something that
/// cannot be an archive, which is settled at the CLI boundary without a core
/// call.
#[track_caller]
fn too_large(source: &str, size: u64) -> CliError {
    CliError::usage(format!(
        "{source} is too large ({size} bytes; the maximum is {MAX_ARCHIVE_BYTES})"
    ))
}

/// The `data export` result. The archive goes to the destination the handler
/// already chose (a file, or stdout); this carries what the render seam still
/// needs to emit - the inline archive (stdout destination) or the status line (a
/// file), and the warnings for the `--json` summary.
struct ExportResult {
    archive_to_stdout: bool,
    archive: String,
    out_path: Option<String>,
    warnings: Vec<ExportWarning>,
}

impl CommandResult for ExportResult {
    fn json_data(&self) -> Value {
        let mut data = serde_json::Map::new();
        data.insert("summary".to_owned(), json!({ "warnings": self.warnings }));
        if self.archive_to_stdout {
            data.insert("archive".to_owned(), Value::String(self.archive.clone()));
        }
        if let Some(path) = &self.out_path {
            data.insert("out".to_owned(), Value::String(path.clone()));
        }
        Value::Object(data)
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.archive_to_stdout {
            // The scriptable form: the raw archive is the stdout payload.
            writeln!(out, "{}", self.archive)
        } else if let Some(path) = &self.out_path {
            writeln!(out, "Exported user data to {path}")
        } else {
            Ok(())
        }
    }
}

/// The `data inspect` result: the archive manifest.
struct InspectResult {
    manifest: UserDataManifest,
}

impl CommandResult for InspectResult {
    fn json_data(&self) -> Value {
        json!({ "manifest": self.manifest })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "appSettings: {}",
            if self.manifest.has_app_settings {
                "yes"
            } else {
                "no"
            }
        )?;
        writeln!(out, "mods: {}", self.manifest.mods.len())?;
        for m in &self.manifest.mods {
            let facets = [
                m.is_local.then_some("local"),
                m.has_source.then_some("source"),
                m.has_settings.then_some("settings"),
                m.has_config.then_some("config"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            writeln!(out, "  {} {} [{facets}]", m.mod_id, m.version)?;
        }
        Ok(())
    }
}

/// The `data import` result: the per-mod outcome summary and the app-settings
/// intents. Per-mod progress already streamed to stderr during the operation;
/// this is the terminal summary (and the `--json` `data`).
struct ImportResult {
    summary: ImportSummary,
}

impl ImportResult {
    fn count(&self, status: ImportModStatus) -> usize {
        self.summary
            .mods
            .iter()
            .filter(|m| m.status == status)
            .count()
    }
}

impl CommandResult for ImportResult {
    fn json_data(&self) -> Value {
        json!({ "summary": self.summary })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(
            out,
            "Imported: {} installed, {} skipped, {} failed",
            self.count(ImportModStatus::Installed),
            self.count(ImportModStatus::Skipped),
            self.count(ImportModStatus::Failed),
        )?;
        // Name the failed mods so the summary is actionable on stdout too (they
        // also streamed to stderr during the run).
        for m in &self.summary.mods {
            if m.status == ImportModStatus::Failed {
                let reason = m.message.as_deref().unwrap_or("import failed");
                writeln!(out, "  failed: {} ({reason})", m.mod_id)?;
            }
        }
        if let Some(intents) = &self.summary.app_settings {
            if intents.requires_restart {
                writeln!(out, "Windhawk restart requested.")?;
            } else if intents.requires_notify {
                writeln!(out, "Tray notified; engine will pick up the change.")?;
            }
        }
        Ok(())
    }

    /// A partial import (the operation completed but at least one mod failed)
    /// still prints its full summary, but exits nonzero. Exit 7 (compile-failed),
    /// the dominant per-mod failure class, per the CLI spec's exit table.
    fn exit_code(&self) -> i32 {
        if self.count(ImportModStatus::Failed) > 0 {
            7
        } else {
            0
        }
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;
    use windhawk_core_protocol::{AppSettingsIntents, ImportModOutcome, ManifestModEntry};

    #[test]
    fn import_partial_failure_lists_the_failures_and_exits_nonzero() {
        let result = ImportResult {
            summary: ImportSummary {
                mods: vec![
                    ImportModOutcome {
                        mod_id: "taskbar-clock".to_owned(),
                        status: ImportModStatus::Installed,
                        message: None,
                    },
                    ImportModOutcome {
                        mod_id: "skipped-mod".to_owned(),
                        status: ImportModStatus::Skipped,
                        message: Some("already installed (--on-conflict skip)".to_owned()),
                    },
                    ImportModOutcome {
                        mod_id: "broken-mod".to_owned(),
                        status: ImportModStatus::Failed,
                        message: Some("Compilation failed".to_owned()),
                    },
                ],
                app_settings: Some(AppSettingsIntents {
                    requires_restart: true,
                    requires_notify: false,
                }),
            },
        };
        // The counts, the named failure, and the restart line; a failed mod exits
        // nonzero (7) even though the operation itself completed.
        assert_eq!(
            render_text(&result),
            "Imported: 1 installed, 1 skipped, 1 failed\n\
             \x20 failed: broken-mod (Compilation failed)\n\
             Windhawk restart requested.\n"
        );
        assert_eq!(result.exit_code(), 7);
        assert_eq!(
            result.json_data(),
            json!({
                "summary": {
                    "mods": [
                        { "modId": "taskbar-clock", "status": "installed" },
                        { "modId": "skipped-mod", "status": "skipped", "message": "already installed (--on-conflict skip)" },
                        { "modId": "broken-mod", "status": "failed", "message": "Compilation failed" }
                    ],
                    "appSettings": { "requiresRestart": true, "requiresNotify": false }
                }
            })
        );
    }

    #[test]
    fn import_clean_success_exits_zero_with_no_failure_lines() {
        let result = ImportResult {
            summary: ImportSummary {
                mods: vec![ImportModOutcome {
                    mod_id: "taskbar-clock".to_owned(),
                    status: ImportModStatus::Installed,
                    message: None,
                }],
                app_settings: None,
            },
        };
        assert_eq!(
            render_text(&result),
            "Imported: 1 installed, 0 skipped, 0 failed\n"
        );
        assert_eq!(result.exit_code(), 0);
    }

    #[test]
    fn export_to_file_reports_the_path_and_carries_warnings_in_json() {
        let result = ExportResult {
            archive_to_stdout: false,
            archive: "{...}".to_owned(),
            out_path: Some("backup.json".to_owned()),
            warnings: vec![ExportWarning {
                mod_id: "local@x".to_owned(),
                message: "settings omitted".to_owned(),
            }],
        };
        assert_eq!(render_text(&result), "Exported user data to backup.json\n");
        assert_eq!(
            result.json_data(),
            json!({
                "summary": { "warnings": [{ "modId": "local@x", "message": "settings omitted" }] },
                "out": "backup.json"
            })
        );
    }

    #[test]
    fn export_to_stdout_emits_the_raw_archive_and_inlines_it_in_json() {
        let result = ExportResult {
            archive_to_stdout: true,
            archive: "{\n  \"format\": \"windhawk-user-data-v1\"\n}".to_owned(),
            out_path: None,
            warnings: vec![],
        };
        assert_eq!(
            render_text(&result),
            "{\n  \"format\": \"windhawk-user-data-v1\"\n}\n"
        );
        assert_eq!(
            result.json_data(),
            json!({
                "summary": { "warnings": [] },
                "archive": "{\n  \"format\": \"windhawk-user-data-v1\"\n}"
            })
        );
    }

    #[test]
    fn inspect_renders_the_manifest_rows() {
        let result = InspectResult {
            manifest: UserDataManifest {
                has_app_settings: true,
                mods: vec![
                    ManifestModEntry {
                        mod_id: "taskbar-clock".to_owned(),
                        is_local: false,
                        version: "1.2.0".to_owned(),
                        name: Some("Taskbar Clock".to_owned()),
                        has_source: false,
                        has_settings: true,
                        has_config: true,
                    },
                    ManifestModEntry {
                        mod_id: "local@my-mod".to_owned(),
                        is_local: true,
                        version: "0.1".to_owned(),
                        name: None,
                        has_source: true,
                        has_settings: false,
                        has_config: false,
                    },
                ],
            },
        };
        assert_eq!(
            render_text(&result),
            "appSettings: yes\n\
             mods: 2\n\
             \x20 taskbar-clock 1.2.0 [settings, config]\n\
             \x20 local@my-mod 0.1 [local, source]\n"
        );
    }

    #[test]
    fn build_selection_maps_flags_to_the_selection_shape() {
        let args = DataExportArgs {
            out: "-".to_owned(),
            force: false,
            app_settings: false,
            no_app_settings: true,
            mods: "a,b".to_owned(),
            settings: false,
            no_settings: false,
            config: false,
            no_config: true,
            skip_settings: Some("a".to_owned()),
            with_settings: None,
            skip_config: None,
            with_config: Some("b".to_owned()),
            offline: false,
        };
        let selection = build_selection(&SelectionFlags::from_export(&args)).unwrap();
        assert!(!selection.app_settings);
        assert_eq!(
            selection.mods,
            ModScope::Ids {
                ids: vec!["a".to_owned(), "b".to_owned()]
            }
        );
        // Defaults: settings on (unset), config off (--no-config).
        assert_eq!(
            selection.defaults,
            FacetToggles {
                settings: true,
                config: false
            }
        );
        assert_eq!(
            selection.per_mod.get("a").unwrap().settings,
            Some(false) // --skip-settings a
        );
        assert_eq!(
            selection.per_mod.get("b").unwrap().config,
            Some(true) // --with-config b
        );
    }

    #[test]
    fn conflicting_facet_lists_are_a_usage_error() {
        let args = DataExportArgs {
            out: "-".to_owned(),
            force: false,
            app_settings: false,
            no_app_settings: false,
            mods: "all".to_owned(),
            settings: false,
            no_settings: false,
            config: false,
            no_config: false,
            skip_settings: Some("a".to_owned()),
            with_settings: Some("a".to_owned()),
            skip_config: None,
            with_config: None,
            offline: false,
        };
        assert!(build_selection(&SelectionFlags::from_export(&args)).is_err());
    }
}
