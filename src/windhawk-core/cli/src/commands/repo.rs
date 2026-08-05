//! The `repo` group: `repo list`, `repo versions`, `repo show`. All three are
//! network reads served through the C ABI async path (`fetchCatalog` /
//! `fetchModVersions` / `fetchRepoModSource` emit no progress, so the async
//! invoke drains straight to the terminal event). The catalog is passed through
//! verbatim - the core returns it unchanged (protocol/repo.rs), so `repo list`
//! navigates the catalog `Value` and re-emits `metadata`/`details` as-is rather
//! than round-tripping them through a typed DTO that would drop unknown fields.

use std::io::{self, Write};

use serde_json::{Value, json};
use windhawk_core_protocol::{
    FetchCatalogParams, FetchModVersionsParams, FetchRepoModSourceParams, InitialSettings,
    ListInstalledModsParams, ListInstalledModsResult, ModMetadata, ModVersionInfo,
    SyncCatalogToProfileRequest,
};

use crate::Environment;
use crate::args::{RepoCommand, RepoListArgs};
use crate::commands::parse::{parse_mod_source, reject_initial_settings_error, require_metadata};
use crate::commands::render::{
    MetadataHeader, write_description_and_readme, write_metadata_header,
};
use crate::commands::{app_settings, check_for_updates, language, warn_load_errors};
use crate::error::CliError;
use crate::output::CommandResult;

pub fn dispatch(
    env: &Environment,
    command: RepoCommand,
) -> Result<Box<dyn CommandResult>, CliError> {
    match command {
        RepoCommand::List(args) => list(env, args),
        RepoCommand::Versions { id } => versions(env, &id),
        RepoCommand::Show { id, version } => show(env, &id, version.as_deref()),
    }
}

// ---------------------------------------------------------------------------
// repo list
// ---------------------------------------------------------------------------

fn list(env: &Environment, args: RepoListArgs) -> Result<Box<dyn CommandResult>, CliError> {
    let settings = app_settings(env)?;

    // Network read; the catalog is returned verbatim (no progress events).
    let catalog = env.core.invoke_async(
        "fetchCatalog",
        &FetchCatalogParams {
            language: language(&settings),
        },
        |_| {},
    )?;

    // Record the catalog's latest versions in the user profile so it stays in
    // sync across GUI/CLI access. Unlike the GUI, the CLI posts no tray "new
    // updates found" notification: only `app settings set` spawns windhawk.exe.
    // The catalog travels opaquely (the request DTO carries the FULL Value, not
    // the lossy parse projection).
    env.core.invoke(
        "syncCatalogToProfile",
        &SyncCatalogToProfileRequest {
            catalog: catalog.clone(),
        },
    )?;

    // The installed-state join (--with-installed) is a pure read: the sync above
    // already did any needed profile write, so this does NOT sync (syncProfile
    // false).
    let installed = if args.with_installed {
        let result: ListInstalledModsResult = env.core.invoke_as(
            "listInstalledMods",
            &ListInstalledModsParams {
                language: language(&settings),
                check_for_updates: check_for_updates(&settings),
                sync_profile: false,
            },
        )?;
        warn_load_errors(env, &result);
        Some(result)
    } else {
        None
    };

    let mut ids: Vec<&str> = catalog
        .get("mods")
        .and_then(Value::as_object)
        .map(|mods| mods.keys().map(String::as_str).collect())
        .unwrap_or_default();
    ids.sort_unstable();

    let mut rows = Vec::new();
    for id in ids {
        let entry = &catalog["mods"][id];
        let metadata = entry.get("metadata").cloned().unwrap_or(Value::Null);
        let details = entry.get("details").cloned().unwrap_or(Value::Null);

        // Join installed state only when the mod is actually installed (has a
        // config or parseable metadata), matching the TS guard. The rule is the
        // shared `InstalledModListEntry::is_installed()`, single-sourced in
        // protocol and called by the UI's catalog overlay too, so it cannot
        // drift.
        let installed_json = installed.as_ref().and_then(|result| {
            let entry = result.mods.get(id)?;
            if !entry.is_installed() {
                return None;
            }
            Some(json!({
                "metadata": entry.metadata,
                "config": entry.config,
                "userRating": entry.user_rating,
            }))
        });

        rows.push(RepoRow {
            id: id.to_owned(),
            metadata,
            details,
            installed: installed_json,
        });
    }

    Ok(Box::new(RepoListResult { mods: rows }))
}

struct RepoRow {
    id: String,
    metadata: Value,
    details: Value,
    installed: Option<Value>,
}

