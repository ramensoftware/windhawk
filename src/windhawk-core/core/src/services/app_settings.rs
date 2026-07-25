//! `services::app_settings`: the app and engine settings read/write, the
//! restart/notify predicates exposed as `previewAppSettingsEffects`, and the
//! non-portable side effects - the installer-language write and the
//! `WindhawkUpdateTask` / `WindhawkRunUITask` scheduled-task toggling through
//! the Processes port (logged-as-warning, never fatal), reproducing
//! `services/appSettings.ts`.

use serde_json::Value;
use windhawk_core_domain::{DEFAULT_LANGUAGE, language_to_installer_lcid};
use windhawk_core_ports::{CancelToken, ProcessRequest, SettingsTree};
use windhawk_core_protocol::{
    AppSettings, AppSettingsIntents, AppSettingsPatch, AppSettingsPatchParams, DEFAULT_THEME,
    EngineSettings, EngineSettingsPatch,
};

use crate::callbacks::LogLevel;
use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::services::settings_io::{
    open_tree, read_array, read_bool, read_number, read_string, write_array, write_bool,
    write_number, write_string,
};
use crate::services::wire::to_value_result;
use crate::session::SessionInner;

/// `getAppSettings`: the full read with defaults; portable mode reports
/// `disableRunUIScheduledTask` as `null` (the task only exists non-portable).
pub fn get(session: &SessionInner, _params: Value) -> Result<Value, CoreError> {
    to_value_result("getAppSettings", &read_app_settings(session)?)
}

/// The typed read behind `getAppSettings`, for in-core callers
/// (`services::user_data`'s export and import), so composing services do not
/// round-trip through the wire `Value` shape. Callers that need a consistent
/// multi-field snapshot hold the `AppSettings` lock themselves (dispatch does
/// for the command; import/export do around their direct calls).
pub(crate) fn read_app_settings(session: &SessionInner) -> Result<AppSettings, CoreError> {
    let storage = session.storage();
    let app = open_tree(storage, &storage.app_settings_tree(), false)?;
    let engine = open_tree(storage, &storage.engine_settings_tree(), false)?;
    let app: &dyn SettingsTree = &*app;
    let engine: &dyn SettingsTree = &*engine;

    let disable_run_ui_scheduled_task = if storage.portable() {
        None
    } else {
        Some(read_bool(app, "DisableRunUIScheduledTask")?)
    };

    let settings = AppSettings {
        language: read_string(app, "Language")?.unwrap_or_else(|| DEFAULT_LANGUAGE.to_owned()),
        theme: read_string(app, "Theme")?.unwrap_or_else(|| DEFAULT_THEME.to_owned()),
        disable_update_check: read_bool(app, "DisableUpdateCheck")?,
        disable_run_ui_scheduled_task,
        dev_mode_opt_out: read_bool(app, "DevModeOptOut")?,
        hide_tray_icon: read_bool(app, "HideTrayIcon")?,
        always_compile_mods_locally: read_bool(app, "AlwaysCompileModsLocally")?,
        dont_auto_show_toolkit: read_bool(app, "DontAutoShowToolkit")?,
        mod_tasks_dialog_delay: read_number(app, "ModTasksDialogDelay", 2000)?,
        safe_mode: read_bool(app, "SafeMode")?,
        logging_verbosity: read_number(app, "LoggingVerbosity", 0)?,
        engine: EngineSettings {
            logging_verbosity: read_number(engine, "LoggingVerbosity", 0)?,
            include: read_array(engine, "Include")?,
            exclude: read_array(engine, "Exclude")?,
            inject_into_critical_processes: read_bool(engine, "InjectIntoCriticalProcesses")?,
            inject_into_incompatible_programs: read_bool(engine, "InjectIntoIncompatiblePrograms")?,
            inject_into_games: read_bool(engine, "InjectIntoGames")?,
        },
    };
    Ok(settings)
}

/// `previewAppSettingsEffects`: the restart/notify predicates only, no write.
pub fn preview(_session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: AppSettingsPatchParams = decode_params("previewAppSettingsEffects", params)?;
    to_value_result("previewAppSettingsEffects", &intents(&params.patch))
}

/// `applyAppSettings`: side effects then the storage write; returns the same
/// intents `previewAppSettingsEffects` would.
pub fn apply(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: AppSettingsPatchParams = decode_params("applyAppSettings", params)?;
    to_value_result("applyAppSettings", &apply_patch(session, &params.patch)?)
}

