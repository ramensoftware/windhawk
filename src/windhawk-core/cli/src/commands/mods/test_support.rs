//! Shared `render_tests` fixtures for the `mod`-group command modules. Homed in
//! one place so the 12-field `ModConfig` literal has a single owner (reused by
//! the field-table drift guard in `config.rs`) and is not re-spelled per
//! command file, which would reintroduce exactly the drift the guard catches.

use serde_json::json;
use windhawk_core_protocol::{ModConfig, ModMetadata};

/// A ModConfig with caller-chosen `disabled`; other fields are install
/// defaults. Built through serde so the 12-field struct need not be spelled out
/// per test. Shared with the field-table drift guard (`config_table_guard`), so
/// its 12-field literal has one home.
pub(super) fn config(disabled: bool) -> ModConfig {
    serde_json::from_value(json!({
        "libraryFileName": "happy-mod_1.2.3",
        "disabled": disabled,
        "loggingEnabled": true,
        "debugLoggingEnabled": false,
        "include": [],
        "exclude": [],
        "includeCustom": [],
        "excludeCustom": [],
        "includeExcludeCustomOnly": false,
        "patternsMatchCriticalSystemProcesses": false,
        "architecture": ["x86-64"],
        "version": "1.2.3",
    }))
    .unwrap()
}

pub(super) fn happy_metadata() -> ModMetadata {
    ModMetadata {
        id: Some("happy-mod".to_owned()),
        name: Some("Happy Mod".to_owned()),
        version: Some("1.2.3".to_owned()),
        author: Some("Tester".to_owned()),
        description: Some("A test mod.".to_owned()),
        architecture: Some(vec!["x86-64".to_owned()]),
        ..Default::default()
    }
}
