//! The `inspectUserData` projection: the compact manifest a front-end reads to
//! build an import selection over a specific archive, without carrying source or
//! settings values. Pure over an already-decoded archive; `core` maps this to
//! the protocol manifest DTO for the wire.

use super::{ArchiveMod, UserDataArchive};
use crate::mod_id::ModId;

/// The manifest for one archive: what it carries at the top level plus a
/// per-mod availability summary (which facets the archive actually carries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveManifest {
    pub has_app_settings: bool,
    pub mods: Vec<ManifestMod>,
}

/// One mod's manifest row: its identity plus which facets are present, so a
/// front-end can show an accurate "available in this archive" state per part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestMod {
    pub mod_id: String,
    /// Whether `mod_id` is a `local@` id, resolved here so a front-end need not
    /// re-implement the prefix rule to label the row.
    pub is_local: bool,
    pub version: String,
    pub name: Option<String>,
    /// Whether the archive embeds this mod's source. `false` marks a
    /// reference-only repository mod, whose import needs the network - a
    /// front-end can warn when importing such an archive offline.
    pub has_source: bool,
    pub has_settings: bool,
    pub has_config: bool,
}

/// Project `archive` to its manifest.
pub fn manifest(archive: &UserDataArchive) -> ArchiveManifest {
    ArchiveManifest {
        has_app_settings: archive.app_settings.is_some(),
        mods: archive.mods.iter().map(manifest_mod).collect(),
    }
}

fn manifest_mod(m: &ArchiveMod) -> ManifestMod {
    ManifestMod {
        mod_id: m.mod_id.clone(),
        is_local: ModId::str_is_local(&m.mod_id),
        version: m.version.clone(),
        name: m.name.clone(),
        has_source: m.source.is_some(),
        has_settings: m.settings.is_some(),
        has_config: m.config.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ArchiveMod, ArchiveModConfig, FORMAT_TAG};
    use super::*;

    #[test]
    fn projects_metadata_and_per_mod_availability() {
        let archive = UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: Some(serde_json::json!({ "language": "en" })),
            mods: vec![
                // A reference-only repository mod with settings but no config.
                ArchiveMod {
                    mod_id: "taskbar-clock".to_owned(),
                    version: "1.2.0".to_owned(),
                    name: Some("Taskbar Clock".to_owned()),
                    source: None,
                    settings: Some(serde_json::json!({ "ShowSeconds": 1 })),
                    config: None,
                },
                // A local mod: source embedded, config carried, no settings.
                ArchiveMod {
                    mod_id: "local@my-mod".to_owned(),
                    version: "0.1".to_owned(),
                    name: None,
                    source: Some("// ==WindhawkMod==\n".to_owned()),
                    settings: None,
                    config: Some(ArchiveModConfig::default()),
                },
            ],
        };

        let m = manifest(&archive);
        assert!(m.has_app_settings);
        assert_eq!(
            m.mods,
            vec![
                ManifestMod {
                    mod_id: "taskbar-clock".to_owned(),
                    is_local: false,
                    version: "1.2.0".to_owned(),
                    name: Some("Taskbar Clock".to_owned()),
                    has_source: false,
                    has_settings: true,
                    has_config: false,
                },
                ManifestMod {
                    mod_id: "local@my-mod".to_owned(),
                    is_local: true,
                    version: "0.1".to_owned(),
                    name: None,
                    has_source: true,
                    has_settings: false,
                    has_config: true,
                },
            ]
        );
    }

    #[test]
    fn an_app_settings_only_archive_has_no_mods_and_flags_app_settings() {
        let archive = UserDataArchive {
            format: FORMAT_TAG.to_owned(),
            app_settings: Some(serde_json::json!({ "language": "en" })),
            mods: vec![],
        };
        let m = manifest(&archive);
        assert!(m.has_app_settings);
        assert!(m.mods.is_empty());
    }
}
