//! `mod show`: the installed mod's metadata, README, declared settings, and
//! config, rendered as the shared metadata block plus the `State:` line.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{InitialSettings, ModConfig, ModMetadata};

use crate::Environment;
use crate::commands::parse::{parse_mod_source, reject_initial_settings_error, require_metadata};
use crate::commands::render::{
    MetadataHeader, write_description_and_readme, write_metadata_header,
};
use crate::commands::{app_settings, language};
use crate::error::CliError;
use crate::output::CommandResult;

pub(super) fn show(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    let config = super::require_config(env, id)?;
    let source = super::require_source(env, id)?;
    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings))?;

    // A malformed stored source surfaces as a generic failure (exit 1), matching
    // the TS direct-extract behavior - the source was valid when installed, so a
    // parse failure now is an internal problem, not a usage error.
    let origin = format!("installed mod '{id}'");
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::generic(format!("Failed to parse metadata from {origin}: {message}"))
    })?;
    reject_initial_settings_error(&origin, parsed.errors.initial_settings)?;

    Ok(Box::new(ModShowResult {
        id: id.to_owned(),
        metadata,
        readme: parsed.readme,
        initial_settings: parsed.initial_settings,
        config,
    }))
}

struct ModShowResult {
    id: String,
    metadata: ModMetadata,
    readme: Option<String>,
    initial_settings: Option<InitialSettings>,
    config: ModConfig,
}

impl CommandResult for ModShowResult {
    fn json_data(&self) -> Value {
        json!({
            "id": self.id,
            "metadata": self.metadata,
            "readme": self.readme,
            "initialSettings": self.initial_settings,
            "config": self.config,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        write_metadata_header(
            out,
            &MetadataHeader {
                id: &self.id,
                name: self.metadata.name.as_deref().unwrap_or(""),
                version: self.metadata.version.as_deref().unwrap_or(""),
                author: self.metadata.author.as_deref().unwrap_or(""),
                architecture: self.metadata.architecture.as_deref(),
            },
        )?;
        // The State line sits between the header and the Description/README tail,
        // so it stays at this call site rather than folding into either helper.
        let enabled = !self.config.disabled;
        writeln!(
            out,
            "State:         {}",
            if enabled { "enabled" } else { "disabled" }
        )?;
        write_description_and_readme(
            out,
            self.metadata.description.as_deref(),
            self.readme.as_deref(),
        )
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::commands::mods::test_support::{config, happy_metadata};
    use crate::output::render_text;

    #[test]
    fn mod_show_renders_the_full_block() {
        let result = ModShowResult {
            id: "happy-mod".to_owned(),
            metadata: happy_metadata(),
            readme: Some("Line one\nLine two\n".to_owned()),
            initial_settings: None,
            config: config(false),
        };
        assert_eq!(
            render_text(&result),
            "ID:            happy-mod\n\
             Name:          Happy Mod\n\
             Version:       1.2.3\n\
             Author:        Tester\n\
             Architectures: x86-64\n\
             State:         enabled\n\
             \nDescription:\n  A test mod.\n\
             \nREADME:\nLine one\nLine two\n"
        );
        let json = result.json_data();
        assert_eq!(json["id"], json!("happy-mod"));
        assert_eq!(json["readme"], json!("Line one\nLine two\n"));
        assert_eq!(json["initialSettings"], json!(null));
    }

    #[test]
    fn mod_show_minimal_omits_optional_blocks() {
        // No architecture / description / readme: those blocks are skipped, and a
        // disabled mod reports State: disabled.
        let result = ModShowResult {
            id: "bare".to_owned(),
            metadata: ModMetadata {
                id: Some("bare".to_owned()),
                version: Some("0.1".to_owned()),
                ..Default::default()
            },
            readme: None,
            initial_settings: None,
            config: config(true),
        };
        assert_eq!(
            render_text(&result),
            "ID:            bare\n\
             Name:          \n\
             Version:       0.1\n\
             Author:        \n\
             State:         disabled\n"
        );
    }
}
