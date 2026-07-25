//! Settings-block compatibility fixups for already-published mod VERSIONS whose
//! YAML js-yaml accepts but yaml-rust2 (stricter, and int32-only) rejects,
//! found by a sweep over every published version. Each fixup is pinned to the
//! EXACT (mod id, version) tuples that need it, so a future release of the mod
//! does NOT inherit the shim and is forced to be authored cleanly.
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
//! replacements (line-ending agnostic, kept within a line). The
//! SEMANTIC-rejection divergences (a duplicate settings id; a `$options`
//! dropdown on a non-string value, which the UI never renders) are likewise
//! stable, so they are literal replacements that DELETE the offending block
//! (the removed metadata is dead - the engine flatten drops it either way, so
//! the store stays byte-identical).
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

/// Category: options-on-nonstring. A `$options` dropdown declared on a NUMBER
/// value. The UI renders a dropdown only for a string setting (a string scalar,
/// or each element of a string array); for a number it shows a plain number
/// input and never reads `$options`, so the annotation is dead metadata. The
/// `validate` pass rejects `$options` on a non-string value, so strip the dead
/// block to keep the shipped version parsing. The engine flatten drops every
/// `$options` regardless, so the store stays byte-identical - only the ignored
/// dropdown is gone. audioswap's `deviceCount: 2` maps 2..6 to "N Devices"; a
/// clean re-author would quote the value (its option keys are already the
/// string forms) or drop the dropdown.
const AUDIOSWAP_OPTIONS_ON_NUMBER: Workaround = Workaround {
    keys: &[("audioswap", &["1.3.0", "1.4.0"])],
    combine_surrogates: false,
    replacements: &[
        (
            concat!(
                "  $options:\n",
                "    - 2: 2 Devices\n",
                "    - 3: 3 Devices\n",
                "    - 4: 4 Devices\n",
                "    - 5: 5 Devices\n",
                "    - 6: 6 Devices\n",
            ),
            "",
        ),
        (
            concat!(
                "  $options:\r\n",
                "    - 2: 2 Devices\r\n",
                "    - 3: 3 Devices\r\n",
                "    - 4: 4 Devices\r\n",
                "    - 5: 5 Devices\r\n",
                "    - 6: 6 Devices\r\n",
            ),
            "",
        ),
    ],
};

/// Category: options-on-nonstring (see `AUDIOSWAP_OPTIONS_ON_NUMBER`).
/// hover-text-magnifier's `zoomLevel: 250` maps preset zoom percentages to their
/// own labels; the same dead-dropdown-on-a-number case, its own payload.
const HOVER_TEXT_MAGNIFIER_OPTIONS_ON_NUMBER: Workaround = Workaround {
    keys: &[("hover-text-magnifier", &["1.3.2", "1.3.4"])],
    combine_surrogates: false,
    replacements: &[
        (
            concat!(
                "  $options:\n",
                "    - 150: 150%\n",
                "    - 200: 200%\n",
                "    - 250: 250%\n",
                "    - 300: 300%\n",
                "    - 350: 350%\n",
                "    - 400: 400%\n",
                "    - 500: 500%\n",
            ),
            "",
        ),
        (
            concat!(
                "  $options:\r\n",
                "    - 150: 150%\r\n",
                "    - 200: 200%\r\n",
                "    - 250: 250%\r\n",
                "    - 300: 300%\r\n",
                "    - 350: 350%\r\n",
                "    - 400: 400%\r\n",
                "    - 500: 500%\r\n",
            ),
            "",
        ),
    ],
};

/// Category: options-on-nonstring (see `AUDIOSWAP_OPTIONS_ON_NUMBER`).
/// translucent-flyouts-controller nests fourteen `*ThemeColorizationType: 1`
/// items (dark/light across several element groups), each carrying the SAME
/// integer-keyed dropdown (0..8 -> `Immersive*`), so one replacement removes all
/// fourteen. Its string-valued sibling dropdowns key on words (`start_hover`
/// etc.) - a distinct block this does NOT match - so those keep their dropdown.
const TRANSLUCENT_FLYOUTS_OPTIONS_ON_NUMBER: Workaround = Workaround {
    keys: &[(
        "translucent-flyouts-controller",
        &["1.0.0", "1.0.1", "1.1.0"],
    )],
    combine_surrogates: false,
    replacements: &[
        (
            concat!(
                "    $options:\n",
                "      - 0: ImmersiveStartBackground\n",
                "      - 1: ImmersiveStartHoverBackground\n",
                "      - 2: ImmersiveSystemAccent\n",
                "      - 3: ImmersiveSystemAccentDark1\n",
                "      - 4: ImmersiveSystemAccentDark2\n",
                "      - 5: ImmersiveSystemAccentDark3\n",
                "      - 6: ImmersiveSystemAccentLight1\n",
                "      - 7: ImmersiveSystemAccentLight2\n",
                "      - 8: ImmersiveSystemAccentLight3\n",
            ),
            "",
        ),
        (
            concat!(
                "    $options:\r\n",
                "      - 0: ImmersiveStartBackground\r\n",
                "      - 1: ImmersiveStartHoverBackground\r\n",
                "      - 2: ImmersiveSystemAccent\r\n",
                "      - 3: ImmersiveSystemAccentDark1\r\n",
                "      - 4: ImmersiveSystemAccentDark2\r\n",
                "      - 5: ImmersiveSystemAccentDark3\r\n",
                "      - 6: ImmersiveSystemAccentLight1\r\n",
                "      - 7: ImmersiveSystemAccentLight2\r\n",
                "      - 8: ImmersiveSystemAccentLight3\r\n",
            ),
            "",
        ),
    ],
};

