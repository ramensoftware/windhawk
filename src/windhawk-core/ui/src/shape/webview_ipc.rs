//! Typed serde mirrors of the webview IPC message shapes, and the contract version
//! this host implements.
//!
//! The webview IPC contract is single-sourced in the @windhawk/webview-ipc-contract
//! package (the TypeScript front-end and the VSCode extension import it directly).
//! This module is the Rust mirror, serving two roles:
//!
//!  1. Drift detection: the round-trip test below proves every struct matches the
//!     package's shared fixture corpus (windhawk-webview-ipc-contract/fixtures), which
//!     the package also type-checks against the TypeScript contract - so one corpus
//!     ties all three languages together.
//!  2. Construction: the shapes the host BUILDS itself are emitted through these
//!     structs so the wire cannot drift from the contract - the settings/handshake
//!     projection (`getInitialAppSettings`, `setNewAppSettings`), the launch, installer,
//!     and write replies, the `updateInstalledModsDetails` projection, and the wrapped
//!     source/versions/featured/settings envelopes (whose forwarded leaves stay `Value`).
//!     A struct no constructor has adopted yet is `#[allow(dead_code)]`: the fixture
//!     round-trip test is its only user until one does.
//!
//! The pass-through replies - `getAppSettings`, `getInstalledMods`, `getModConfig`,
//! and the like, which forward a core value verbatim so a field the core adds is never
//! dropped - keep emitting untyped Values on purpose. Where such a reply has a struct
//! here (today only `getModConfig`), it exists solely to pin the shape in the fixture
//! test, never to re-serialize the core's output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use windhawk_core_protocol::{ModConfig, SourceLocation};

/// The webview IPC contract version this host implements, stamped into the
/// `getInitialAppSettings` reply and asserted by the webview on the handshake. Kept in
/// lockstep with `contract-version.json` in the @windhawk/webview-ipc-contract package
/// (the round-trip test asserts equality); that JSON is the cross-language canonical
/// value. This is distinct from the core (DLL) contract version in
/// `windhawk_core_protocol::CONTRACT_VERSION`, a different boundary.
pub const WEBVIEW_IPC_CONTRACT_VERSION: &str = "1.2.0";

/// Serialize a contract-mirror struct to its wire `Value`. The mirror structs are
/// plain data - named fields over `String`/`bool`/`i64`/`Value`/`BTreeMap` - so
/// `serde_json::to_value` cannot fail; this centralizes that infallible conversion so
/// the emission sites read as one call and the "this cannot fail" reasoning has a
/// single home.
pub fn to_wire<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("webview IPC mirror struct serializes")
}

/// `appUISettings`: the shell-facing settings subset the front-end's app-level
/// indicators read. Modelled as the union across hosts: `theme` is optional because
/// only the Tauri host carries it (the native shell persists the UI theme), while the
/// VSCode host omits it. The native projection always fills it.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppUiSettings {
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub dev_mode_opt_out: bool,
    pub logging_enabled: bool,
    pub update_is_available: bool,
    pub update_is_available_bleeding_edge: bool,
    pub safe_mode: bool,
}

/// `getInitialAppSettings` reply: the bootstrap handshake. `contractVersion` lets the
/// webview assert host/front-end agreement; `appUISettings` is a `Partial` (the
/// front-end tolerates a missing field), which the native host fills completely.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetInitialAppSettingsReply {
    pub contract_version: String,
    #[serde(rename = "appUISettings")]
    pub app_ui_settings: AppUiSettings,
}

/// `setNewAppSettings` event: the same projection the handshake carries, pushed on an
/// app-settings change so the front-end's indicators refresh.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetNewAppSettings {
    // Explicit rename: camelCase would lowercase the `UI` acronym to `appUiSettings`.
    // `rename_all` is still carried so a future field lands on the wire as camelCase.
    #[serde(rename = "appUISettings")]
    pub app_ui_settings: AppUiSettings,
}

/// `updateDownloadProgress` / `devToolsInstallDownloadProgress` event payload.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    /// 0-100.
    pub progress: u32,
}

