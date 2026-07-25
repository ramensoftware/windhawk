//! DTOs of the `parseModSource` command, mirroring `ParsedModSource`,
//! `ModMetadata`, and the `InitialSettings` family in `windhawk-vscode`'s
//! `src/coreClient/contract.ts`, and `src/services/types.ts` of the TypeScript
//! implementation they replace.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Params of `parseModSource`. Unknown fields are tolerated (additive
/// evolution).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ParseModSourceParams {
    pub source: String,
    pub language: String,
}

/// Params of `appendToModIdAndName` (the new-mod / fork source transform):
/// the suffixes to append to the `@id` and `@name[:lang]` metadata lines, each
/// optional (absent or empty = leave that field alone).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppendToModIdAndNameParams {
    pub source: String,
    #[serde(default)]
    pub append_to_id: Option<String>,
    #[serde(default)]
    pub append_to_name: Option<String>,
}

/// `ModMetadata` of `src/services/types.ts`: every field optional; absent
/// fields are omitted from the JSON, exactly like the TS object.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub twitter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_options: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub donate_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<Vec<String>>,
}

/// `InitialSettingsValue` of `src/services/types.ts`. (dropped the `Null`
/// variant: its sole producer, `domain::SettingValue::Null`, was never
/// constructed - validation rejects null/float leaves upstream - so no fixture
/// this DTO serializes carries a `null`.)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum InitialSettingsValue {
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    NumberArray(Vec<serde_json::Number>),
    StringArray(Vec<String>),
    Settings(InitialSettings),
    SettingsArray(Vec<InitialSettings>),
}

/// `InitialSettingItem` of `src/services/types.ts`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitialSettingItem {
    pub key: String,
    pub value: InitialSettingsValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Display options: one single-entry `{value: label}` object per option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<BTreeMap<String, String>>>,
}

pub type InitialSettings = Vec<InitialSettingItem>;

/// Per-section error strings of `ParsedModSource`; entries are set only for
/// sections that failed to parse (TS: properties assigned only on error).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParsedModSourceErrors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_settings: Option<String>,
}

/// Result of `parseModSource`: the three sections parse independently; a
/// failed or absent section is `null` (never omitted - the TS object always
/// carries all four properties).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedModSource {
    pub metadata: Option<ModMetadata>,
    pub readme: Option<String>,
    pub initial_settings: Option<InitialSettings>,
    pub errors: ParsedModSourceErrors,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_mod_source_serializes_nulls_and_omits_absent_errors() {
        let parsed = ParsedModSource {
            metadata: None,
            readme: None,
            initial_settings: None,
            errors: ParsedModSourceErrors {
                metadata: Some("err".into()),
                ..Default::default()
            },
        };
        let v = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "metadata": null,
                "readme": null,
                "initialSettings": null,
                "errors": {"metadata": "err"}
            })
        );
    }

    #[test]
    fn settings_value_distinguishes_variants() {
        let v: InitialSettingsValue = serde_json::from_str("1").unwrap();
        assert_eq!(v, InitialSettingsValue::Number(1.into()));

        let v: InitialSettingsValue = serde_json::from_str(r#"["a", "b"]"#).unwrap();
        assert_eq!(
            v,
            InitialSettingsValue::StringArray(vec!["a".into(), "b".into()])
        );

        let nested = r#"[{"key": "inner", "value": true}]"#;
        let v: InitialSettingsValue = serde_json::from_str(nested).unwrap();
        match v {
            InitialSettingsValue::Settings(items) => {
                assert_eq!(items[0].key, "inner");
                assert_eq!(items[0].value, InitialSettingsValue::Bool(true));
            }
            other => panic!("expected settings, got {other:?}"),
        }
    }

    #[test]
    fn metadata_round_trips_camel_case() {
        let m = ModMetadata {
            id: Some("test-mod".into()),
            compiler_options: Some("-lcomctl32".into()),
            donate_url: Some("https://example.com".into()),
            ..Default::default()
        };
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "id": "test-mod",
                "compilerOptions": "-lcomctl32",
                "donateUrl": "https://example.com"
            })
        );
    }
}
