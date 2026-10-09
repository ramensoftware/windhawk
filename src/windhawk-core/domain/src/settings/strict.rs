//! The strict rules: what the grammar accepts only so that a block written for
//! a later Windhawk still parses, rejected for a check of the block as
//! written. Opt-in (`extract_initial_settings_strict`), and run last, on a
//! tree every other pass has accepted:
//! - a `$_` key, which the grammar ignores whatever its value;
//! - a `$format` the front-end does not know, or one the item's control does
//!   not read;
//! - a default that is not a value of its `$format` (`123456` for `colorRgb`).
//!
//! The UI draws every row of an object array with its template row, the first
//! row at its path, so a later row's default is held to the template's
//! `$format`, and a `$format` of its own, which no control reads, must repeat
//! the template's.

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use super::validate::{annotation_flag, annotation_value, is_plain_param_key, value_is_numeric};
use super::{float_literal, parse_annotation_key, scalar_key_to_string};

/// The kind of item whose control a format changes.
enum Applies {
    /// A string leaf - a string scalar, or each element of a string array -
    /// that is neither a dropdown nor a `$float`. A default must satisfy
    /// `accepts`; `expected` names the form in an error.
    StringLeaf {
        accepts: fn(&str) -> bool,
        expected: &'static str,
    },
    /// A number, integer or `$float`, with a `$min` below its `$max`.
    Number,
}

const FORMATS: &[(&str, Applies)] = &[
    (
        "colorRgb",
        Applies::StringLeaf {
            accepts: |text| is_hex_color(text, 6),
            expected: "a #RRGGBB color",
        },
    ),
    (
        "colorArgb",
        Applies::StringLeaf {
            accepts: |text| is_hex_color(text, 8),
            expected: "a #AARRGGBB color",
        },
    ),
    (
        "filePath",
        Applies::StringLeaf {
            accepts: is_empty_or_absolute_path,
            expected: "empty or an absolute path",
        },
    ),
    (
        "folderPath",
        Applies::StringLeaf {
            accepts: is_empty_or_absolute_path,
            expected: "empty or an absolute path",
        },
    ),
    (
        "fontFamily",
        Applies::StringLeaf {
            accepts: |_| true,
            expected: "a font family name",
        },
    ),
    (
        "hotkey",
        Applies::StringLeaf {
            accepts: is_empty_or_hotkey,
            expected: "empty or a hotkey such as ctrl+alt+84",
        },
    ),
    ("slider", Applies::Number),
];

pub(super) fn check_settings_array(items: &[Yaml]) -> Result<(), String> {
    check_items(items, "instance", None)
}

/// The group at the template path, whose items a group's items are drawn with
/// by key, and its path; `None` when that is the group itself.
type Templates<'a> = Option<(&'a [Yaml], &'a str)>;

fn check_items(items: &[Yaml], path: &str, templates: Templates) -> Result<(), String> {
    for (i, item) in items.iter().enumerate() {
        let Yaml::Hash(map) = item else {
            continue;
        };
        let item_path = format!("{path}[{i}]");
        let template = templates.and_then(|(group, group_path)| {
            let (j, template) = item_with_same_key(group, map)?;
            Some((template, format!("{group_path}[{j}]")))
        });
        match template {
            Some((template, template_path)) => {
                check_item(map, &item_path, template, &template_path)?
            }
            None => check_item(map, &item_path, map, &item_path)?,
        }
    }
    Ok(())
}

/// The entries of an array value: the items of a group, or the rows of an
/// object array, each drawn with the first row at the template path.
fn check_entries(entries: &[Yaml], path: &str, templates: Templates) -> Result<(), String> {
    if !matches!(entries.first(), Some(Yaml::Array(_))) {
        return check_items(entries, path, templates);
    }
    let (rows, rows_path) = templates.unwrap_or((entries, path));
    let template_row = match rows.first() {
        Some(Yaml::Array(row)) => row.as_slice(),
        _ => &[],
    };
    let template_path = format!("{rows_path}[0]");
    for (j, entry) in entries.iter().enumerate() {
        if let Yaml::Array(row) = entry {
            check_items(
                row,
                &format!("{path}[{j}]"),
                Some((template_row, &template_path)),
            )?;
        }
    }
    Ok(())
}