/// The typed write behind `applyAppSettings`, for in-core callers
/// (`services::user_data`'s import, which drives it under its own exclusive
/// `AppSettings` lock): side effects then the storage write, returning the
/// presence-based intents of the applied patch.
pub(crate) fn apply_patch(
    session: &SessionInner,
    patch: &AppSettingsPatch,
) -> Result<AppSettingsIntents, CoreError> {
    let storage = session.storage();
    let result = intents(patch);

    if storage.portable() {
        // Portable mode has no scheduled task: a present bool is rejected. Absent
        // and a present `null` both decode to `None` and are tolerated as a
        // no-op (a portable settings round-trip echoes the `null` we reported).
        if patch.disable_run_ui_scheduled_task.is_some() {
            return Err(CoreError::invalid_request(
                "Cannot set disableRunUIScheduledTask in portable mode",
            ));
        }
    } else {
        if let Some(language) = &patch.language
            && let Some(lcid) = language_to_installer_lcid(language)
        {
            let override_key = session
                .config()
                .debug_overrides
                .installer_reg_key
                .as_deref();
            if let Err(e) = session
                .deps()
                .installer_language
                .set_installer_language(lcid, override_key)
            {
                // The TS catches and warns; a failed installer write never
                // fails the command. This enriches the warning with the OsError
                // reason (the OS-call context) - a host-observable LogFn line,
                // the third intended exception to constraint 1, pinned by no
                // fixture - instead of discarding it as the old bool did.
                session.log(
                    LogLevel::Warn,
                    format!(
                        "Failed to set installer language: {}{}",
                        e.message,
                        e.os_error_suffix()
                    ),
                );
            }
        }
        if let Some(disable_update_check) = patch.disable_update_check {
            enable_scheduled_task(session, "WindhawkUpdateTask", !disable_update_check);
        }
        // A present bool toggles the task (enable iff not disabled); absent/null
        // (`None`) does nothing. (A non-portable `null` once enabled the task;
        // that arm was dropped - no client sends a non-portable `null`.)
        if let Some(disable) = patch.disable_run_ui_scheduled_task {
            enable_scheduled_task(session, "WindhawkRunUITask", !disable);
        }
    }

    serialize(session, patch)?;
    Ok(result)
}

/// `shouldRestartApp` || `shouldNotifyTrayProgram`, packaged. Presence-based:
/// a field in the patch counts whether or not its value differs from the
/// stored one (the GUI/CLI send only the fields the user changed, so presence
/// approximates change there). `services::user_data`'s import instead computes
/// these over [`changed_subset`], because an archived patch carries every
/// allowlisted field and presence alone would demand a restart for a no-op
/// re-import.
pub(crate) fn intents(patch: &AppSettingsPatch) -> AppSettingsIntents {
    AppSettingsIntents {
        requires_restart: should_restart(patch),
        requires_notify: should_notify(patch),
    }
}

