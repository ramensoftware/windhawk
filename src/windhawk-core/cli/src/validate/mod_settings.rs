//! Key validation for `mod settings set` (the `commands/mod.ts` settings half)
//! and the per-key half of `mod settings write`.
//!
//! A key is a flat storage key in the engine's convention (scalar = `key`,
//! nested object = `parent.child`, scalar array = `parent[i]`, object array =
//! `parent[i].child`). [`resolve_setting_key_type`] is the authority: it walks a
//! mod's declared initial settings to type-check ONE key, accepting ANY array
//! index because Windhawk arrays are dynamic (the source declares a template;
//! the runtime array grows unboundedly). An object array's schema is its FIRST
//! declared group - the UI reads keys from that element alone, and the domain
//! rejects later groups that are not subsets of it, so the first element is the
//! authoritative shape at every index. [`flatten_setting_key_types`] enumerates
//! that first-group template for the "valid keys" hint shown when a key does not
//! resolve. Both keep the boolean-vs-number distinction the engine's own
//! flattening drops, the `$float` flag the item carries (a decimal held as a
//! string, which the engine cannot tell from any other string), and the
//! `$min` / `$max` bounds of a number item, so `set` can type-check and
//! range-check input before it writes.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use windhawk_core_protocol::{InitialSettingItem, InitialSettings, InitialSettingsValue};

use crate::error::CliError;
use crate::validate::int_range::parse_int32_setting;

/// A flattened setting leaf's declared scalar type. The distinction drives input
/// validation in [`parse_setting_input`]. The number kinds carry the item's
/// `$min` / `$max` bounds (an `f64` inside, so the enum is `PartialEq` and not
/// `Eq`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SettingLeafType {
    Boolean,
    Number(Bounds),
    String,
    /// A `$float` setting: stored as a string, but only decimal text, or the
    /// empty text of an unset one, is valid.
    Float(Bounds),
}

/// The declared `$min` / `$max` of a number item, each absent when the item
/// declares none. Compared as `f64` on both kinds: an int32 is exact in one,
/// and a `$float` bound parsed from the same text as the input compares the
/// same way everywhere.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Bounds {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Bounds {
    fn of(item: &InitialSettingItem) -> Bounds {
        Bounds {
            min: item.min.as_ref().and_then(serde_json::Number::as_f64),
            max: item.max.as_ref().and_then(serde_json::Number::as_f64),
        }
    }

    /// The range as the valid-keys hint spells it beside the type: `1..5`,
    /// `0.5..`, `..10`; empty when unbounded.
    fn describe(self) -> String {
        match (self.min, self.max) {
            (None, None) => String::new(),
            (min, max) => format!(
                " {}..{}",
                min.map(number_text).unwrap_or_default(),
                max.map(number_text).unwrap_or_default()
            ),
        }
    }
}

/// A bound's text: the shortest decimal that round-trips it, the spelling a
/// `$float` value is held in (`1`, `0.5`).
fn number_text(value: f64) -> String {
    value.to_string()
}

impl SettingLeafType {
    /// The type as the valid-keys hint names it, with the range where the item
    /// declares one: `boolean`, `number 1..5`, `string`, `float 0.5..`.
    pub fn describe(self) -> String {
        match self {
            SettingLeafType::Boolean => "boolean".to_owned(),
            SettingLeafType::Number(bounds) => format!("number{}", bounds.describe()),
            SettingLeafType::String => "string".to_owned(),
            SettingLeafType::Float(bounds) => format!("float{}", bounds.describe()),
        }
    }
}

/// The leaf type of a string-valued item: `Float` when the item carries the
/// `$float` flag (the value's text is a decimal), `String` otherwise.
fn string_leaf_type(item: &InitialSettingItem) -> SettingLeafType {
    if item.float {
        SettingLeafType::Float(Bounds::of(item))
    } else {
        SettingLeafType::String
    }
}

/// The leaf type of a number-valued item, carrying its bounds.
fn number_leaf_type(item: &InitialSettingItem) -> SettingLeafType {
    SettingLeafType::Number(Bounds::of(item))
}

/// Flatten a mod's declared initial settings into a `{ flat-key -> leaf type }`
/// map (the TS `flattenSettingKeyTypes`). A `BTreeMap` so the valid-keys error
/// list comes out sorted.
pub fn flatten_setting_key_types(settings: &InitialSettings) -> BTreeMap<String, SettingLeafType> {
    let mut out = BTreeMap::new();
    flatten_items(settings, "", &mut out);
    out
}