/// One item's keys and nested items in mapping order, then its `$format`: the
/// one on `template`, the item the UI draws it with.
fn check_item(map: &Hash, path: &str, template: &Hash, template_path: &str) -> Result<(), String> {
    let is_template = std::ptr::eq(map, template);
    let mut param = None;
    for (key, value) in map.iter() {
        let key = scalar_key_to_string(key);
        if key.starts_with("$_") {
            return Err(format!("{path}.{key} is not an allowed property"));
        }
        if !is_plain_param_key(&key) {
            continue;
        }
        let key_path = format!("{path}.{key}");
        if let Yaml::Array(entries) = value {
            let nested_path = format!("{template_path}.{key}");
            let templates = match param_of(template) {
                Some((_, Yaml::Array(nested))) if !is_template => {
                    Some((nested.as_slice(), nested_path.as_str()))
                }
                _ => None,
            };
            check_entries(entries, &key_path, templates)?;
        }
        param = Some((key_path, value));
    }
    let format = annotation_value(template, "format");
    if annotation_value(map, "format").is_some_and(|own| Some(own) != format) {
        return Err(format!(
            "{path}.$format has no effect outside the template row: the form reads \
             {template_path}.$format"
        ));
    }
    match (format, param) {
        (Some(Yaml::String(format)), Some((param_path, value))) => {
            check_format(template, template_path, format, &param_path, value)
        }
        _ => Ok(()),
    }
}

/// The item's setting key and its value.
fn param_of(map: &Hash) -> Option<(String, &Yaml)> {
    map.iter().find_map(|(key, value)| {
        let key = scalar_key_to_string(key);
        is_plain_param_key(&key).then_some((key, value))
    })
}

/// The item of `group` with the setting key of `map`, and its index.
fn item_with_same_key<'a>(group: &'a [Yaml], map: &Hash) -> Option<(usize, &'a Hash)> {
    let (key, _) = param_of(map)?;
    group.iter().enumerate().find_map(|(j, item)| match item {
        Yaml::Hash(item) if param_of(item).is_some_and(|(k, _)| k == key) => Some((j, item)),
        _ => None,
    })
}

fn check_format(
    map: &Hash,
    path: &str,
    format: &str,
    param_path: &str,
    value: &Yaml,
) -> Result<(), String> {
    let Some((_, applies)) = FORMATS.iter().find(|(name, _)| *name == format) else {
        let names: Vec<&str> = FORMATS.iter().map(|(name, _)| *name).collect();
        return Err(format!(
            "{path}.$format must be one of {}: {format}",
            names.join(", ")
        ));
    };
    let float = annotation_flag(map, "float");
    match applies {
        Applies::StringLeaf { accepts, expected } => {
            let dropdown_or_float = [
                ("float", float),
                ("dynamicSelect", annotation_flag(map, "dynamicSelect")),
                ("options", has_options(map)),
            ];
            if let Some((annotation, _)) = dropdown_or_float.iter().find(|(_, set)| *set) {
                return Err(format!(
                    "{path}.$format: {format} has no effect beside ${annotation}"
                ));
            }
            let Some(leaves) = string_leaves(value, param_path) else {
                return Err(format!(
                    "{param_path} must be a string or array of strings to use $format: {format}"
                ));
            };
            for (leaf_path, text) in leaves {
                if !accepts(text) {
                    return Err(format!("{leaf_path} default '{text}' is not {expected}"));
                }
            }
            Ok(())
        }
        Applies::Number => {
            if !value_is_numeric(value, float) {
                return Err(format!(
                    "{param_path} must be a number or array of numbers to use $format: {format}"
                ));
            }
            let bound = |name| annotation_value(map, name).and_then(float_literal);
            match (bound("min"), bound("max")) {
                (Some(min), Some(max)) if min < max => Ok(()),
                (Some(_), Some(_)) => Err(format!(
                    "{path}.$format: {format} needs a $min below its $max"
                )),
                _ => Err(format!("{path}.$format: {format} needs both $min and $max")),
            }
        }
    }
}

/// Whether the item declares `$options`, in any language.
fn has_options(map: &Hash) -> bool {
    map.iter().any(|(key, _)| {
        matches!(
            parse_annotation_key(&scalar_key_to_string(key)),
            Some(("options", _))
        )
    })
}

