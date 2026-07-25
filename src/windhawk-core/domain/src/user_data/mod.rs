//! The user-data export/import archive: the on-disk byte format (a single
//! pretty-printed UTF-8 JSON document), its in-memory model, serialization,
//! deserialization, validation, and the manifest projection the import UI reads.
//!
//! Pure data and pure logic - no I/O, no session. The effectful export/import
//! transaction (aggregating reads, orchestrating installs) lives in
//! `core::services::user_data`; this module is the single owner of the bytes,
//! the way `profile` owns the user-profile document. This model IS the byte
//! format.

use serde::{Deserialize, Serialize};

mod deserialize;
mod manifest;
mod serialize;
mod validate;

pub use deserialize::{MAX_ARCHIVE_BYTES, deserialize};
pub use manifest::{ArchiveManifest, ManifestMod, manifest};
pub use serialize::serialize;
pub use validate::validate;

/// The `format` tag every archive carries. It encodes the archive version, so a
/// document with any other value - an older or newer format, or not an archive at
/// all - is rejected before anything is read (`validate`).
pub const FORMAT_TAG: &str = "windhawk-user-data-v1";

/// A deserialization or validation failure. A thin message newtype (like
/// `MetadataError` / `SettingsParseError`), read via `Display`: every archive
/// failure maps to one protocol `INVALID_REQUEST` at the command edge, so no
/// per-failure taxonomy is warranted here.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct ArchiveError(String);

impl ArchiveError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// The whole archive document. Serializes to the exact on-disk bytes
/// (`serde_json` pretty, 2-space, matching `JSON.stringify(x, null, 2)`); the
/// field declaration order here IS the on-disk field order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDataArchive {
    /// Always [`FORMAT_TAG`]. Defaulted on decode so a document that omits it
    /// fails the format check with a clear message rather than a raw decode
    /// error.
    #[serde(default)]
    pub format: String,
    /// The exported app settings (an allowlist object), present only when app
    /// settings were selected. Opaque here; `core` projects it through the
    /// allowlist on export and decodes it as an `AppSettingsPatch` on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_settings: Option<serde_json::Value>,
    /// The exported mods, in installed-id order. Always present; may be empty
    /// (an app-settings-only archive carries `[]`).
    pub mods: Vec<ArchiveMod>,
}

/// One exported mod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMod {
    /// The persisted storage id: a bare repository id or a `local@` id. The
    /// identity used for install, collision checks, and import selection.
    pub mod_id: String,
    /// Whether `mod_id` is a `local@` id. Redundant with the prefix but
    /// explicit, so a consumer need not re-derive the rule; a mismatch is a
    /// validation error. Omitted when `false` (a repository mod) to keep the
    /// document lean, and defaulted back to `false` on decode.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_local: bool,
    /// The installed version at export time. Load-bearing for a reference-only
    /// repository mod (the version an import fetches); informational for a local
    /// mod.
    pub version: String,
    /// Display name from the source metadata, so `inspect` can label a mod
    /// without parsing every embedded source. Never authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The verbatim `.wh.cpp` source. Always present for a local mod (it exists
    /// nowhere else); for a repository mod, present only under an offline export
    /// (absent under the reference-only default, where import fetches it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The runtime settings (a flat key->value map, canonicalized to declared
    /// types at export), present only when settings were selected for this mod
    /// AND the resolved map is non-empty. An empty map carries nothing, so the
    /// exporter drops it rather than emitting `"settings": {}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
    /// The user-owned config fields, present only when config was selected for
    /// this mod.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ArchiveModConfig>,
}

/// The user-owned subset of a mod's config, and ONLY that subset. The five
/// install-owned fields (`libraryFileName`/`include`/`exclude`/`architecture`/
/// `version`) are recomputed on every install and are never carried, so a
/// restore cannot clobber the values install computes. Each field is omitted
/// when it holds its default (`false` / empty), keeping the document lean; an
/// all-default config serializes to `{}` (and the exporter drops it entirely).
/// A field absent on decode takes that same default (via the container
/// `#[serde(default)]`), so the omit-when-default round-trip is exact.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchiveModConfig {
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub logging_enabled: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub debug_logging_enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include_custom: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_custom: Vec<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub include_exclude_custom_only: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub patterns_match_critical_system_processes: bool,
}

/// A serde `skip_serializing_if` predicate: a `false` field is the default, so it
/// is omitted from the archive (the field defaults back to `false` on decode).
fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative archive touching every optional field: app settings, a
    /// reference-only repository mod (no `source`), a canonicalized `settings`
    /// map, and a full `config`.
    fn sample() -> UserDataArchive {
        UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: Some(serde_json::json!({
                "language": "en",
                "disableUpdateCheck": false
            })),
            mods: vec![ArchiveMod {
                mod_id: "taskbar-clock".to_owned(),
                is_local: false,
                version: "1.2.0".to_owned(),
                name: Some("Taskbar Clock".to_owned()),
                source: None,
                settings: Some(serde_json::json!({
                    "ShowSeconds": 1,
                    "TopMost.enabled": 0,
                    "Formats[0].value": "HH:mm"
                })),
                config: Some(ArchiveModConfig {
                    disabled: false,
                    logging_enabled: false,
                    debug_logging_enabled: false,
                    include_custom: vec!["myapp.exe".to_owned()],
                    exclude_custom: vec![],
                    include_exclude_custom_only: false,
                    patterns_match_critical_system_processes: false,
                }),
            }],
        }
    }

    #[test]
    fn serialize_then_deserialize_is_identity() {
        let archive = sample();
        let bytes = serialize(&archive);
        let round_tripped = deserialize(&bytes).expect("the sample archive is valid");
        assert_eq!(round_tripped, archive);
    }

    #[test]
    fn pretty_output_matches_the_fixed_serialization() {
        // The exact on-disk bytes: 2-space pretty, struct field order,
        // `isLocal` omitted (a reference-only repository mod, so `false`), config
        // carrying only its non-default fields (`includeCustom`), `source` omitted
        // (reference-only), empty array inline, keys in insertion order.
        let expected = "{\n  \"format\": \"windhawk-user-data-v1\",\n  \"appSettings\": {\n    \"language\": \"en\",\n    \"disableUpdateCheck\": false\n  },\n  \"mods\": [\n    {\n      \"modId\": \"taskbar-clock\",\n      \"version\": \"1.2.0\",\n      \"name\": \"Taskbar Clock\",\n      \"settings\": {\n        \"ShowSeconds\": 1,\n        \"TopMost.enabled\": 0,\n        \"Formats[0].value\": \"HH:mm\"\n      },\n      \"config\": {\n        \"includeCustom\": [\n          \"myapp.exe\"\n        ]\n      }\n    }\n  ]\n}";
        assert_eq!(serialize(&sample()), expected);
    }

    #[test]
    fn an_app_settings_only_archive_serializes_with_an_empty_mods_array() {
        let archive = UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: None,
            mods: vec![],
        };
        assert_eq!(
            serialize(&archive),
            "{\n  \"format\": \"windhawk-user-data-v1\",\n  \"mods\": []\n}"
        );
    }

    #[test]
    fn a_local_mod_round_trips_with_its_embedded_source() {
        let archive = UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: None,
            mods: vec![ArchiveMod {
                mod_id: "local@my-mod".to_owned(),
                is_local: true,
                version: "0.1".to_owned(),
                name: None,
                source: Some("// ==WindhawkMod==\n// @id my-mod\n".to_owned()),
                settings: None,
                config: None,
            }],
        };
        let round_tripped = deserialize(&serialize(&archive)).unwrap();
        assert_eq!(round_tripped, archive);
    }
}
