//! Hand-rolled scanners reproducing the block-extraction regexes of the TS
//! implementation (`src/services/modSource.ts`):
//!
//! - metadata: `/^\/\/[ \t]+==X==[ \t]*$([\s\S]+?)^\/\/[ \t]+==\/X==[ \t]*$/m`
//! - readme/settings: the same opening/closing marker lines around a
//!   `/* ... */` comment, with the content `\s`-trimmed.
//!
//! JS multiline `^`/`$` anchor at line terminators; we treat `\n` and `\r`
//! as the terminators (so CRLF files match like LF) and do not reproduce
//! JS's additional U+2028/U+2029, which never occur in C++ mod source.
//! Likewise the `\s` trimming below uses Rust's `char::is_whitespace`
//! rather than reproducing JS's exact `\s` set (the two differ only on
//! U+FEFF).
//!
//! One RECORDED DEVIATION from those regexes: a leading UTF-8 BOM does not
//! hide the first line (see [`is_line_start`]).

use crate::text::bom_len;

/// A line terminator for `^`/`$` anchoring.
fn is_line_terminator(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// True if `pos` is the start of a line in `s` (JS multiline `^`), where a
/// leading BOM does not count as text: offset 0 and the offset just past a BOM
/// both open the first line.
///
/// RECORDED DEVIATION from the TS regexes, which anchor `^` at offset 0 alone
/// and so cannot match a marker line the BOM sits in front of - the reason a
/// `.wh.cpp` saved with a BOM by a Windows editor reported no metadata block at
/// all. It only ever turns a source the TS build rejected outright into one
/// that parses, so no source that parsed before can change meaning.
fn is_line_start(s: &str, pos: usize) -> bool {
    if pos == 0 || pos == bom_len(s) {
        return true;
    }
    s[..pos].chars().next_back().is_some_and(is_line_terminator)
}

/// True if the character at `pos` ends a line (JS multiline `$` matches
/// just before it), or `pos` is the end of the string.
fn is_line_end(s: &str, pos: usize) -> bool {
    s[pos..].chars().next().is_none_or(is_line_terminator)
}

/// Match `//[ \t]+==<marker>==[ \t]*$` at `pos` (which must be a line
/// start); on success return the position of the `$` (just past the
/// trailing `[ \t]*` run).
fn match_marker_line(s: &str, pos: usize, marker: &str) -> Option<usize> {
    let rest = s[pos..].strip_prefix("//")?;
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.len() == rest.len() {
        return None; // [ \t]+ needs at least one
    }
    let after_marker = after_ws.strip_prefix(marker)?;
    let after_trail = after_marker.trim_start_matches([' ', '\t']);
    let end = s.len() - after_trail.len();
    is_line_end(s, end).then_some(end)
}

/// Iterator over the line-start positions of `s`, in order. Agrees with
/// [`is_line_start`], BOM included; the positions are offsets into `s` itself,
/// so a caller that returns byte ranges keeps addressing the original text.
fn line_starts(s: &str) -> impl Iterator<Item = usize> + '_ {
    let bom = bom_len(s);
    std::iter::once(0).chain((bom != 0).then_some(bom)).chain(
        s.char_indices()
            .filter(|(_, c)| is_line_terminator(*c))
            .map(|(i, c)| i + c.len_utf8()),
    )
}

/// The byte range `[content_start, content_end)` of the metadata block content
/// (between the marker lines, including the leading line terminator, exactly
/// like the regex group). First opening line with a closing line wins; an
/// opening line with no closing line is skipped, like regex backtracking would.
pub(crate) fn find_metadata_block_range(s: &str, name: &str) -> Option<(usize, usize)> {
    let open = format!("=={name}==");
    let close = format!("==/{name}==");
    for p in line_starts(s) {
        let Some(content_start) = match_marker_line(s, p, &open) else {
            continue;
        };
        for q in line_starts(s) {
            // `([\s\S]+?)` needs at least one character of content.
            if q <= content_start {
                continue;
            }
            if match_marker_line(s, q, &close).is_some() {
                return Some((content_start, q));
            }
        }
    }
    None
}

/// Find the metadata block: content between the `==<name>==` and
/// `==/<name>==` marker lines (including the leading line terminator,
/// exactly like the regex group).
pub(crate) fn find_metadata_block<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    find_metadata_block_range(s, name).map(|(start, end)| &s[start..end])
}