/// The subset of `patch` whose values differ from `current`, field by field;
/// an engine group left with no differing field collapses to absent. Feeding
/// this to [`intents`] yields change-based restart/notify intents - what
/// applying `patch` would actually alter - which is how import gates and
/// reports an archived patch (see [`intents`]).
///
/// The exhaustive destructures (no `..`) make a NEW patch field a COMPILE
/// error here, so it must be classified against its current value rather than
/// silently passing through as always-changed or never-changed.
pub(crate) fn changed_subset(current: &AppSettings, patch: &AppSettingsPatch) -> AppSettingsPatch {
    fn diff<T: PartialEq + Clone>(patch: &Option<T>, current: &T) -> Option<T> {
        match patch {
            Some(v) if v != current => Some(v.clone()),
            _ => None,
        }
    }
    let AppSettingsPatch {
        language,
        theme,
        disable_update_check,
        disable_run_ui_scheduled_task,
        dev_mode_opt_out,
        hide_tray_icon,
        always_compile_mods_locally,
        dont_auto_show_toolkit,
        mod_tasks_dialog_delay,
        safe_mode,
        logging_verbosity,
        engine,
    } = patch;
    let engine = engine
        .as_ref()
        .map(|e| {
            let EngineSettingsPatch {
                logging_verbosity,
                include,
                exclude,
                inject_into_critical_processes,
                inject_into_incompatible_programs,
                inject_into_games,
            } = e;
            EngineSettingsPatch {
                logging_verbosity: diff(logging_verbosity, &current.engine.logging_verbosity),
                include: diff(include, &current.engine.include),
                exclude: diff(exclude, &current.engine.exclude),
                inject_into_critical_processes: diff(
                    inject_into_critical_processes,
                    &current.engine.inject_into_critical_processes,
                ),
                inject_into_incompatible_programs: diff(
                    inject_into_incompatible_programs,
                    &current.engine.inject_into_incompatible_programs,
                ),
                inject_into_games: diff(inject_into_games, &current.engine.inject_into_games),
            }
        })
        .filter(EngineSettingsPatch::has_any);
    AppSettingsPatch {
        language: diff(language, &current.language),
        theme: diff(theme, &current.theme),
        disable_update_check: diff(disable_update_check, &current.disable_update_check),
        // The current value is mode-dependent (`None` in portable), so a
        // present bool counts as changed unless the current mode reports that
        // same bool.
        disable_run_ui_scheduled_task: disable_run_ui_scheduled_task
            .filter(|v| current.disable_run_ui_scheduled_task != Some(*v)),
        dev_mode_opt_out: diff(dev_mode_opt_out, &current.dev_mode_opt_out),
        hide_tray_icon: diff(hide_tray_icon, &current.hide_tray_icon),
        always_compile_mods_locally: diff(
            always_compile_mods_locally,
            &current.always_compile_mods_locally,
        ),
        dont_auto_show_toolkit: diff(dont_auto_show_toolkit, &current.dont_auto_show_toolkit),
        mod_tasks_dialog_delay: diff(mod_tasks_dialog_delay, &current.mod_tasks_dialog_delay),
        safe_mode: diff(safe_mode, &current.safe_mode),
        logging_verbosity: diff(logging_verbosity, &current.logging_verbosity),
        engine,
    }
}

fn should_restart(patch: &AppSettingsPatch) -> bool {
    patch.safe_mode.is_some()
        || patch.logging_verbosity.is_some()
        || patch.engine.as_ref().is_some_and(|e| e.has_any())
}

fn should_notify(patch: &AppSettingsPatch) -> bool {
    patch.language.is_some()
        || patch.disable_update_check.is_some()
        || patch.hide_tray_icon.is_some()
        || patch.dont_auto_show_toolkit.is_some()
        || patch.mod_tasks_dialog_delay.is_some()
}

/// Write the present fields per location, only opening (and thus creating) a
/// tree when it has at least one field - the TS `collectForWrite` +
/// "writeAllFields only if non-empty". Explicit `if let Some` loops calling the
/// shared `write_*` codec helpers (no `WriteVal`/`apply_ops` accumulator). Field
/// WRITE-ORDER is non-observable end-to-end (the registry/INI backends write per
/// key and the C++ engine reads by key, so order reaches no consumer); the
/// `CONFIG_FIELDS`-style order is kept for human consistency only, not to
/// satisfy a gate. The OPEN DECISION (open only when a group has a present
/// field) is the observable side effect, pinned by the empty-tree open-policy
/// characterization test.
fn serialize(session: &SessionInner, patch: &AppSettingsPatch) -> Result<(), CoreError> {
    let storage = session.storage();

    if app_patch_has_any(patch) {
        let mut tree = open_tree(storage, &storage.app_settings_tree(), true)?;
        let tree = tree.as_mut();
        if let Some(v) = &patch.language {
            write_string(tree, "Language", v)?;
        }
        if let Some(v) = &patch.theme {
            write_string(tree, "Theme", v)?;
        }
        if let Some(v) = patch.disable_update_check {
            write_bool(tree, "DisableUpdateCheck", v)?;
        }
        // disableRunUIScheduledTask is written only for a present bool (the TS
        // `!= null ? {...} : rest`); portable mode never reaches here with one.
        // Absent/null (`None`) neither writes nor opens the tree, so it is
        // excluded from `app_patch_has_any` too.
        if let Some(v) = patch.disable_run_ui_scheduled_task {
            write_bool(tree, "DisableRunUIScheduledTask", v)?;
        }
        if let Some(v) = patch.dev_mode_opt_out {
            write_bool(tree, "DevModeOptOut", v)?;
        }
        if let Some(v) = patch.hide_tray_icon {
            write_bool(tree, "HideTrayIcon", v)?;
        }
        if let Some(v) = patch.always_compile_mods_locally {
            write_bool(tree, "AlwaysCompileModsLocally", v)?;
        }
        if let Some(v) = patch.dont_auto_show_toolkit {
            write_bool(tree, "DontAutoShowToolkit", v)?;
        }
        if let Some(v) = patch.mod_tasks_dialog_delay {
            write_number(tree, "ModTasksDialogDelay", v)?;
        }
        if let Some(v) = patch.safe_mode {
            write_bool(tree, "SafeMode", v)?;
        }
        if let Some(v) = patch.logging_verbosity {
            write_number(tree, "LoggingVerbosity", v)?;
        }
    }

    if let Some(engine) = &patch.engine
        && engine.has_any()
    {
        let mut tree = open_tree(storage, &storage.engine_settings_tree(), true)?;
        let tree = tree.as_mut();
        if let Some(v) = engine.logging_verbosity {
            write_number(tree, "LoggingVerbosity", v)?;
        }
        if let Some(v) = &engine.include {
            write_array(tree, "Include", v)?;
        }
        if let Some(v) = &engine.exclude {
            write_array(tree, "Exclude", v)?;
        }
        if let Some(v) = engine.inject_into_critical_processes {
            write_bool(tree, "InjectIntoCriticalProcesses", v)?;
        }
        if let Some(v) = engine.inject_into_incompatible_programs {
            write_bool(tree, "InjectIntoIncompatiblePrograms", v)?;
        }
        if let Some(v) = engine.inject_into_games {
            write_bool(tree, "InjectIntoGames", v)?;
        }
    }
    Ok(())
}

