//! `services::user_data`: the user-data export/import transaction. It owns the
//! three halves - `export` (aggregate the reads into a canonical archive),
//! `inspect` (a pure manifest/validation over an archive string), and the async
//! `import` (install-then-patch each selected mod, apply app settings). The byte
//! format lives in `domain::user_data`; this service reuses the existing
//! `app_settings` / `mods` / `install` / `repo` services rather than
//! duplicating them.
//!
//! Export is a best-effort read-only snapshot: it never writes to disk, the
//! registry, or the network, and a mod whose source will not parse is exported
//! without its settings and noted in the summary rather than aborting the whole
//! export.
//!
//! Import is best-effort per mod, not transactional: a mod that fails to fetch,
//! compile, or install is recorded in the summary and the loop continues. It
//! installs each mod PARKED (`disabled`, `loggingEnabled` off) and enables it
//! last through the config patch, so a to-be-enabled mod goes live only once its
//! settings and config are restored. The archived app settings are re-projected
//! through the export allowlist at prepare (a hand-edited archive cannot flip
//! `safeMode` or the other excluded fields), and the restart gate and reported
//! intents are computed over what the patch actually CHANGES against the target,
//! not over field presence. Each install self-locks its own commit; import
//! takes the keyed `Mod` lock itself around the settings and config writes it
//! drives directly (per-sub-operation, not one continuous hold, which would
//! deadlock against the install's own keyed-lock acquisition).

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Map, Number, Value};
use windhawk_core_domain::{
    self as domain, DEFAULT_LANGUAGE, FlatSettingType, ModId, SettingItem,
    extract_initial_settings_for_engine,
    user_data::{ArchiveMod, ArchiveModConfig, FORMAT_TAG, UserDataArchive},
};
use windhawk_core_protocol::{
    AppSettings, AppSettingsIntents, AppSettingsPatch, ConflictPolicy, EngineSettings,
    EngineSettingsPatch, ExportSummary, ExportUserDataParams, ExportUserDataResult, ExportWarning,
    ImportAppSettingsProgress, ImportAppSettingsStatus, ImportModOutcome, ImportModStatus,
    ImportOptions, ImportProgress, ImportProgressItem, ImportProgressStatus, ImportSummary,
    ImportUserDataParams, ImportUserDataResult, InspectUserDataParams, InspectUserDataResult,
    InstallModParams, InstalledModListEntry, ListInstalledModsParams, ListInstalledModsResult,
    ManifestModEntry, ModConfig, ModConfigPatch, ModScope, ModScopeKeyword, TrayAction,
    UserDataManifest, UserDataSelection,
};

use crate::convert::metadata_to_protocol;
use crate::dispatch::decode_params;
use crate::error::{CoreError, CoreErrorKind};
use crate::runtime::{OpContext, PreparedOp};
use crate::services::app_settings;
use crate::services::install::{engine_items_to_map, orchestrate::run_install};
use crate::services::mods::{
    apply_mod_config_patch, list_installed, read_mod_config, read_mod_settings, write_mod_settings,
};
use crate::services::profile::read_modify_write;
use crate::services::repo::fetch_mod_source;
use crate::services::tray::notify_tray_action;
use crate::services::wire::{file_err, to_value_result};
use crate::session::SessionInner;

/// `exportUserData`: aggregate the selected reads into a canonical archive and
/// return `{ archive, summary }`. Synchronous - only local reads, no network or
/// compile.
pub fn export(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ExportUserDataParams = decode_params("exportUserData", params)?;
    let selection = &params.selection;
    let offline = params.options.offline;

    // One app-settings read serves both the listing language and the optional
    // allowlist projection into the archive. Taken under the AppSettings read
    // lock (as the getAppSettings dispatch would), so a concurrent apply cannot
    // tear the multi-field snapshot.
    let app = {
        let _guard = session
            .app_settings_lock()
            .read()
            .unwrap_or_else(|e| e.into_inner());
        app_settings::read_app_settings(session)?
    };
    let language = if app.language.is_empty() {
        DEFAULT_LANGUAGE
    } else {
        app.language.as_str()
    };
    let app_settings_out = if selection.app_settings {
        Some(project_app_settings(&app)?)
    } else {
        None
    };

    // The installed set as a pure read (no profile sync, no update check).
    let list = list_installed(
        session,
        &ListInstalledModsParams {
            language: language.to_owned(),
            check_for_updates: false,
            sync_profile: false,
        },
    )?;

    let selected = resolve_scope(&selection.mods, &list)?;

    let mut mods = Vec::new();
    let mut warnings = Vec::new();
    // `list.mods` is a BTreeMap, so iteration is installed-id order regardless of
    // the scope that selected them; the stable sort below then groups local mods
    // last, keeping the installed-id order within each group.
    for (id, entry) in &list.mods {
        if !selected.contains(id) {
            continue;
        }
        if let Some(archive_mod) = export_mod(
            session,
            selection,
            offline,
            language,
            id,
            entry,
            &mut warnings,
        )? {
            mods.push(archive_mod);
        }
    }

    // Local (`local@`) mods sort to the end of the array, after all repository
    // mods, as the archive format requires. `sort_by_key` is stable, so the
    // within-group installed-id order the loop produced is preserved; `false`
    // (repository) orders before `true` (local).
    mods.sort_by_key(|m| ModId::str_is_local(&m.mod_id));

    // A mod that failed to load entirely (its listing entry never formed) is
    // invisible to the loop above; surface it as a warning when a keyword scope
    // would have included it, so a backup is never silently missing a mod. A
    // load-errored mod that still has a listing entry (a config-derived entry
    // whose source failed to parse) flows through `export_mod`, which warns on
    // its own terms; an explicit id naming a load-errored mod is a selection
    // error (`resolve_scope`).
    for load_error in &list.load_errors {
        if list.mods.contains_key(&load_error.mod_id) {
            continue;
        }
        let in_scope = match &selection.mods {
            ModScope::Keyword(ModScopeKeyword::All) => true,
            ModScope::Keyword(ModScopeKeyword::AllExceptLocal) => {
                !ModId::str_is_local(&load_error.mod_id)
            }
            ModScope::Keyword(ModScopeKeyword::None) | ModScope::Ids { .. } => false,
        };
        if in_scope {
            warnings.push(warn(
                &load_error.mod_id,
                format!(
                    "the mod could not be loaded, so it was not exported ({})",
                    load_error.error
                ),
            ));
        }
    }

    let archive = UserDataArchive {
        format: FORMAT_TAG.to_owned(),
        app_settings: app_settings_out,
        mods,
    };

    to_value_result(
        "exportUserData",
        &ExportUserDataResult {
            archive: domain::user_data::serialize(&archive),
            summary: ExportSummary { warnings },
        },
    )
}

