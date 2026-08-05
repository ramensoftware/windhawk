//! Small pure text transforms shared across services.

/// A byte order mark (U+FEFF). Not part of the text it prefixes: a Windows
/// editor may write one at the start of a file, and readers are expected to
/// ignore it. The one home for the BOM policy, so the scanners and the
/// user-data decode cannot disagree about what a BOM is.
const BOM: char = '\u{feff}';

/// The byte length of a leading BOM, or 0 when the text does not start with
/// one. Callers that work in byte offsets over the ORIGINAL text use this
/// rather than [`strip_bom`], so the offsets they hand back stay valid.
pub(crate) fn bom_len(text: &str) -> usize {
    if text.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    }
}

/// The text with a leading BOM removed, or unchanged when it has none.
pub(crate) fn strip_bom(text: &str) -> &str {
    &text[bom_len(text)..]
}

/// Normalize all line endings to CRLF, matching the TS
/// `text.replace(/\r\n|\r|\n/g, '\r\n')` the repository client applies to
/// fetched mod source (and the install flow persists). Collapse CRLF and lone
/// CR to LF first, then expand every LF to CRLF, so mixed endings converge.
pub fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bom_is_measured_and_stripped_only_at_the_start() {
        assert_eq!(bom_len("\u{feff}text"), 3);
        assert_eq!(strip_bom("\u{feff}text"), "text");
        // No BOM, and a U+FEFF anywhere but the start, are both left alone.
        assert_eq!(bom_len("text"), 0);
        assert_eq!(strip_bom("text"), "text");
        assert_eq!(strip_bom("a\u{feff}b"), "a\u{feff}b");
        assert_eq!(strip_bom(""), "");
        // Only ONE BOM is a mark; a second is text.
        assert_eq!(strip_bom("\u{feff}\u{feff}text"), "\u{feff}text");
    }

    #[test]
    fn normalizes_mixed_endings_to_crlf() {
        assert_eq!(normalize_crlf("a\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\r\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\rb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\r\nb\nc\rd"), "a\r\nb\r\nc\r\nd");
        assert_eq!(normalize_crlf(""), "");
        assert_eq!(normalize_crlf("no endings"), "no endings");
    }
}
