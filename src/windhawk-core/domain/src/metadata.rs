//! Metadata-block parsing: `extractMetadata` of the TS implementation,
//! including line parsing, localization, duplicate detection, and
//! validation, with identical user-facing error messages.

use crate::language::best_language_match;
use crate::model::{MetadataError, ModMetadata};
use crate::scan::find_metadata_block;

/// A known single-valued metadata key (a `SingleLocalizable` or plain `Single`
/// classification - both assign one string). The single home for the
/// single-value key set; `set_single` matches it EXHAUSTIVELY, so a new field
/// is a compile error there rather than a runtime `unreachable!`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SingleKey {
    Id,
    Version,
    Github,
    Twitter,
    Homepage,
    CompilerOptions,
    License,
    DonateUrl,
    Name,
    Description,
    Author,
}

/// A known multi-valued metadata key (`Multi` classification - collects every
/// occurrence into a `Vec`). `set_multi` matches it EXHAUSTIVELY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MultiKey {
    Include,
    Exclude,
    Architecture,
}

/// How a known metadata key is classified: which duplicate rule applies AND
/// which typed key it routes to. The single source of truth for key
/// classification: the classifier carries the typed `SingleKey`/`MultiKey` so
/// assignment is exhaustive (no `unreachable!`); `SingleLocalizable` vs
/// `Single` still distinguishes the dedup rule even though both assign one
/// string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Classified {
    /// Localizable single value: a duplicate `(key, language)` pair is rejected,
    /// then the best language match for the requested language wins.
    SingleLocalizable(SingleKey),
    /// Plain single value: localization is rejected and more than one
    /// occurrence is a duplicate.
    Single(SingleKey),
    /// Multi value: localization is rejected and every occurrence is collected.
    Multi(MultiKey),
}

/// Classify a metadata key (with one leading `_` already stripped by the
/// caller), or `None` for an unknown key. This `match` is the one home for the
/// key set; a duplicate key would be a compile error (duplicate match arm).
fn classify(key: &str) -> Option<Classified> {
    use Classified::{Multi, Single, SingleLocalizable};
    Some(match key {
        "id" => Single(SingleKey::Id),
        "version" => Single(SingleKey::Version),
        "github" => Single(SingleKey::Github),
        "twitter" => Single(SingleKey::Twitter),
        "homepage" => Single(SingleKey::Homepage),
        "compilerOptions" => Single(SingleKey::CompilerOptions),
        "license" => Single(SingleKey::License),
        "donateUrl" => Single(SingleKey::DonateUrl),
        "name" => SingleLocalizable(SingleKey::Name),
        "description" => SingleLocalizable(SingleKey::Description),
        "author" => SingleLocalizable(SingleKey::Author),
        "include" => Multi(MultiKey::Include),
        "exclude" => Multi(MultiKey::Exclude),
        "architecture" => Multi(MultiKey::Architecture),
        _ => return None,
    })
}

struct MetaValue {
    language: Option<String>,
    value: String,
}

/// One parsed `// @key[:lang] value` line.
struct MetaLine<'a> {
    key_raw: &'a str,
    language: Option<&'a str>,
    value: &'a str,
}

/// Parse one (right-trimmed, non-empty) metadata line:
/// `^\/\/[ \t]+@(_?[a-zA-Z]+)(?::([a-z]{2}(?:-[A-Z]{2})?))?[ \t]+(.*)$`.
fn parse_metadata_line(line: &str) -> Option<MetaLine<'_>> {
    let rest = line.strip_prefix("//")?;
    let after_ws = rest.trim_start_matches([' ', '\t']);
    if after_ws.len() == rest.len() {
        return None; // [ \t]+ needs at least one
    }
    let after_at = after_ws.strip_prefix('@')?;

    // `_?[a-zA-Z]+`: an optional underscore, then a maximal alphabetic run.
    let key_body = after_at.strip_prefix('_').unwrap_or(after_at);
    let alpha_len = key_body
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(key_body.len());
    if alpha_len == 0 {
        return None;
    }
    let key_len = (after_at.len() - key_body.len()) + alpha_len;
    let key_raw = &after_at[..key_len];
    let after_key = &after_at[key_len..];

    // Optional `:lang`, then mandatory `[ \t]+`; like the regex, a failed
    // language match falls back to requiring whitespace right after the key.
    if let Some(after_colon) = after_key.strip_prefix(':')
        && let Some((language, after_lang)) = parse_language(after_colon)
        && let Some(value) = strip_ws_then_value(after_lang)
    {
        return Some(MetaLine {
            key_raw,
            language: Some(language),
            value,
        });
    }
    let value = strip_ws_then_value(after_key)?;
    Some(MetaLine {
        key_raw,
        language: None,
        value,
    })
}

