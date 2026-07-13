//! The vendored new-mod template: a crate-owned copy of the extension's
//! `files/mod_template.wh.cpp`, embedded in the binary. The core deliberately
//! does not serve the template (the new-mod source template stays a front-end
//! asset) and the front-end does not send it with the message, so `windhawk-ui`
//! owns its own copy outright and reads it here.
//!
//! It is a one-time copy, not a synced artifact: the crate's copy and the
//! extension's are two independent scaffolds during the transition (benign - a
//! template is only a starting point the user edits at once), and `windhawk-ui`
//! becomes the template's sole home once the extension's copy is retired.

/// The new-mod source `createNewMod` seeds a fresh workspace with. Parseable by
/// `parseModSource`: the `@id` line names the default mod id the collision loop
/// starts from.
pub const MOD_TEMPLATE: &str = include_str!("mod_template.wh.cpp");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_a_parseable_windhawk_mod_with_an_id() {
        // The handler relies on the metadata block and a non-empty `@id`; guard both
        // so a bad edit to the vendored copy fails here, not at runtime.
        assert!(MOD_TEMPLATE.contains("// ==WindhawkMod=="));
        assert!(MOD_TEMPLATE.contains("// ==/WindhawkMod=="));
        assert!(MOD_TEMPLATE.contains("// @id "));
        assert!(MOD_TEMPLATE.is_ascii());
    }
}
