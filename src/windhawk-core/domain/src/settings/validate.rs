//! Schema validation, mirroring the jsonschema document of the TS
//! implementation. Errors carry a short instance path description; the TS
//! jsonschema message text is not reproduced (documented divergence). Runs as
//! its own pass BEFORE transform, so the first-error order is observable and
//! trivially identical to the reference.

use yaml_rust2::Yaml;

use super::{parse_annotation_key, scalar_key_to_string};
use crate::model::{SettingItem, SettingValue};

pub(super) fn validate_settings_array(items: &[Yaml]) -> Result<(), String> {
    if items.is_empty() {
        return Err("settings must be a non-empty array".to_owned());
    }
    for (i, item) in items.iter().enumerate() {
        validate_settings_item(item, &format!("instance[{i}]"))?;
    }
    reject_duplicate_ids(items, "instance")?;
    Ok(())
}

/// Reject two settings items in the same array that share a parameter key (id):
/// both flatten to the SAME engine name (`<prefix>.<key>`), so the install
/// store would keep only one. js-yaml / the TS engine silently collapse such a
/// repeated key last-write-wins; we treat the ambiguity as invalid and reject
/// it (a stricter rule than the reference, like the int32 / no-float number
/// rule). The one shipped mod that relied on the collapse is pinned in
/// `workarounds::apply_settings_workarounds`, so a future release is forced to
/// author it without the duplicate. Sibling arrays and indexed (`key[i]`)
/// entries get distinct prefixes and are validated under their own call, so only
/// a same-level repeat is flagged.
fn reject_duplicate_ids(items: &[Yaml], path: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let Yaml::Hash(map) = item else {
            continue;
        };
        for (key, _) in map.iter() {
            let key = scalar_key_to_string(key);
            if is_plain_param_key(&key) && !seen.insert(key.clone()) {
                return Err(format!("{path} has a duplicate settings id '{key}'"));
            }
        }
    }
    Ok(())
}

fn validate_settings_item(item: &Yaml, path: &str) -> Result<(), String> {
    let Yaml::Hash(map) = item else {
        return Err(format!("{path} is not a settings object"));
    };
    if map.is_empty() {
        return Err(format!("{path} is an empty settings object"));
    }
    let mut param: Option<(String, &Yaml)> = None;
    let mut options: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in map.iter() {
        let key = scalar_key_to_string(key);
        let key_path = format!("{path}.{key}");
        if is_plain_param_key(&key) {
            validate_param_value(value, &key_path)?;
            param.get_or_insert((key_path, value));
        } else if is_annotation_key(&key, &["name", "description"]) {
            if !matches!(value, Yaml::String(_)) {
                return Err(format!("{key_path} must be a string"));
            }
        } else if is_annotation_key(&key, &["options"]) {
            validate_options_value(value, &key_path)?;
            options.push((key_path, option_values(value)));
        } else {
            return Err(format!("{key_path} is not an allowed property"));
        }
    }
    reject_mismatched_option_languages(&options)?;
    let has_options = !options.is_empty();
    // `$options` is a dropdown of value->label choices the UI renders ONLY for a
    // string LEAF: a string scalar, or each element of a string array. Every
    // other value type - a number, a boolean, a number array, a nested settings
    // group - renders a control (number input, switch, sub-form) that never
    // reads `$options`, so a dropdown on it is dead metadata. Reject it here.
    // This is a STRICTER rule than the reference (its jsonschema does not tie
    // `$options` to the value type); shipped versions that carry such a dropdown
    // are pinned in `workarounds::apply_settings_workarounds`.
    if has_options
        && let Some((param_path, value)) = &param
        && !value_takes_options(value)
    {
        return Err(format!(
            "{param_path} must be a string or array of strings to use $options"
        ));
    }
    Ok(())
}