/// Whether any APP-LEVEL field is present (the open decision for the app tree).
/// `disable_run_ui_scheduled_task` counts ONLY when it is `Some` (a present
/// bool); absent and present-null both decode to `None`, which neither writes
/// nor opens the tree.
///
/// The exhaustive destructure (no `..`) makes a NEW `AppSettingsPatch` field a
/// COMPILE error here, so it must be classified rather than silently dropped:
/// either it writes to the app tree (add `|| field.is_some()`) or it does not
/// and is bound-and-ignored like `engine` (its own tree, gated separately by
/// `engine.has_any()` in `serialize`). This must stay in lockstep with
/// `serialize`'s app-tree write loop, including the present-bool (`Some`)
/// condition for the scheduled task; the compiler enforces presence, not that
/// per-field condition.
fn app_patch_has_any(patch: &AppSettingsPatch) -> bool {
    let AppSettingsPatch {
        language,
        theme,
        disable_update_check,
        disable_run_ui_scheduled_task,
        dev_mode_opt_out,
        hide_tray_icon,
        always_compile_mods_locally,
        dont_auto_show_toolkit,
        mod_tasks_dialog_delay,
        safe_mode,
        logging_verbosity,
        // The engine group has its OWN tree, opened separately in `serialize`
        // when `engine.has_any()`, so it does NOT contribute to the app tree's
        // open decision.
        engine: _,
    } = patch;
    language.is_some()
        || theme.is_some()
        || disable_update_check.is_some()
        || disable_run_ui_scheduled_task.is_some()
        || dev_mode_opt_out.is_some()
        || hide_tray_icon.is_some()
        || always_compile_mods_locally.is_some()
        || dont_auto_show_toolkit.is_some()
        || mod_tasks_dialog_delay.is_some()
        || safe_mode.is_some()
        || logging_verbosity.is_some()
}

