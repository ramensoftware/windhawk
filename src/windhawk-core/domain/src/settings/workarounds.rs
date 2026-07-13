//! Settings-block compatibility fixups for already-published mod VERSIONS whose
//! YAML js-yaml accepts but yaml-rust2 (stricter, and int32-only) rejects,
//! found by the corpus run over every published version. Each fixup is pinned
//! to the EXACT (mod id, version) tuples that need it, so a future release of
//! the mod does NOT inherit the shim and is forced to be authored cleanly.
//! These keep shipped mods working; they are not a general parser relaxation,
//! and the underlying yaml-rust2 / int32 rules are unchanged. The caller gates
//! this to store installs only (see the parent module doc); a locally-authored
//! mod never reaches here, so authoring a fresh mod under a shimmed (id,
//! version) still errors.
//!
//! This pre-parse stage takes and returns the RAW string block (it runs before
//! YAML parsing, between block extraction and the parse), so the
//! surrogate/multiline fixes can repair what yaml-rust2 would otherwise reject.
//!
//! The two PARSE divergences (surrogate escapes, multi-line quoted continuations)
//! are fixed with generic transforms - the content varies across a mod's
//! versions (different emoji, single- vs double-quoted, 0- vs 2-space
//! continuations), so a literal-string fixup per version would be fragile. The
//! number-VALIDATION divergences (a float or out-of-range integer) are value
//! specific and stable across the affected versions, so they are literal
//! replacements (line-ending agnostic, kept within a line).
//!
//! The four former keyed lookups are data-driven into ONE `WORKAROUNDS` table
//! of product rows (each a `combine_surrogates` bool AND a `replacements` list,
//! the independent composable steps), one row per DISTINCT payload, each keyed
//! by a SET of (id, versions) so a shared payload lives once. Each row is a
//! NAMED const carrying its full rationale prose, so the intent the four named
//! functions documented for free survives the consolidation and stays bound to
//! a thing a maintainer must touch to add a version.

use std::borrow::Cow;

use crate::mod_id::{ModId, Version};

/// One workaround class: the (id, versions) it covers, plus the two independent
/// composable steps to apply (a PRODUCT, not a sum - a matched key can in
/// principle need both). `combine_surrogates` is a `bool` standing in for the
/// one transform-style fixup today; a second transform would get a second NAMED
/// bool, not a `Vec<fn>` (the fn-pointer indirection this set rejects).
struct Workaround {
    /// The (id, versions) tuples this row covers; the prose lives once per row
    /// regardless of how many versions/ids share the payload.
    keys: &'static [(&'static str, &'static [&'static str])],
    combine_surrogates: bool,
    replacements: &'static [(&'static str, &'static str)],
}

impl Workaround {
    fn matches(&self, id: &str, version: &str) -> bool {
        self.keys
            .iter()
            .any(|(rid, versions)| *rid == id && versions.contains(&version))
    }
}

/// Category: surrogate. Versions whose settings embed emoji as UTF-16
/// surrogate-pair `\u` escapes. taskbar-clock-customization added the weather
/// emoji in 1.6 and still ships them; its next release must drop them or write
/// them as a literal codepoint / `\U` escape.
const SURROGATE_TASKBAR_CLOCK: Workaround = Workaround {
    keys: &[(
        "taskbar-clock-customization",
        &[
            "1.6", "1.6.1", "1.6.2", "1.6.3", "1.7", "1.7.1", "1.7.2", "1.7.3", "1.7.4",
        ],
    )],
    combine_surrogates: true,
    replacements: &[],
};

/// Category: multiline. A multi-line double-quoted scalar whose continuation
/// js-yaml folds but yaml-rust2 rejects for indentation: collapse it to the
/// single-line fold form (a literal replacement - a quote-context fold over the
/// whole block is too fragile across mods, e.g. an apostrophe in a plain
/// scalar). The office-ui mods wrap a double-quoted `$description` after an
/// escaped `\n` at a 2-space indent (every such en + zh-CN line). The two ids
/// (`office-ui-reverter` and `-universal`) share this exact payload.
const OFFICE_UI_MULTILINE: Workaround = Workaround {
    keys: &[
        ("office-ui-reverter", &["1.0.0"]),
        ("office-ui-reverter-universal", &["1.0", "1.0.1", "1.1.0"]),
    ],
    combine_surrogates: false,
    replacements: &[("\\n\r\n  ", "\\n "), ("\\n\n  ", "\\n ")],
};

/// Category: multiline. explorer-double-f2-rename-extension 2.1 wraps a
/// SINGLE-quoted scalar across a break at a ZERO indent (a different shape from
/// the office-ui 2-space double-quoted case, so its own payload).
const EXPLORER_DOUBLE_F2_MULTILINE: Workaround = Workaround {
    keys: &[("explorer-double-f2-rename-extension", &["2.1"])],
    combine_surrogates: false,
    replacements: &[
        ("triple\r\nF2?", "triple F2?"),
        ("triple\nF2?", "triple F2?"),
    ],
};

