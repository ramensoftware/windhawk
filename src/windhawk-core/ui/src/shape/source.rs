//! Source-data merge: combine a mod source with the
//! metadata/readme/initialSettings extracted from it into the `data` object
//! shared by `getModSourceData` (stored source) and the
//! `getRepositoryModSourceData` composite (fetched source). The extension
//! represents a missing source (or a parse that produced nothing) inline as
//! `null` fields rather than as a failed reply, so [`source_data`] is total
//! over (source present?, parsed present?).

use serde_json::{Value, json};
use windhawk_core_protocol::ParsedModSource;

/// The inner `data` object both source-data replies carry: `{ source, metadata,
/// readme, initialSettings }`. With no source, every field is `null`; with a
/// source but no parse result (a parse that errored - the extension never reaches
/// this, but the one-reply invariant requires a reply anyway), only `source` is
/// present. Every field is explicit `null`, not omitted (the TS object always
/// carries all four).
pub fn source_data(source: Option<&str>, parsed: Option<&ParsedModSource>) -> Value {
    match (source, parsed) {
        (Some(source), Some(parsed)) => json!({
            "source": source,
            "metadata": parsed.metadata,
            "readme": parsed.readme,
            "initialSettings": parsed.initial_settings,
        }),
        (Some(source), None) => json!({
            "source": source,
            "metadata": Value::Null,
            "readme": Value::Null,
            "initialSettings": Value::Null,
        }),
        (None, _) => json!({
            "source": Value::Null,
            "metadata": Value::Null,
            "readme": Value::Null,
            "initialSettings": Value::Null,
        }),
    }
}

/// Shape the `getModSourceData` reply: `{ modId, data: <source_data> }`.
pub fn mod_source_data_reply(
    mod_id: &str,
    source: Option<&str>,
    parsed: Option<&ParsedModSource>,
) -> Value {
    json!({ "modId": mod_id, "data": source_data(source, parsed) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windhawk_core_protocol::{ModMetadata, ParsedModSourceErrors};

    fn parsed() -> ParsedModSource {
        ParsedModSource {
            metadata: Some(ModMetadata {
                id: Some("m".to_owned()),
                name: Some("M".to_owned()),
                ..Default::default()
            }),
            readme: Some("# M".to_owned()),
            initial_settings: None,
            errors: ParsedModSourceErrors::default(),
        }
    }

    #[test]
    fn merges_source_and_parsed_sections() {
        let reply = mod_source_data_reply("m", Some("// src"), Some(&parsed()));
        assert_eq!(reply["modId"], json!("m"));
        assert_eq!(reply["data"]["source"], json!("// src"));
        assert_eq!(reply["data"]["metadata"]["id"], json!("m"));
        assert_eq!(reply["data"]["readme"], json!("# M"));
        // An absent section is explicit null, not omitted.
        assert_eq!(reply["data"]["initialSettings"], Value::Null);
    }

    #[test]
    fn missing_source_yields_all_null_data() {
        let reply = mod_source_data_reply("m", None, None);
        assert_eq!(
            reply,
            json!({
                "modId": "m",
                "data": {
                    "source": null,
                    "metadata": null,
                    "readme": null,
                    "initialSettings": null,
                }
            })
        );
    }

    #[test]
    fn source_present_but_unparsed_keeps_source_only() {
        let reply = mod_source_data_reply("m", Some("// src"), None);
        assert_eq!(reply["data"]["source"], json!("// src"));
        assert_eq!(reply["data"]["metadata"], Value::Null);
    }
}