/// The texts of a string leaf - a string scalar, or each element of a
/// non-empty array of strings - with their paths; `None` for any other value.
fn string_leaves<'a>(value: &'a Yaml, path: &str) -> Option<Vec<(String, &'a str)>> {
    match value {
        Yaml::String(text) => Some(vec![(path.to_owned(), text.as_str())]),
        Yaml::Array(items) if !items.is_empty() => items
            .iter()
            .enumerate()
            .map(|(i, item)| match item {
                Yaml::String(text) => Some((format!("{path}[{i}]"), text.as_str())),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// `#` and exactly `digits` hex digits, in either case.
fn is_hex_color(text: &str, digits: usize) -> bool {
    text.strip_prefix('#')
        .is_some_and(|hex| hex.len() == digits && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Empty, or an absolute Windows path: a drive root (`C:\`, `C:/`) or a UNC
/// path (`\\server\share`).
fn is_empty_or_absolute_path(text: &str) -> bool {
    let is_separator = |b: u8| b == b'\\' || b == b'/';
    match text.as_bytes() {
        [] => true,
        [drive, b':', separator, ..] => drive.is_ascii_alphabetic() && is_separator(*separator),
        [first, second, _, ..] => is_separator(*first) && is_separator(*second),
        _ => false,
    }
}

/// Empty (no hotkey), or `[ctrl+][alt+][shift+][win+]<vk>`: the modifiers
/// lowercase and in that order, then a virtual-key code in decimal, 1 to 254,
/// with no leading zero.
fn is_empty_or_hotkey(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let mut parts: Vec<&str> = text.split('+').collect();
    let Some(vk) = parts.pop() else {
        return false;
    };
    // `any` consumes the modifiers up to the one it finds, so each part must
    // name a modifier after the previous part's.
    let mut modifiers = ["ctrl", "alt", "shift", "win"].into_iter();
    parts.into_iter().all(|part| modifiers.any(|m| m == part))
        && !vk.starts_with('0')
        && vk.bytes().all(|b| b.is_ascii_digit())
        && vk.parse::<u8>().is_ok_and(|vk| vk != u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::super::{extract_initial_settings, extract_initial_settings_strict};

    fn settings_src(yaml: &str) -> String {
        format!("// ==WindhawkModSettings==\n/*\n{yaml}\n*/\n// ==/WindhawkModSettings==\n")
    }

    fn strict_error(yaml: &str) -> String {
        let src = settings_src(yaml);
        assert!(
            extract_initial_settings(&src, "en").is_ok(),
            "the lenient parse must accept {yaml:?}"
        );
        extract_initial_settings_strict(&src, "en")
            .expect_err(yaml)
            .to_string()
    }

    fn assert_strict_ok(yaml: &str) {
        let src = settings_src(yaml);
        let strict = extract_initial_settings_strict(&src, "en");
        assert!(strict.is_ok(), "{yaml:?}: {strict:?}");
        assert_eq!(strict, extract_initial_settings(&src, "en"), "{yaml:?}");
    }

    #[test]
    fn a_block_with_every_format_in_place_parses_as_it_does_leniently() {
        assert_strict_ok(concat!(
            "- rgb: '#3399ff'\n",
            "  $format: colorRgb\n",
            "- argb: ['#80FFFFFF', '#00000000']\n",
            "  $format: colorArgb\n",
            "- file: ''\n",
            "  $format: filePath\n",
            "- folder: 'C:\\Windows'\n",
            "  $format: folderPath\n",
            "- font: Segoe UI\n",
            "  $format: fontFamily\n",
            "- key: ctrl+alt+84\n",
            "  $format: hotkey\n",
            "- level: 5\n",
            "  $min: 0\n",
            "  $max: 10\n",
            "  $format: slider\n",
            "- opacity: '0.85'\n",
            "  $float: true\n",
            "  $min: 0\n",
            "  $max: 1\n",
            "  $format: slider\n",
            "- plain: x",
        ));
    }

    #[test]
    fn underscored_keys_are_rejected_at_any_depth() {
        for (yaml, cause) in [
            (
                "- a: x\n  $_new: 1",
                "instance[0].$_new is not an allowed property",
            ),
            (
                "- a: x\n  $name: A\n  $_name:fr: B",
                "instance[0].$_name:fr is not an allowed property",
            ),
            (
                "- g:\n  - inner: 1\n    $_new: 1",
                "instance[0].g[0].$_new is not an allowed property",
            ),
            (
                "- rows:\n  - - a: 1\n  - - a: 2\n      $_new: 1",
                "instance[0].rows[1][0].$_new is not an allowed property",
            ),
            (
                "- a: x\n  #! $_new: 1\n  #! $format: colorRgb",
                "instance[0].$_new is not an allowed property",
            ),
        ] {
            assert_eq!(
                strict_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
    }

    #[test]
    fn the_lenient_errors_come_first() {
        let src = settings_src("- a: x\n  $_new: 1\n  $bogus: 1");
        assert_eq!(
            extract_initial_settings_strict(&src, "en")
                .unwrap_err()
                .to_string(),
            "Failed to parse settings: instance[0].$bogus is not an allowed property"
        );
    }

    #[test]
    fn an_unknown_format_is_rejected() {
        for format in ["colorRGB", "color", "x"] {
            assert_eq!(
                strict_error(&format!("- a: x\n  $format: {format}")),
                format!(
                    "Failed to parse settings: instance[0].$format must be one of colorRgb, \
                     colorArgb, filePath, folderPath, fontFamily, hotkey, slider: {format}"
                )
            );
        }
    }

    #[test]
    fn a_string_format_on_another_value_type_is_rejected() {
        for value in ["1", "true", "[1, 2]", "\n  - inner: x"] {
            assert_eq!(
                strict_error(&format!("- a: {value}\n  $format: fontFamily")),
                "Failed to parse settings: instance[0].a must be a string or array of strings \
                 to use $format: fontFamily",
                "{value}"
            );
        }
    }

    #[test]
    fn a_string_format_beside_a_dropdown_or_float_is_rejected() {
        for (annotations, beside) in [
            ("$options:\n  - a: A\n  - b: B", "$options"),
            ("$options:fr:\n  - a: A\n  - b: B", "$options"),
            ("$dynamicSelect: true", "$dynamicSelect"),
            ("$float: true", "$float"),
        ] {
            let value = if beside == "$float" { "'1.5'" } else { "a" };
            assert_eq!(
                strict_error(&format!("- x: {value}\n  {annotations}\n  $format: hotkey")),
                format!(
                    "Failed to parse settings: instance[0].$format: hotkey has no effect beside {beside}"
                ),
                "{annotations}"
            );
        }
    }

    #[test]
    fn a_slider_needs_a_number_with_both_bounds() {
        for (yaml, cause) in [
            (
                "- a: x\n  $format: slider",
                "instance[0].a must be a number or array of numbers to use $format: slider",
            ),
            (
                "- a: 1\n  $format: slider",
                "instance[0].$format: slider needs both $min and $max",
            ),
            (
                "- a: 1\n  $min: 0\n  $format: slider",
                "instance[0].$format: slider needs both $min and $max",
            ),
            (
                "- a: 1\n  $min: 1\n  $max: 1\n  $format: slider",
                "instance[0].$format: slider needs a $min below its $max",
            ),
        ] {
            assert_eq!(
                strict_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        assert_strict_ok("- a: [1, 2]\n  $min: 0\n  $max: 9\n  $format: slider");
    }

    #[test]
    fn a_default_must_be_a_value_of_its_format() {
        for (format, expected, values) in [
            (
                "colorRgb",
                "a #RRGGBB color",
                &["'123456'", "'#12345'", "'#GG0000'", "''", "'#80FFFFFF'"][..],
            ),
            ("colorArgb", "a #AARRGGBB color", &["'#FFFFFF'"]),
            (
                "filePath",
                "empty or an absolute path",
                &["'sounds\\ding.wav'", "'%windir%\\ding.wav'"],
            ),
            ("folderPath", "empty or an absolute path", &["'C:'"]),
            (
                "hotkey",
                "empty or a hotkey such as ctrl+alt+84",
                &[
                    "Ctrl+84",
                    "alt+ctrl+84",
                    "ctrl+ctrl+84",
                    "ctrl+T",
                    "ctrl+084",
                    "'0'",
                    "'255'",
                    "ctrl+",
                ],
            ),
        ] {
            for value in values {
                let text = value.trim_matches('\'');
                assert_eq!(
                    strict_error(&format!("- a: {value}\n  $format: {format}")),
                    format!(
                        "Failed to parse settings: instance[0].a default '{text}' is not {expected}"
                    ),
                    "{format}: {value}"
                );
            }
        }
        for (format, value) in [
            ("colorRgb", "'#AbCdEf'"),
            ("colorArgb", "'#80ffffff'"),
            ("filePath", "'c:/x/y.wav'"),
            ("filePath", "'\\\\server\\share\\y.wav'"),
            ("fontFamily", "''"),
            ("hotkey", "''"),
            ("hotkey", "'115'"),
            ("hotkey", "ctrl+alt+shift+win+254"),
            ("hotkey", "shift+1"),
        ] {
            assert_strict_ok(&format!("- a: {value}\n  $format: {format}"));
        }
    }

    #[test]
    fn each_element_of_a_string_array_is_checked() {
        assert_eq!(
            strict_error("- a: ['#FFFFFF', 'FFFFFF']\n  $format: colorRgb"),
            "Failed to parse settings: instance[0].a[1] default 'FFFFFF' is not a #RRGGBB color"
        );
    }

    #[test]
    fn formats_are_checked_in_nested_groups_and_every_object_array_row() {
        for (yaml, cause) in [
            (
                "- g:\n  - c: '#FFF'\n    $format: colorRgb",
                "instance[0].g[0].c default '#FFF' is not a #RRGGBB color",
            ),
            (
                "- rows:\n  - - c: '#FFFFFF'\n      $format: colorRgb\n  - - c: red",
                "instance[0].rows[1][0].c default 'red' is not a #RRGGBB color",
            ),
            (
                concat!(
                    "- rows:\n",
                    "  - - g:\n",
                    "      - c: '#FFFFFF'\n",
                    "        $format: colorRgb\n",
                    "  - - g:\n",
                    "      - c: red",
                ),
                "instance[0].rows[1][0].g[0].c default 'red' is not a #RRGGBB color",
            ),
            (
                concat!(
                    "- rows:\n",
                    "  - - sub:\n",
                    "      - - c: '#FFFFFF'\n",
                    "          $format: colorRgb\n",
                    "  - - sub:\n",
                    "      - - c: '#000000'\n",
                    "      - - c: red",
                ),
                "instance[0].rows[1][0].sub[1][0].c default 'red' is not a #RRGGBB color",
            ),
        ] {
            assert_eq!(
                strict_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        assert_strict_ok(concat!(
            "- rows:\n",
            "  - - c: '#FFFFFF'\n",
            "      $format: colorRgb\n",
            "    - n: 1\n",
            "  - - n: 2\n",
            "    - c: '#000000'\n",
            "  - - n: 3",
        ));
    }

    #[test]
    fn a_format_outside_the_template_row_must_repeat_the_templates() {
        for (yaml, cause) in [
            (
                "- rows:\n  - - c: x\n  - - c: '#FFFFFF'\n      $format: colorRgb",
                "instance[0].rows[1][0].$format has no effect outside the template row: \
                 the form reads instance[0].rows[0][0].$format",
            ),
            (
                concat!(
                    "- rows:\n",
                    "  - - c: '#FFFFFF'\n",
                    "      $format: colorRgb\n",
                    "  - - c: '#000000'\n",
                    "      $format: colorArgb",
                ),
                "instance[0].rows[1][0].$format has no effect outside the template row: \
                 the form reads instance[0].rows[0][0].$format",
            ),
            (
                concat!(
                    "- rows:\n",
                    "  - - sub:\n",
                    "      - - c: x\n",
                    "  - - sub:\n",
                    "      - - c: '#FFFFFF'\n",
                    "          $format: colorRgb",
                ),
                "instance[0].rows[1][0].sub[0][0].$format has no effect outside the template \
                 row: the form reads instance[0].rows[0][0].sub[0][0].$format",
            ),
        ] {
            assert_eq!(
                strict_error(yaml),
                format!("Failed to parse settings: {cause}"),
                "{yaml}"
            );
        }
        assert_strict_ok(concat!(
            "- rows:\n",
            "  - - c: '#FFFFFF'\n",
            "      $format: colorRgb\n",
            "    - sub:\n",
            "      - - k: ctrl+84\n",
            "          $format: hotkey\n",
            "  - - c: '#000000'\n",
            "      $format: colorRgb\n",
            "    - sub:\n",
            "      - - k: ''\n",
            "          $format: hotkey\n",
            "      - - k: alt+65\n",
            "          $format: hotkey",
        ));
    }
}
