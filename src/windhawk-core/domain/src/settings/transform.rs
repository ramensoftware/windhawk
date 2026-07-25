//! The YAML -> typed-tree transformer: `parseSettings` /
//! `parseSettingsValueAnnotated` / `parseSettingsValue` of the TS
//! implementation. Runs on schema-validated input (the `validate` pass ran
//! first), so the YAML shapes are known and this pass relies on that.

use yaml_rust2::Yaml;

use super::{parse_annotation_key, scalar_key_to_string};
use crate::language::best_language_match;
use crate::model::{SettingItem, SettingValue, SettingsParseError};

pub(super) fn parse_settings(
    items: &[Yaml],
    language: &str,
) -> Result<Vec<SettingItem>, SettingsParseError> {
    items
        .iter()
        .map(|item| parse_item_annotated(item, language))
        .collect()
}

fn parse_item_annotated(item: &Yaml, language: &str) -> Result<SettingItem, SettingsParseError> {
    let Yaml::Hash(map) = item else {
        // Schema validation guarantees a mapping.
        return Err(SettingsParseError::new("Missing settings key"));
    };

    let entries: Vec<(String, &Yaml)> = map
        .iter()
        .map(|(k, v)| (scalar_key_to_string(k), v))
        .collect();

    let actual: Vec<&(String, &Yaml)> = entries
        .iter()
        .filter(|(k, _)| !k.starts_with('$'))
        .collect();
    if actual.is_empty() {
        return Err(SettingsParseError::new("Missing settings key"));
    }
    if actual.len() > 1 {
        return Err(SettingsParseError::new("More than one settings key"));
    }
    let (actual_key, actual_value) = actual[0];

    // Group `$param[:lang]` annotations by base name, in first-seen order. The
    // `$base[:lang]` split is the shared `super::parse_annotation_key`.
    type Candidates<'a> = Vec<(Option<String>, &'a Yaml)>;
    let mut groups: Vec<(&str, Candidates)> = Vec::new();
    for (key, value) in &entries {
        let Some((base, lang)) = parse_annotation_key(key) else {
            continue;
        };
        let lang = lang.map(str::to_owned);
        match groups.iter_mut().find(|(b, _)| *b == base) {
            Some((_, candidates)) => candidates.push((lang, *value)),
            None => groups.push((base, vec![(lang, *value)])),
        }
    }

    let mut result = SettingItem {
        key: actual_key.clone(),
        value: parse_value(actual_value, language)?,
        name: None,
        description: None,
        options: None,
    };

    for (base, candidates) in groups {
        let chosen = *best_language_match(language, &candidates);
        match base {
            "name" => result.name = yaml_string(chosen),
            "description" => result.description = yaml_string(chosen),
            "options" => result.options = Some(options_pairs(chosen)),
            // Schema validation restricts annotations to the three above.
            _ => {}
        }
    }

    Ok(result)
}

fn yaml_string(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn options_pairs(value: &Yaml) -> Vec<(String, String)> {
    let Yaml::Array(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let Yaml::Hash(map) = item else {
                return None;
            };
            map.iter()
                .next()
                .map(|(k, v)| (scalar_key_to_string(k), yaml_string(v).unwrap_or_default()))
        })
        .collect()
}

fn parse_value(value: &Yaml, language: &str) -> Result<SettingValue, SettingsParseError> {
    match value {
        Yaml::Boolean(b) => Ok(SettingValue::Bool(*b)),
        Yaml::Integer(i) => Ok(SettingValue::Number((*i).into())),
        Yaml::String(s) => Ok(SettingValue::String(s.clone())),
        Yaml::Array(items) => parse_array_value(items, language),
        // Validation already rejected floats, out-of-range integers, and null
        // parameter values upstream, so no other YAML shape reaches here. The
        // arm cannot be deleted (the foreign yaml-rust2 `Yaml` enum forces an
        // exhaustive match), so it is an explicit Err rather than a silent dead
        // `Null` value (drops the unrepresentable `SettingValue::Null`).
        _ => Err(SettingsParseError::new("unsupported value type")),
    }
}

/// The kind of a settings-array's elements. The `validate` pass classifies by
/// WHOLE-array homogeneity and ENFORCES it; this transform-side classifier
/// reads only the FIRST element, which is sound ONLY because validation runs
/// first and guarantees the array is homogeneous. The two share this kind
/// CONCEPT, not the mechanism (validate's number test spans floats to reject
/// them; this one only ever sees the int32 a validated array carries).
enum ArrayKind {
    Numbers,
    Strings,
    SettingsArrays,
    Settings,
}

impl ArrayKind {
    fn of(items: &[Yaml]) -> ArrayKind {
        match items.first() {
            Some(Yaml::Integer(_)) => ArrayKind::Numbers,
            Some(Yaml::String(_)) => ArrayKind::Strings,
            Some(Yaml::Array(_)) => ArrayKind::SettingsArrays,
            _ => ArrayKind::Settings,
        }
    }
}

fn parse_array_value(items: &[Yaml], language: &str) -> Result<SettingValue, SettingsParseError> {
    // Classify by the first element via the shared `ArrayKind` (the schema has
    // already enforced homogeneity, and validated every number is int32); the
    // per-element conversions differ by kind, so each arm keeps its own map.
    match ArrayKind::of(items) {
        ArrayKind::Numbers => Ok(SettingValue::NumberArray(
            items
                .iter()
                .map(|v| match v {
                    Yaml::Integer(i) => Ok((*i).into()),
                    _ => Err(SettingsParseError::new("Missing settings key")),
                })
                .collect::<Result<_, _>>()?,
        )),
        ArrayKind::Strings => Ok(SettingValue::StringArray(
            items.iter().filter_map(yaml_string).collect(),
        )),
        ArrayKind::SettingsArrays => Ok(SettingValue::SettingsArray(
            items
                .iter()
                .map(|v| match v {
                    Yaml::Array(inner) => parse_settings(inner, language),
                    _ => Err(SettingsParseError::new("Missing settings key")),
                })
                .collect::<Result<_, _>>()?,
        )),
        ArrayKind::Settings => Ok(SettingValue::Settings(parse_settings(items, language)?)),
    }
}
