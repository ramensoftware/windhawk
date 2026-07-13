//! The shared session parse path and the pure post-parse classifiers.
//! `parse_mod_source` is the one `parseModSource` invoke + decode the five
//! session-bearing sites share (`mod show` / `mod settings set` / `mod
//! compile`, the `mod install` / `mod update` pipeline via
//! `extract_and_reconcile`, and `repo show`); `source meta` keeps its own
//! stateless invoke and is the one site NOT routed through it. The two
//! classifiers run the GENUINELY shared post-parse checks over an
//! already-parsed [`ParsedModSource`]; the per-command variation - which checks
//! run, and whether a parse failure is a usage (exit 2) or generic (exit 1)
//! error - stays at each call site, the latter through the constructor it
//! passes in.

use windhawk_core_protocol::{ModMetadata, ParseModSourceParams, ParsedModSource};

use crate::Environment;
use crate::error::CliError;

/// The session parse path the five session-bearing sites share verbatim: invoke
/// `parseModSource` with `{source, language}` and decode into
/// [`ParsedModSource`]. The caller passes the `language` it derived from the
/// command's single AppSettings fetch (`language(&settings)`), so this helper
/// takes no settings dependency of its own.
pub(crate) fn parse_mod_source(
    env: &Environment,
    source: &str,
    language: &str,
) -> Result<ParsedModSource, CliError> {
    Ok(env.core.invoke_as(
        "parseModSource",
        &ParseModSourceParams {
            source: source.to_owned(),
            language: language.to_owned(),
        },
    )?)
}

/// The metadata-None check: return the parsed `metadata`, or build a parse error
/// from the `errors.metadata` message (falling back to a fixed text when absent)
/// through the caller's `on_parse_error` constructor. Both inputs are taken BY
/// VALUE, so the metadata is returned owned with no call-site clone. The
/// constructor is the only per-site variation: `mod show` / `repo show` classify
/// a malformed stored/fetched source as `generic` (exit 1), the install path and
/// `source meta` as `usage` (exit 2, the latter wrapping the file name).
pub(crate) fn require_metadata(
    metadata: Option<ModMetadata>,
    parse_error: Option<String>,
    on_parse_error: impl FnOnce(String) -> CliError,
) -> Result<ModMetadata, CliError> {
    metadata.ok_or_else(|| {
        on_parse_error(parse_error.unwrap_or_else(|| "Failed to parse mod metadata".to_owned()))
    })
}

/// The `errors.initialSettings -> generic` check: a malformed settings block in
/// an already-installed or fetched source is an internal problem (exit 1), not a
/// usage error. Takes the `errors.initial_settings` message BY VALUE. Used by
/// `mod show`, `mod settings set`, and `repo show`; the install path and
/// `source meta` never read it.
pub(crate) fn reject_initial_settings_error(parse_error: Option<String>) -> Result<(), CliError> {
    match parse_error {
        Some(message) => Err(CliError::generic(message)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_metadata_returns_present_metadata() {
        let meta = ModMetadata {
            id: Some("m".to_owned()),
            ..Default::default()
        };
        let got = require_metadata(Some(meta.clone()), None, CliError::usage).unwrap();
        assert_eq!(got.id, meta.id);
    }

    #[test]
    fn require_metadata_uses_the_message_and_the_caller_constructor() {
        // The constructor classifies the failure (the message is the
        // errors.metadata string when present): usage (exit 2) for a
        // user-supplied source, generic (exit 1) for a stored one.
        let usage =
            require_metadata(None, Some("bad meta".to_owned()), CliError::usage).unwrap_err();
        assert_eq!(usage.exit_code(), 2);
        assert_eq!(usage.message(), "bad meta");

        let generic =
            require_metadata(None, Some("bad meta".to_owned()), CliError::generic).unwrap_err();
        assert_eq!(generic.exit_code(), 1);
        assert_eq!(generic.message(), "bad meta");
    }

    #[test]
    fn require_metadata_falls_back_to_a_fixed_message() {
        // No errors.metadata string: the fixed fallback text.
        let err = require_metadata(None, None, CliError::generic).unwrap_err();
        assert_eq!(err.message(), "Failed to parse mod metadata");
    }

    #[test]
    fn reject_initial_settings_error_passes_none_and_rejects_some() {
        assert!(reject_initial_settings_error(None).is_ok());
        let err = reject_initial_settings_error(Some("bad settings".to_owned())).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.message(), "bad settings");
    }
}
