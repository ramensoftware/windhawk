//! The `appUISettings` subset: the slice of `AppSettings` the front-end's shell
//! consumes, mirroring the extension's `_getAppUISettings`. `devModeOptOut` is
//! passed in as a parameter rather than read from `settings`, so this shaper
//! stays a pure projection; the caller supplies the value it reports (the real
//! stored setting for the native build). The same shape backs both the
//! `getInitialAppSettings` reply and the `setNewAppSettings` event.

use serde_json::{Value, json};
use windhawk_core_protocol::AppSettings;

/// Project `AppSettings` (plus the resolved `devModeOptOut` and the update
/// availability) into the `AppUISettings` object. `loggingEnabled` mirrors the
/// extension: either the app or the engine logging verbosity being above zero.
pub fn app_ui_settings(
    settings: &AppSettings,
    dev_mode_opt_out: bool,
    update_is_available: bool,
    update_is_available_bleeding_edge: bool,
) -> Value {
    json!({
        "language": settings.language,
        "devModeOptOut": dev_mode_opt_out,
        "loggingEnabled": settings.logging_verbosity > 0 || settings.engine.logging_verbosity > 0,
        "updateIsAvailable": update_is_available,
        "updateIsAvailableBleedingEdge": update_is_available_bleeding_edge,
        "safeMode": settings.safe_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windhawk_core_protocol::EngineSettings;

    fn settings(app_verbosity: i64, engine_verbosity: i64) -> AppSettings {
        AppSettings {
            language: "de".to_owned(),
            disable_update_check: false,
            disable_run_ui_scheduled_task: None,
            dev_mode_opt_out: false,
            hide_tray_icon: false,
            always_compile_mods_locally: false,
            dont_auto_show_toolkit: false,
            mod_tasks_dialog_delay: 2000,
            safe_mode: true,
            logging_verbosity: app_verbosity,
            engine: EngineSettings {
                logging_verbosity: engine_verbosity,
                include: vec![],
                exclude: vec![],
                inject_into_critical_processes: false,
                inject_into_incompatible_programs: false,
                inject_into_games: false,
            },
        }
    }

    #[test]
    fn projects_the_handed_dev_mode_opt_out() {
        // The projection reports the devModeOptOut it is handed (here true),
        // independent of the stored setting (false): it is a pure function of its
        // parameter.
        let ui = app_ui_settings(&settings(0, 0), true, true, false);
        assert_eq!(
            ui,
            json!({
                "language": "de",
                "devModeOptOut": true,
                "loggingEnabled": false,
                "updateIsAvailable": true,
                "updateIsAvailableBleedingEdge": false,
                "safeMode": true,
            })
        );
    }

    #[test]
    fn logging_enabled_is_app_or_engine_verbosity_above_zero() {
        assert_eq!(
            app_ui_settings(&settings(1, 0), true, false, false)["loggingEnabled"],
            json!(true)
        );
        assert_eq!(
            app_ui_settings(&settings(0, 2), true, false, false)["loggingEnabled"],
            json!(true)
        );
        assert_eq!(
            app_ui_settings(&settings(0, 0), true, false, false)["loggingEnabled"],
            json!(false)
        );
    }
}