/// Build one mod's archive entry, or `None` when it cannot be represented (a
/// local mod whose source is missing, or a mod with no recorded version); a
/// per-mod issue is pushed onto `warnings` in either case.
fn export_mod(
    session: &SessionInner,
    selection: &UserDataSelection,
    offline: bool,
    language: &str,
    id: &str,
    entry: &InstalledModListEntry,
    warnings: &mut Vec<ExportWarning>,
) -> Result<Option<ArchiveMod>, CoreError> {
    let is_local = ModId::str_is_local(id);
    let version = installed_version(entry);
    if version.is_empty() {
        warnings.push(warn(
            id,
            "the mod has no recorded version, so it was not exported",
        ));
        return Ok(None);
    }
    let name = entry.metadata.as_ref().and_then(|m| m.name.clone());

    let (want_settings, want_config) = facet_toggles(selection, id);
    // A repository mod's source embeds only under an offline export; a local
    // mod's source is always embedded (it exists nowhere else). The source is
    // read locally whenever settings are selected, to canonicalize against its
    // declared types, even when it is not embedded.
    let embed = is_local || offline;
    let need_source = embed || want_settings;
    let source_text = if need_source {
        read_source(session, id)?
    } else {
        None
    };

    if need_source && source_text.is_none() {
        if is_local {
            warnings.push(warn(
                id,
                "the mod's source file is missing on disk, so it was not exported",
            ));
            return Ok(None);
        }
        // A repository mod whose source is missing on disk (a broken install):
        // export it reference-only, without settings.
        let mut message = String::from(
            "the mod source is not available on disk, so its settings were not exported",
        );
        if offline {
            message.push_str("; this offline archive references the mod instead of embedding it");
        }
        warnings.push(warn(id, message));
    }

    let settings = if want_settings {
        canonical_settings(session, language, id, source_text.as_deref(), warnings)?
    } else {
        None
    };

    // An all-default config carries no information (import resets a processed
    // mod's user-owned config to those same defaults), so it is dropped to keep
    // the archive lean.
    let config = if want_config {
        entry
            .config
            .as_ref()
            .map(project_user_owned_config)
            .filter(|config| *config != ArchiveModConfig::default())
    } else {
        None
    };

    Ok(Some(ArchiveMod {
        mod_id: id.to_owned(),
        version,
        name,
        source: if embed { source_text } else { None },
        settings,
        config,
    }))
}

/// The mod's runtime settings, canonicalized to the source-declared types and
/// emitted in the source's flattened declaration order, or `None` (with a
/// warning) when the source is missing or will not parse.
fn canonical_settings(
    session: &SessionInner,
    language: &str,
    id: &str,
    source_text: Option<&str>,
    warnings: &mut Vec<ExportWarning>,
) -> Result<Option<Value>, CoreError> {
    // A missing source was already warned about in `export_mod`.
    let Some(source) = source_text else {
        return Ok(None);
    };
    match domain::extract_initial_settings(source, language) {
        Ok(items) => {
            let items = items.unwrap_or_default();
            // Under the keyed Mod read lock (as the getModSettings dispatch
            // would take), so a concurrent setModSettings - a clear-then-write
            // of the whole section - cannot be seen half-done as an empty map.
            let raw = {
                let mod_lock = session.mod_lock(id);
                let _guard = mod_lock.read().unwrap_or_else(|e| e.into_inner());
                read_mod_settings(session, id)?
            };
            let settings = canonicalize_settings(&items, &raw);
            // An empty map (nothing stored, or every stored key stale and
            // dropped) carries nothing, so omit it rather than emitting an empty
            // `settings` object. `None` also makes `inspect` report the facet as
            // absent, consistent with the archive.
            if settings.as_object().is_some_and(Map::is_empty) {
                Ok(None)
            } else {
                Ok(Some(settings))
            }
        }
        Err(_) => {
            warnings.push(warn(
                id,
                "the mod source could not be parsed, so its settings were not exported",
            ));
            Ok(None)
        }
    }
}

