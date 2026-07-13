//! Pure logic of windhawk-core: mod source metadata/readme/initial-settings
//! parsing. No I/O, no Win32, no clocks; compilable and testable on any host
//! OS.
//!
//! The normative reference for parsing behavior is the TypeScript
//! implementation it replaces (`src/services/modSource.ts` in the front-end
//! repository); the regex semantics of that file are reproduced here by
//! hand-rolled scanners (the dependency policy admits no regex crate).
//! Full-corpus extraction parity is the exit criterion; known message-level
//! divergences (YAML parser diagnostics, settings schema diagnostics) are
//! documented at their sites.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod compile_targets;
mod dll_name;
mod installer_lang;
mod language;
mod metadata;
mod mod_id;
mod model;
mod profile;
mod scan;
mod settings;
mod settings_codec;
mod text;
mod transform;
mod version;

pub use compile_targets::{ArchTarget, CompilationTarget, subfolders_for_arch, targets_for_arch};
pub use dll_name::{compiled_dll_name, ends_with_random_suffix, lcg_next_six, lcg_seed};
pub use installer_lang::language_to_installer_lcid;
pub use language::{DEFAULT_LANGUAGE, best_language_match};
pub use metadata::extract_metadata;
pub use mod_id::{ModId, Version};
pub use model::{
    EngineSettingValue, MetadataError, ModMetadata, SettingItem, SettingValue, SettingsParseError,
};
pub use profile::Profile;
pub use scan::extract_readme;
pub use settings::{extract_initial_settings, extract_initial_settings_for_engine};
pub use settings_codec::{bool_to_int, int_to_bool, join_pipe, split_pipe};
pub use text::normalize_crlf;
pub use transform::append_to_id_and_name;
pub use version::{coerce as coerce_version, higher_version, is_pre_release, is_update_available};