/// The wire error object a reply carries on failure: a stable `code`, a human
/// `message`, an optional failing-resource `path`, and an optional origin `location`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WireErrorDto {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
}

/// `createNewMod` / `editMod` / `forkMod` reply: an empty object on success,
/// `{ uiMissing: true }` when the dev tools are absent, or `{ error }` on failure.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DevActionReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_missing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireErrorDto>,
}

/// `getModConfig` reply: the mod's config, or `null` when the mod has none. `config`
/// is always present on the wire (never skipped), matching `ModConfig | null`.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetModConfigReply {
    pub mod_id: String,
    pub config: Option<ModConfig>,
}

/// The installer-terminal reply, shared by `startUpdate` / `startInstallDevTools`
/// (their async terminal AND sync start-failure paths) and by the `cancelUpdate` /
/// `cancelInstallDevTools` replies (which never carry an error). NB the `error` is
/// the failure MESSAGE STRING - the installer terminals' convention - NOT a
/// `WireErrorDto` object (see the module doc's two error conventions). Skipped when
/// absent, so a success is the bare `{ succeeded: true }`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallerReply {
    pub succeeded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `updateInstalledModsDetails` event: the profile watcher's re-derived per-mod
/// update-availability + rating subset, keyed by mod id.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstalledModsDetails {
    pub details: BTreeMap<String, InstalledModDetailEntry>,
}

/// One `updateInstalledModsDetails` entry. `userRating` is `i64` - the type end to
/// end (`windhawk_core_protocol::InstalledModListEntry.user_rating`, the value the
/// event projects from), serialized as a JSON integer, so it round-trips losslessly;
/// `f64` would break the exact round-trip (`3` vs `3.0`).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModDetailEntry {
    pub update_available: bool,
    pub user_rating: i64,
}

/// The write replies that echo only `{ modId, succeeded }`: `deleteMod`,
/// `setModSettings`, `updateModConfig`. On failure the error object is ATTACHED to
/// the serialized reply (see `commands::mods::finish_write`), not modelled here, so
/// `reply::error_object` stays its single owner - the DTO guards the base shape, the
/// attached object guards the error. `Default` lets a call site build the echo fields
/// and leave `succeeded` for `finish_write` to stamp (it owns that flag).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct WriteReply {
    pub mod_id: String,
    pub succeeded: bool,
}

/// `enableMod` reply: `{ modId, enabled, succeeded }`. `enabled` echoes the requested
/// state regardless of `succeeded` (matching the extension). The failure error is
/// attached, not modelled here (see [`WriteReply`]). `Default` lets the call site
/// build the echo fields and leave `succeeded` for `finish_write` to stamp.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnableModReply {
    pub mod_id: String,
    pub enabled: bool,
    pub succeeded: bool,
}

/// `updateModRating` reply: `{ modId, rating, succeeded }`. `rating` echoes the
/// requested value (`i64`, matching `windhawk_core_protocol::SetModRatingParams.rating`)
/// regardless of `succeeded`. The failure error is attached, not modelled here (see
/// [`WriteReply`]). `Default` lets the call site build the echo fields and leave
/// `succeeded` for `finish_write` to stamp.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModRatingReply {
    pub mod_id: String,
    pub rating: i64,
    pub succeeded: bool,
}

// --- Wrapped envelopes ---------------------------------------------------------
//
// The structs below type only the host-built ENVELOPE (key names, presence, the
// null-on-failure shape); the substantive leaves stay `serde_json::Value` because
// they are either a value forwarded verbatim from `session.invoke(...)` (re-serializing
// through a DTO could drop a field the core adds and the front-end tolerates) or a
// `Partial<...>` the front-end sent and the host echoes back (a full-struct DTO would
// reject it or fill defaults it never sent). This is a thinner guard than a fully
// constructed struct.

/// `updateAppSettings` reply envelope: `{ appSettings, succeeded }`. `appSettings`
/// echoes the front-end's `Partial<AppSettings>` patch verbatim, so it stays a
/// `Value`. The failure error is ATTACHED (see `commands::app`), not modelled here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppSettingsReply {
    pub app_settings: Value,
    pub succeeded: bool,
}