fn flatten_items(
    settings: &[InitialSettingItem],
    prefix: &str,
    out: &mut BTreeMap<String, SettingLeafType>,
) {
    for item in settings {
        let key = if prefix.is_empty() {
            item.key.clone()
        } else {
            format!("{prefix}.{}", item.key)
        };
        flatten_value(item, &key, out);
    }
}

fn flatten_value(
    item: &InitialSettingItem,
    key: &str,
    out: &mut BTreeMap<String, SettingLeafType>,
) {
    match &item.value {
        InitialSettingsValue::Bool(_) => {
            out.insert(key.to_owned(), SettingLeafType::Boolean);
        }
        InitialSettingsValue::Number(_) => {
            out.insert(key.to_owned(), number_leaf_type(item));
        }
        InitialSettingsValue::String(_) => {
            out.insert(key.to_owned(), string_leaf_type(item));
        }
        // Scalar arrays: each index is a leaf of the element type (an empty array
        // carries no type info, so it contributes no keys).
        InitialSettingsValue::NumberArray(items) => {
            for i in 0..items.len() {
                out.insert(format!("{key}[{i}]"), number_leaf_type(item));
            }
        }
        InitialSettingsValue::StringArray(items) => {
            for i in 0..items.len() {
                out.insert(format!("{key}[{i}]"), string_leaf_type(item));
            }
        }
        // A nested object: its leaves live at the current key's namespace.
        InitialSettingsValue::Settings(items) => {
            flatten_items(items, key, out);
        }
        // An object array: the schema is the FIRST declared group (later groups
        // are subset default rows), so enumerate that template once at `key[0]`.
        InitialSettingsValue::SettingsArray(groups) => {
            if let Some(first) = groups.first() {
                flatten_items(first, &format!("{key}[0]"), out);
            }
        }
    }
}

/// Resolve a flat storage key to its declared leaf type, tolerating ARBITRARY
/// array indices. Windhawk array settings are dynamic: a mod's source declares a
/// template, but the runtime array can hold any number of elements - the engine
/// stores/reads `key[0]`, `key[1]`, ... unboundedly. An object array's schema is
/// its FIRST declared group (the UI's schema; the domain guarantees later groups
/// are subsets of it), so every index resolves against that first group. So
/// `items[7].icon` is a valid settable key even when the source declares only
/// `items[0]`. Type-checking against the fixed set that
/// [`flatten_setting_key_types`] enumerates would wrongly reject every index
/// past the template; this walks the declared structure instead, accepting any
/// index at an array node and taking the leaf type from the first group.
///
/// Returns `None` when the key names no declared setting: an unknown base name,
/// an index into a non-array (or a missing index into an array), or a path that
/// stops above a scalar leaf.
pub fn resolve_setting_key_type(settings: &InitialSettings, key: &str) -> Option<SettingLeafType> {
    // `group` is the settings list the next segment resolves within; it narrows
    // as we descend into nested groups and array elements.
    let mut group = settings;
    let mut segments = key.split('.').peekable();
    while let Some(segment) = segments.next() {
        let (name, index) = parse_segment(segment)?;
        let item = group.iter().find(|it| it.key == name)?;
        let is_last = segments.peek().is_none();
        match &item.value {
            // Scalars: a leaf. Valid only as the final segment and without an
            // index (an index into a scalar is not a real key).
            InitialSettingsValue::Bool(_) => {
                return (is_last && index.is_none()).then_some(SettingLeafType::Boolean);
            }
            InitialSettingsValue::Number(_) => {
                return (is_last && index.is_none()).then_some(number_leaf_type(item));
            }
            InitialSettingsValue::String(_) => {
                return (is_last && index.is_none()).then_some(string_leaf_type(item));
            }
            // Scalar arrays: `key[i]` is a leaf of the element type for any `i`.
            InitialSettingsValue::NumberArray(_) => {
                return (is_last && index.is_some()).then_some(number_leaf_type(item));
            }
            InitialSettingsValue::StringArray(_) => {
                return (is_last && index.is_some()).then_some(string_leaf_type(item));
            }
            // Nested object: no index; descend into its group for the rest.
            InitialSettingsValue::Settings(inner) => {
                if is_last || index.is_some() {
                    return None;
                }
                group = inner;
            }
            // Object array: `key[i]` selects the schema for any `i` from the
            // FIRST declared group (the UI schema; later groups are subsets of
            // it); descend into that group for the rest of the path.
            InitialSettingsValue::SettingsArray(groups) => {
                let template = groups.first()?;
                if index.is_none() || is_last {
                    return None;
                }
                group = template;
            }
        }
    }
    // Every group/array segment above continues the loop, so reaching here means
    // the key named a non-leaf node (or was empty): not settable.
    None
}