/// Category: number. A float default js-yaml coerces to an integer but
/// yaml-rust2 rejects (the int32 / no-float strictness). Stable across the
/// affected versions, so a literal replacement is safe.
const TASKBAR_MUSIC_LOUNGE_NUMBER: Workaround = Workaround {
    keys: &[("taskbar-music-lounge", &["4.0", "4.0.1", "4.0.2"])],
    combine_surrogates: false,
    replacements: &[("ButtonScale: 1.0", "ButtonScale: 1")],
};

/// Category: number. The same float-default case as taskbar-music-lounge, for a
/// different value (its own payload).
const UIRIBBON_INSETTING_NUMBER: Workaround = Workaround {
    keys: &[("uiribbon-insetting-fix", &["1.0"])],
    combine_surrogates: false,
    replacements: &[(
        "custom_caption_button_width_ratio: 5.0",
        "custom_caption_button_width_ratio: 5",
    )],
};

/// Category: number. A uint32 ARGB color out of i32 range whose i32 bitcast
/// stores the same 32 bits the mod reads back as UINT32 (and the same value the
/// TS engine store lands on). Stable across the affected versions, so a literal
/// replacement is safe.
const NUMBERED_TASKBAR_COLORS: Workaround = Workaround {
    keys: &[("numbered-taskbar", &["1.0.0"])],
    combine_surrogates: false,
    replacements: &[
        ("fontColor: 0xFFFFFFFF", "fontColor: -1"),
        ("outlineColor: 0xFF000000", "outlineColor: -16777216"),
    ],
};

/// Category: duplicate-id. A shipped version whose settings repeat one parameter
/// key across sibling items, which js-yaml / the TS engine collapse
/// last-write-wins but we now reject (the `validate::reject_duplicate_ids`
/// strictness). scroll-window-opacity 1.0.3 writes `modifierKey` as an array of
/// six `{value, $name}` items - a malformed dropdown - that all flatten to the
/// one engine key `modifierKey.value`; the engine store keeps the last
/// (`shift`), and the mod's C++ reads `modifierKey` (not `modifierKey.value`) so
/// the value is never used anyway (it always falls back to ctrl+alt). Drop the
/// first five options so only `value: shift` remains: the engine store state is
/// byte-identical and the mod behaves exactly as before. (Not fixing the mod - a
/// clean re-author would key the dropdown at `modifierKey` to match the C++.)
const SCROLL_WINDOW_OPACITY_DUPLICATE: Workaround = Workaround {
    keys: &[("scroll-window-opacity", &["1.0.3"])],
    combine_surrogates: false,
    replacements: &[
        (
            "  - value: ctrl+alt\n    $name: Ctrl + Alt\n  \
             - value: ctrl+shift\n    $name: Ctrl + Shift\n  \
             - value: alt+shift\n    $name: Alt + Shift\n  \
             - value: ctrl\n    $name: Ctrl only\n  \
             - value: alt\n    $name: Alt only\n",
            "",
        ),
        (
            "  - value: ctrl+alt\r\n    $name: Ctrl + Alt\r\n  \
             - value: ctrl+shift\r\n    $name: Ctrl + Shift\r\n  \
             - value: alt+shift\r\n    $name: Alt + Shift\r\n  \
             - value: ctrl\r\n    $name: Ctrl only\r\n  \
             - value: alt\r\n    $name: Alt only\r\n",
            "",
        ),
    ],
};

/// The workaround table. One row per DISTINCT payload (~7), each keyed by a SET
/// of (id, versions). The drift guard (`no_id_version_pair_is_in_two_rows`)
/// asserts no (id, version) appears in two rows.
///
/// Apply order is a DEFENSIVE convention, NOT exercised by current data (the
/// rows partition cleanly, so no (id, version) needs more than one fixup, hence
/// no application-order characterization test): the surrogate combine runs
/// FIRST, then `replacements` in array order. Should a future (id, version) ever
/// need two fixup CLASSES, concatenate their replacements in the historical pass
/// order (multiline, then number, then duplicate-id), because a later
/// replacement can act on an earlier one's output.
const WORKAROUNDS: &[Workaround] = &[
    SURROGATE_TASKBAR_CLOCK,
    OFFICE_UI_MULTILINE,
    EXPLORER_DOUBLE_F2_MULTILINE,
    TASKBAR_MUSIC_LOUNGE_NUMBER,
    UIRIBBON_INSETTING_NUMBER,
    NUMBERED_TASKBAR_COLORS,
    SCROLL_WINDOW_OPACITY_DUPLICATE,
];