/// `[a-z]{2}(?:-[A-Z]{2})?`, with the optional region part dropped when the
/// whole pattern cannot continue (regex backtracking).
fn parse_language(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || !bytes[..2].iter().all(u8::is_ascii_lowercase) {
        return None;
    }
    if bytes.len() >= 5 && bytes[2] == b'-' && bytes[3..5].iter().all(u8::is_ascii_uppercase) {
        // Prefer the region form; the caller falls back via parse_language
        // returning the short form if whitespace does not follow. Mirror the
        // regex by trying long-then-short here.
        if strip_ws_then_value(&s[5..]).is_some() {
            return Some((&s[..5], &s[5..]));
        }
    }
    Some((&s[..2], &s[2..]))
}

/// `[ \t]+(.*)$`: at least one space/tab, then the value (the line is
/// already right-trimmed, so the remainder is the value).
fn strip_ws_then_value(s: &str) -> Option<&str> {
    let value = s.trim_start_matches([' ', '\t']);
    (value.len() != s.len()).then_some(value)
}

/// Truncate a failing line for the parse-error message, mirroring the TS
/// `length > 20 ? slice(0, 17) + '...'` (in characters).
fn truncate_for_error(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() > 20 {
        let head: String = chars[..17].iter().collect();
        format!("{head}...")
    } else {
        line.to_owned()
    }
}

/// `extractMetadataRaw`: the ordered multimap of raw metadata keys to their
/// (language, value) occurrences.
fn extract_metadata_raw(mod_source: &str) -> Result<Vec<(String, Vec<MetaValue>)>, MetadataError> {
    let Some(block) = find_metadata_block(mod_source, "WindhawkMod") else {
        return Err(MetadataError::new(
            "Couldn't find a metadata block in the source code",
        ));
    };

    let mut result: Vec<(String, Vec<MetaValue>)> = Vec::new();
    for line in block.split('\n') {
        let line_trimmed = line.trim_end();
        if line_trimmed.is_empty() {
            continue;
        }
        let Some(parsed) = parse_metadata_line(line_trimmed) else {
            return Err(MetadataError::new(format!(
                "Couldn't parse metadata line: {}",
                truncate_for_error(line_trimmed)
            )));
        };
        let value = MetaValue {
            language: parsed.language.map(str::to_owned),
            value: parsed.value.to_owned(),
        };
        match result.iter_mut().find(|(k, _)| k == parsed.key_raw) {
            Some((_, values)) => values.push(value),
            None => result.push((parsed.key_raw.to_owned(), vec![value])),
        }
    }
    Ok(result)
}

fn validate_metadata(metadata: &ModMetadata) -> Result<(), MetadataError> {
    let mod_id = metadata.id.as_deref().unwrap_or("");
    if mod_id.is_empty() {
        return Err(MetadataError::new(
            "Mod id must be specified in the source code",
        ));
    }
    if !mod_id
        .chars()
        .all(|c| matches!(c, '0'..='9' | 'a'..='z' | '-'))
    {
        return Err(MetadataError::new(
            "Mod id must only contain the following characters: 0-9, a-z, and a hyphen (-)",
        ));
    }

    for (category, paths) in [
        ("include", &metadata.include),
        ("exclude", &metadata.exclude),
    ] {
        for path in paths.iter().flatten() {
            if path.contains(['/', '"', '<', '>', '|']) {
                return Err(MetadataError::new(format!(
                    "Mod {category} path contains one of the forbidden characters: / \" < > |"
                )));
            }
        }
    }

    const SUPPORTED_ARCHITECTURE: &[&str] = &["x86", "x86-64", "amd64", "arm64"];
    for architecture in metadata.architecture.iter().flatten() {
        if !SUPPORTED_ARCHITECTURE.contains(&architecture.as_str()) {
            return Err(MetadataError::new(format!(
                "Mod architecture must be one of {}: {architecture}",
                SUPPORTED_ARCHITECTURE.join(", ")
            )));
        }
    }

    Ok(())
}