/// Resolve the scope against the installed set, returning the storage ids to
/// export. An explicit id that is not installed is a selection error.
fn resolve_scope(
    scope: &ModScope,
    list: &ListInstalledModsResult,
) -> Result<BTreeSet<String>, CoreError> {
    match scope {
        ModScope::Keyword(ModScopeKeyword::All) => Ok(list.mods.keys().cloned().collect()),
        ModScope::Keyword(ModScopeKeyword::AllExceptLocal) => Ok(list
            .mods
            .keys()
            .filter(|id| !ModId::str_is_local(id))
            .cloned()
            .collect()),
        ModScope::Keyword(ModScopeKeyword::None) => Ok(BTreeSet::new()),
        ModScope::Ids { ids } => {
            let mut set = BTreeSet::new();
            for id in ids {
                if !list.mods.contains_key(id) {
                    // Distinguish a mod that IS on disk but failed to load from
                    // one that is simply absent, so the error names the real
                    // problem.
                    if let Some(load_error) = list.load_errors.iter().find(|le| &le.mod_id == id) {
                        return Err(CoreError::invalid_request(format!(
                            "cannot export mod {id:?}: it failed to load ({})",
                            load_error.error
                        )));
                    }
                    return Err(CoreError::invalid_request(format!(
                        "cannot export mod {id:?}: it is not installed"
                    )));
                }
                set.insert(id.clone());
            }
            Ok(set)
        }
    }
}

/// The per-mod settings/config toggles: the `defaults`, overridden by any
/// `perMod` entry for this id.
fn facet_toggles(selection: &UserDataSelection, id: &str) -> (bool, bool) {
    let per = selection.per_mod.get(id);
    let settings = per
        .and_then(|p| p.settings)
        .unwrap_or(selection.defaults.settings);
    let config = per
        .and_then(|p| p.config)
        .unwrap_or(selection.defaults.config);
    (settings, config)
}

/// The installed version: the source metadata's version, falling back to the
/// config's mirrored version, then empty (which `export_mod` treats as
/// unexportable).
fn installed_version(entry: &InstalledModListEntry) -> String {
    entry
        .metadata
        .as_ref()
        .and_then(|m| m.version.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| entry.config.as_ref().map(|c| c.version.clone()))
        .unwrap_or_default()
}

/// Read a mod's stored source, or `None` when the source file is absent.
fn read_source(session: &SessionInner, mod_id: &str) -> Result<Option<String>, CoreError> {
    match session
        .deps()
        .files
        .read(&session.storage().mod_source_file(mod_id))
    {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.is_not_found() => Ok(None),
        Err(e) => Err(file_err(e)),
    }
}

/// Canonicalize a mod's raw settings map (`getModSettings` shape) against its
/// parsed source: type each value to its declared type, drop a key the source no
/// longer declares (a stale store key), and emit the survivors in the source's
/// flattened declaration order. The result is independent of the exporting
/// storage mode.
fn canonicalize_settings(items: &[SettingItem], raw: &Map<String, Value>) -> Value {
    let mut resolved: Vec<(Vec<usize>, String, Value)> = Vec::new();
    for (key, value) in raw {
        if let Some(setting) = domain::resolve_flat_setting(items, key) {
            resolved.push((
                setting.order,
                key.clone(),
                canonicalize_value(value, setting.ty),
            ));
        }
        // A key that resolves to no declared leaf is stale and dropped.
    }
    resolved.sort_by(|a, b| a.0.cmp(&b.0));

    let mut map = Map::new();
    for (_, key, value) in resolved {
        map.insert(key, value);
    }
    Value::Object(map)
}

/// Type one raw setting value to its declared type: a boolean as `0`/`1`, a
/// number as a 32-bit integer, a string verbatim. A portable-mode value read
/// back as a string (`"5"`) becomes the typed value (`5`).
fn canonicalize_value(raw: &Value, ty: FlatSettingType) -> Value {
    match ty {
        FlatSettingType::Bool => Value::Number(Number::from(i32::from(coerce_i32(raw) != 0))),
        FlatSettingType::Number => Value::Number(Number::from(coerce_i32(raw))),
        FlatSettingType::String => match raw {
            Value::String(s) => Value::String(s.clone()),
            Value::Number(n) => Value::String(n.to_string()),
            Value::Bool(b) => Value::String(b.to_string()),
            _ => Value::String(String::new()),
        },
    }
}

/// A raw setting value (a JSON number from a registry DWORD, or a JSON string
/// from a portable INI) as an `i32`. A value that does not parse or does not fit
/// - neither of which can occur for a real store value of a number/bool setting
/// - collapses to `0`.
fn coerce_i32(raw: &Value) -> i32 {
    match raw {
        Value::Number(n) => n.as_i64().and_then(|v| i32::try_from(v).ok()).unwrap_or(0),
        Value::String(s) => s.trim().parse::<i32>().unwrap_or(0),
        _ => 0,
    }
}

/// Project the seven user-owned config fields; the five install-owned fields are
/// never carried (they are recomputed on install).
fn project_user_owned_config(config: &ModConfig) -> ArchiveModConfig {
    ArchiveModConfig {
        disabled: config.disabled,
        logging_enabled: config.logging_enabled,
        debug_logging_enabled: config.debug_logging_enabled,
        include_custom: config.include_custom.clone(),
        exclude_custom: config.exclude_custom.clone(),
        include_exclude_custom_only: config.include_exclude_custom_only,
        patterns_match_critical_system_processes: config.patterns_match_critical_system_processes,
    }
}