struct RepoListResult {
    mods: Vec<RepoRow>,
}

impl CommandResult for RepoListResult {
    fn json_data(&self) -> Value {
        let mods: Vec<Value> = self
            .mods
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                obj.insert("id".to_owned(), Value::String(row.id.clone()));
                obj.insert("metadata".to_owned(), row.metadata.clone());
                obj.insert("details".to_owned(), row.details.clone());
                // `installed` is present only with --with-installed and an
                // installed mod (matches JSON.stringify omitting undefined).
                if let Some(installed) = &row.installed {
                    obj.insert("installed".to_owned(), installed.clone());
                }
                Value::Object(obj)
            })
            .collect();
        json!({ "mods": mods })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        for row in &self.mods {
            let version = str_field(&row.metadata, "version");
            let name = str_field(&row.metadata, "name");
            let marker = if row.installed.is_some() {
                "\t[installed]"
            } else {
                ""
            };
            writeln!(out, "{}\t{}\t{}{}", row.id, version, name, marker)?;
        }
        Ok(())
    }
}

/// A string metadata field, or "" when absent or non-string (the TS `?? ''`).
fn str_field(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

// ---------------------------------------------------------------------------
// repo versions
// ---------------------------------------------------------------------------

fn versions(env: &Environment, id: &str) -> Result<Box<dyn CommandResult>, CliError> {
    // Shape validation and isPreRelease derivation happen in the core; a 404
    // maps to MOD_NOT_IN_REPO (exit 5), other failures to REPO_UNREACHABLE
    // (exit 6).
    let versions: Vec<ModVersionInfo> = env.core.invoke_async_as(
        "fetchModVersions",
        &FetchModVersionsParams {
            mod_id: id.to_owned(),
        },
        |_| {},
    )?;

    Ok(Box::new(RepoVersionsResult {
        id: id.to_owned(),
        versions,
    }))
}

struct RepoVersionsResult {
    id: String,
    versions: Vec<ModVersionInfo>,
}

impl CommandResult for RepoVersionsResult {
    fn json_data(&self) -> Value {
        json!({ "id": self.id, "versions": self.versions })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        for v in &self.versions {
            let iso = iso_from_unix(&v.timestamp);
            let mark = if v.is_pre_release {
                "\t[pre-release]"
            } else {
                ""
            };
            writeln!(out, "{}\t{}{}", v.version, iso, mark)?;
        }
        Ok(())
    }
}

/// Format a Unix timestamp (seconds, possibly fractional) as an ISO-8601 UTC
/// string with milliseconds and a `Z` suffix - the exact shape of JS
/// `new Date(ts * 1000).toISOString()`. No date crate: a self-contained
/// civil-from-days conversion (Howard Hinnant's algorithm).
fn iso_from_unix(timestamp: &serde_json::Number) -> String {
    let total_ms = (timestamp.as_f64().unwrap_or(0.0) * 1000.0).round() as i64;
    let secs = total_ms.div_euclid(1000);
    let ms = total_ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{ms:03}Z")
}

/// (year, month, day) from a count of days since the Unix epoch (1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// repo show
// ---------------------------------------------------------------------------

fn show(
    env: &Environment,
    id: &str,
    version: Option<&str>,
) -> Result<Box<dyn CommandResult>, CliError> {
    // Source arrives CRLF-normalized from the core (network read; 404 -> exit 5,
    // other failures -> exit 6).
    let source: String = env.core.invoke_async_as(
        "fetchRepoModSource",
        &FetchRepoModSourceParams {
            mod_id: id.to_owned(),
            version: version.map(str::to_owned),
        },
        |_| {},
    )?;

    let settings = app_settings(env)?;
    let parsed = parse_mod_source(env, &source, &language(&settings))?;

    // Parse failures of a fetched source surface as a generic failure (exit 1),
    // matching the TS direct-extract behavior.
    let origin = format!("repository mod '{id}'");
    let metadata = require_metadata(parsed.metadata, parsed.errors.metadata, |message| {
        CliError::generic(format!("Failed to parse metadata from {origin}: {message}"))
    })?;
    reject_initial_settings_error(&origin, parsed.errors.initial_settings)?;

    let resolved_version = metadata
        .version
        .clone()
        .or_else(|| version.map(str::to_owned))
        .unwrap_or_default();

    Ok(Box::new(RepoShowResult {
        id: id.to_owned(),
        version: resolved_version,
        metadata,
        readme: parsed.readme,
        initial_settings: parsed.initial_settings,
    }))
}

struct RepoShowResult {
    id: String,
    version: String,
    metadata: ModMetadata,
    readme: Option<String>,
    initial_settings: Option<InitialSettings>,
}

impl CommandResult for RepoShowResult {
    fn json_data(&self) -> Value {
        json!({
            "id": self.id,
            "version": self.version,
            "metadata": self.metadata,
            "readme": self.readme,
            "initialSettings": self.initial_settings,
        })
    }

    fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        write_metadata_header(
            out,
            &MetadataHeader {
                id: &self.id,
                name: self.metadata.name.as_deref().unwrap_or(""),
                // The resolved repo version, not metadata.version.
                version: &self.version,
                author: self.metadata.author.as_deref().unwrap_or(""),
                architecture: self.metadata.architecture.as_deref(),
            },
        )?;
        write_description_and_readme(
            out,
            self.metadata.description.as_deref(),
            self.readme.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_from_unix_matches_to_iso_string() {
        // 2020-09-13T12:26:40.000Z (a known epoch landmark).
        assert_eq!(
            iso_from_unix(&serde_json::Number::from(1_600_000_000)),
            "2020-09-13T12:26:40.000Z"
        );
        // The epoch itself.
        assert_eq!(
            iso_from_unix(&serde_json::Number::from(0)),
            "1970-01-01T00:00:00.000Z"
        );
        // A pre-release-style later timestamp.
        assert_eq!(
            iso_from_unix(&serde_json::Number::from(1_700_000_000)),
            "2023-11-14T22:13:20.000Z"
        );
    }
}

/// Golden (snapshot) tests of the compute-then-render seam for the `repo`
/// results: the row formats (with the `[installed]` and `[pre-release]`
/// markers), the verbatim catalog `metadata`/`details` pass-through, and the
/// `repo show` block, with no network or session.
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::output::render_text;

    #[test]
    fn repo_list_rows_and_installed_marker() {
        let result = RepoListResult {
            mods: vec![
                RepoRow {
                    id: "alpha".to_owned(),
                    metadata: json!({ "id": "alpha", "version": "1.0", "name": "Alpha" }),
                    details: json!({ "users": 5 }),
                    installed: None,
                },
                RepoRow {
                    id: "beta".to_owned(),
                    metadata: json!({ "id": "beta", "version": "2.0", "name": "Beta" }),
                    details: json!({}),
                    installed: Some(json!({ "metadata": null, "config": null, "userRating": 0 })),
                },
            ],
        };
        assert_eq!(
            render_text(&result),
            "alpha\t1.0\tAlpha\nbeta\t2.0\tBeta\t[installed]\n"
        );
        let json = result.json_data();
        // metadata/details pass through verbatim; `installed` only on beta.
        assert_eq!(json["mods"][0]["details"], json!({ "users": 5 }));
        assert_eq!(json["mods"][0].get("installed"), None);
        assert!(json["mods"][1]["installed"].is_object());
    }

    #[test]
    fn repo_versions_marks_pre_releases() {
        let versions: Vec<ModVersionInfo> = serde_json::from_value(json!([
            { "version": "1.0", "timestamp": 1_600_000_000, "isPreRelease": false },
            { "version": "2.0-beta.1", "timestamp": 1_700_000_000, "isPreRelease": true },
        ]))
        .unwrap();
        let result = RepoVersionsResult {
            id: "some-mod".to_owned(),
            versions,
        };
        assert_eq!(
            render_text(&result),
            "1.0\t2020-09-13T12:26:40.000Z\n\
             2.0-beta.1\t2023-11-14T22:13:20.000Z\t[pre-release]\n"
        );
        assert_eq!(result.json_data()["id"], json!("some-mod"));
    }

    #[test]
    fn repo_show_renders_metadata_block() {
        let result = RepoShowResult {
            id: "repo-mod".to_owned(),
            version: "1.0.0".to_owned(),
            metadata: ModMetadata {
                id: Some("repo-mod".to_owned()),
                name: Some("Repo Mod".to_owned()),
                version: Some("1.0.0".to_owned()),
                author: Some("Tester".to_owned()),
                architecture: Some(vec!["x86-64".to_owned()]),
                ..Default::default()
            },
            readme: None,
            initial_settings: None,
        };
        assert_eq!(
            render_text(&result),
            "ID:            repo-mod\n\
             Name:          Repo Mod\n\
             Version:       1.0.0\n\
             Author:        Tester\n\
             Architectures: x86-64\n"
        );
        assert_eq!(result.json_data()["version"], json!("1.0.0"));
    }
}
