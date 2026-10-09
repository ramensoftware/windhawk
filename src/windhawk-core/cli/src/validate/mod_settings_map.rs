//! Whole-map validation for `mod settings write`, which replaces a mod's
//! settings wholesale. Beyond each key's type, the map must hold every
//! declared setting outside an array and index every array from 0 without
//! gaps: the engine reads an absent key as 0 or empty, not as its declared
//! default, and a mod typically reads an array up to its first missing index,
//! so a leaf left out or an index skipped silently changes what the mod sees.
//! An array element may leave out leaves of its template, as the elements a
//! mod declares as an array's default can.

use std::collections::{BTreeSet, HashSet};

use serde_json::{Map, Value};
use windhawk_core_protocol::{InitialSettingItem, InitialSettings, InitialSettingsValue};

use crate::error::CliError;
use crate::validate::mod_settings::{parse_segment, parse_setting_json, resolve_setting_key_type};

/// Validate `input`, the complete flat settings map for mod `id`, against the
/// mod's declared settings and its `stored` map, and return the map to write:
/// each declared key's value normalized by [`parse_setting_json`], in input
/// order. A key the source does not declare is kept only when `stored` holds
/// it with the same value (install merges never prune, so the store can carry
/// keys an older version declared), and counts toward no structural rule.
/// Every problem is collected into one usage error (exit 2), one per line.
#[track_caller]
pub fn validate_settings_map(
    id: &str,
    initial_settings: &InitialSettings,
    stored: &Map<String, Value>,
    input: &Map<String, Value>,
) -> Result<Map<String, Value>, CliError> {
    let mut problems = Vec::new();
    let mut output = Map::new();
    // The declared keys present, whether or not their value type-checks, so a
    // mistyped key is not also reported as missing.
    let mut present: HashSet<&str> = HashSet::new();
    for (key, value) in input {
        let declared_type =
            resolve_setting_key_type(initial_settings, key).filter(|_| has_canonical_indices(key));
        match declared_type {
            Some(ty) => {
                present.insert(key);
                match parse_setting_json(key, ty, value) {
                    Ok(normalized) => {
                        output.insert(key.clone(), normalized);
                    }
                    Err(error) => problems.push(error.message().to_owned()),
                }
            }
            None => match stored.get(key) {
                Some(stored_value) if stored_value == value => {
                    output.insert(key.clone(), value.clone());
                }
                Some(_) => problems.push(format!(
                    "Key '{key}' is not a declared setting of mod '{id}', and its value differs \
                     from the stored one; an undeclared key can only be kept unchanged or left out."
                )),
                None => problems.push(format!(
                    "Key '{key}' is not a declared setting of mod '{id}'."
                )),
            },
        }
    }
    check_group(initial_settings, "", &present, false, &mut problems);

    if problems.is_empty() {
        Ok(output)
    } else {
        Err(CliError::usage(problems.join("\n")))
    }
}

/// Whether every array index in `key` is spelled the way the engine formats
/// one. Resolution reads `items[01]` and `items[+1]` as index 1, but those
/// spellings name keys the mod never reads.
fn has_canonical_indices(key: &str) -> bool {
    key.split('.').all(|segment| match parse_segment(segment) {
        Some((name, Some(index))) => segment == format!("{name}[{index}]"),
        _ => true,
    })
}

/// Check that the `present` keys cover the declared `items` under `prefix`:
/// every scalar leaf unless the group is `in_element` of an array, and for
/// every array, at any depth, indices running from 0 without gaps.
fn check_group(
    items: &[InitialSettingItem],
    prefix: &str,
    present: &HashSet<&str>,
    in_element: bool,
    problems: &mut Vec<String>,
) {
    for item in items {
        let key = if prefix.is_empty() {
            item.key.clone()
        } else {
            format!("{prefix}.{}", item.key)
        };
        match &item.value {
            InitialSettingsValue::Bool(_)
            | InitialSettingsValue::Number(_)
            | InitialSettingsValue::String(_) => {
                if !in_element && !present.contains(key.as_str()) {
                    problems.push(format!("Declared setting '{key}' is missing."));
                }
            }
            InitialSettingsValue::NumberArray(_) | InitialSettingsValue::StringArray(_) => {
                check_indices(&key, &present_indices(present, &key, false), problems);
            }
            InitialSettingsValue::Settings(inner) => {
                check_group(inner, &key, present, in_element, problems)
            }
            // The first group is the template at every index, as in resolution.
            InitialSettingsValue::SettingsArray(groups) => {
                let Some(template) = groups.first() else {
                    continue;
                };
                let indices = present_indices(present, &key, true);
                check_indices(&key, &indices, problems);
                for index in indices {
                    check_group(
                        template,
                        &format!("{key}[{index}]"),
                        present,
                        true,
                        problems,
                    );
                }
            }
        }
    }
}