/// Whether a `$options` dropdown is meaningful on a setting value: only a string
/// scalar, or an array whose every element is a string (each rendered as its own
/// dropdown). A number, a boolean, a number array, or a nested settings value
/// renders a control that ignores `$options`.
fn value_takes_options(value: &Yaml) -> bool {
    match value {
        Yaml::String(_) => true,
        Yaml::Array(items) => {
            !items.is_empty() && items.iter().all(|v| matches!(v, Yaml::String(_)))
        }
        _ => false,
    }
}

/// `^[0-9A-Za-z_-]+$`
fn is_plain_param_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^\$(name|description|options)(:[a-z]{2}(-[A-Z]{2})?)?$`. The `$base[:lang]`
/// split is the shared `super::parse_annotation_key` (the same split transform
/// uses); the name-set and lang-SHAPE checks below are validate-side only
/// (transform relies on this validation having run).
fn is_annotation_key(key: &str, names: &[&str]) -> bool {
    let Some((base, lang)) = parse_annotation_key(key) else {
        return false;
    };
    if !names.contains(&base) {
        return false;
    }
    match lang {
        None => true,
        Some(lang) => {
            let b = lang.as_bytes();
            (b.len() == 2 && b.iter().all(u8::is_ascii_lowercase))
                || (b.len() == 5
                    && b[..2].iter().all(u8::is_ascii_lowercase)
                    && b[2] == b'-'
                    && b[3..].iter().all(u8::is_ascii_uppercase))
        }
    }
}

/// A number value must be an int32-ranged integer; floats and
/// out-of-range integers are rejected (see the module doc). Callers gate
/// on `Integer | Real`, so the `_` arm is the float (`Real`) case.
fn validate_number(value: &Yaml, path: &str) -> Result<(), String> {
    match value {
        Yaml::Integer(i) if i32::try_from(*i).is_ok() => Ok(()),
        Yaml::Integer(i) => Err(format!(
            "{path} must be a 32-bit integer; {i} is out of range"
        )),
        _ => Err(format!(
            "{path} must be an integer; floating-point numbers are not supported"
        )),
    }
}

/// A parameter value: boolean | int32 | string | settings array | array of
/// int32 | array of strings | array of settings arrays.
fn validate_param_value(value: &Yaml, path: &str) -> Result<(), String> {
    let items = match value {
        Yaml::Boolean(_) | Yaml::String(_) => return Ok(()),
        Yaml::Integer(_) | Yaml::Real(_) => return validate_number(value, path),
        Yaml::Array(items) => items,
        _ => return Err(format!("{path} has an unsupported value type")),
    };
    if items.is_empty() {
        return Err(format!("{path} must not be an empty array"));
    }
    // Classify by WHOLE-array homogeneity and ENFORCE it. The transform pass
    // later reads only the first element (the shared `transform::ArrayKind`
    // concept), which is sound ONLY because this validation guarantees the
    // array is homogeneous. The number test spans Integer | Real so a float
    // array is recognized as a number array here and rejected by
    // `validate_number`, a case transform never sees.
    let all_numbers = items
        .iter()
        .all(|v| matches!(v, Yaml::Integer(_) | Yaml::Real(_)));
    let all_strings = items.iter().all(|v| matches!(v, Yaml::String(_)));
    if all_strings {
        return Ok(());
    }
    if all_numbers {
        for (i, v) in items.iter().enumerate() {
            validate_number(v, &format!("{path}[{i}]"))?;
        }
        return Ok(());
    }
    // A nested settings array ({"$ref": "#"}) ...
    if items.iter().all(|v| matches!(v, Yaml::Hash(_))) {
        return validate_settings_array_at(items, path);
    }
    // ... or an array of settings arrays ({"items": {"$ref": "#"}}).
    if items.iter().all(|v| matches!(v, Yaml::Array(_))) {
        for (i, v) in items.iter().enumerate() {
            // Always an Array here (guaranteed by the `all` above); the `if let`
            // just binds it without a defensive panic arm.
            if let Yaml::Array(inner) = v {
                validate_settings_array_at(inner, &format!("{path}[{i}]"))?;
            }
        }
        return Ok(());
    }
    Err(format!("{path} has an unsupported value type"))
}

fn validate_settings_array_at(items: &[Yaml], path: &str) -> Result<(), String> {
    if items.is_empty() {
        return Err(format!("{path} must not be an empty array"));
    }
    for (i, item) in items.iter().enumerate() {
        validate_settings_item(item, &format!("{path}[{i}]"))?;
    }
    reject_duplicate_ids(items, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Object-array shape check (runs on the typed tree, after transform)
// ---------------------------------------------------------------------------

/// Reject an object array (`SettingValue::SettingsArray`) whose groups are not
/// type-compatible SUBSETS of the TEMPLATE group governing their path. The
/// settings UI derives an object array's schema from the first element at the
/// template path (`ModSettingsYaml.describeSetting` -> `children: first`,
/// applied to every row) - `items[0]`, then `items[0].subItems[0]`, and so on -
/// so a key a group declares that its template does not, or declares with a
/// conflicting type, is unreachable from the form and mistyped in the store. Key
/// ORDER and MISSING keys are fine: a reordered default row (an annotated
/// template first, plain rows after) and a partial default row (overriding a
/// subset of the fields) are the common, legitimate patterns. Only an EXTRA or
/// TYPE-CONFLICTING key is rejected.
///
/// The template is taken by PATH, not from each array instance: a nested array
/// under a default row (`items[2].subItems`) is governed by the template's own
/// nested array (`items[0].subItems[0]`), because the rows of that nested array
/// are data, not schema - a submenu declared on the fourth row of a menu the
/// third default row defines is perfectly reachable in the form.
///
/// This is a STRICTER rule than the reference (its jsonschema validates each
/// group independently and never cross-checks them); a sweep over the published
/// mods found NO version that violates it, so no `workarounds` pin is needed.
/// It runs on the TYPED tree because it needs the transform's element-type
/// classification, which the pre-transform Yaml does not carry.
pub(super) fn reject_incompatible_object_arrays(items: &[SettingItem]) -> Result<(), String> {
    // The top-level items are their own schema, so each pairs with itself.
    check_group(items, items, "")
}

/// Check `group` against the `schema` group that governs its path, then descend
/// pairwise. `prefix` is the group's own flat path, empty at the top level.
fn check_group(schema: &[SettingItem], group: &[SettingItem], prefix: &str) -> Result<(), String> {
    for item in group {
        // The enclosing array check rejects a key the schema does not declare
        // before the descent gets here, so an absent match is only the top-level
        // case, where the schema IS the group.
        if let Some(schema_item) = schema.iter().find(|si| si.key == item.key) {
            let key = if prefix.is_empty() {
                item.key.clone()
            } else {
                format!("{prefix}.{}", item.key)
            };
            check_value(&schema_item.value, &item.value, &key)?;
        }
    }
    Ok(())
}

/// Descend a value paired with the schema value at its path. The kinds match:
/// the enclosing array check has already established that the schema type covers
/// the value's.
fn check_value(schema: &SettingValue, value: &SettingValue, key: &str) -> Result<(), String> {
    match (schema, value) {
        (SettingValue::Settings(schema_group), SettingValue::Settings(inner)) => {
            check_group(schema_group, inner, key)
        }
        (SettingValue::SettingsArray(schema_groups), SettingValue::SettingsArray(groups)) => {
            let Some(template) = schema_groups.first() else {
                return Ok(());
            };
            for (i, group) in groups.iter().enumerate() {
                if let Some(bad_key) = first_incompatible_key(template, group) {
                    return Err(format!(
                        "object array '{key}' entry {i} has key '{bad_key}' not compatible with the template entry"
                    ));
                }
                check_group(template, group, &format!("{key}[{i}]"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The first key in `group` that the `template` group does not declare with a
/// covering type, or `None` when every key in `group` is compatible.
fn first_incompatible_key(template: &[SettingItem], group: &[SettingItem]) -> Option<String> {
    group
        .iter()
        .find(|gi| {
            !template
                .iter()
                .any(|ti| ti.key == gi.key && type_covers(&ti.value, &gi.value))
        })
        .map(|gi| gi.key.clone())
}

/// Whether the `template` value can represent the `group` value: the same scalar
/// or array KIND, or - for a nested group - a recursive subset (the group's keys
/// are a covered subset of the template's). A nested object array only has to be
/// an object array: `check_value` descends into it and compares EVERY one of its
/// groups against the template's own nested template, which reports a violation
/// against the exact inner path instead of against the enclosing key.
fn type_covers(template: &SettingValue, group: &SettingValue) -> bool {
    use SettingValue::{Bool, Number, NumberArray, Settings, SettingsArray, String, StringArray};
    match (template, group) {
        (Bool(_), Bool(_)) | (Number(_), Number(_)) | (String(_), String(_)) => true,
        (NumberArray(_), NumberArray(_)) | (StringArray(_), StringArray(_)) => true,
        (Settings(tg), Settings(gg)) => first_incompatible_key(tg, gg).is_none(),
        (SettingsArray(_), SettingsArray(_)) => true,
        _ => false,
    }
}

/// `$options`: an array of at least two single-property objects with
/// string values.
fn validate_options_value(value: &Yaml, path: &str) -> Result<(), String> {
    let Yaml::Array(items) = value else {
        return Err(format!("{path} must be an array of options"));
    };
    if items.len() < 2 {
        return Err(format!("{path} must have at least two options"));
    }
    for (i, item) in items.iter().enumerate() {
        let Yaml::Hash(map) = item else {
            return Err(format!("{path}[{i}] must be an object"));
        };
        if map.len() != 1 {
            return Err(format!("{path}[{i}] must have exactly one property"));
        }
        for (_, label) in map.iter() {
            if !matches!(label, Yaml::String(_)) {
                return Err(format!("{path}[{i}] must map to a string"));
            }
        }
    }
    Ok(())
}

/// The option VALUES of a `$options` list - the single property key of each
/// entry, which is what a selection stores - in declaration order. Reads a list
/// `validate_options_value` has accepted, so a non-conforming entry cannot occur
/// and is skipped rather than reported.
fn option_values(value: &Yaml) -> Vec<String> {
    let Yaml::Array(items) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match item {
            Yaml::Hash(map) => map.iter().next().map(|(key, _)| scalar_key_to_string(key)),
            _ => None,
        })
        .collect()
}

/// Every `$options[:lang]` variant of one settings item must offer the SAME set
/// of option values; only the LABELS are translated. The value is what a
/// selection stores and what the mod's C++ reads back, so a variant that adds or
/// drops one makes the stored value depend on the display language: an option
/// only one language offers writes a value the mod does not handle, and a value
/// stored under one language has no entry to render under another. Option ORDER
/// may differ - it is presentation only, and each label travels with its own
/// value.
///
/// `variants` are the item's `$options[:lang]` lists paired with their key paths,
/// in declaration order; each is compared against the first, which makes all of
/// them equal transitively.
fn reject_mismatched_option_languages(variants: &[(String, Vec<String>)]) -> Result<(), String> {
    let Some((base_path, base_values)) = variants.first() else {
        return Ok(());
    };
    for (path, values) in &variants[1..] {
        if let Some(extra) = values.iter().find(|&v| !base_values.contains(v)) {
            return Err(format!(
                "{path} has option '{extra}' that {base_path} does not declare"
            ));
        }
        if let Some(missing) = base_values.iter().find(|&v| !values.contains(v)) {
            return Err(format!(
                "{path} is missing option '{missing}' that {base_path} declares"
            ));
        }
    }
    Ok(())
}
