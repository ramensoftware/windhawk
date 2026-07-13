//! Mod-source transforms, a port of `ModSource.appendToIdAndName`
//! (`src/services/modSource.ts`): the new-mod / fork flows append a suffix to
//! the `@id` (once) and every `@name[:lang]` metadata line, in place, leaving
//! everything else byte-for-byte unchanged.
//!
//! The TS does this with three nested regex replaces; this reproduces their
//! semantics with hand-rolled line scanning (the dependency policy admits no
//! regex crate). The metadata block is located with the same scanner the parser
//! uses; within it, each line is matched against `//[ \t]+@<field>[:lang]?[ \t]+`
//! and the suffix is inserted between the value and the line's trailing
//! whitespace, exactly where the TS replacement `$1$2<suffix>$3` puts it.

use crate::scan::find_metadata_block_range;

/// `appendToIdAndName`: append `append_to_id` to the `@id` line (the first one)
/// and `append_to_name` to every `@name[:lang]` line, within the
/// `==WindhawkMod==` metadata block. An empty/absent suffix is a no-op for that
/// field (the TS `if (appendToId)` / `if (appendToName)` truthiness gate); a
/// source with no metadata block is returned unchanged.
pub fn append_to_id_and_name(
    source: &str,
    append_to_id: Option<&str>,
    append_to_name: Option<&str>,
) -> String {
    let Some((start, end)) = find_metadata_block_range(source, "WindhawkMod") else {
        return source.to_owned();
    };

    let mut block = source[start..end].to_owned();
    if let Some(suffix) = append_to_id.filter(|s| !s.is_empty()) {
        block = transform_lines(&block, true, |line| {
            append_to_field_line(line, "@id", false, suffix)
        });
    }
    if let Some(suffix) = append_to_name.filter(|s| !s.is_empty()) {
        block = transform_lines(&block, false, |line| {
            append_to_field_line(line, "@name", true, suffix)
        });
    }

    format!("{}{}{}", &source[..start], block, &source[end..])
}

/// Apply `transform` to each line of `content` (the text between line
/// terminators, which `^`/`$` anchor), preserving the exact terminators. When
/// `first_only`, only the first line `transform` accepts is changed. `\r` and
/// `\n` are each treated as a terminator, like the JS multiline anchors, so a
/// CRLF file round-trips byte for byte.
fn transform_lines(
    content: &str,
    first_only: bool,
    transform: impl Fn(&str) -> Option<String>,
) -> String {
    let mut out = String::with_capacity(content.len());
    let mut done = false;
    let mut i = 0;
    while i < content.len() {
        let nl = content[i..].find(['\r', '\n']).map(|r| i + r);
        let line_end = nl.unwrap_or(content.len());
        let line = &content[i..line_end];

        match if first_only && done {
            None
        } else {
            transform(line)
        } {
            Some(new_line) => {
                out.push_str(&new_line);
                done = true;
            }
            None => out.push_str(line),
        }

        match nl {
            Some(term) => {
                // Append the single terminator char (\r or \n) verbatim; a CRLF
                // is two such iterations with an empty line in between.
                let term_char = content[term..].chars().next().unwrap_or('\n');
                out.push(term_char);
                i = term + term_char.len_utf8();
            }
            None => i = content.len(),
        }
    }
    out
}

/// Match `//[ \t]+@<field>[:lang]?[ \t]+<value><trailing ws>` against one line
/// and return it with `suffix` inserted between the value and the trailing
/// whitespace (the TS `$1$2<suffix>$3`), or `None` if the line does not match.
/// `allow_lang` permits the `@name:en` / `@name:en-US` localized forms.
fn append_to_field_line(line: &str, field: &str, allow_lang: bool, suffix: &str) -> Option<String> {
    // `//` then `[ \t]+` (at least one).
    let after_slashes = line.strip_prefix("//")?;
    let after_ws1 = after_slashes.trim_start_matches([' ', '\t']);
    if after_ws1.len() == after_slashes.len() {
        return None;
    }
    // The field keyword, then the optional `:lang`.
    let after_field = after_ws1.strip_prefix(field)?;
    let after_field = if allow_lang {
        strip_optional_lang(after_field)
    } else {
        after_field
    };
    // `[ \t]+` after the field (at least one) - the rest is value + trailing ws.
    let rest = after_field.trim_start_matches([' ', '\t']);
    if rest.len() == after_field.len() {
        return None;
    }

    // value = `.*?` (lazy), trailing = `[ \t]*$` (greedy): the value is `rest`
    // with its trailing whitespace run stripped; the suffix goes between them.
    let trail_len: usize = rest
        .chars()
        .rev()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(char::len_utf8)
        .sum();
    let value_end = rest.len() - trail_len;
    let group1_len = line.len() - rest.len();

    let mut out = String::with_capacity(line.len() + suffix.len());
    out.push_str(&line[..group1_len + value_end]); // group1 + value
    out.push_str(suffix);
    out.push_str(&rest[value_end..]); // trailing whitespace
    Some(out)
}

