//! Small pure text transforms shared across services.

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
    fn normalizes_mixed_endings_to_crlf() {
        assert_eq!(normalize_crlf("a\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\r\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\rb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\r\nb\nc\rd"), "a\r\nb\r\nc\r\nd");
        assert_eq!(normalize_crlf(""), "");
        assert_eq!(normalize_crlf("no endings"), "no endings");
    }
}