/// Reject the first localized (`@key:lang`) occurrence of a non-localizable
/// key. The byte-identical check the multi-value and single-value branches
/// shared (factors it to one home). The duplicate checks stay branch-local
/// because they differ - a localizable key rejects a duplicate `(key,
/// language)` pair, a plain single rejects `values.len() > 1` - so they are NOT
/// bundled into one mode-flagged helper.
fn reject_localized(key: &str, values: &[MetaValue]) -> Result<(), MetadataError> {
    for item in values {
        if let Some(l) = &item.language {
            return Err(MetadataError::new(format!(
                "Metadata parameter can't be localized: {key}:{l}"
            )));
        }
    }
    Ok(())
}

/// Assign a single-value key. Exhaustive over `SingleKey`, so adding a key is a
/// compile error here (the A3 replacement for `set_field`'s `unreachable!`).
fn set_single(metadata: &mut ModMetadata, key: SingleKey, value: String) {
    match key {
        SingleKey::Id => metadata.id = Some(value),
        SingleKey::Version => metadata.version = Some(value),
        SingleKey::Github => metadata.github = Some(value),
        SingleKey::Twitter => metadata.twitter = Some(value),
        SingleKey::Homepage => metadata.homepage = Some(value),
        SingleKey::CompilerOptions => metadata.compiler_options = Some(value),
        SingleKey::License => metadata.license = Some(value),
        SingleKey::DonateUrl => metadata.donate_url = Some(value),
        SingleKey::Name => metadata.name = Some(value),
        SingleKey::Description => metadata.description = Some(value),
        SingleKey::Author => metadata.author = Some(value),
    }
}

/// Assign a multi-value key. Exhaustive over `MultiKey`.
fn set_multi(metadata: &mut ModMetadata, key: MultiKey, value: Vec<String>) {
    match key {
        MultiKey::Include => metadata.include = Some(value),
        MultiKey::Exclude => metadata.exclude = Some(value),
        MultiKey::Architecture => metadata.architecture = Some(value),
    }
}

