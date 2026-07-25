//! Flat-key settings resolution against the parsed `==WindhawkModSettings==`
//! tree: the declared scalar type (boolean, number, or string) of a flattened
//! settings key, and its canonical position for export ordering.
//!
//! Both are what `core`'s export canonicalization needs and neither is served by
//! the engine flattener (`flatten.rs`): the flattener collapses booleans into
//! the stored `0`/`1` integer (losing the bool/number/string distinction an
//! export types against) and emits only the source's DEFAULT rows (so it cannot
//! place a store key whose array index runs past those defaults - a user can add
//! list entries). This walk keeps the type distinction and resolves any index by
//! collapsing it to the element template - `List[7]` against the array's element
//! type, `Matrix[7].cell` against the first group's `cell`.
//!
//! The canonical order is the source's flattened declaration order with array
//! entries by ascending index. It is expressed as a sort key
//! ([`FlatSetting::order`]): the sequence of positions along the key's path - a
//! field's declaration index within its group, then an array subscript. Sort
//! the store's keys by this and they land in canonical order regardless of how
//! the backend enumerated them, which is what makes two installs with identical
//! semantic state export byte-identical archives. A key the template declares no
//! scalar leaf for - a stale key left by a since-changed mod version, or one that
//! lands on a group rather than a leaf - resolves to `None` and is dropped.

use crate::model::{SettingItem, SettingValue};

/// The declared scalar type of a flat settings key. Booleans are kept distinct
/// from numbers (both store as an integer, but an export types a boolean as
/// `0`/`1` and a number as an `i32`), unlike the engine flattener, which stores
/// both as `EngineSettingValue::Int`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlatSettingType {
    Bool,
    Number,
    String,
}

/// A resolved flat key: its declared type and its canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatSetting {
    pub ty: FlatSettingType,
    /// The canonical sort key: positions along the key's path, a field's
    /// declaration index then an array subscript at each level. Ordering store
    /// keys by this (lexicographically) yields the source's flattened
    /// declaration order, array entries by ascending index.
    pub order: Vec<usize>,
}

/// Resolve `flat_key` against the parsed settings tree, returning its declared
/// type and canonical order, or `None` when the tree declares no scalar leaf at
/// that key (an unknown/stale key, a key landing on a group, or a malformed
/// key).
pub fn resolve_flat_setting(items: &[SettingItem], flat_key: &str) -> Option<FlatSetting> {
    let mut order = Vec::new();
    let ty = resolve(items, flat_key, &mut order)?;
    Some(FlatSetting { ty, order })
}

/// The declared type of `flat_key`, when only the type is needed. `None` under
/// the same conditions as [`resolve_flat_setting`].
pub fn resolve_flat_setting_type(items: &[SettingItem], flat_key: &str) -> Option<FlatSettingType> {
    resolve_flat_setting(items, flat_key).map(|s| s.ty)
}

/// Whether `flat_key` is well-formed flat notation: `.`-joined segments of
/// `[0-9A-Za-z_-]`, each optionally carrying a `[<digits>]` subscript. This is
/// the grammar alone, with no settings tree to resolve against -
/// [`resolve_flat_setting`] answers the stronger "does this tree declare a leaf
/// there", which a caller holding no parsed source cannot ask.
///
/// The charset is the schema's parameter-key charset, so every key the
/// flattener can emit passes. A caller that hands keys to a storage backend
/// wants this: a name outside the grammar is not merely unknown, it may not be
/// expressible in the store at all.
pub fn is_valid_flat_key(flat_key: &str) -> bool {
    let mut rest = flat_key;
    loop {
        // `split_segment` consumes a trailing `.` into an empty remainder, which
        // reads as end-of-key below, so the dangling separator is caught here.
        if rest.ends_with('.') {
            return false;
        }
        let Some((name, _, tail)) = split_segment(rest) else {
            return false;
        };
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return false;
        }
        if tail.is_empty() {
            return true;
        }
        rest = tail;
    }
}