/// Find a `/* ... */` block wrapped in `==<name>==` / `==/<name>==` marker
/// lines and return the comment content trimmed of `\s` on both sides
/// (regex: `\s*\/\*\s*([\s\S]+?)\s*\*\/\s*` between the markers, lazy
/// content, so the first `*/` followed by the closing marker line wins).
pub(crate) fn find_comment_block<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("=={name}==");
    let close = format!("==/{name}==");
    'open: for p in line_starts(s) {
        let Some(open_end) = match_marker_line(s, p, &open) else {
            continue;
        };
        // `\s*` then the literal `/*`.
        let after_ws = s[open_end..].trim_start_matches(char::is_whitespace);
        let Some(comment) = after_ws.strip_prefix("/*") else {
            continue;
        };
        let comment_start = s.len() - comment.len();
        // Lazy content: take the first `*/` whose tail matches
        // `\s*^==/name== line`; on tail mismatch extend to the next `*/`.
        let mut search_from = comment_start;
        while let Some(rel) = s[search_from..].find("*/") {
            let star_slash = search_from + rel;
            let content = s[comment_start..star_slash].trim_matches(char::is_whitespace);
            search_from = star_slash + 2;
            if content.is_empty() {
                continue; // `([\s\S]+?)` needs at least one character
            }
            let tail = s[star_slash + 2..].trim_start_matches(char::is_whitespace);
            let close_pos = s.len() - tail.len();
            if is_line_start(s, close_pos) && match_marker_line(s, close_pos, &close).is_some() {
                return Some(content);
            }
            // No closing marker after this `*/`: a later `*/` may still
            // satisfy the lazy match; if none does, regex backtracking
            // would move on to the next opening marker occurrence.
            if s[search_from..].find("*/").is_none() {
                continue 'open;
            }
        }
    }
    None
}

/// `extractReadme` of the TS implementation: the readme comment block, or
/// `None` when absent. Never fails.
pub fn extract_readme(mod_source: &str) -> Option<String> {
    find_comment_block(mod_source, "WindhawkModReadme").map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_block_basic() {
        let src = "// ==WindhawkMod==\n// @id test\n// ==/WindhawkMod==\n";
        assert_eq!(
            find_metadata_block(src, "WindhawkMod"),
            Some("\n// @id test\n")
        );
    }

    #[test]
    fn metadata_block_crlf_and_trailing_ws() {
        let src = "// ==WindhawkMod== \t\r\n// @id test\r\n// ==/WindhawkMod==\t\r\n";
        assert_eq!(
            find_metadata_block(src, "WindhawkMod"),
            Some("\r\n// @id test\r\n")
        );
    }

    #[test]
    fn metadata_block_requires_line_start_and_ws() {
        // Marker not at a line start.
        let src = " // ==WindhawkMod==\n// @id x\n// ==/WindhawkMod==\n";
        assert_eq!(find_metadata_block(src, "WindhawkMod"), None);
        // No whitespace between // and marker.
        let src = "//==WindhawkMod==\n// @id x\n// ==/WindhawkMod==\n";
        assert_eq!(find_metadata_block(src, "WindhawkMod"), None);
    }

    #[test]
    fn metadata_block_skips_opening_without_closing() {
        let src =
            "// ==WindhawkMod==\nno closing\n// ==WindhawkMod==\n// @id x\n// ==/WindhawkMod==\n";
        // The first opening marker still matches: the lazy content extends
        // past the second opening marker to the single closing marker.
        assert_eq!(
            find_metadata_block(src, "WindhawkMod"),
            Some("\nno closing\n// ==WindhawkMod==\n// @id x\n")
        );
    }

    #[test]
    fn a_leading_bom_does_not_hide_the_first_line() {
        // A `.wh.cpp` saved with a BOM by a Windows editor opens with the marker
        // line, so only a BOM-aware line start can match it.
        let src = "\u{feff}// ==WindhawkMod==\n// @id test\n// ==/WindhawkMod==\n";
        assert_eq!(
            find_metadata_block(src, "WindhawkMod"),
            Some("\n// @id test\n")
        );
        // The returned range still addresses the ORIGINAL text, BOM included.
        let (start, end) = find_metadata_block_range(src, "WindhawkMod").unwrap();
        assert_eq!(&src[start..end], "\n// @id test\n");

        let readme = "\u{feff}// ==WindhawkModReadme==\n/*\nHello\n*/\n// ==/WindhawkModReadme==\n";
        assert_eq!(extract_readme(readme).as_deref(), Some("Hello"));
    }

    #[test]
    fn a_bom_opens_only_the_first_line() {
        // Elsewhere U+FEFF is text, not a mark: it fails the marker line just
        // like any other character before the `//`.
        let src = "// ==WindhawkMod==\n// @id x\n\u{feff}// ==/WindhawkMod==\n";
        assert_eq!(find_metadata_block(src, "WindhawkMod"), None);
    }

    #[test]
    fn readme_block_trims_and_requires_markers() {
        let src = "// ==WindhawkModReadme==\n/*\nHello\nWorld\n*/\n// ==/WindhawkModReadme==\n";
        assert_eq!(extract_readme(src).as_deref(), Some("Hello\nWorld"));
        assert_eq!(extract_readme("no readme here"), None);
    }

    #[test]
    fn readme_block_with_inner_comment_end_uses_first_valid_tail() {
        // The first */ is not followed by the closing marker line, so the
        // lazy content extends to the next one.
        let src = "// ==WindhawkModReadme==\n/*\nA */ B\n*/\n// ==/WindhawkModReadme==\n";
        assert_eq!(extract_readme(src).as_deref(), Some("A */ B"));
    }

    #[test]
    fn readme_closing_marker_must_start_its_line() {
        let src = "// ==WindhawkModReadme==\n/*\nHello\n*/ // ==/WindhawkModReadme==\n";
        assert_eq!(extract_readme(src), None);
    }
}
