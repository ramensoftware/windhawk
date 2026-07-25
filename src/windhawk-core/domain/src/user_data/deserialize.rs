//! Deserialize (and validate) the archive bytes into the typed model. Enforces a
//! size bound before parsing so a hostile document cannot exhaust memory on the
//! pure `inspect`/`import` parse, decodes the JSON, then runs the semantic
//! checks of `validate`. The result is a fully-validated archive ready for the
//! manifest projection (`inspect`) or the import transaction.

use super::{ArchiveError, UserDataArchive, validate};

/// The largest archive this accepts, in bytes. A generous cap - an offline
/// export embeds every mod's source, so a large real archive is legitimate -
/// that still rejects a document too big to parse safely. A safety limit, not a
/// semantic one. This is the authoritative value; the wire contract mirrors it
/// as `windhawk_core_protocol::MAX_ARCHIVE_BYTES` for the consumers that cannot
/// name a domain type, and a core test pins the two together.
pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

/// Parse and validate `text` into an archive. Errors on an oversized input,
/// invalid JSON, a structural mismatch (e.g. a wrong-typed `config` field), or
/// any semantic-validation failure (`validate`).
pub fn deserialize(text: &str) -> Result<UserDataArchive, ArchiveError> {
    if text.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveError::new(format!(
            "archive is too large ({} bytes; the maximum is {MAX_ARCHIVE_BYTES})",
            text.len()
        )));
    }
    // Tolerate a UTF-8 BOM: an archive is meant to be hand-editable, and a
    // Windows editor may save one. Handled here so every consumer (CLI file or
    // stdin, both GUI hosts) accepts it, not just one front-end.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let archive: UserDataArchive = serde_json::from_str(text).map_err(|e| {
        ArchiveError::new(format!("archive is not a valid user-data document: {e}"))
    })?;
    validate::validate(&archive)?;
    Ok(archive)
}

#[cfg(test)]
mod tests {
    use super::super::{ArchiveModConfig, FORMAT_TAG, serialize};
    use super::*;

    #[test]
    fn oversized_input_is_rejected_before_parsing() {
        let text = "x".repeat(MAX_ARCHIVE_BYTES + 1);
        let err = deserialize(&text).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn a_utf8_bom_is_tolerated() {
        // A hand-edited archive saved by a Windows editor may carry a BOM; the
        // decode strips it rather than failing with a raw parse error.
        let text = "\u{feff}{\n  \"format\": \"windhawk-user-data-v1\",\n  \"mods\": []\n}";
        let archive = deserialize(text).unwrap();
        assert_eq!(archive.format, FORMAT_TAG);
    }

    #[test]
    fn invalid_json_is_rejected() {
        let err = deserialize("{not json").unwrap_err().to_string();
        assert!(err.contains("not a valid user-data document"), "{err}");
    }

    #[test]
    fn a_wrong_typed_config_field_fails_to_decode() {
        let text = r#"{
  "format": "windhawk-user-data-v1",
  "mods": [
    { "modId": "m", "isLocal": false, "version": "1",
      "config": { "disabled": "yes" } }
  ]
}"#;
        assert!(deserialize(text).is_err());
    }

    #[test]
    fn a_partial_config_decodes_missing_fields_at_their_defaults() {
        // A hand-edited archive may carry a subset of the seven config fields;
        // the container `#[serde(default)]` fills the rest with the ModConfig
        // default (false / empty), not a decode error.
        let text = r#"{
  "format": "windhawk-user-data-v1",
  "mods": [
    { "modId": "m", "isLocal": false, "version": "1",
      "config": { "disabled": true } }
  ]
}"#;
        let archive = deserialize(text).unwrap();
        let config = archive.mods[0].config.as_ref().unwrap();
        assert_eq!(
            config,
            &ArchiveModConfig {
                disabled: true,
                ..ArchiveModConfig::default()
            }
        );
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        // A newer producer may add fields an older consumer does not model; the
        // decode drops them rather than failing.
        let text = r#"{
  "format": "windhawk-user-data-v1",
  "futureTopLevelField": 42,
  "mods": [
    { "modId": "m", "isLocal": false, "version": "1", "futurePerMod": true }
  ]
}"#;
        let archive = deserialize(text).unwrap();
        assert_eq!(archive.mods.len(), 1);
    }

    #[test]
    fn round_trips_a_minimal_archive_through_serialize() {
        let text = r#"{
  "format": "windhawk-user-data-v1",
  "mods": []
}"#;
        let archive = deserialize(text).unwrap();
        assert_eq!(archive.format, FORMAT_TAG);
        // Re-serializing the decoded archive reproduces the canonical bytes.
        assert_eq!(serialize(&archive), text);
    }
}