/// The workaround table. One row per DISTINCT payload (~10), each keyed by a SET
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
    AUDIOSWAP_OPTIONS_ON_NUMBER,
    HOVER_TEXT_MAGNIFIER_OPTIONS_ON_NUMBER,
    TRANSLUCENT_FLYOUTS_OPTIONS_ON_NUMBER,
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

    #[test]
    fn workaround_strips_a_shipped_dead_options_dropdown_on_a_number() {
        // audioswap 1.4.0's `deviceCount: 2` carries a `$options` dropdown the UI
        // never renders for a number; the workaround strips it so the shipped
        // version parses. The value is unchanged and the dropdown is gone.
        let src = mod_src(
            "audioswap",
            "1.4.0",
            concat!(
                "- deviceCount: 2\n",
                "  $name: Number of Devices\n",
                "  $description: How many audio devices to cycle through (2 to 6)\n",
                "  $options:\n",
                "    - 2: 2 Devices\n",
                "    - 3: 3 Devices\n",
                "    - 4: 4 Devices\n",
                "    - 5: 5 Devices\n",
                "    - 6: 6 Devices\n",
                // A trailing item, so the stripped block ends with the interior
                // newline the workaround matches (as in the shipped source),
                // not a trailing one the settings-block trim would remove.
                "- swapMode: click_to_swap",
            ),
        );
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        assert_eq!(items[0].key, "deviceCount");
        assert_eq!(items[0].value, SettingValue::Number(2.into()));
        assert!(items[0].options.is_none());
        assert_eq!(items[1].key, "swapMode");
    }

    /// The integer-keyed dropdown fourteen `*ThemeColorizationType: 1` items
    /// share (0..8 -> `Immersive*`), at the nested 4-space `$options` indent.
    const TFC_INT_OPTIONS: &str = concat!(
        "    $options:\n",
        "      - 0: ImmersiveStartBackground\n",
        "      - 1: ImmersiveStartHoverBackground\n",
        "      - 2: ImmersiveSystemAccent\n",
        "      - 3: ImmersiveSystemAccentDark1\n",
        "      - 4: ImmersiveSystemAccentDark2\n",
        "      - 5: ImmersiveSystemAccentDark3\n",
        "      - 6: ImmersiveSystemAccentLight1\n",
        "      - 7: ImmersiveSystemAccentLight2\n",
        "      - 8: ImmersiveSystemAccentLight3\n",
    );

    #[test]
    fn workaround_strips_repeated_number_dropdowns_and_keeps_a_string_sibling() {
        // translucent-flyouts-controller nests many `*ThemeColorizationType: 1`
        // items sharing ONE integer-keyed dropdown, so a single replacement
        // removes every copy. A string-valued sibling keys its dropdown on words
        // (a distinct block), so it keeps its options.
        let yaml = format!(
            "- group:\n  - darkModeThemeColorizationType: 1\n{TFC_INT_OPTIONS}  \
             - lightModeThemeColorizationType: 1\n{TFC_INT_OPTIONS}  \
             - enableThemeColorization: use_global\n    $options:\n      - no: No\n      - yes: Yes"
        );
        let src = mod_src("translucent-flyouts-controller", "1.1.0", &yaml);
        let items = extract_initial_settings(&src, "en").unwrap().unwrap();
        let SettingValue::Settings(inner) = &items[0].value else {
            panic!("expected nested settings, got {:?}", items[0].value);
        };
        assert_eq!(inner.len(), 3);
        // Both integer dropdowns stripped; the string dropdown kept.
        assert_eq!(inner[0].value, SettingValue::Number(1.into()));
        assert!(inner[0].options.is_none());
        assert_eq!(inner[1].value, SettingValue::Number(1.into()));
        assert!(inner[1].options.is_none());
        assert_eq!(inner[2].value, SettingValue::String("use_global".into()));
        assert!(inner[2].options.is_some());
    }

    #[test]
    fn options_on_number_workaround_is_pinned_to_the_exact_version() {
        // A version outside the pinned set is NOT shimmed - the author must drop
        // the dead dropdown, so it is rejected as usual.
        let src = mod_src(
            "audioswap",
            "1.5.0",
            "- deviceCount: 2\n  $options:\n    - 2: 2 Devices\n    - 3: 3 Devices",
        );
        let err = extract_initial_settings(&src, "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("deviceCount")
                && err.contains("must be a string or array of strings to use $options"),
            "got: {err}"
        );
    }
}