/// Split one dotted key segment into its base name and optional array index:
/// `icon` -> (`icon`, None), `items[3]` -> (`items`, Some(3)). Returns None for a
/// malformed segment (empty name, missing/extra brackets, or a non-numeric
/// index), which the caller treats as an unknown key.
pub(super) fn parse_segment(segment: &str) -> Option<(&str, Option<usize>)> {
    match segment.split_once('[') {
        None => (!segment.is_empty()).then_some((segment, None)),
        Some((name, rest)) => {
            let digits = rest.strip_suffix(']')?;
            if name.is_empty() || digits.is_empty() {
                return None;
            }
            Some((name, Some(digits.parse::<usize>().ok()?)))
        }
    }
}

/// Parse a raw CLI string into the JSON value to store for a setting of the
/// declared `ty` (the TS `parseSettingInput`). A boolean is normalized to the
/// number `1`/`0` the engine stores; a number is range-checked; a string is
/// stored verbatim; a float must be finite decimal text, stored as the
/// shortest text that round-trips it (`1.0` stores `1`), the same rendering
/// the core gives a `$float` default, or the empty text of an unset float,
/// stored as it is. A number of either kind is then held to the item's
/// `$min` / `$max`.
#[track_caller]
pub fn parse_setting_input(key: &str, ty: SettingLeafType, raw: &str) -> Result<Value, CliError> {
    match ty {
        SettingLeafType::Boolean => match raw {
            "true" | "1" => Ok(json!(1)),
            "false" | "0" => Ok(json!(0)),
            _ => Err(CliError::usage(format!(
                "Setting '{key}' is declared as boolean; value must be one of true/false/1/0, got '{raw}'."
            ))),
        },
        SettingLeafType::Number(bounds) => {
            let n = parse_int32_setting(key, raw)?;
            // An i64 in the int32 range is exact as an f64.
            check_bounds(key, raw, n as f64, bounds)?;
            Ok(json!(n))
        }
        SettingLeafType::String => Ok(Value::String(raw.to_owned())),
        SettingLeafType::Float(_) if raw.is_empty() => Ok(Value::String(String::new())),
        SettingLeafType::Float(bounds) => match raw.parse::<f64>() {
            Ok(v) if v.is_finite() => {
                check_bounds(key, raw, v, bounds)?;
                Ok(Value::String(v.to_string()))
            }
            _ => Err(CliError::usage(format!(
                "Setting '{key}' is declared as a decimal number; got '{raw}'."
            ))),
        },
    }
}

/// Read a JSON value into the value to store for a setting of the declared
/// `ty`, by the rules of [`parse_setting_input`]: a number is read as its
/// decimal text, and a string as the text itself, which is also how a value
/// read back from the portable store arrives (`"1"` and `"7"` where the
/// registry gives `1` and `7`). A boolean setting also takes `true` / `false`;
/// a string setting takes only a string. Any other kind is refused, naming
/// the declared type.
#[track_caller]
pub fn parse_setting_json(
    key: &str,
    ty: SettingLeafType,
    value: &Value,
) -> Result<Value, CliError> {
    match (ty, value) {
        (SettingLeafType::String, Value::String(s)) => Ok(Value::String(s.clone())),
        (SettingLeafType::String, _) => Err(json_type_error(key, ty, value)),
        (SettingLeafType::Boolean, Value::Bool(b)) => Ok(json!(i32::from(*b))),
        (_, Value::Number(n)) => parse_setting_input(key, ty, &n.to_string()),
        (_, Value::String(s)) => parse_setting_input(key, ty, s),
        _ => Err(json_type_error(key, ty, value)),
    }
}

