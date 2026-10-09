//! Schema validation, mirroring the jsonschema document of the TS
//! implementation. Errors carry a short instance path description; the TS
//! jsonschema message text is not reproduced (documented divergence). Runs as
//! its own pass BEFORE transform, so the first-error order is observable and
//! trivially identical to the reference.

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use super::{float_literal, number_in_text, parse_annotation_key, scalar_key_to_string};
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
    // `$float` changes what a valid parameter value (and a valid `$min` /
    // `$max`) is and may be written after them, so it is peeked ahead of the
    // loop; each value is then checked in place, in mapping order, and no
    // error moves. A `$float` that is not a boolean is reported by the loop
    // when it reaches the key.
    let float = annotation_flag(map, "float");
    let mut param: Option<(String, &Yaml)> = None;
    let mut options: Vec<(String, Vec<String>)> = Vec::new();
    for (key, value) in map.iter() {
        let key = scalar_key_to_string(key);
        let key_path = format!("{path}.{key}");
        if is_plain_param_key(&key) {
            validate_param_value(value, &key_path, float)?;
            param.get_or_insert((key_path, value));
        } else if is_annotation_key(&key, &["name", "description"]) {
            if !matches!(value, Yaml::String(_)) {
                return Err(format!("{key_path} must be a string"));
            }
        } else if is_annotation_key(&key, &["options"]) {
            validate_options_value(value, &key_path)?;
            options.push((key_path, option_values(value)));
        } else if is_exact_annotation_key(&key, "format") {
            // Any non-empty string: the core forwards it and never interprets
            // it, so an unknown format is a plain field downstream, not an error
            // here. Empty is rejected only because it names nothing.
            if !matches!(value, Yaml::String(s) if !s.is_empty()) {
                return Err(format!("{key_path} must be a non-empty string"));
            }
        } else if is_exact_annotation_key(&key, "float")
            || is_exact_annotation_key(&key, "dynamicSelect")
        {
            if !matches!(value, Yaml::Boolean(_)) {
                return Err(format!("{key_path} must be a boolean"));
            }
        } else if is_exact_annotation_key(&key, "min") || is_exact_annotation_key(&key, "max") {
            validate_bound(value, &key_path, float)?;
        } else if is_exact_annotation_key(&key, "showIf") || is_exact_annotation_key(&key, "hideIf")
        {
            validate_conditions(value, &key_path)?;
        } else if key.starts_with("$_") {
            // Ignored for forward compatibility, whatever the value; a newer
            // core may decide explicitly to handle both `$new` and `$_new`.
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
    // `$dynamicSelect` is the same dropdown with runtime-supplied entries, so
    // it takes the same value types.
    if annotation_flag(map, "dynamicSelect")
        && let Some((param_path, value)) = &param
        && !value_takes_options(value)
    {
        return Err(format!(
            "{param_path} must be a string or array of strings to use $dynamicSelect"
        ));
    }
    if float
        && let Some((param_path, value)) = &param
        && !value_is_numeric(value, float)
    {
        return Err(format!(
            "{param_path} must be a number or array of numbers to use $float"
        ));
    }
    let min = annotation_value(map, "min").and_then(float_literal);
    let max = annotation_value(map, "max").and_then(float_literal);
    for (name, bound) in [("min", min), ("max", max)] {
        if bound.is_some()
            && let Some((param_path, value)) = &param
            && !value_is_numeric(value, float)
        {
            return Err(format!(
                "{param_path} must be a number or array of numbers to use ${name}"
            ));
        }
    }
    if let Some((param_path, value)) = &param {
        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            return Err(format!(
                "{param_path} declares a $min greater than its $max"
            ));
        }
        reject_default_outside_bounds(value, param_path, min, max, float)?;
    }
    Ok(())
}