/// `extractMetadata`: parse and validate the metadata block, selecting the
/// best language for localizable parameters.
pub fn extract_metadata(mod_source: &str, language: &str) -> Result<ModMetadata, MetadataError> {
    let metadata_raw = extract_metadata_raw(mod_source)?;

    let mut result = ModMetadata::default();

    for (key_raw, values) in metadata_raw {
        // The TS implementation classifies on the key with one leading
        // underscore stripped, so `@_id` is treated as `@id`.
        let key = key_raw.strip_prefix('_').unwrap_or(&key_raw);

        match classify(key) {
            Some(Classified::SingleLocalizable(single_key)) => {
                let mut languages: Vec<&Option<String>> = Vec::new();
                for item in &values {
                    if languages.contains(&&item.language) {
                        let suffix = item
                            .language
                            .as_ref()
                            .map(|l| format!(":{l}"))
                            .unwrap_or_default();
                        return Err(MetadataError::new(format!(
                            "Duplicate metadata parameter: {key}{suffix}"
                        )));
                    }
                    languages.push(&item.language);
                }
                let candidates: Vec<(Option<String>, &str)> = values
                    .iter()
                    .map(|v| (v.language.clone(), v.value.as_str()))
                    .collect();
                let value = (*best_language_match(language, &candidates)).to_owned();
                set_single(&mut result, single_key, value);
            }
            Some(Classified::Multi(multi_key)) => {
                reject_localized(key, &values)?;
                let value = values.into_iter().map(|v| v.value).collect();
                set_multi(&mut result, multi_key, value);
            }
            Some(Classified::Single(single_key)) => {
                reject_localized(key, &values)?;
                if values.len() > 1 {
                    return Err(MetadataError::new(format!(
                        "Duplicate metadata parameter: {key}"
                    )));
                }
                let value = values
                    .into_iter()
                    .next()
                    .map(|v| v.value)
                    .unwrap_or_default();
                set_single(&mut result, single_key, value);
            }
            None => {
                if key_raw.starts_with('_') {
                    // Ignore for forward compatibility.
                } else {
                    return Err(MetadataError::new(format!(
                        "Unsupported metadata parameter: {key}"
                    )));
                }
            }
        }
    }

    validate_metadata(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASIC: &str = "\
// ==WindhawkMod==
// @id              taskbar-clock
// @name            Taskbar Clock
// @name:fr         Horloge
// @description     A clock mod
// @version         1.0.1
// @include         explorer.exe
// @include         dwm.exe
// @architecture    x86-64
// @compilerOptions -lcomctl32
// ==/WindhawkMod==
";

    #[test]
    fn parses_a_full_block() {
        let m = extract_metadata(BASIC, "en").unwrap();
        assert_eq!(m.id.as_deref(), Some("taskbar-clock"));
        assert_eq!(m.name.as_deref(), Some("Taskbar Clock"));
        assert_eq!(m.description.as_deref(), Some("A clock mod"));
        assert_eq!(m.version.as_deref(), Some("1.0.1"));
        assert_eq!(
            m.include.as_deref(),
            Some(&["explorer.exe".to_owned(), "dwm.exe".to_owned()][..])
        );
        assert_eq!(m.architecture.as_deref(), Some(&["x86-64".to_owned()][..]));
        assert_eq!(m.compiler_options.as_deref(), Some("-lcomctl32"));
    }

    #[test]
    fn localized_name_is_selected_by_language() {
        let m = extract_metadata(BASIC, "fr").unwrap();
        assert_eq!(m.name.as_deref(), Some("Horloge"));
        // Non-localized fallback for a language with no candidate.
        let m = extract_metadata(BASIC, "de").unwrap();
        assert_eq!(m.name.as_deref(), Some("Taskbar Clock"));
    }

    #[test]
    fn missing_block_is_the_canonical_error() {
        assert_eq!(
            extract_metadata("// just code", "en")
                .unwrap_err()
                .to_string(),
            "Couldn't find a metadata block in the source code"
        );
    }

    #[test]
    fn unparsable_line_is_truncated_in_the_error() {
        let src = "// ==WindhawkMod==\n// not-a-metadata-line at all, longer than twenty\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Couldn't parse metadata line: // not-a-metadata..."
        );
    }

    #[test]
    fn duplicate_single_value_is_rejected() {
        let src = "// ==WindhawkMod==\n// @id a\n// @id b\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Duplicate metadata parameter: id"
        );
    }

    #[test]
    fn duplicate_localized_name_is_rejected_with_language() {
        let src =
            "// ==WindhawkMod==\n// @id a\n// @name:fr X\n// @name:fr Y\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Duplicate metadata parameter: name:fr"
        );
    }

    #[test]
    fn localized_multi_value_is_rejected() {
        let src = "// ==WindhawkMod==\n// @id a\n// @include:fr x.exe\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Metadata parameter can't be localized: include:fr"
        );
    }

    #[test]
    fn unknown_parameter_is_rejected_but_underscored_is_ignored() {
        let src = "// ==WindhawkMod==\n// @id a\n// @bogus x\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Unsupported metadata parameter: bogus"
        );
        let src = "// ==WindhawkMod==\n// @id a\n// @_bogus x\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap().id.as_deref(),
            Some("a")
        );
    }

    #[test]
    fn underscored_known_key_is_classified_as_the_known_key() {
        // `@_id` strips to `id` before classification, like the TS code.
        let src = "// ==WindhawkMod==\n// @_id my-id\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap().id.as_deref(),
            Some("my-id")
        );
    }

    #[test]
    fn id_validation() {
        let src = "// ==WindhawkMod==\n// @name X\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Mod id must be specified in the source code"
        );
        let src = "// ==WindhawkMod==\n// @id Bad_Id\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Mod id must only contain the following characters: 0-9, a-z, and a hyphen (-)"
        );
    }

    #[test]
    fn forbidden_path_characters_are_rejected() {
        let src = "// ==WindhawkMod==\n// @id a\n// @include a/b.exe\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Mod include path contains one of the forbidden characters: / \" < > |"
        );
    }

    #[test]
    fn unsupported_architecture_is_rejected() {
        let src = "// ==WindhawkMod==\n// @id a\n// @architecture mips\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Mod architecture must be one of x86, x86-64, amd64, arm64: mips"
        );
    }

    #[test]
    fn region_language_parses_and_invalid_language_fails_the_line() {
        let src = "// ==WindhawkMod==\n// @id a\n// @name:pt-BR Nome\n// ==/WindhawkMod==\n";
        let m = extract_metadata(src, "pt-br").unwrap();
        assert_eq!(m.name.as_deref(), Some("Nome"));

        // `:en-us` is not a valid language tag; the regex then requires
        // whitespace right after the key and fails on the colon.
        let src = "// ==WindhawkMod==\n// @id a\n// @name:en-us X\n// ==/WindhawkMod==\n";
        assert_eq!(
            extract_metadata(src, "en").unwrap_err().to_string(),
            "Couldn't parse metadata line: // @name:en-us X"
        );
    }

    #[test]
    fn value_keeps_inner_whitespace_and_drops_trailing() {
        let src = "// ==WindhawkMod==\n// @id a\n// @name  Two  Words \t\n// ==/WindhawkMod==\n";
        let m = extract_metadata(src, "en").unwrap();
        assert_eq!(m.name.as_deref(), Some("Two  Words"));
    }

    // "No duplicate classification key" is a compile-time property: duplicate
    // keys would be duplicate `match` arms in `classify`.

    #[test]
    fn classification_routes_every_known_key_to_its_field_and_kind() {
        // Drives the classifier for EVERY known key with a distinct sentinel,
        // then destructures the result with NO trailing `..`. Three compile-time
        // backstops guard the routing: a new ModMetadata field that no key
        // targets is a COMPILE error here (the exhaustive destructure); a new
        // SingleKey/MultiKey variant with no
        // assignment arm is a COMPILE error in set_single/set_multi (exhaustive
        // matches); and a forgotten routing surfaces as a value mismatch below.
        // The multi keys carry two/one occurrences to also pin the Multi kind's
        // collect-all behavior alongside the single/localizable routing.
        let src = "\
// ==WindhawkMod==
// @id              id-val
// @version         ver-val
// @github          gh-val
// @twitter         tw-val
// @homepage        hp-val
// @compilerOptions co-val
// @license         lic-val
// @donateUrl       du-val
// @name            name-val
// @description     desc-val
// @author          auth-val
// @include         inc-a
// @include         inc-b
// @exclude         exc-a
// @architecture    x86-64
// ==/WindhawkMod==
";
        let m = extract_metadata(src, "en").unwrap();
        let ModMetadata {
            id,
            version,
            github,
            twitter,
            homepage,
            compiler_options,
            license,
            donate_url,
            name,
            description,
            author,
            include,
            exclude,
            architecture,
        } = m;
        assert_eq!(id.as_deref(), Some("id-val"));
        assert_eq!(version.as_deref(), Some("ver-val"));
        assert_eq!(github.as_deref(), Some("gh-val"));
        assert_eq!(twitter.as_deref(), Some("tw-val"));
        assert_eq!(homepage.as_deref(), Some("hp-val"));
        assert_eq!(compiler_options.as_deref(), Some("co-val"));
        assert_eq!(license.as_deref(), Some("lic-val"));
        assert_eq!(donate_url.as_deref(), Some("du-val"));
        assert_eq!(name.as_deref(), Some("name-val"));
        assert_eq!(description.as_deref(), Some("desc-val"));
        assert_eq!(author.as_deref(), Some("auth-val"));
        // Multi kind: every occurrence collected, in order.
        assert_eq!(
            include.as_deref(),
            Some(&["inc-a".to_owned(), "inc-b".to_owned()][..])
        );
        assert_eq!(exclude.as_deref(), Some(&["exc-a".to_owned()][..]));
        assert_eq!(architecture.as_deref(), Some(&["x86-64".to_owned()][..]));
    }
}