/// `setNewModConfig` event: `{ modId, config }`. `config` echoes the front-end's
/// `Partial<ModConfig>` patch verbatim, so it stays a `Value`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetNewModConfig {
    pub mod_id: String,
    pub config: Value,
}

/// The inner `data` object both source-data replies carry: `{ source, metadata,
/// readme, initialSettings }`. `metadata`/`readme`/`initialSettings` are forwarded
/// from the parse, kept as `Value`; every field is present, null when absent (never
/// omitted).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceData {
    pub source: Value,
    pub metadata: Value,
    pub readme: Value,
    pub initial_settings: Value,
}

/// `getModSourceData` reply: `{ modId, data }`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetModSourceDataReply {
    pub mod_id: String,
    pub data: SourceData,
}

/// `getRepositoryModSourceData` reply: `{ modId, version?, data }`. `version` is
/// echoed only when the request carried one.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetRepositoryModSourceDataReply {
    pub mod_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub data: SourceData,
}

/// `getModVersions` reply: `{ modId, versions }`. `versions` is the core version
/// list, forwarded as a `Value` (an empty list on failure).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetModVersionsReply {
    pub mod_id: String,
    pub versions: Value,
}

/// `getFeaturedMods` reply: `{ featuredMods }`. The projected featured subset, or
/// `null` on failure/empty. A thin guard: the envelope key and the null-on-failure
/// shape, not the entries.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetFeaturedModsReply {
    pub featured_mods: Value,
}

/// `getModSettings` reply: `{ modId, settings }`. `settings` is the core runtime
/// settings map, forwarded as a `Value`. The failure error is attached (see
/// `commands::mods`), not modelled here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetModSettingsReply {
    pub mod_id: String,
    pub settings: Value,
}

/// `exportUserData` reply: `{ succeeded, summary?, canceled? }`. `summary` is the
/// core's export summary, forwarded verbatim as a `Value`; `canceled` marks a
/// dismissed Save dialog (a benign no-op). The failure error is ATTACHED out-of-band
/// (see `commands::userdata`), not modelled here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExportUserDataReply {
    pub succeeded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canceled: Option<bool>,
}

/// `inspectUserData` reply: `{ succeeded, manifest?, archive?, canceled? }`. `manifest`
/// is the core manifest, forwarded as a `Value`; `archive` echoes the read bytes so a
/// subsequent import needs no second file read; `canceled` marks a dismissed Open
/// dialog. The failure error is ATTACHED out-of-band, not modelled here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InspectUserDataReply {
    pub succeeded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canceled: Option<bool>,
}

/// `importUserData` reply (the async terminal): `{ succeeded, summary? }`. `summary`
/// is the core's import summary (per-mod outcomes + app-settings intents), forwarded
/// as a `Value`. The failure error is ATTACHED out-of-band (the pump's terminal
/// handler), not modelled here.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportUserDataReply {
    pub succeeded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value>,
}