/// The declared default must lie within the item's `$min` / `$max` bounds: a
/// declaration whose default breaks its own rule is an authoring error to
/// report at parse time, not a default for the editor to draw as invalid.
/// Reads a scalar or an array the key loop has accepted as numeric (under
/// `float`, a string holding a number included); a value that is not a number
/// is left to the applicability check.
fn reject_default_outside_bounds(
    value: &Yaml,
    path: &str,
    min: Option<f64>,
    max: Option<f64>,
    float: bool,
) -> Result<(), String> {
    if min.is_none() && max.is_none() {
        return Ok(());
    }
    let within = |v: &Yaml| {
        leaf_number(v, float)
            .is_none_or(|v| min.is_none_or(|min| v >= min) && max.is_none_or(|max| v <= max))
    };
    match value {
        Yaml::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                if !within(item) {
                    return Err(format!(
                        "{path}[{i}] default is outside its $min/$max range"
                    ));
                }
            }
            Ok(())
        }
        _ if !within(value) => Err(format!("{path} default is outside its $min/$max range")),
        _ => Ok(()),
    }
}

/// A `$showIf` / `$hideIf` map, by shape alone: every key a `.`-joined setting
/// reference, every value a boolean, an int32 integer or a string, or a
/// non-empty list of one of those kinds. Which setting a reference names and
/// whether the values fit it needs the whole tree, so that is
/// `conditions::resolve_conditions`, on the typed tree.
fn validate_conditions(value: &Yaml, path: &str) -> Result<(), String> {
    let Yaml::Hash(map) = value else {
        return Err(format!("{path} must be a non-empty map"));
    };
    if map.is_empty() {
        return Err(format!("{path} must be a non-empty map"));
    }
    for (key, value) in map.iter() {
        let reference = scalar_key_to_string(key);
        if !is_setting_reference(&reference) {
            return Err(format!(
                "{path} key '{reference}' is not a setting reference"
            ));
        }
        let (values, listed) = match value {
            Yaml::Array(items) => (items.as_slice(), true),
            _ => (std::slice::from_ref(value), false),
        };
        let kinds: Option<Vec<_>> = values.iter().map(condition_value_kind).collect();
        if values.is_empty() || kinds.is_none_or(|kinds| kinds.iter().any(|k| *k != kinds[0])) {
            return Err(format!(
                "{path} value for '{reference}' must be a boolean, an integer, a string, or a list of one of them"
            ));
        }
        // A number takes the int32 rule at its own position: a float or an
        // out-of-range integer is never a value a setting holds.
        for (i, value) in values.iter().enumerate() {
            if matches!(value, Yaml::Integer(_) | Yaml::Real(_)) {
                let value_path = if listed {
                    format!("{path}.{reference}[{i}]")
                } else {
                    format!("{path}.{reference}")
                };
                validate_number(value, &value_path)?;
            }
        }
    }
    Ok(())
}

/// The kind a condition value is compared under; `None` for a value that is
/// not a scalar.
fn condition_value_kind(value: &Yaml) -> Option<ConditionValueKind> {
    match value {
        Yaml::Boolean(_) => Some(ConditionValueKind::Bool),
        Yaml::Integer(_) | Yaml::Real(_) => Some(ConditionValueKind::Number),
        Yaml::String(_) => Some(ConditionValueKind::String),
        _ => None,
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum ConditionValueKind {
    Bool,
    Number,
    String,
}

/// A `$showIf` / `$hideIf` key: one or more parameter keys joined by `.`,
/// naming a setting relative to the annotated item (resolved by
/// `conditions`).
fn is_setting_reference(reference: &str) -> bool {
    reference.split('.').all(is_plain_param_key)
}

/// Whether the item sets the boolean annotation `$name` to `true`. A read, not
/// a check: a non-boolean value reads as `false` here and is reported by the
/// key loop at its own position.
pub(super) fn annotation_flag(map: &Hash, name: &str) -> bool {
    matches!(annotation_value(map, name), Some(Yaml::Boolean(true)))
}

/// The value of the exact annotation `$name` on the item, if written.
pub(super) fn annotation_value<'a>(map: &'a Hash, name: &str) -> Option<&'a Yaml> {
    map.iter()
        .find(|(key, _)| is_exact_annotation_key(&scalar_key_to_string(key), name))
        .map(|(_, value)| value)
}

/// Whether a setting value is numeric - a number scalar, or an array whose
/// every element is a number - which is what `$float` and `$min` / `$max`
/// apply to. Under `float` a string holding a number is one too. Any other
/// string, a boolean, or a group has no number to make decimal or to bound.
pub(super) fn value_is_numeric(value: &Yaml, float: bool) -> bool {
    match value {
        Yaml::Array(items) => {
            !items.is_empty() && items.iter().all(|v| leaf_number(v, float).is_some())
        }
        _ => leaf_number(value, float).is_some(),
    }
}