/// Refuse a JSON value of a kind a setting of `ty` cannot hold (usage error,
/// exit 2).
#[track_caller]
fn json_type_error(key: &str, ty: SettingLeafType, value: &Value) -> CliError {
    let expected = match ty {
        SettingLeafType::Boolean => "boolean",
        SettingLeafType::Number(_) => "a number",
        SettingLeafType::String => "a string",
        SettingLeafType::Float(_) => "a decimal number",
    };
    let got = match value {
        Value::Array(_) => "an array".to_owned(),
        Value::Object(_) => "an object".to_owned(),
        scalar => scalar.to_string(),
    };
    CliError::usage(format!(
        "Setting '{key}' is declared as {expected}; got {got}."
    ))
}

/// Refuse a parsed number outside the item's declared bounds (usage error,
/// exit 2), naming the side it crossed or the whole range when both are set.
#[track_caller]
fn check_bounds(key: &str, raw: &str, value: f64, bounds: Bounds) -> Result<(), CliError> {
    let below = bounds.min.is_some_and(|min| value < min);
    let above = bounds.max.is_some_and(|max| value > max);
    if !below && !above {
        return Ok(());
    }
    let range = match (bounds.min, bounds.max) {
        (Some(min), Some(max)) => {
            format!("between {} and {}", number_text(min), number_text(max))
        }
        (Some(min), None) => format!("at least {}", number_text(min)),
        (None, Some(max)) => format!("at most {}", number_text(max)),
        (None, None) => unreachable!("a value cannot cross an absent bound"),
    };
    Err(CliError::usage(format!(
        "Setting '{key}' must be {range}, got '{raw}'."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNBOUNDED_NUMBER: SettingLeafType = SettingLeafType::Number(Bounds {
        min: None,
        max: None,
    });
    const UNBOUNDED_FLOAT: SettingLeafType = SettingLeafType::Float(Bounds {
        min: None,
        max: None,
    });

    fn parse(source: &str) -> InitialSettings {
        serde_json::from_str(source).expect("parse initial settings")
    }

    #[test]
    fn flattens_scalars_and_nested_objects_and_arrays() {
        // A scalar boolean, a scalar number, a scalar string, a string array, a
        // nested object, and an array of objects.
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "count", "value": 3},
                {"key": "label", "value": "hi"},
                {"key": "names", "value": ["a", "b"]},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "items", "value": [[{"key": "name", "value": "x"}]]}
            ]"#,
        );
        let flat = flatten_setting_key_types(&settings);

        assert_eq!(flat["flag"], SettingLeafType::Boolean);
        assert_eq!(flat["count"], UNBOUNDED_NUMBER);
        assert_eq!(flat["label"], SettingLeafType::String);
        assert_eq!(flat["names[0]"], SettingLeafType::String);
        assert_eq!(flat["names[1]"], SettingLeafType::String);
        assert_eq!(flat["group.inner"], UNBOUNDED_NUMBER);
        assert_eq!(flat["items[0].name"], SettingLeafType::String);
    }

    #[test]
    fn float_items_flatten_and_resolve_as_float_leaves() {
        // The flag lives on the ITEM, so a `$float` string scalar and each
        // element of a `$float` string array are `Float` leaves, while an
        // unflagged string stays `String`. An object array's rows resolve
        // against the template group's items, which carry the flag.
        let settings = parse(
            r#"[
                {"key": "opacity", "value": "0.85", "float": true},
                {"key": "weights", "value": ["0.25", "1"], "float": true},
                {"key": "label", "value": "hi"},
                {"key": "rows", "value": [[{"key": "scale", "value": "1", "float": true}]]}
            ]"#,
        );
        let flat = flatten_setting_key_types(&settings);
        assert_eq!(flat["opacity"], UNBOUNDED_FLOAT);
        assert_eq!(flat["weights[0]"], UNBOUNDED_FLOAT);
        assert_eq!(flat["weights[1]"], UNBOUNDED_FLOAT);
        assert_eq!(flat["label"], SettingLeafType::String);
        assert_eq!(flat["rows[0].scale"], UNBOUNDED_FLOAT);

        let r = |k: &str| resolve_setting_key_type(&settings, k);
        assert_eq!(r("opacity"), Some(UNBOUNDED_FLOAT));
        assert_eq!(r("weights[9]"), Some(UNBOUNDED_FLOAT));
        assert_eq!(r("label"), Some(SettingLeafType::String));
        assert_eq!(r("rows[3].scale"), Some(UNBOUNDED_FLOAT));
    }

    #[test]
    fn bounded_items_flatten_and_resolve_with_their_bounds() {
        // The bounds live on the ITEM like the float flag: a scalar, each
        // element of an array, and an object array's rows through the template
        // group carry them; an item without them resolves unbounded.
        let settings = parse(
            r#"[
                {"key": "rating", "value": 3, "min": 1, "max": 5},
                {"key": "weights", "value": [1, 2], "max": 10},
                {"key": "opacity", "value": "0.85", "float": true, "min": 0.5},
                {"key": "count", "value": 3},
                {"key": "rows", "value": [[{"key": "scale", "value": "1", "float": true, "min": 0, "max": 2}]]}
            ]"#,
        );
        let bounded = |min: Option<f64>, max: Option<f64>| Bounds { min, max };
        let flat = flatten_setting_key_types(&settings);
        assert_eq!(
            flat["rating"],
            SettingLeafType::Number(bounded(Some(1.0), Some(5.0)))
        );
        assert_eq!(
            flat["weights[1]"],
            SettingLeafType::Number(bounded(None, Some(10.0)))
        );
        assert_eq!(
            flat["opacity"],
            SettingLeafType::Float(bounded(Some(0.5), None))
        );
        assert_eq!(flat["count"], UNBOUNDED_NUMBER);
        assert_eq!(
            flat["rows[0].scale"],
            SettingLeafType::Float(bounded(Some(0.0), Some(2.0)))
        );

        let r = |k: &str| resolve_setting_key_type(&settings, k);
        assert_eq!(
            r("weights[7]"),
            Some(SettingLeafType::Number(bounded(None, Some(10.0))))
        );
        assert_eq!(
            r("rows[3].scale"),
            Some(SettingLeafType::Float(bounded(Some(0.0), Some(2.0))))
        );

        // The valid-keys hint spells the range beside the type.
        assert_eq!(flat["rating"].describe(), "number 1..5");
        assert_eq!(flat["weights[0]"].describe(), "number ..10");
        assert_eq!(flat["opacity"].describe(), "float 0.5..");
        assert_eq!(flat["count"].describe(), "number");
        assert_eq!(SettingLeafType::Boolean.describe(), "boolean");
        assert_eq!(SettingLeafType::String.describe(), "string");
    }

    #[test]
    fn empty_arrays_contribute_no_keys() {
        let settings = parse(r#"[{"key": "empty", "value": []}]"#);
        assert!(flatten_setting_key_types(&settings).is_empty());
    }

    #[test]
    fn resolves_scalars_nested_and_arbitrary_array_indices() {
        // A scalar, a nested group, a scalar array, and an object array declared
        // with a single template element (the common shape, e.g. tray `items`).
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "names", "value": ["a"]},
                {"key": "items", "value": [[
                    {"key": "action", "value": 0},
                    {"key": "icon", "value": ""},
                    {"key": "label", "value": "x"}
                ]]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);

        assert_eq!(r("flag"), Some(SettingLeafType::Boolean));
        assert_eq!(r("group.inner"), Some(UNBOUNDED_NUMBER));

        // A scalar array accepts any index past the one declared element.
        assert_eq!(r("names[0]"), Some(SettingLeafType::String));
        assert_eq!(r("names[9]"), Some(SettingLeafType::String));

        // The reported bug: an object-array child at an index the template does
        // not literally declare still resolves to the template's leaf type.
        assert_eq!(r("items[0].icon"), Some(SettingLeafType::String));
        assert_eq!(r("items[1].icon"), Some(SettingLeafType::String));
        assert_eq!(r("items[42].action"), Some(UNBOUNDED_NUMBER));
    }

    #[test]
    fn object_array_resolves_against_the_first_group_at_any_index() {
        // A multi-group object array (the first group is the full template, the
        // second a subset default row). Every index - and every template key,
        // even one absent from the subset row - resolves against the first group.
        let settings = parse(
            r#"[
                {"key": "buttons", "value": [
                    [{"key": "preset", "value": "custom"}, {"key": "name", "value": ""}],
                    [{"key": "preset", "value": "settings"}]
                ]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);
        assert_eq!(r("buttons[0].preset"), Some(SettingLeafType::String));
        assert_eq!(r("buttons[1].name"), Some(SettingLeafType::String));
        assert_eq!(r("buttons[9].name"), Some(SettingLeafType::String));
    }

    #[test]
    fn rejects_non_leaf_and_malformed_keys() {
        let settings = parse(
            r#"[
                {"key": "flag", "value": true},
                {"key": "group", "value": [{"key": "inner", "value": 1}]},
                {"key": "names", "value": ["a"]},
                {"key": "items", "value": [[{"key": "icon", "value": ""}]]}
            ]"#,
        );
        let r = |k: &str| resolve_setting_key_type(&settings, k);

        assert_eq!(r("nope"), None); // unknown base name
        assert_eq!(r("flag[0]"), None); // index into a scalar
        assert_eq!(r("group"), None); // stops above a group
        assert_eq!(r("names"), None); // scalar array needs an index
        assert_eq!(r("items"), None); // object array needs an index
        assert_eq!(r("items[0]"), None); // object-array element is not a leaf
        assert_eq!(r("items[0].nope"), None); // unknown child of the template
        assert_eq!(r("items[].icon"), None); // empty index
        assert_eq!(r("items[x].icon"), None); // non-numeric index
    }

    #[test]
    fn parses_typed_input() {
        assert_eq!(
            parse_setting_input("k", SettingLeafType::Boolean, "true").unwrap(),
            json!(1)
        );
        assert_eq!(
            parse_setting_input("k", SettingLeafType::Boolean, "0").unwrap(),
            json!(0)
        );
        assert_eq!(
            parse_setting_input("k", UNBOUNDED_NUMBER, "42").unwrap(),
            json!(42)
        );
        assert_eq!(
            parse_setting_input("k", SettingLeafType::String, "raw").unwrap(),
            json!("raw")
        );
    }

    #[test]
    fn float_input_is_stored_as_canonical_decimal_text() {
        for (raw, stored) in [
            ("0.85", "0.85"),
            ("1.0", "1"),
            ("1", "1"),
            ("1.50", "1.5"),
            ("1e3", "1000"),
            (".5", "0.5"),
            ("-2", "-2"),
        ] {
            assert_eq!(
                parse_setting_input("k", UNBOUNDED_FLOAT, raw).unwrap(),
                json!(stored),
                "{raw}"
            );
        }
    }

    #[test]
    fn empty_float_input_is_stored_as_it_is_whatever_the_bounds() {
        let bounded = SettingLeafType::Float(Bounds {
            min: Some(0.5),
            max: Some(1.0),
        });
        for ty in [UNBOUNDED_FLOAT, bounded] {
            assert_eq!(parse_setting_input("k", ty, "").unwrap(), json!(""));
        }
    }

    #[test]
    fn rejects_non_decimal_float_input() {
        // Anything `wcstod` would not read as the intended number is refused
        // before it reaches the store, where the mod would read it as 0.
        for raw in [
            " ", "abc", "1,5", "inf", "-inf", "nan", "NaN", "1 ", " 1", "0x10",
        ] {
            let err = parse_setting_input("k", UNBOUNDED_FLOAT, raw).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{raw:?}");
            assert!(
                err.message().contains("declared as a decimal number"),
                "{raw:?}: {}",
                err.message()
            );
        }
    }

    #[test]
    fn rejects_bad_typed_input() {
        let err = parse_setting_input("k", SettingLeafType::Boolean, "yes").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.message().contains("declared as boolean"));

        let err = parse_setting_input("k", UNBOUNDED_NUMBER, "1.5").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn bounded_input_is_held_to_the_range() {
        let number =
            |min: Option<f64>, max: Option<f64>| SettingLeafType::Number(Bounds { min, max });
        let float =
            |min: Option<f64>, max: Option<f64>| SettingLeafType::Float(Bounds { min, max });
        let refused = |ty: SettingLeafType, raw: &str| {
            let err = parse_setting_input("k", ty, raw).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{raw}");
            err.message().to_owned()
        };

        // Each of the three messages, by which bounds the item declares.
        assert_eq!(
            refused(number(Some(1.0), Some(5.0)), "7"),
            "Setting 'k' must be between 1 and 5, got '7'."
        );
        assert_eq!(
            refused(number(Some(1.0), None), "0"),
            "Setting 'k' must be at least 1, got '0'."
        );
        assert_eq!(
            refused(float(None, Some(0.5)), "0.75"),
            "Setting 'k' must be at most 0.5, got '0.75'."
        );

        // The bounds are inclusive; a boundary value passes on both kinds.
        assert_eq!(
            parse_setting_input("k", number(Some(1.0), Some(5.0)), "5").unwrap(),
            json!(5)
        );
        assert_eq!(
            parse_setting_input("k", number(Some(1.0), Some(5.0)), "1").unwrap(),
            json!(1)
        );
        // A `$float` bound compares numerically, not textually.
        assert_eq!(
            parse_setting_input("k", float(None, Some(0.5)), "0.50").unwrap(),
            json!("0.5")
        );

        // The type check comes first: a non-number is refused as such, not as
        // out of range.
        assert!(refused(number(Some(1.0), Some(5.0)), "x").contains("must be an integer"));
        assert!(refused(float(Some(1.0), None), "x").contains("declared as a decimal number"));
    }

    #[test]
    fn json_input_is_normalized_like_argv_text() {
        let ok = |ty: SettingLeafType, value: Value| parse_setting_json("k", ty, &value).unwrap();

        // A boolean takes JSON true/false and the stored 0/1, as a number or as
        // the portable store's text.
        assert_eq!(ok(SettingLeafType::Boolean, json!(true)), json!(1));
        assert_eq!(ok(SettingLeafType::Boolean, json!(false)), json!(0));
        assert_eq!(ok(SettingLeafType::Boolean, json!(1)), json!(1));
        assert_eq!(ok(SettingLeafType::Boolean, json!("0")), json!(0));

        assert_eq!(ok(UNBOUNDED_NUMBER, json!(-7)), json!(-7));
        assert_eq!(ok(UNBOUNDED_NUMBER, json!("7")), json!(7));

        // A float takes a number or decimal text, stored canonical either way.
        assert_eq!(ok(UNBOUNDED_FLOAT, json!(0.85)), json!("0.85"));
        assert_eq!(ok(UNBOUNDED_FLOAT, json!(1.0)), json!("1"));
        assert_eq!(ok(UNBOUNDED_FLOAT, json!("1.50")), json!("1.5"));
        assert_eq!(ok(UNBOUNDED_FLOAT, json!("")), json!(""));

        // A string is stored verbatim, even when it reads as a number.
        assert_eq!(ok(SettingLeafType::String, json!("7")), json!("7"));
        assert_eq!(ok(SettingLeafType::String, json!("")), json!(""));
    }

    #[test]
    fn rejects_json_input_of_the_wrong_kind() {
        let refused = |ty: SettingLeafType, value: Value| {
            let err = parse_setting_json("k", ty, &value).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{value}");
            err.message().to_owned()
        };

        // A kind the type cannot hold names the declared type.
        assert_eq!(
            refused(SettingLeafType::Boolean, json!(null)),
            "Setting 'k' is declared as boolean; got null."
        );
        assert_eq!(
            refused(UNBOUNDED_NUMBER, json!(true)),
            "Setting 'k' is declared as a number; got true."
        );
        assert_eq!(
            refused(SettingLeafType::String, json!(7)),
            "Setting 'k' is declared as a string; got 7."
        );
        assert_eq!(
            refused(SettingLeafType::String, json!(false)),
            "Setting 'k' is declared as a string; got false."
        );
        assert_eq!(
            refused(UNBOUNDED_FLOAT, json!([1])),
            "Setting 'k' is declared as a decimal number; got an array."
        );
        assert_eq!(
            refused(UNBOUNDED_NUMBER, json!({ "a": 1 })),
            "Setting 'k' is declared as a number; got an object."
        );

        // A number or text the type cannot read gets the argv message.
        assert!(refused(SettingLeafType::Boolean, json!(2)).contains("declared as boolean"));
        assert!(refused(UNBOUNDED_NUMBER, json!(1.5)).contains("must be an integer, got '1.5'"));
        assert!(refused(UNBOUNDED_NUMBER, json!("abc")).contains("must be an integer"));
        assert!(refused(UNBOUNDED_NUMBER, json!(3_000_000_000_i64)).contains("32-bit integer"));
        assert!(refused(UNBOUNDED_FLOAT, json!("abc")).contains("declared as a decimal number"));

        // The bounds hold whichever kind carried the number.
        let rating = SettingLeafType::Number(Bounds {
            min: Some(1.0),
            max: Some(5.0),
        });
        assert_eq!(
            refused(rating, json!(7)),
            "Setting 'k' must be between 1 and 5, got '7'."
        );
    }
}