/// Project the full app settings through the archive allowlist: an
/// `AppSettingsPatch` carrying every allowlisted field and NONE of the two
/// excluded ones (`safeMode`, `disableRunUIScheduledTask`). An allowlist, so a
/// future app setting is excluded from the archive by default. The result
/// decodes as an `AppSettingsPatch` on import, so the same shape produces and
/// consumes it.
///
/// The exhaustive destructures (no `..`) make a NEW `AppSettings` field a
/// COMPILE error here, so it must be explicitly classified - allowlisted into
/// the archive, or bound-and-ignored like the two excluded fields - rather
/// than silently vanishing from every backup.
fn project_app_settings(app: &AppSettings) -> Result<Value, CoreError> {
    let AppSettings {
        language,
        theme,
        disable_update_check,
        // disableRunUIScheduledTask: excluded (mode-dependent, per-install).
        disable_run_ui_scheduled_task: _,
        dev_mode_opt_out,
        hide_tray_icon,
        always_compile_mods_locally,
        dont_auto_show_toolkit,
        mod_tasks_dialog_delay,
        // safeMode: excluded (a transient troubleshooting toggle).
        safe_mode: _,
        logging_verbosity,
        engine,
    } = app;
    let EngineSettings {
        logging_verbosity: engine_logging_verbosity,
        include,
        exclude,
        inject_into_critical_processes,
        inject_into_incompatible_programs,
        inject_into_games,
    } = engine;
    let patch = AppSettingsPatch {
        language: Some(language.clone()),
        theme: Some(theme.clone()),
        disable_update_check: Some(*disable_update_check),
        disable_run_ui_scheduled_task: None,
        dev_mode_opt_out: Some(*dev_mode_opt_out),
        hide_tray_icon: Some(*hide_tray_icon),
        always_compile_mods_locally: Some(*always_compile_mods_locally),
        dont_auto_show_toolkit: Some(*dont_auto_show_toolkit),
        mod_tasks_dialog_delay: Some(*mod_tasks_dialog_delay),
        safe_mode: None,
        logging_verbosity: Some(*logging_verbosity),
        engine: Some(EngineSettingsPatch {
            logging_verbosity: Some(*engine_logging_verbosity),
            include: Some(include.clone()),
            exclude: Some(exclude.clone()),
            inject_into_critical_processes: Some(*inject_into_critical_processes),
            inject_into_incompatible_programs: Some(*inject_into_incompatible_programs),
            inject_into_games: Some(*inject_into_games),
        }),
    };
    serde_json::to_value(&patch)
        .map_err(|e| CoreError::internal(format!("exportUserData: app-settings projection: {e}")))
}

/// A per-mod export warning.
fn warn(mod_id: &str, message: impl Into<String>) -> ExportWarning {
    ExportWarning {
        mod_id: mod_id.to_owned(),
        message: message.into(),
    }
}

/// `inspectUserData`: validate the archive string and project it to a manifest.
/// Pure (no session state), so it is served on the session-free transport too.
pub fn inspect(params: Value) -> Result<Value, CoreError> {
    let params: InspectUserDataParams = decode_params("inspectUserData", params)?;
    let archive = domain::user_data::deserialize(&params.archive)
        .map_err(|e| CoreError::invalid_request(e.to_string()))?;
    let manifest = domain::user_data::manifest(&archive);
    to_value_result(
        "inspectUserData",
        &InspectUserDataResult {
            manifest: manifest_to_protocol(manifest),
        },
    )
}

fn manifest_to_protocol(manifest: domain::user_data::ArchiveManifest) -> UserDataManifest {
    UserDataManifest {
        has_app_settings: manifest.has_app_settings,
        mods: manifest
            .mods
            .into_iter()
            .map(|m| ManifestModEntry {
                mod_id: m.mod_id,
                is_local: m.is_local,
                version: m.version,
                name: m.name,
                has_source: m.has_source,
                has_settings: m.has_settings,
                has_config: m.has_config,
            })
            .collect(),
    }
}

////////////////////////////////////////////////////////////////////////////
// importUserData

/// One selected mod to import: the archive entry plus the facet selection that
/// decides what is overlaid on the clean baseline (a deselected facet still
/// resets to fresh-install defaults, OQ11).
struct PlannedMod {
    archive: ArchiveMod,
    want_settings: bool,
    want_config: bool,
}

/// The validated, selection-resolved import plan captured by the operation body.
struct ImportPlan {
    mods: Vec<PlannedMod>,
    /// The archive's app-settings patch, present only when app settings were
    /// selected AND the archive carries them. Decoded and allowlist-stripped at
    /// prepare, so the body applies exactly the fields the format carries.
    app_settings: Option<AppSettingsPatch>,
    options: ImportOptions,
}