/// Match the first segment's name against the group, record its declaration
/// index, then descend into the value it names.
fn resolve(items: &[SettingItem], key: &str, order: &mut Vec<usize>) -> Option<FlatSettingType> {
    let (name, index, rest) = split_segment(key)?;
    let (decl_index, item) = items.iter().enumerate().find(|(_, it)| it.key == name)?;
    order.push(decl_index);
    resolve_in_value(&item.value, index, rest, order)
}

/// Match the segment's shape - subscripted or not, terminal (nothing after it)
/// or with a `.child` remainder - against the value it lands on, recording an
/// array subscript in the order as it goes. Any shape/value mismatch is `None`.
fn resolve_in_value(
    value: &SettingValue,
    index: Option<usize>,
    rest: &str,
    order: &mut Vec<usize>,
) -> Option<FlatSettingType> {
    let terminal = rest.is_empty();
    match (value, index) {
        // Scalars: a bare terminal segment (`Scalar`).
        (SettingValue::Bool(_), None) if terminal => Some(FlatSettingType::Bool),
        (SettingValue::Number(_), None) if terminal => Some(FlatSettingType::Number),
        (SettingValue::String(_), None) if terminal => Some(FlatSettingType::String),
        // Scalar arrays: an indexed terminal segment (`List[i]`); every element
        // shares the array's element type.
        (SettingValue::NumberArray(_), Some(i)) if terminal => {
            order.push(i);
            Some(FlatSettingType::Number)
        }
        (SettingValue::StringArray(_), Some(i)) if terminal => {
            order.push(i);
            Some(FlatSettingType::String)
        }
        // A nested group (`Group.child`): recurse into the group.
        (SettingValue::Settings(inner), None) if !terminal => resolve(inner, rest, order),
        // An object array (`Array[i].child`): recurse against the element
        // template - the first group; later groups are validated to be
        // type-compatible subsets, so the first carries every declared key.
        (SettingValue::SettingsArray(groups), Some(i)) if !terminal => {
            order.push(i);
            resolve(groups.first()?, rest, order)
        }
        _ => None,
    }
}