/// Toggle a scheduled task via `schtasks.exe /change /tn <task> /enable|/disable`
/// (honoring the `schtasksPath` debug override). A nonzero exit is a warning,
/// a spawn failure an error; neither fails the command.
fn enable_scheduled_task(session: &SessionInner, task_name: &str, enable: bool) {
    let program = session
        .config()
        .debug_overrides
        .schtasks_path
        .clone()
        .unwrap_or_else(|| "schtasks.exe".to_owned());
    let request = ProcessRequest {
        program,
        args: vec![
            "/change".to_owned(),
            "/tn".to_owned(),
            task_name.to_owned(),
            if enable { "/enable" } else { "/disable" }.to_owned(),
        ],
        ..Default::default()
    };
    let cancel = CancelToken::new();
    match session.deps().processes.run_capture(&request, &cancel) {
        Ok(output) if output.exit_code != 0 => {
            let mut message = String::from("schtasks.exe error");
            let stderr = output.stderr.trim();
            let filtered = stderr
                .strip_prefix("ERROR:")
                .map(str::trim_start)
                .unwrap_or(stderr);
            if !filtered.is_empty() {
                message.push_str(": ");
                message.push_str(filtered);
            }
            session.log(LogLevel::Warn, message);
        }
        Ok(_) => {}
        Err(e) => session.log(LogLevel::Error, e.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A current-settings value with distinctive non-default fields to diff
    /// against.
    fn current() -> AppSettings {
        AppSettings {
            language: "en".to_owned(),
            theme: DEFAULT_THEME.to_owned(),
            disable_update_check: false,
            disable_run_ui_scheduled_task: Some(false),
            dev_mode_opt_out: false,
            hide_tray_icon: false,
            always_compile_mods_locally: false,
            dont_auto_show_toolkit: false,
            mod_tasks_dialog_delay: 2000,
            safe_mode: false,
            logging_verbosity: 0,
            engine: EngineSettings {
                logging_verbosity: 0,
                include: vec!["a.exe".to_owned()],
                exclude: vec![],
                inject_into_critical_processes: false,
                inject_into_incompatible_programs: false,
                inject_into_games: false,
            },
        }
    }

    /// A patch carrying every current value verbatim (the no-op re-import).
    fn identical_patch() -> AppSettingsPatch {
        let cur = current();
        AppSettingsPatch {
            language: Some(cur.language.clone()),
            theme: None,
            disable_update_check: Some(cur.disable_update_check),
            disable_run_ui_scheduled_task: None,
            dev_mode_opt_out: Some(cur.dev_mode_opt_out),
            hide_tray_icon: Some(cur.hide_tray_icon),
            always_compile_mods_locally: Some(cur.always_compile_mods_locally),
            dont_auto_show_toolkit: Some(cur.dont_auto_show_toolkit),
            mod_tasks_dialog_delay: Some(cur.mod_tasks_dialog_delay),
            safe_mode: None,
            logging_verbosity: Some(cur.logging_verbosity),
            engine: Some(EngineSettingsPatch {
                logging_verbosity: Some(cur.engine.logging_verbosity),
                include: Some(cur.engine.include.clone()),
                exclude: Some(cur.engine.exclude.clone()),
                inject_into_critical_processes: Some(cur.engine.inject_into_critical_processes),
                inject_into_incompatible_programs: Some(
                    cur.engine.inject_into_incompatible_programs,
                ),
                inject_into_games: Some(cur.engine.inject_into_games),
            }),
        }
    }

    #[test]
    fn an_identical_patch_diffs_to_nothing() {
        // The no-op case: a full-allowlist patch equal to the current settings
        // changes nothing, so its change-based intents are all false.
        let subset = changed_subset(&current(), &identical_patch());
        assert!(!intents(&subset).requires_restart);
        assert!(!intents(&subset).requires_notify);
        assert!(
            subset.engine.is_none(),
            "an all-equal engine group collapses"
        );
        assert!(subset.language.is_none());
    }

    #[test]
    fn only_differing_fields_survive_the_diff() {
        let mut patch = identical_patch();
        patch.logging_verbosity = Some(2); // differs (restart-class)
        patch.language = Some("de".to_owned()); // differs (notify-class)
        if let Some(engine) = &mut patch.engine {
            engine.include = Some(vec!["b.exe".to_owned()]); // differs (restart-class)
        }

        let subset = changed_subset(&current(), &patch);
        assert!(intents(&subset).requires_restart);
        assert!(intents(&subset).requires_notify);
        assert_eq!(subset.logging_verbosity, Some(2));
        assert_eq!(subset.language.as_deref(), Some("de"));
        let engine = subset.engine.expect("the differing engine field survives");
        assert_eq!(engine.include, Some(vec!["b.exe".to_owned()]));
        // Equal fields inside the surviving engine group are still dropped.
        assert!(engine.logging_verbosity.is_none());
    }

    #[test]
    fn the_scheduled_task_field_diffs_against_the_mode_dependent_current() {
        // Equal to the current bool: dropped. Differing, or present while the
        // portable mode reports None: kept (counts as a change).
        let mut patch = identical_patch();
        patch.disable_run_ui_scheduled_task = Some(false);
        assert!(
            changed_subset(&current(), &patch)
                .disable_run_ui_scheduled_task
                .is_none()
        );
        patch.disable_run_ui_scheduled_task = Some(true);
        assert_eq!(
            changed_subset(&current(), &patch).disable_run_ui_scheduled_task,
            Some(true)
        );
        let mut portable = current();
        portable.disable_run_ui_scheduled_task = None;
        patch.disable_run_ui_scheduled_task = Some(false);
        assert_eq!(
            changed_subset(&portable, &patch).disable_run_ui_scheduled_task,
            Some(false)
        );
    }
}
