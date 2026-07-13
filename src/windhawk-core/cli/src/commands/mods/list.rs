//! `mod list`: the installed-mods listing, with the enabled/disabled and
//! update-available filters.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{ListInstalledModsParams, ListInstalledModsResult, ModConfig};

use crate::Environment;
use crate::args::ModListArgs;
use crate::commands::{app_settings, check_for_updates, language, warn_load_errors};
use crate::error::CliError;
use crate::output::CommandResult;

pub(super) fn list(
    env: &Environment,
    args: ModListArgs,
) -> Result<Box<dyn CommandResult>, CliError> {
    if args.enabled && args.disabled {
        return Err(CliError::usage(
            "--enabled and --disabled are mutually exclusive",
        ));
    }

    let settings = app_settings(env)?;

    // syncProfile mirrors the GUI's installed-mods query: per-mod refresh plus
    // removed-mod cleanup, persisted if anything changed. The update-check gate
    // mirrors the extension: when checks are disabled, no mod reports an update.
    let result: ListInstalledModsResult = env.core.invoke_as(
        "listInstalledMods",
        &ListInstalledModsParams {
            language: language(&settings),
            check_for_updates: check_for_updates(&settings),
            sync_profile: true,
        },
    )?;

    warn_load_errors(env, &result);

    // The result map is a BTreeMap, already sorted by mod id (a stable wire
    // contract); iterate it in that order.
    let mut rows = Vec::new();
    for (id, entry) in &result.mods {
        let enabled = !entry.config.as_ref().map(|c| c.disabled).unwrap_or(false);
        let version = entry
            .metadata
            .as_ref()
            .and_then(|m| m.version.clone())
            .unwrap_or_default();

        if args.enabled && !enabled {
            continue;
        }
        if args.disabled && enabled {
            continue;
        }
        if args.update_available && !entry.update_available {
            continue;
        }

        rows.push(ListRow {
            id: id.clone(),
            version,
            name: entry.metadata.as_ref().and_then(|m| m.name.clone()),
            author: entry.metadata.as_ref().and_then(|m| m.author.clone()),
            description: entry.metadata.as_ref().and_then(|m| m.description.clone()),
            enabled,
            update_available: entry.update_available,
            user_rating: entry.user_rating,
            config: entry.config.clone(),
        });
    }

    Ok(Box::new(ModListResult { mods: rows }))
}

struct ListRow {
    id: String,
    version: String,
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    enabled: bool,
    update_available: bool,
    user_rating: i64,
    config: Option<ModConfig>,
}

impl ListRow {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "version": self.version,
            "name": self.name,
            "author": self.author,
            "description": self.description,
            "enabled": self.enabled,
            "updateAvailable": self.update_available,
            "userRating": self.user_rating,
            "config": self.config,
        })
    }
}

struct ModListResult {
    mods: Vec<ListRow>,
}

impl CommandResult for ModListResult {
    fn json_data(&self) -> Value {
        json!({ "mods": self.mods.iter().map(ListRow::to_json).collect::<Vec<_>>() })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        for row in &self.mods {
            let state = if row.enabled { "enabled" } else { "disabled" };
            let mark = if row.update_available {
                "\t[update]"
            } else {
                ""
            };
            let name = row.name.as_deref().unwrap_or("");
            writeln!(
                out,
                "{}\t{}\t{}{}\t{}",
                row.id, row.version, state, mark, name
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::commands::mods::test_support::config;
    use crate::output::render_text;

    #[test]
    fn mod_list_empty_renders_nothing() {
        let result = ModListResult { mods: vec![] };
        assert_eq!(render_text(&result), "");
        assert_eq!(result.json_data(), json!({ "mods": [] }));
    }

    #[test]
    fn mod_list_marks_update_and_disabled_state() {
        let result = ModListResult {
            mods: vec![
                ListRow {
                    id: "alpha".to_owned(),
                    version: "1.0".to_owned(),
                    name: Some("Alpha".to_owned()),
                    author: None,
                    description: None,
                    enabled: true,
                    update_available: false,
                    user_rating: 0,
                    config: Some(config(false)),
                },
                ListRow {
                    id: "beta".to_owned(),
                    version: "2.0".to_owned(),
                    name: Some("Beta".to_owned()),
                    author: None,
                    description: None,
                    enabled: false,
                    update_available: true,
                    user_rating: 3,
                    config: Some(config(true)),
                },
            ],
        };
        // enabled, no marker; then disabled with the [update] marker.
        assert_eq!(
            render_text(&result),
            "alpha\t1.0\tenabled\tAlpha\nbeta\t2.0\tdisabled\t[update]\tBeta\n"
        );
        let json = result.json_data();
        assert_eq!(json["mods"][0]["enabled"], json!(true));
        assert_eq!(json["mods"][1]["updateAvailable"], json!(true));
        assert_eq!(json["mods"][1]["userRating"], json!(3));
    }
}