/// The indices at which the `present` keys populate the array `key`: `key[i]`
/// itself for a scalar array, any `key[i].child` for an object array.
fn present_indices(present: &HashSet<&str>, key: &str, object: bool) -> BTreeSet<usize> {
    present
        .iter()
        .filter_map(|k| {
            let (index, rest) = k.strip_prefix(key)?.strip_prefix('[')?.split_once(']')?;
            let in_element = if object {
                rest.starts_with('.')
            } else {
                rest.is_empty()
            };
            if in_element { index.parse().ok() } else { None }
        })
        .collect()
}

/// Report the first index the array `key` skips below its largest one.
fn check_indices(key: &str, indices: &BTreeSet<usize>, problems: &mut Vec<String>) {
    // Sorted and distinct, so the first gap is where an index outruns its rank.
    if let Some((missing, next)) = indices
        .iter()
        .enumerate()
        .find(|&(rank, &index)| rank != index)
    {
        problems.push(format!(
            "Array '{key}' is missing index {missing} (index {next} is present); \
             array indices must run from 0 without gaps."
        ));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A boolean, a bounded number, a string, a `$float`, a nested group, a
    /// scalar array, and an object array whose template nests a scalar array.
    fn declared() -> InitialSettings {
        serde_json::from_value(json!([
            { "key": "flag", "value": true },
            { "key": "count", "value": 3, "min": 1, "max": 5 },
            { "key": "label", "value": "hi" },
            { "key": "opacity", "value": "0.5", "float": true },
            { "key": "group", "value": [{ "key": "inner", "value": 1 }] },
            { "key": "names", "value": ["a"] },
            { "key": "items", "value": [[
                { "key": "action", "value": 0 },
                { "key": "label", "value": "x" },
                { "key": "tags", "value": ["t"] }
            ]] }
        ]))
        .expect("parse initial settings")
    }

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    /// A full map: every scalar leaf, two names, and two items.
    fn full() -> Value {
        json!({
            "flag": 1,
            "count": 3,
            "label": "hi",
            "opacity": "0.5",
            "group.inner": 1,
            "names[0]": "a",
            "names[1]": "b",
            "items[0].action": 1,
            "items[0].label": "first",
            "items[0].tags[0]": "red",
            "items[1].action": 2,
            "items[1].label": "second",
        })
    }

    fn validate(stored: Value, input: Value) -> Result<Map<String, Value>, CliError> {
        validate_settings_map("m", &declared(), &map(stored), &map(input))
    }

    fn problems(stored: Value, input: Value) -> Vec<String> {
        let err = validate(stored, input).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        err.message().lines().map(str::to_owned).collect()
    }

    #[test]
    fn a_stored_map_round_trips() {
        // The registry's typed values come back as they are.
        assert_eq!(validate(full(), full()).unwrap(), map(full()));

        // The portable store's text comes back typed, in input order.
        let portable = json!({
            "flag": "1",
            "count": "3",
            "label": "hi",
            "opacity": "0.5",
            "group.inner": "1",
            "names[0]": "a",
            "items[0].action": "1",
            "items[0].label": "first",
        });
        let written = validate(portable.clone(), portable).unwrap();
        assert_eq!(
            Value::Object(written),
            json!({
                "flag": 1,
                "count": 3,
                "label": "hi",
                "opacity": "0.5",
                "group.inner": 1,
                "names[0]": "a",
                "items[0].action": 1,
                "items[0].label": "first",
            })
        );
    }

    #[test]
    fn values_are_normalized_for_storage() {
        let mut input = full();
        input["flag"] = json!(false);
        input["opacity"] = json!(1.0);
        let written = validate(json!({}), input).unwrap();
        assert_eq!(written["flag"], json!(0));
        assert_eq!(written["opacity"], json!("1"));

        let mut input = full();
        input["opacity"] = json!("");
        assert_eq!(validate(json!({}), input).unwrap()["opacity"], json!(""));
    }

    #[test]
    fn array_elements_can_be_removed_renumbered_reordered_or_emptied() {
        // Item 0 removed, item 1 renumbered to 0; one name dropped.
        let mut removed = map(full());
        for key in [
            "items[1].action",
            "items[1].label",
            "items[0].tags[0]",
            "names[1]",
        ] {
            removed.remove(key);
        }
        removed.insert("items[0].action".to_owned(), json!(2));
        removed.insert("items[0].label".to_owned(), json!("second"));
        assert!(validate(full(), Value::Object(removed)).is_ok());

        // The two items swapped, keys in a different order than stored.
        let reordered = json!({
            "items[1].label": "first",
            "items[1].action": 1,
            "items[1].tags[0]": "red",
            "items[0].label": "second",
            "items[0].action": 2,
            "flag": 1,
            "count": 3,
            "label": "hi",
            "opacity": "0.5",
            "group.inner": 1,
        });
        assert!(validate(full(), reordered).is_ok());

        // Every array empty: only the scalar leaves remain.
        let emptied = json!({
            "flag": 1,
            "count": 3,
            "label": "hi",
            "opacity": "0.5",
            "group.inner": 1,
        });
        assert!(validate(full(), emptied).is_ok());
    }

    #[test]
    fn a_declared_leaf_left_out_is_missing() {
        let mut input = map(full());
        input.remove("count");
        input.remove("group.inner");
        assert_eq!(
            problems(full(), Value::Object(input)),
            [
                "Declared setting 'count' is missing.",
                "Declared setting 'group.inner' is missing.",
            ]
        );
    }

    #[test]
    fn an_element_may_leave_out_leaves_of_its_template() {
        // Element 1 without its label, and element 2 present through a nested
        // array key alone, as a mod's declared default elements can be.
        let mut input = map(full());
        input.remove("items[1].label");
        input.insert("items[2].tags[0]".to_owned(), json!("blue"));
        assert!(validate(full(), Value::Object(input)).is_ok());
    }

    #[test]
    fn array_indices_must_run_from_zero_without_gaps() {
        let mut input = map(full());
        input.remove("names[0]");
        input.insert("items[3].action".to_owned(), json!(3));
        input.insert("items[3].label".to_owned(), json!("fourth"));
        input.insert("items[3].tags[1]".to_owned(), json!("late"));
        assert_eq!(
            problems(full(), Value::Object(input)),
            [
                "Array 'names' is missing index 0 (index 1 is present); \
                 array indices must run from 0 without gaps.",
                "Array 'items' is missing index 2 (index 3 is present); \
                 array indices must run from 0 without gaps.",
                "Array 'items[3].tags' is missing index 0 (index 1 is present); \
                 array indices must run from 0 without gaps.",
            ]
        );
    }

    #[test]
    fn an_undeclared_key_is_rejected_unless_stored_unchanged() {
        let mut stored = map(full());
        stored.insert("old".to_owned(), json!("x"));
        stored.insert("items[5].gone".to_owned(), json!(1));

        // Kept as stored, and an element key it names does not count as
        // present: no gap is reported for items[5].
        let written =
            validate(Value::Object(stored.clone()), Value::Object(stored.clone())).unwrap();
        assert_eq!(written["old"], json!("x"));
        assert_eq!(written["items[5].gone"], json!(1));

        // A changed value, or a key the store does not hold, is rejected.
        let mut input = stored.clone();
        input.insert("old".to_owned(), json!("y"));
        input.insert("nope".to_owned(), json!(1));
        assert_eq!(
            problems(Value::Object(stored), Value::Object(input)),
            [
                "Key 'old' is not a declared setting of mod 'm', and its value differs from \
                 the stored one; an undeclared key can only be kept unchanged or left out.",
                "Key 'nope' is not a declared setting of mod 'm'.",
            ]
        );
    }

    #[test]
    fn an_index_spelled_unlike_the_engine_is_not_declared() {
        let mut input = map(full());
        input.insert("names[02]".to_owned(), json!("c"));
        input.insert("items[+1].label".to_owned(), json!("x"));
        assert_eq!(
            problems(full(), Value::Object(input)),
            [
                "Key 'names[02]' is not a declared setting of mod 'm'.",
                "Key 'items[+1].label' is not a declared setting of mod 'm'.",
            ]
        );
    }

    #[test]
    fn each_kind_of_value_is_type_checked() {
        let mut input = map(full());
        input.insert("flag".to_owned(), json!("yes"));
        input.insert("count".to_owned(), json!(9));
        input.insert("label".to_owned(), json!(7));
        input.insert("opacity".to_owned(), json!(null));
        input.insert("names[0]".to_owned(), json!(["a"]));
        assert_eq!(
            problems(full(), Value::Object(input)),
            [
                "Setting 'flag' is declared as boolean; value must be one of true/false/1/0, \
                 got 'yes'.",
                "Setting 'count' must be between 1 and 5, got '9'.",
                "Setting 'label' is declared as a string; got 7.",
                "Setting 'opacity' is declared as a decimal number; got null.",
                "Setting 'names[0]' is declared as a string; got an array.",
            ]
        );
    }

    #[test]
    fn every_problem_is_reported_together_in_input_then_declaration_order() {
        let input = json!({
            "nope": 1,
            "count": "many",
            "items[1].action": 1,
        });
        assert_eq!(
            problems(json!({}), input),
            [
                "Key 'nope' is not a declared setting of mod 'm'.",
                "Setting 'count' must be an integer, got 'many'.",
                "Declared setting 'flag' is missing.",
                "Declared setting 'label' is missing.",
                "Declared setting 'opacity' is missing.",
                "Declared setting 'group.inner' is missing.",
                "Array 'items' is missing index 0 (index 1 is present); \
                 array indices must run from 0 without gaps.",
            ]
        );
    }

    #[test]
    fn a_mod_without_settings_takes_only_its_stored_keys() {
        let none = InitialSettings::default();
        let stored = map(json!({ "leftover": "1" }));
        let check = |input: Value| validate_settings_map("m", &none, &stored, &map(input));

        assert!(check(json!({})).unwrap().is_empty());
        assert_eq!(
            check(json!({ "leftover": "1" })).unwrap(),
            map(json!({ "leftover": "1" }))
        );
        assert_eq!(
            check(json!({ "fresh": 1 })).unwrap_err().message(),
            "Key 'fresh' is not a declared setting of mod 'm'."
        );
    }
}