/// The number a leaf holds: a number literal's, or under `float` the one a
/// string default's text resolves to.
fn leaf_number(value: &Yaml, float: bool) -> Option<f64> {
    match value {
        Yaml::String(text) if float => float_literal(&number_in_text(text)?),
        _ => float_literal(value),
    }
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
pub(super) fn is_plain_param_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `^\$(name|description|options)(:[a-z]{2}(-[A-Z]{2})?)?$` - the localizable
/// annotations; `$format`, `$float`, `$dynamicSelect`, `$min`, `$max`,
/// `$showIf` and `$hideIf` are the exact-match set of
/// `is_exact_annotation_key`. The `$base[:lang]` split
/// is the shared `super::parse_annotation_key` (the same split transform
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

/// `^\$<name>$`, with no language suffix: `$format`, `$float`, `$dynamicSelect`,
/// `$min`, `$max`, `$showIf` and `$hideIf` govern the stored value's semantics
/// or name other settings, not a display string, so a per-language variant
/// (`$float:fr`) is rejected as an unknown key.
fn is_exact_annotation_key(key: &str, name: &str) -> bool {
    key.strip_prefix('$') == Some(name)
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

/// A `$float` number: an integer, or a real that is finite. `.inf`/`.nan` have
/// no decimal text a mod's `wcstod` could read back.
fn validate_float(value: &Yaml, path: &str) -> Result<(), String> {
    match float_literal(value) {
        Some(v) if v.is_finite() => Ok(()),
        _ => Err(format!("{path} must be a finite number")),
    }
}

/// A `$float` string default: text that resolves, as a bare scalar would, to
/// a finite number, so `"0.85"` declares what `0.85` does. The spelling for a
/// block that must also parse on a Windhawk before `$float`, which stores the
/// string as it is where it would truncate the number.
fn validate_float_text(text: &str, path: &str) -> Result<(), String> {
    match number_in_text(text).and_then(|node| float_literal(&node)) {
        Some(v) if v.is_finite() => Ok(()),
        _ => Err(format!(
            "{path} must be a number, or a string holding one, to use $float"
        )),
    }
}

/// A `$min` / `$max` value: a number under the item's own number rule, so a
/// bound on an integer item is an int32 and one on a `$float` item a finite
/// number.
fn validate_bound(value: &Yaml, path: &str, float: bool) -> Result<(), String> {
    if !matches!(value, Yaml::Integer(_) | Yaml::Real(_)) {
        return Err(format!("{path} must be a number"));
    }
    if float {
        validate_float(value, path)
    } else {
        validate_number(value, path)
    }
}

/// A parameter value: boolean | int32 | string | settings array | array of
/// int32 | array of strings | array of settings arrays. Under `$float`, the
/// number leaves take `validate_float`'s rule instead of the int32 one, and
/// the string leaves must hold a number.
fn validate_param_value(value: &Yaml, path: &str, float: bool) -> Result<(), String> {
    let validate_leaf_number: fn(&Yaml, &str) -> Result<(), String> = if float {
        validate_float
    } else {
        validate_number
    };
    let items = match value {
        Yaml::Boolean(_) => return Ok(()),
        Yaml::String(text) if float => return validate_float_text(text, path),
        Yaml::String(_) => return Ok(()),
        Yaml::Integer(_) | Yaml::Real(_) => return validate_leaf_number(value, path),
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
    // array is recognized as a number array here: without `$float` it is
    // rejected by `validate_number`, a case transform never sees; with it,
    // transform classifies the same way.
    let all_numbers = items
        .iter()
        .all(|v| matches!(v, Yaml::Integer(_) | Yaml::Real(_)));
    let all_strings = items.iter().all(|v| matches!(v, Yaml::String(_)));
    if all_strings {
        if float {
            for (i, v) in items.iter().enumerate() {
                if let Yaml::String(text) = v {
                    validate_float_text(text, &format!("{path}[{i}]"))?;
                }
            }
        }
        return Ok(());
    }
    if all_numbers {
        for (i, v) in items.iter().enumerate() {
            validate_leaf_number(v, &format!("{path}[{i}]"))?;
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