/// `importUserData`: validate the archive and selection synchronously (failures
/// are reported before an operation id exists), then run the install-then-patch
/// transaction on the operation thread. Async because it compiles.
pub fn prepare_import(session: &Arc<SessionInner>, params: Value) -> Result<PreparedOp, CoreError> {
    let params: ImportUserDataParams = decode_params("importUserData", params)?;
    let archive = domain::user_data::deserialize(&params.archive)
        .map_err(|e| CoreError::invalid_request(e.to_string()))?;

    let mods = resolve_import_scope(&archive, &params.selection)?;

    // Offline: refuse before any change if any selected mod is reference-only
    // (would need a network fetch), naming them.
    if params.options.offline {
        let missing: Vec<&str> = mods
            .iter()
            .filter(|p| p.archive.source.as_deref().unwrap_or("").is_empty())
            .map(|p| p.archive.mod_id.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(CoreError::invalid_request(format!(
                "offline import requires every selected mod to embed its source, but these are reference-only: {}",
                missing.join(", ")
            )));
        }
    }

    // The archived app-settings patch, decoded once and re-projected through
    // the export allowlist: the two fields the format never carries
    // (`safeMode`, `disableRunUIScheduledTask`) are stripped, so a hand-edited
    // archive cannot flip them - `safeMode` would silently disable the whole
    // engine on restore, and the scheduled-task field is rejected outright by
    // portable-mode `applyAppSettings`. Import applies exactly the fields the
    // allowlist carries.
    let app_settings = if params.selection.app_settings {
        match &archive.app_settings {
            Some(value) => {
                let mut patch: AppSettingsPatch =
                    serde_json::from_value(value.clone()).map_err(|e| {
                        CoreError::invalid_request(format!(
                            "archive appSettings is not a valid patch: {e}"
                        ))
                    })?;
                strip_excluded_app_settings(&mut patch);
                Some(patch)
            }
            None => None,
        }
    } else {
        None
    };

    // Development-tools gate, enforced BEFORE any change: an import that will compile
    // a mod locally needs the compiler. When the development tools are not installed
    // (an empty compiler path), refuse up front - the GUIs turn this into the
    // install-dev-tools prompt, the CLI exits env-invalid - rather than applying the
    // app settings and then failing every local compile per mod. A selected mod
    // compiles locally when the import forces it (offline / --no-precompiled), when
    // the effective `alwaysCompileModsLocally` is on, or because it is a local mod
    // (which has no precompiled build anywhere). An app-settings-only import (no mods)
    // compiles nothing, so it is never gated.
    let will_compile_locally = !mods.is_empty()
        && (mods
            .iter()
            .any(|planned| ModId::str_is_local(&planned.archive.mod_id))
            || params.options.offline
            || params.options.no_precompiled
            || import_uses_local_compile(session, app_settings.as_ref())?);
    if will_compile_locally && session.storage().info().compiler_path.is_empty() {
        return Err(CoreError::dev_tools_missing(
            "the import must compile mods locally, but the development tools (the compiler) are not installed; install them and try again",
        ));
    }

    // App-settings restart gate, enforced BEFORE any change and computed over
    // what the archived patch would actually CHANGE (its diff against the
    // current settings), not over field presence: an archived patch carries
    // every allowlisted field, so presence alone would demand a restart even
    // for a no-op re-import. The subset is advisory - the target can change
    // between this gate and the apply, the same preview-then-apply window
    // `app settings set` has - so the summary's intents are recomputed at
    // apply time under the lock (`import_body`).
    if let Some(patch) = &app_settings {
        let current = {
            let _guard = session
                .app_settings_lock()
                .read()
                .unwrap_or_else(|e| e.into_inner());
            app_settings::read_app_settings(session)?
        };
        let changed = app_settings::changed_subset(&current, patch);
        if app_settings::intents(&changed).requires_restart && !params.options.confirm_app_restart {
            return Err(CoreError::restart_required(
                "importing these app settings requires a Windhawk restart; pass confirmAppRestart (the CLI's --confirm-app-restart) to proceed",
            ));
        }
    }

    let plan = ImportPlan {
        mods,
        app_settings,
        options: params.options,
    };
    let session = session.clone();
    Ok(PreparedOp(Box::new(move |ctx| {
        import_body(&session, &plan, ctx)
    })))
}

/// Resolve the selection scope against the ARCHIVE's mods (not the target's
/// installed set), returning the mods to process with their per-mod facet
/// toggles. An explicit id the archive does not carry is a selection error.
fn resolve_import_scope(
    archive: &UserDataArchive,
    selection: &UserDataSelection,
) -> Result<Vec<PlannedMod>, CoreError> {
    if let ModScope::Ids { ids } = &selection.mods {
        let present: BTreeSet<&str> = archive.mods.iter().map(|m| m.mod_id.as_str()).collect();
        for id in ids {
            if !present.contains(id.as_str()) {
                return Err(CoreError::invalid_request(format!(
                    "cannot import mod {id:?}: it is not in the archive"
                )));
            }
        }
    }

    let mut planned = Vec::new();
    for m in &archive.mods {
        let in_scope = match &selection.mods {
            ModScope::Keyword(ModScopeKeyword::All) => true,
            ModScope::Keyword(ModScopeKeyword::AllExceptLocal) => !ModId::str_is_local(&m.mod_id),
            ModScope::Keyword(ModScopeKeyword::None) => false,
            ModScope::Ids { ids } => ids.iter().any(|id| id == &m.mod_id),
        };
        if !in_scope {
            continue;
        }
        let (want_settings, want_config) = facet_toggles(selection, &m.mod_id);
        planned.push(PlannedMod {
            archive: m.clone(),
            want_settings,
            want_config,
        });
    }
    Ok(planned)
}

/// Enforce the archive allowlist on the consuming side: drop the two fields
/// the format never carries (`safeMode`, `disableRunUIScheduledTask`) from a
/// decoded `appSettings` patch. Stripping (rather than erroring) treats them
/// like any field the format does not model - ignored - so import applies
/// exactly the fields the allowlist carries.
fn strip_excluded_app_settings(patch: &mut AppSettingsPatch) {
    patch.safe_mode = None;
    patch.disable_run_ui_scheduled_task = None;
}

/// Whether the import's effective `alwaysCompileModsLocally` is on: the archived
/// value when the imported app-settings patch carries it (import applies the
/// patch before the mod loop, so it governs the compiles), otherwise the
/// target's current setting. Read only when the explicit compile-locally
/// signals are all off, so the common path takes no extra read.
fn import_uses_local_compile(
    session: &Arc<SessionInner>,
    patch: Option<&AppSettingsPatch>,
) -> Result<bool, CoreError> {
    if let Some(value) = patch.and_then(|p| p.always_compile_mods_locally) {
        return Ok(value);
    }
    let _guard = session
        .app_settings_lock()
        .read()
        .unwrap_or_else(|e| e.into_inner());
    Ok(app_settings::read_app_settings(session)?.always_compile_mods_locally)
}