/// Split a flat key into its first segment and the remainder, returning
/// `(name, index, rest)`: `name` is the segment's key, `index` its `[i]`
/// subscript (or `None`), and `rest` the path after this segment with the `.`
/// separator (or the closing `]`) consumed - empty at the end of the key. `None`
/// for a malformed key: empty, a missing/empty/non-numeric/overflowing
/// subscript, or trailing junk after `]`.
fn split_segment(key: &str) -> Option<(&str, Option<usize>, &str)> {
    if key.is_empty() {
        return None;
    }
    // The name runs up to the first structural character; keys never contain a
    // literal `.` or `[` (the schema restricts them to `[0-9A-Za-z_-]`), so this
    // splits cleanly.
    let name_end = key.find(['.', '[']).unwrap_or(key.len());
    let name = &key[..name_end];
    if name.is_empty() {
        return None;
    }
    let after = &key[name_end..];
    if let Some(after_open) = after.strip_prefix('[') {
        // A subscript: digits up to `]`, then the end of the key or `.rest`.
        let close = after_open.find(']')?;
        let digits = &after_open[..close];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // An index too large to store is treated as unresolvable (dropped),
        // like a stale key; it cannot occur in a real store.
        let index: usize = digits.parse().ok()?;
        let tail = &after_open[close + 1..];
        let rest = match tail.strip_prefix('.') {
            Some(rest) => rest,
            None if tail.is_empty() => "",
            // Anything other than end-of-key or `.` after `]` is malformed.
            None => return None,
        };
        Some((name, Some(index), rest))
    } else if let Some(rest) = after.strip_prefix('.') {
        Some((name, None, rest))
    } else {
        // `name_end == key.len()`, so `after` is empty: a bare terminal name.
        Some((name, None, ""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a settings YAML body into the typed tree the resolver walks.
    fn parse(yaml: &str) -> Vec<SettingItem> {
        let src =
            format!("// ==WindhawkModSettings==\n/*\n{yaml}\n*/\n// ==/WindhawkModSettings==\n");
        super::super::extract_initial_settings(&src, "en")
            .unwrap()
            .unwrap()
    }

    /// The canonical order of `key`, or `None` when it does not resolve.
    fn order(items: &[SettingItem], key: &str) -> Option<Vec<usize>> {
        resolve_flat_setting(items, key).map(|s| s.order)
    }

    #[test]
    fn resolves_scalar_types() {
        let items = parse("- boolOpt: true\n- numOpt: 5\n- strOpt: hi");
        assert_eq!(
            resolve_flat_setting_type(&items, "boolOpt"),
            Some(FlatSettingType::Bool)
        );
        assert_eq!(
            resolve_flat_setting_type(&items, "numOpt"),
            Some(FlatSettingType::Number)
        );
        assert_eq!(
            resolve_flat_setting_type(&items, "strOpt"),
            Some(FlatSettingType::String)
        );
    }

    #[test]
    fn resolves_nested_group_children() {
        let items = parse("- group:\n  - inner: true\n  - label: hi");
        assert_eq!(
            resolve_flat_setting_type(&items, "group.inner"),
            Some(FlatSettingType::Bool)
        );
        assert_eq!(
            resolve_flat_setting_type(&items, "group.label"),
            Some(FlatSettingType::String)
        );
    }

    #[test]
    fn resolves_deeply_nested_group_children() {
        let items = parse("- outer:\n  - inner:\n    - leaf: 3");
        assert_eq!(
            resolve_flat_setting_type(&items, "outer.inner.leaf"),
            Some(FlatSettingType::Number)
        );
        assert_eq!(order(&items, "outer.inner.leaf"), Some(vec![0, 0, 0]));
    }

    #[test]
    fn resolves_scalar_array_elements_collapsing_the_index() {
        let items = parse("- nums:\n  - 1\n  - 2\n- names:\n  - x");
        assert_eq!(
            resolve_flat_setting_type(&items, "nums[0]"),
            Some(FlatSettingType::Number)
        );
        // An index past the declared defaults still resolves - it collapses to
        // the element type.
        assert_eq!(
            resolve_flat_setting_type(&items, "nums[7]"),
            Some(FlatSettingType::Number)
        );
        assert_eq!(
            resolve_flat_setting_type(&items, "names[3]"),
            Some(FlatSettingType::String)
        );
    }

    #[test]
    fn resolves_object_array_child_against_the_first_group() {
        let items = parse("- matrix:\n  - - cell: a\n  - - cell: b");
        assert_eq!(
            resolve_flat_setting_type(&items, "matrix[0].cell"),
            Some(FlatSettingType::String)
        );
        // An index past the declared rows collapses to the first (template) group.
        assert_eq!(
            resolve_flat_setting_type(&items, "matrix[9].cell"),
            Some(FlatSettingType::String)
        );
    }

    #[test]
    fn resolves_a_scalar_array_inside_an_object_array() {
        let items = parse("- rows:\n  - - nums:\n      - 1\n      - 2");
        assert_eq!(
            resolve_flat_setting_type(&items, "rows[0].nums[5]"),
            Some(FlatSettingType::Number)
        );
    }

    #[test]
    fn unknown_or_non_leaf_keys_are_none() {
        let items = parse("- group:\n  - inner: true\n- scalar: 1");
        // A stale/unknown key.
        assert_eq!(resolve_flat_setting(&items, "missing"), None);
        // A group is not a scalar leaf.
        assert_eq!(resolve_flat_setting(&items, "group"), None);
        // A child of a group that the group does not declare.
        assert_eq!(resolve_flat_setting(&items, "group.nope"), None);
        // Indexing a scalar is a shape mismatch.
        assert_eq!(resolve_flat_setting(&items, "scalar[0]"), None);
        // A child of a scalar is a shape mismatch.
        assert_eq!(resolve_flat_setting(&items, "scalar.child"), None);
    }

    #[test]
    fn malformed_keys_are_none() {
        let items = parse("- nums:\n  - 1");
        assert_eq!(resolve_flat_setting(&items, ""), None);
        assert_eq!(resolve_flat_setting(&items, "nums[]"), None);
        assert_eq!(resolve_flat_setting(&items, "nums[x]"), None);
        assert_eq!(resolve_flat_setting(&items, "nums[0]x"), None);
        assert_eq!(resolve_flat_setting(&items, ".nums"), None);
        assert_eq!(resolve_flat_setting(&items, "[0]"), None);
    }

    #[test]
    fn flat_key_grammar_accepts_every_shape_the_flattener_emits() {
        for key in [
            "Scalar",
            "a-b_C9",
            "group.inner",
            "outer.inner.leaf",
            "list[0]",
            "list[10]",
            "matrix[2].cell",
            "rows[0].nums[5]",
        ] {
            assert!(is_valid_flat_key(key), "{key} must be valid");
        }
    }

    #[test]
    fn flat_key_grammar_rejects_malformed_and_unstorable_keys() {
        for key in [
            // Malformed notation.
            "",
            ".",
            "a.",
            ".a",
            "a..b",
            "a[]",
            "a[x]",
            "a[0]x",
            "[0]",
            // Outside the parameter-key charset. These matter beyond
            // well-formedness: the key is handed to a storage backend as a value
            // name, and the portable INI cannot express a name carrying a line
            // break, an `=`, or a leading `[` without writing extra lines into a
            // file that also holds the mod's `[Mod]` config.
            "[Mod]",
            "a\r\n[Mod]\r\nLibraryFileName=evil.dll\r\nb",
            "a\nb",
            "a=b",
            "a b",
            " a",
            "a;b",
            "a/b",
            "a\\b",
        ] {
            assert!(!is_valid_flat_key(key), "{key:?} must be rejected");
        }
    }

    #[test]
    fn order_encodes_declaration_index_then_array_index() {
        // a: scalar (decl 0); matrix: object array (decl 1, element
        // [cell (0), label (1)]); b: scalar (decl 2).
        let items = parse("- a: 1\n- matrix:\n  - - cell: x\n    - label: y\n- b: 2");
        assert_eq!(order(&items, "a"), Some(vec![0]));
        assert_eq!(order(&items, "matrix[0].cell"), Some(vec![1, 0, 0]));
        assert_eq!(order(&items, "matrix[0].label"), Some(vec![1, 0, 1]));
        assert_eq!(order(&items, "matrix[1].cell"), Some(vec![1, 1, 0]));
        assert_eq!(order(&items, "b"), Some(vec![2]));
    }

    #[test]
    fn sorting_store_keys_by_order_yields_canonical_declaration_order() {
        // The load-bearing property: store keys in any enumeration order - here
        // shuffled, with array rows past the single declared default and a stale
        // key mixed in - drop the stale one and sort to the canonical sequence.
        let items =
            parse("- a: 1\n- list:\n  - 10\n- matrix:\n  - - cell: x\n    - label: y\n- b: hi");
        let mut keys = vec![
            "b",
            "matrix[2].label",
            "list[0]",
            "matrix[0].label",
            "a",
            "list[2]",
            "matrix[0].cell",
            "matrix[2].cell",
            "stale",
        ];
        keys.retain(|k| resolve_flat_setting(&items, k).is_some());
        keys.sort_by_key(|k| resolve_flat_setting(&items, k).unwrap().order);
        assert_eq!(
            keys,
            vec![
                "a",
                "list[0]",
                "list[2]",
                "matrix[0].cell",
                "matrix[0].label",
                "matrix[2].cell",
                "matrix[2].label",
                "b",
            ]
        );
    }
}