/// Consume an optional `:[a-z]{2}(-[A-Z]{2})?` language tag (the TS
/// `(?::(?:[a-z]{2}(?:-[A-Z]{2})?))?`). A `:` not followed by a valid tag is
/// left in place, so the subsequent `[ \t]+` check fails on it - matching the
/// regex's optional group declining to match.
fn strip_optional_lang(s: &str) -> &str {
    let Some(rest) = s.strip_prefix(':') else {
        return s;
    };
    let b = rest.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_lowercase() && b[1].is_ascii_lowercase() {
        if b.len() >= 5 && b[2] == b'-' && b[3].is_ascii_uppercase() && b[4].is_ascii_uppercase() {
            return &rest[5..];
        }
        return &rest[2..];
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "// ==WindhawkMod==\n// @id          my-mod\n// @name        My Mod\n// @name:fr     Mon Mod\n// @description  Desc\n// ==/WindhawkMod==\n\nvoid Wh_ModInit() {}\n";

    #[test]
    fn appends_to_id_and_every_name_in_place() {
        let out = append_to_id_and_name(SRC, Some("-fork"), Some(" - Fork"));
        assert!(out.contains("// @id          my-mod-fork\n"));
        assert!(out.contains("// @name        My Mod - Fork\n"));
        assert!(out.contains("// @name:fr     Mon Mod - Fork\n"));
        // Non-targeted lines are untouched.
        assert!(out.contains("// @description  Desc\n"));
        assert!(out.ends_with("void Wh_ModInit() {}\n"));
    }

    #[test]
    fn id_is_appended_only_to_the_first_id_line() {
        let src = "// ==WindhawkMod==\n// @id one\n// @id two\n// ==/WindhawkMod==\n";
        let out = append_to_id_and_name(src, Some("-x"), None);
        assert!(out.contains("// @id one-x\n"));
        assert!(out.contains("// @id two\n"));
    }

    #[test]
    fn empty_or_absent_suffix_is_a_noop_for_that_field() {
        // Empty id suffix leaves @id alone; the name suffix still applies.
        let out = append_to_id_and_name(SRC, Some(""), Some("!"));
        assert!(out.contains("// @id          my-mod\n"));
        assert!(out.contains("// @name        My Mod!\n"));
        // Absent both: unchanged.
        assert_eq!(append_to_id_and_name(SRC, None, None), SRC);
    }

    #[test]
    fn no_metadata_block_returns_the_source_unchanged() {
        assert_eq!(
            append_to_id_and_name("// just code\n", Some("-fork"), Some(" Fork")),
            "// just code\n"
        );
    }

    #[test]
    fn trailing_whitespace_is_preserved_after_the_suffix() {
        let src = "// ==WindhawkMod==\r\n// @id   my-mod  \r\n// ==/WindhawkMod==\r\n";
        let out = append_to_id_and_name(src, Some("-fork"), None);
        // The suffix lands before the trailing spaces, and CRLF survives.
        assert!(out.contains("// @id   my-mod-fork  \r\n"));
    }

    #[test]
    fn id_prefix_must_be_exactly_at_id() {
        // `@idea` is not `@id`; left unchanged.
        let src = "// ==WindhawkMod==\n// @idea foo\n// ==/WindhawkMod==\n";
        assert_eq!(append_to_id_and_name(src, Some("-x"), None), src);
    }

    #[test]
    fn name_with_invalid_lang_tag_is_not_matched() {
        // `@name:bad` has no valid two-letter tag, so the `[ \t]+` falls on the
        // `:` and the line does not match (the regex's optional group declines).
        let src = "// ==WindhawkMod==\n// @name:bad Foo\n// ==/WindhawkMod==\n";
        assert_eq!(append_to_id_and_name(src, None, Some("!")), src);
    }
}