/// The operation body: apply app settings up front, then install-then-patch each
/// selected mod, emitting per-mod progress and returning the summary. A cancel
/// stops the loop; already-applied mods (and the app settings) stay applied.
fn import_body(
    session: &Arc<SessionInner>,
    plan: &ImportPlan,
    ctx: &OpContext,
) -> Result<Value, CoreError> {
    // App settings FIRST (before the mod loop), so the imported
    // `alwaysCompileModsLocally` governs the import's own compiles and the
    // restart/notify intents are collected up front. Applied in one write under
    // the exclusive AppSettings lock (import drives `apply_patch` directly, not
    // through the dispatch that normally resolves that lock). The FULL patch is
    // applied - a restore rewrites every carried field and re-syncs the side
    // effects (installer language, scheduled tasks) - but the reported intents
    // are computed over the changed subset read under the same lock: what this
    // apply actually alters decides the restart/notify action, not field
    // presence (the prepare-time gate previews the same diff).
    //
    // The tray action fires HERE, right after the write, not from the summary
    // once the whole import returns: a restart-class change begins the engine
    // restart in the background while the mod loop still runs (rather than
    // waiting for every mod to finish first), and it still fires if a later mod
    // cancels the import - the settings are already on disk, so the engine must
    // be told regardless of how the mod loop ends.
    let mut app_intents: Option<AppSettingsIntents> = None;
    if let Some(patch) = &plan.app_settings {
        emit_app_settings_progress(ctx, ImportAppSettingsStatus::Applying);
        let intents = {
            let _guard = session
                .app_settings_lock()
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let current = app_settings::read_app_settings(session)?;
            let changed = app_settings::changed_subset(&current, patch);
            app_settings::apply_patch(session, patch)?;
            app_settings::intents(&changed)
        };
        emit_app_settings_progress(ctx, ImportAppSettingsStatus::Applied);
        notify_tray_for_intents(session, &intents);
        app_intents = Some(intents);
    }

    // Read the (possibly just-imported) app settings for the language and the
    // compile-vs-download decision.
    let app = {
        let _guard = session
            .app_settings_lock()
            .read()
            .unwrap_or_else(|e| e.into_inner());
        app_settings::read_app_settings(session)?
    };
    let language = if app.language.is_empty() {
        DEFAULT_LANGUAGE.to_owned()
    } else {
        app.language.clone()
    };
    let compile_locally =
        plan.options.offline || plan.options.no_precompiled || app.always_compile_mods_locally;

    let total = plan.mods.len();
    let mut outcomes = Vec::with_capacity(total);
    for (index, planned) in plan.mods.iter().enumerate() {
        // A whole-operation cancel between mods ends the import; what completed
        // stays applied (an import is not transactional across mods).
        ctx.check_canceled()?;
        let mod_id = planned.archive.mod_id.clone();

        // Conflict policy: skip an already-installed mod under `skip`. The
        // existence read runs under the keyed Mod read lock (as the
        // getModConfig dispatch would take); a read FAILURE fails this mod
        // rather than being treated as "not installed" - that would silently
        // convert the user's `skip` into an overwrite.
        if plan.options.on_conflict == ConflictPolicy::Skip {
            let installed = {
                let mod_lock = session.mod_lock(&mod_id);
                let _guard = mod_lock.read().unwrap_or_else(|e| e.into_inner());
                read_mod_config(session, &mod_id)
            };
            match installed {
                Ok(Some(_)) => {
                    outcomes.push(emit_outcome(
                        ctx,
                        &mod_id,
                        index,
                        total,
                        ImportModStatus::Skipped,
                        Some("already installed (--on-conflict skip)".to_owned()),
                    ));
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    outcomes.push(emit_outcome(
                        ctx,
                        &mod_id,
                        index,
                        total,
                        ImportModStatus::Failed,
                        Some(format!("could not check the installed state: {e}")),
                    ));
                    continue;
                }
            }
        }

        emit_progress(
            ctx,
            &mod_id,
            index,
            total,
            ImportProgressStatus::Installing,
            None,
        );
        // Stamp the mod dimension onto every event the install emits (a local
        // compile's `compileTarget`), so it is attributed to the right mod.
        ctx.set_progress_stamp(mod_stamp(&mod_id, index, total));
        let result = import_one_mod(session, ctx, planned, &language, compile_locally);
        ctx.set_progress_stamp(Map::new());

        let outcome = match result {
            Ok(()) => emit_outcome(ctx, &mod_id, index, total, ImportModStatus::Installed, None),
            // A cancel propagates and stops the loop; a real failure is recorded
            // per mod and the loop continues (best-effort, not transactional).
            Err(e) if matches!(e.kind(), CoreErrorKind::Canceled) => return Err(e),
            Err(e) => emit_outcome(
                ctx,
                &mod_id,
                index,
                total,
                ImportModStatus::Failed,
                Some(e.to_string()),
            ),
        };
        outcomes.push(outcome);
    }

    to_value_result(
        "importUserData",
        &ImportUserDataResult {
            summary: ImportSummary {
                mods: outcomes,
                app_settings: app_intents,
            },
        },
    )
}