/// Apply the per-(mod id, version) settings-block fixup for the source's own
/// `@id`/`@version`, if a `WORKAROUNDS` row matches. Returns the block unchanged
/// (borrowed) when no row matches. The newtype pair makes the call swap-safe.
pub(super) fn apply_settings_workarounds<'a>(
    mod_id: Option<&ModId>,
    version: Option<&Version>,
    block: &'a str,
) -> Cow<'a, str> {
    let (Some(id), Some(version)) = (mod_id, version) else {
        return Cow::Borrowed(block);
    };
    // The table keys on the raw (id, version) strings, so unwrap to &str here
    // (the matched-immediately boundary).
    let (id, version) = (id.as_str(), version.as_str());

    let Some(row) = WORKAROUNDS.iter().find(|w| w.matches(id, version)) else {
        return Cow::Borrowed(block);
    };

    let mut out = block.to_owned();
    if row.combine_surrogates {
        out = combine_surrogate_pairs(&out);
    }
    // The literal replacements match both `\n` and `\r\n` variants where a line
    // break is involved, so they apply to the raw CRLF repo source and the
    // CRLF-normalized installed source alike.
    for &(from, to) in row.replacements {
        out = out.replace(from, to);
    }
    Cow::Owned(out)
}

/// Combine each `\u<high>\u<low>` UTF-16 surrogate-pair escape into one
/// `\U00xxxxxx` 8-digit escape (the same codepoint). js-yaml is a UTF-16 host and
/// combines such pairs (mods embed emoji this way); yaml-rust2 is a UTF-8 host
/// and rejects a lone surrogate escape. A high+low pair is unambiguous - it only
/// makes sense as a double-quoted escape - so this scans the whole block without
/// tracking quote context (which a stray apostrophe in a plain scalar would
/// derail); a `\\u...` literal-backslash sequence does not match, as the two `\u`
/// would not be back to back.
fn combine_surrogate_pairs(block: &str) -> String {
    let chars: Vec<char> = block.chars().collect();
    let mut out = String::with_capacity(block.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\'
            && chars.get(i + 1) == Some(&'u')
            && let Some(hi) = read_hex4(&chars, i + 2)
            && (0xD800..=0xDBFF).contains(&hi)
            && chars.get(i + 6) == Some(&'\\')
            && chars.get(i + 7) == Some(&'u')
            && let Some(lo) = read_hex4(&chars, i + 8)
            && (0xDC00..=0xDFFF).contains(&lo)
        {
            let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
            out.push_str(&format!("\\U{cp:08X}"));
            i += 12;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Parse exactly four hex digits at `start` into a UTF-16 code unit, or `None`.
fn read_hex4(chars: &[char], start: usize) -> Option<u32> {
    if start + 4 > chars.len() {
        return None;
    }
    let mut value = 0u32;
    for &c in &chars[start..start + 4] {
        value = value * 16 + c.to_digit(16)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EngineSettingValue, SettingValue};
    use crate::settings::{extract_initial_settings, extract_initial_settings_for_engine};

    /// A full mod source: a metadata block (so the workaround table can read
    /// `@id`/`@version`) plus the settings block.
    fn mod_src(id: &str, version: &str, yaml: &str) -> String {
        format!(
            "// ==WindhawkMod==\n// @id {id}\n// @version {version}\n// ==/WindhawkMod==\n\
             // ==WindhawkModSettings==\n/*\n{yaml}\n*/\n// ==/WindhawkModSettings==\n"
        )
    }

    #[test]
    fn no_id_version_pair_is_in_two_workaround_rows() {
        // The data-drift guard: each (id, version) lands in at most one row, so
        // the apply step's "find the one matching row" is unambiguous.
        let mut seen = std::collections::HashSet::new();
        for row in WORKAROUNDS {
            for (id, versions) in row.keys {
                for v in *versions {
                    assert!(
                        seen.insert((*id, *v)),
                        "(id, version) ('{id}', '{v}') appears in more than one workaround row"
                    );
                }
            }
        }
    }

    #[test]
    fn workaround_rewrites_a_shipped_float_default() {
        // taskbar-clock-customization-style float default js-yaml coerces to int.
        let src = mod_src("taskbar-music-lounge", "4.0.2", "- ButtonScale: 1.0");
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].key, "ButtonScale");
        assert_eq!(items[0].value, SettingValue::Number(1.into()));
    }

    #[test]
    fn workaround_rewrites_shipped_uint32_colors_to_their_i32_bitcast() {
        let src = mod_src(
            "numbered-taskbar",
            "1.0.0",
            "- fontColor: 0xFFFFFFFF\n- outlineColor: 0xFF000000",
        );
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].value, SettingValue::Number((-1).into()));
        assert_eq!(items[1].value, SettingValue::Number((-16777216).into()));
    }

    #[test]
    fn workaround_combines_shipped_surrogate_emoji() {
        let bs = '\\';
        let yaml = format!(
            "- WebContentWeatherFormat: \"%c {bs}uD83C{bs}uDF21{bs}uFE0F%t {bs}uD83C{bs}uDF2C{bs}uFE0F%w\""
        );
        let src = mod_src("taskbar-clock-customization", "1.7.4", &yaml);
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        match &items[0].value {
            SettingValue::String(s) => {
                assert_eq!(s, "%c \u{1F321}\u{FE0F}%t \u{1F32C}\u{FE0F}%w");
            }
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn workaround_collapses_a_shipped_multiline_quoted_description() {
        let yaml = "- visualStyle: default\n  $description: \"Note: ...this mod.\\n\n  Changes will take effect on new Office processes.\"";
        let src = mod_src("office-ui-reverter", "1.0.0", yaml);
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[0].description.as_deref(),
            Some("Note: ...this mod.\n Changes will take effect on new Office processes."),
        );
    }

    #[test]
    fn workaround_folds_a_single_quoted_multiline_continuation() {
        // explorer-double-f2-rename-extension 2.1 style: a SINGLE-quoted scalar
        // wrapped across a break with a zero-indent continuation.
        let yaml = "- reverseCycle: false\n  $description: 'Select the whole name on double F2 and the extension on triple\nF2?'";
        let src = mod_src("explorer-double-f2-rename-extension", "2.1", yaml);
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(
            items[0].description.as_deref(),
            Some("Select the whole name on double F2 and the extension on triple F2?"),
        );
    }

    #[test]
    fn workaround_is_pinned_to_the_exact_version() {
        // A different version of the same mod is NOT fixed up - the new release
        // is forced to author it cleanly, so the float is rejected as usual.
        let src = mod_src("taskbar-music-lounge", "4.0.3", "- ButtonScale: 1.0");
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("ButtonScale") && err.contains("floating-point"),
            "got: {err}"
        );
    }

    /// scroll-window-opacity 1.0.3's exact malformed `modifierKey` dropdown: an
    /// array of six `{value, $name}` items that all flatten to `modifierKey.value`.
    const SCROLL_WINDOW_OPACITY_SETTINGS: &str = concat!(
        "- modifierKey:\n",
        "  - value: ctrl+alt\n    $name: Ctrl + Alt\n",
        "  - value: ctrl+shift\n    $name: Ctrl + Shift\n",
        "  - value: alt+shift\n    $name: Alt + Shift\n",
        "  - value: ctrl\n    $name: Ctrl only\n",
        "  - value: alt\n    $name: Alt only\n",
        "  - value: shift\n    $name: Shift only\n",
        "  $name: Modifier Key(s)\n",
        "  $description: Key(s) to hold while scrolling the mouse wheel to change opacity\n",
        "- opacityStep: 1\n",
        "- minOpacity: 10",
    );

    #[test]
    fn workaround_collapses_the_scroll_window_opacity_duplicate_dropdown() {
        // A store install (apply_workarounds = true) drops the first five
        // options, leaving `value: shift` - the last-write-wins value the engine
        // store kept before, so the engine flatten is byte-identical (the mod
        // behaves exactly as before).
        let src = mod_src(
            "scroll-window-opacity",
            "1.0.3",
            SCROLL_WINDOW_OPACITY_SETTINGS,
        );
        let flat = extract_initial_settings_for_engine(&src, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            flat,
            vec![
                (
                    "modifierKey.value".to_owned(),
                    EngineSettingValue::Str("shift".to_owned())
                ),
                ("opacityStep".to_owned(), EngineSettingValue::Int(1)),
                ("minOpacity".to_owned(), EngineSettingValue::Int(10)),
            ]
        );
    }

    #[test]
    fn engine_workaround_is_skipped_for_a_locally_authored_mod() {
        // A locally-authored mod (apply_workarounds = false, the `local@` install
        // path) is parsed as written, so the duplicate ids are rejected - the
        // author sees the real error instead of the silent store-compat fixup.
        let src = mod_src(
            "scroll-window-opacity",
            "1.0.3",
            SCROLL_WINDOW_OPACITY_SETTINGS,
        );
        let err = extract_initial_settings_for_engine(&src, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate settings id 'value'"), "got: {err}");
    }

    #[test]
    fn duplicate_id_workaround_is_pinned_to_the_exact_version() {
        // A different version of the same mod is NOT fixed up - the new release
        // is forced to author it cleanly, so the duplicate ids are rejected.
        let src = mod_src(
            "scroll-window-opacity",
            "1.0.4",
            SCROLL_WINDOW_OPACITY_SETTINGS,
        );
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate settings id 'value'"), "got: {err}");
    }
}
