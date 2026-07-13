//! Language matching for localizable metadata parameters and settings
//! annotations, reproducing `getBestLanguageMatch` of the TS
//! implementation: exact match, then a more specific language (prefix
//! match), then the parent language (strip the last `-segment`), then the
//! language-neutral candidate, then the first candidate.

/// The fallback UI language (the TS `'en'` default). One home for the literal:
/// it is both the `getAppSettings` `Language` default AND the
/// language-irrelevant placeholder passed where extraction drops localization
/// (engine-flatten / metadata id-version scans), so the leaf values do not
/// depend on it.
pub const DEFAULT_LANGUAGE: &str = "en";

/// Pick the best candidate for `match_language` out of `(language, value)`
/// pairs, where `None` is the language-neutral candidate. Candidate
/// languages are compared lowercased; `match_language` is used as the
/// caller provides it (the TS implementation does not lowercase it).
///
/// `candidates` must be non-empty.
pub fn best_language_match<'a, T>(
    match_language: &str,
    candidates: &'a [(Option<String>, T)],
) -> &'a T {
    let languages: Vec<Option<String>> = candidates
        .iter()
        .map(|(language, _)| language.as_ref().map(|l| l.to_lowercase()))
        .collect();

    let mut iter_language = match_language.to_owned();
    loop {
        // Exact match.
        if let Some(i) = languages
            .iter()
            .position(|l| l.as_deref() == Some(iter_language.as_str()))
        {
            return &candidates[i].1;
        }

        // A more specific language.
        if let Some(i) = languages
            .iter()
            .position(|l| l.as_deref().is_some_and(|l| l.starts_with(&iter_language)))
        {
            return &candidates[i].1;
        }

        match iter_language.rfind('-') {
            None => break,
            Some(pos) => iter_language.truncate(pos),
        }
    }

    // No language.
    if let Some(i) = languages.iter().position(|l| l.is_none()) {
        return &candidates[i].1;
    }

    // No matches of any kind, return the first item.
    &candidates[0].1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(lang: Option<&str>, value: &str) -> (Option<String>, String) {
        (lang.map(str::to_owned), value.to_owned())
    }

    #[test]
    fn exact_match_wins() {
        let candidates = [c(None, "neutral"), c(Some("en"), "english")];
        assert_eq!(best_language_match("en", &candidates), "english");
    }

    #[test]
    fn more_specific_language_matches_by_prefix() {
        let candidates = [c(None, "neutral"), c(Some("en-US"), "us english")];
        assert_eq!(best_language_match("en", &candidates), "us english");
    }

    #[test]
    fn parent_language_is_tried_after_stripping() {
        let candidates = [c(None, "neutral"), c(Some("pt"), "portuguese")];
        assert_eq!(best_language_match("pt-BR", &candidates), "portuguese");
    }

    #[test]
    fn falls_back_to_neutral_then_first() {
        let candidates = [c(Some("fr"), "french"), c(None, "neutral")];
        assert_eq!(best_language_match("ja", &candidates), "neutral");

        let candidates = [c(Some("fr"), "french"), c(Some("de"), "german")];
        assert_eq!(best_language_match("ja", &candidates), "french");
    }

    #[test]
    fn candidate_languages_compare_lowercased() {
        // Metadata languages are recorded as written (e.g. "en-US") and
        // lowercased for comparison.
        let candidates = [c(Some("en-US"), "us"), c(None, "neutral")];
        assert_eq!(best_language_match("en-us", &candidates), "us");
    }
}
