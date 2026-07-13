//! Schema validation, mirroring the jsonschema document of the TS
//! implementation. Errors carry a short instance path description; the TS
//! jsonschema message text is not reproduced (documented divergence). Runs as
//! its own pass BEFORE transform, so the first-error order is observable and
//! trivially identical to the reference.

use yaml_rust2::Yaml;

use super::{parse_annotation_key, scalar_key_to_string};

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
    for (key, value) in map.iter() {
        let key = scalar_key_to_string(key);
        let key_path = format!("{path}.{key}");
        if is_plain_param_key(&key) {
            validate_param_value(value, &key_path)?;
        } else if is_annotation_key(&key, &["name", "description"]) {
            if !matches!(value, Yaml::String(_)) {
                return Err(format!("{key_path} must be a string"));
            }
        } else if is_annotation_key(&key, &["options"]) {
            validate_options_value(value, &key_path)?;
        } else {
            return Err(format!("{key_path} is not an allowed property"));
        }
    }
    Ok(())
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
