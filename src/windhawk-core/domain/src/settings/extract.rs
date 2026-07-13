//! Source-scan helpers for the settings pipeline: reading the mod's
//! `@id`/`@version` to key the per-version workarounds. The settings-block
//! extraction itself is `crate::scan::find_comment_block`, called by the
//! orchestrator in the parent module.

use crate::language::DEFAULT_LANGUAGE;
use crate::mod_id::{ModId, Version};

/// Read the mod's `@id` and `@version` from its metadata block, to key the
/// per-version settings workarounds. A malformed metadata block yields
/// `None` (the mod is broken anyway, so no workaround would help).
pub(super) fn mod_id_and_version(mod_source: &str) -> (Option<ModId>, Option<Version>) {
    match crate::metadata::extract_metadata(mod_source, DEFAULT_LANGUAGE) {
        Ok(metadata) => (
            metadata.id.map(ModId::from),
            metadata.version.map(Version::from),
        ),
        Err(_) => (None, None),
    }
}