/// Install one mod PARKED, then restore its settings and config to the clean
/// baseline. Returns `Ok(())` on success; an `Err` is either a cancel (the
/// caller propagates it) or a real per-mod failure (the caller records it and
/// continues).
fn import_one_mod(
    session: &Arc<SessionInner>,
    ctx: &OpContext,
    planned: &PlannedMod,
    language: &str,
    compile_locally: bool,
) -> Result<(), CoreError> {
    let m = &planned.archive;
    let mod_id = m.mod_id.as_str();
    let is_local = ModId::str_is_local(mod_id);

    // Source: the embedded copy (a local mod always, a repository mod under an
    // offline export), otherwise fetched by id + version for a reference-only
    // repository mod (offline was already refused at prepare).
    let source = match m.source.as_deref().filter(|s| !s.is_empty()) {
        Some(source) => source.to_owned(),
        None => fetch_mod_source(session, mod_id, Some(&m.version), ctx.cancel_token())?,
    };

    // Metadata for the install (its include/exclude/architecture/version and the
    // compiled library name); a source that will not parse fails this mod.
    let parsed = domain::extract_metadata(&source, language)
        .map_err(|e| CoreError::internal(format!("mod source could not be parsed: {e}")))?;

    // The archive's id decides where the source is stored and which mod the
    // profile and the update check believe this is, so it must be the id the
    // source itself declares (bare: a local mod stores under `local@<id>` while
    // its source declares `<id>`). Without the compare, an archive could install
    // arbitrary source under a well-known repository mod's identity. The other
    // install entry points reconcile the two the same way.
    let source_mod_id = parsed.id.as_deref().unwrap_or_default();
    if source_mod_id != ModId::str_bare(mod_id) {
        return Err(CoreError::invalid_request(format!(
            "mod id mismatch: the source declares {source_mod_id:?}, the archive entry is {mod_id:?}"
        )));
    }

    let metadata = metadata_to_protocol(parsed);

    // Install parked: `disabled`/`loggingEnabled` are pinned OFF whatever the
    // archive records, so the mod does not go live until the final config patch.
    // A local mod has no precompiled DLL in any repository, so it ALWAYS compiles
    // locally regardless of the compile-vs-download decision.
    let install_params = InstallModParams {
        storage_id: mod_id.to_owned(),
        source: source.clone(),
        metadata,
        disabled: Some(true),
        logging_enabled: Some(false),
        compile_locally: compile_locally || is_local,
        track_in_profile: !is_local,
        pch_folder: None,
        rename_from_storage_id: None,
    };
    run_install(session, install_params, ctx)?;

    // Settings restore (AFTER install, so the install migration's own seeding
    // does not clobber it): the source's declared defaults overlaid with the
    // archived values (archive wins per key) when settings are selected, else the
    // defaults alone. A clean baseline written wholesale (setModSettings clears
    // the section first), not a merge with the target's prior settings.
    let mut settings = extract_initial_settings_for_engine(&source, !is_local)
        .map(|items| engine_items_to_map(items.unwrap_or_default()))
        .map_err(|e| CoreError::internal(e.to_string()))?;
    if planned.want_settings
        && let Some(Value::Object(archived)) = &m.settings
    {
        for (key, value) in archived {
            settings.insert(key.clone(), value.clone());
        }
    }
    {
        let mod_lock = session.mod_lock(mod_id);
        let _guard = mod_lock.write().unwrap_or_else(|e| e.into_inner());
        write_mod_settings(session, mod_id, &settings)?;
    }

    // Config restore LAST (it flips the enable): the seven user-owned fields at
    // their archived values when config is selected, else at their fresh-install
    // defaults - so the target's prior config never survives (OQ11). The five
    // install-owned fields are never in the patch, so this cannot clobber the
    // values install computed.
    let config = planned
        .want_config
        .then(|| m.config.clone())
        .flatten()
        .unwrap_or_default();
    let patch = user_owned_config_patch(&config);
    let disabled = config.disabled;
    {
        let mod_lock = session.mod_lock(mod_id);
        let _guard = mod_lock.write().unwrap_or_else(|e| e.into_inner());
        apply_mod_config_patch(session, mod_id, &patch)?;
    }

    // Mirror the restored `disabled` into the user profile for non-local mods
    // (the config tree is authoritative for what the engine loads; the profile
    // mirror keeps GUI/CLI reads consistent without an external sync), exactly as
    // `setModEnabled` does. `local@` mods are not tracked. `latestVersion` is
    // deliberately not written by import.
    if !is_local {
        read_modify_write(session, false, |profile| {
            profile.set_mod_disabled(mod_id, disabled);
            (true, ())
        })?;
    }

    Ok(())
}

/// The seven-field `updateModConfig` patch import writes: exactly the user-owned
/// config subset, all fields present. The five install-owned fields
/// (`libraryFileName`/`include`/`exclude`/`architecture`/`version`) are never
/// carried, so restoring config cannot clobber the values install computed (D4);
/// this "only the seven" discipline lives here, not in `updateModConfig`.
fn user_owned_config_patch(config: &ArchiveModConfig) -> ModConfigPatch {
    ModConfigPatch {
        disabled: Some(config.disabled),
        logging_enabled: Some(config.logging_enabled),
        debug_logging_enabled: Some(config.debug_logging_enabled),
        include_custom: Some(config.include_custom.clone()),
        exclude_custom: Some(config.exclude_custom.clone()),
        include_exclude_custom_only: Some(config.include_exclude_custom_only),
        patterns_match_critical_system_processes: Some(
            config.patterns_match_critical_system_processes,
        ),
        // The install-owned fields stay absent (preserve install's output).
        library_file_name: None,
        include: None,
        exclude: None,
        architecture: None,
        version: None,
    }
}