/// `importUserDataProgress` event, in either of two shapes on one channel: a per-mod
/// marker (`status` set - the `installing` start and a terminal
/// `installed`/`skipped`/`failed`) or a forwarded install sub-event (`compileTarget`
/// set - a local compile's target as the user-facing arch label the host maps the
/// clang triple to), each with the mod `{ modId, index, total }` position; or the
/// app-settings step marker (`item` = `appSettings`, `status` = `applying`/`applied`),
/// which carries no mod position. The host forwards the core payload verbatim apart
/// from the `compileTarget` label mapping, so this struct only pins the shape in the
/// fixture test (like [`GetModConfigReply`]). `item` is the union discriminant, present
/// on every event; the mod position fields are optional so the one struct round-trips
/// both the mod markers and the position-less app-settings marker.
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgressEvent {
    pub item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_target: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};

    // The @windhawk/webview-ipc-contract package, a sibling of windhawk-core.
    fn contract_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../windhawk-webview-ipc-contract")
    }

    // Deserialize a fixture's `data` into its DTO, re-serialize, and assert the result
    // is byte-identical (lossless: no field dropped, none invented); then confirm an
    // extra unknown field still deserializes (additive contract evolution).
    fn round_trip<T: DeserializeOwned + Serialize>(command: &str, file: &str, data: &Value) {
        let value: T = serde_json::from_value(data.clone())
            .unwrap_or_else(|e| panic!("{command}/{file}: deserialize into the DTO: {e}"));
        let back = serde_json::to_value(&value)
            .unwrap_or_else(|e| panic!("{command}/{file}: re-serialize: {e}"));
        assert_eq!(
            &back, data,
            "{command}/{file}: DTO round-trip must be lossless"
        );

        let mut with_extra = data.clone();
        if let Value::Object(map) = &mut with_extra {
            map.insert("someFutureField".to_owned(), Value::Bool(true));
            serde_json::from_value::<T>(with_extra)
                .unwrap_or_else(|e| panic!("{command}/{file}: must tolerate unknown fields: {e}"));
        }
    }

    // Guard a reply whose FAILURE payload carries an `error` object the base DTO does
    // not model - it is attached out-of-band by `finish_write` / `reply::attach_error`,
    // keeping `reply::error_object` the error's single owner. Split the fixture the way
    // the runtime does: the base DTO round-trips the payload with `error` removed, and
    // `WireErrorDto` round-trips the `error` object itself. A success fixture (no
    // `error` key) just round-trips the base, so both scenarios route through here.
    fn round_trip_with_attached_error<T: DeserializeOwned + Serialize>(
        command: &str,
        file: &str,
        data: &Value,
    ) {
        let mut base = data.clone();
        let error = base.as_object_mut().and_then(|map| map.remove("error"));
        round_trip::<T>(command, file, &base);
        if let Some(error) = error {
            round_trip::<WireErrorDto>(command, file, &error);
        }
    }

    // Route a fixture to the DTO the host would emit for that command. A command with
    // no arm is an error, so a new host-constructed message forces a mapping here.
    fn check(command: &str, file: &str, data: &Value) {
        match command {
            "getInitialAppSettings" => {
                round_trip::<GetInitialAppSettingsReply>(command, file, data)
            }
            "setNewAppSettings" => round_trip::<SetNewAppSettings>(command, file, data),
            "updateDownloadProgress" | "devToolsInstallDownloadProgress" => {
                round_trip::<ProgressEvent>(command, file, data)
            }
            "createNewMod" | "editMod" | "forkMod" => {
                round_trip::<DevActionReply>(command, file, data)
            }
            "getModConfig" => round_trip::<GetModConfigReply>(command, file, data),
            "startUpdate" | "startInstallDevTools" | "cancelUpdate" | "cancelInstallDevTools" => {
                round_trip::<InstallerReply>(command, file, data)
            }
            "updateInstalledModsDetails" => {
                round_trip::<UpdateInstalledModsDetails>(command, file, data)
            }
            // The write/settings family attaches its failure `error` out-of-band, so it
            // routes through the split guard (base DTO + WireErrorDto).
            "enableMod" => round_trip_with_attached_error::<EnableModReply>(command, file, data),
            "updateModRating" => {
                round_trip_with_attached_error::<UpdateModRatingReply>(command, file, data)
            }
            "deleteMod" | "setModSettings" | "updateModConfig" => {
                round_trip_with_attached_error::<WriteReply>(command, file, data)
            }
            "updateAppSettings" => {
                round_trip_with_attached_error::<UpdateAppSettingsReply>(command, file, data)
            }
            "setNewModConfig" => round_trip::<SetNewModConfig>(command, file, data),
            "getModSourceData" => round_trip::<GetModSourceDataReply>(command, file, data),
            "getRepositoryModSourceData" => {
                round_trip::<GetRepositoryModSourceDataReply>(command, file, data)
            }
            "getModVersions" => round_trip::<GetModVersionsReply>(command, file, data),
            "getFeaturedMods" => round_trip::<GetFeaturedModsReply>(command, file, data),
            "getModSettings" => {
                round_trip_with_attached_error::<GetModSettingsReply>(command, file, data)
            }
            // User-data export/import: export/inspect attach their failure `error`
            // out-of-band (the split guard); the import terminal likewise (the pump
            // attaches it); `cancelImportUserData` reuses the installer `{ succeeded }`
            // reply; the progress event forwards the core payload verbatim.
            "exportUserData" => {
                round_trip_with_attached_error::<ExportUserDataReply>(command, file, data)
            }
            "inspectUserData" => {
                round_trip_with_attached_error::<InspectUserDataReply>(command, file, data)
            }
            "importUserData" => {
                round_trip_with_attached_error::<ImportUserDataReply>(command, file, data)
            }
            "cancelImportUserData" => round_trip::<InstallerReply>(command, file, data),
            "importUserDataProgress" => round_trip::<ImportProgressEvent>(command, file, data),
            other => panic!("{other}/{file}: no DTO mapping for this command"),
        }
    }

    // Every host-constructed command whose emission this module guards. A command with
    // a `check(...)` arm but no fixture is silently never exercised, so this list is the
    // reverse guard: each entry must have at least one fixture in the corpus. Adding a
    // guarded command means adding it here AND shipping a fixture - the two move
    // together.
    const REQUIRED_COMMANDS: &[&str] = &[
        "getInitialAppSettings",
        "setNewAppSettings",
        "updateDownloadProgress",
        "createNewMod",
        "getModConfig",
        "startUpdate",
        "updateInstalledModsDetails",
        "enableMod",
        "updateModRating",
        "deleteMod",
        "setModSettings",
        "updateModConfig",
        "updateAppSettings",
        "setNewModConfig",
        "getModSourceData",
        "getRepositoryModSourceData",
        "getModVersions",
        "getFeaturedMods",
        "getModSettings",
        "exportUserData",
        "inspectUserData",
        "importUserData",
        "cancelImportUserData",
        "importUserDataProgress",
    ];

    #[test]
    fn every_fixture_round_trips_through_its_dto() {
        let dir = contract_dir().join("fixtures");
        let mut count = 0;
        let mut seen = std::collections::BTreeSet::new();
        let entries = fs::read_dir(&dir).unwrap_or_else(|e| {
            panic!(
                "read the webview IPC fixtures at {}: {e}. This test needs the sibling \
                 windhawk-webview-ipc-contract package checked out next to windhawk-core.",
                dir.display()
            )
        });
        for command_entry in entries {
            let command_dir = command_entry.expect("fixtures dir entry").path();
            if !command_dir.is_dir() {
                continue;
            }
            let command = command_dir
                .file_name()
                .and_then(|n| n.to_str())
                .expect("command dir name")
                .to_owned();
            for file_entry in fs::read_dir(&command_dir).expect("read a command dir") {
                let path = file_entry.expect("command dir entry").path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let file = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("fixture file name")
                    .to_owned();
                let fixture: Value =
                    serde_json::from_str(&fs::read_to_string(&path).expect("read a fixture"))
                        .expect("parse a fixture");
                assert_eq!(
                    fixture.get("command").and_then(Value::as_str),
                    Some(command.as_str()),
                    "{command}/{file}: the fixture's command field must match its directory"
                );
                let data = fixture.get("data").expect("fixture has a data field");
                check(&command, &file, data);
                seen.insert(command.clone());
                count += 1;
            }
        }
        assert!(
            count >= 7,
            "expected to exercise the fixture corpus, saw {count}"
        );
        for required in REQUIRED_COMMANDS {
            assert!(
                seen.contains(*required),
                "no fixture exercises the guarded command '{required}'; every command with \
                 a check() arm must ship at least one fixture (they move together)"
            );
        }
    }

    #[test]
    fn contract_version_matches_the_shared_json() {
        let version_path = contract_dir().join("contract-version.json");
        let contents = fs::read_to_string(&version_path).unwrap_or_else(|e| {
            panic!(
                "read {}: {e}. This test needs the sibling windhawk-webview-ipc-contract \
                 package checked out next to windhawk-core.",
                version_path.display()
            )
        });
        let json: Value = serde_json::from_str(&contents).expect("parse contract-version.json");
        assert_eq!(
            json.get("version").and_then(Value::as_str),
            Some(WEBVIEW_IPC_CONTRACT_VERSION),
            "the Rust WEBVIEW_IPC_CONTRACT_VERSION must match the package's contract-version.json"
        );
    }
}