/// The `{ item, modId, index, total }` stamp merged onto a driven install's events,
/// so a forwarded sub-event (e.g. a local compile's `compileTarget`) reads as the
/// same per-mod progress shape - `item: "mod"` included - as the status markers.
fn mod_stamp(mod_id: &str, index: usize, total: usize) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("item".to_owned(), Value::String("mod".to_owned()));
    map.insert("modId".to_owned(), Value::String(mod_id.to_owned()));
    map.insert("index".to_owned(), Value::Number(Number::from(index)));
    map.insert("total".to_owned(), Value::Number(Number::from(total)));
    map
}

/// Emit one per-mod `progress` event.
fn emit_progress(
    ctx: &OpContext,
    mod_id: &str,
    index: usize,
    total: usize,
    status: ImportProgressStatus,
    message: Option<String>,
) {
    let event = ImportProgress {
        item: ImportProgressItem::Mod,
        mod_id: mod_id.to_owned(),
        index,
        total,
        status,
        message,
    };
    ctx.emit_progress(serde_json::to_value(&event).unwrap_or(Value::Null));
}

/// Emit the app-settings step `progress` event. Sent before the mod loop, so no mod
/// `progress_stamp` is active to merge a mod position into it.
fn emit_app_settings_progress(ctx: &OpContext, status: ImportAppSettingsStatus) {
    let event = ImportAppSettingsProgress {
        item: ImportProgressItem::AppSettings,
        status,
    };
    ctx.emit_progress(serde_json::to_value(&event).unwrap_or(Value::Null));
}

/// Poke the tray for the action the just-applied app-settings intents call for:
/// a background engine restart when a restart-class field changed, otherwise the
/// lighter "app settings changed" ping when a notify-class field changed, and
/// nothing when neither did. Restart wins over notify, matching `app settings
/// set`. A no-op re-import (the change-based intents are both false) pokes
/// nothing, so a same-machine restore does not restart the engine for nothing.
fn notify_tray_for_intents(session: &SessionInner, intents: &AppSettingsIntents) {
    let action = if intents.requires_restart {
        TrayAction::RestartBg
    } else if intents.requires_notify {
        TrayAction::AppSettingsChanged
    } else {
        return;
    };
    notify_tray_action(session, action);
}

/// Emit the terminal per-mod progress and build the matching summary outcome.
fn emit_outcome(
    ctx: &OpContext,
    mod_id: &str,
    index: usize,
    total: usize,
    status: ImportModStatus,
    message: Option<String>,
) -> ImportModOutcome {
    let progress_status = match status {
        ImportModStatus::Installed => ImportProgressStatus::Installed,
        ImportModStatus::Skipped => ImportProgressStatus::Skipped,
        ImportModStatus::Failed => ImportProgressStatus::Failed,
    };
    emit_progress(ctx, mod_id, index, total, progress_status, message.clone());
    ImportModOutcome {
        mod_id: mod_id.to_owned(),
        status,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_archive_cap_mirrors_the_domain_one() {
        // A host that picks an archive FILE sizes it against the protocol
        // constant before reading it, and the core rejects the string it is
        // handed against the domain one. Let the two drift and a host would
        // refuse a file the core accepts, or slurp one the core will reject.
        assert_eq!(
            windhawk_core_protocol::MAX_ARCHIVE_BYTES,
            domain::user_data::MAX_ARCHIVE_BYTES as u64
        );
    }

    fn parse(yaml: &str) -> Vec<SettingItem> {
        let src =
            format!("// ==WindhawkModSettings==\n/*\n{yaml}\n*/\n// ==/WindhawkModSettings==\n");
        domain::extract_initial_settings(&src, "en")
            .unwrap()
            .unwrap()
    }

    #[test]
    fn canonicalize_types_and_orders_against_the_template() {
        // A non-alphabetical declaration order and array indices: the archive
        // must follow the template order, not the store's sorted-key order.
        let items = parse("- zed: true\n- alpha: 0\n- names:\n  - x");
        let mut raw = Map::new();
        // Seed in sorted-key order (what the BTreeMap-backed store enumerates).
        raw.insert("alpha".to_owned(), Value::String("5".to_owned())); // number as portable string
        raw.insert("names[0]".to_owned(), Value::String("hi".to_owned()));
        raw.insert("names[2]".to_owned(), Value::String("yo".to_owned()));
        raw.insert("stale".to_owned(), Value::Number(9.into())); // not declared -> dropped
        raw.insert("zed".to_owned(), Value::Number(1.into())); // bool

        let settings = canonicalize_settings(&items, &raw);
        // Template order: zed (decl 0), alpha (decl 1), names[0], names[2]; the
        // stale key is gone; the portable string "5" is typed to 5.
        assert_eq!(
            settings,
            serde_json::json!({ "zed": 1, "alpha": 5, "names[0]": "hi", "names[2]": "yo" })
        );
        // The key order is the template order (preserve_order is on).
        let keys: Vec<&str> = settings
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["zed", "alpha", "names[0]", "names[2]"]);
    }

    #[test]
    fn canonicalize_bool_normalizes_any_nonzero_to_one() {
        let items = parse("- flag: false");
        let mut raw = Map::new();
        raw.insert("flag".to_owned(), Value::Number(7.into()));
        assert_eq!(
            canonicalize_settings(&items, &raw),
            serde_json::json!({ "flag": 1 })
        );
    }
}
