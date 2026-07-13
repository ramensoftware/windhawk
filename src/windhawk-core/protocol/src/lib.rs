//! The wire contract of windhawk-core: serde DTOs mirroring the TypeScript
//! contract module (`src/coreClient/contract.ts` in the front-end repository)
//! 1:1, the request/response/event envelopes, and the error-code enum.
//!
//! This crate is self-contained on purpose: domain types are never re-exported
//! through it, even when shapes coincide; conversions live in the
//! `windhawk-core` application crate. Depends on serde only.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod compile;
mod envelope;
mod error;
mod install;
mod inventory;
mod parse_mod_source;
mod profile;
mod repo;
mod settings;

pub use compile::{
    CompileInstalledModParams, CompileInstalledModResult, NotifyTrayParams, TrayAction,
};
pub use envelope::{OperationEvent, RequestEnvelope, response_err, response_ok};
pub use error::{CompileDetails, ErrorCode, SourceLocation, WireError};
pub use install::{InstallModParams, InstallModResult};
pub use inventory::COMMAND_INVENTORY;
pub use parse_mod_source::{
    AppendToModIdAndNameParams, InitialSettingItem, InitialSettings, InitialSettingsValue,
    ModMetadata, ParseModSourceParams, ParsedModSource, ParsedModSourceErrors,
};
pub use profile::{
    AppUpdateStatus, CatalogForProfileSync, InstalledModListEntry, ListInstalledModsParams,
    ListInstalledModsResult, ModLoadError, ProfileWatchInfo, SetModRatingParams,
    SyncCatalogToProfileParams, SyncCatalogToProfileRequest, SyncCatalogToProfileResult,
};
pub use repo::{
    FetchCatalogParams, FetchModVersionsParams, FetchRepoModSourceParams, ModVersionInfo,
};
pub use settings::{
    AppSettings, AppSettingsIntents, AppSettingsPatch, AppSettingsPatchParams, CoreFsPaths,
    CoreInfo, EngineSettings, EngineSettingsPatch, ModConfig, ModConfigPatch, ModIdParams,
    SetModEnabledParams, SetModLoggingEnabledParams, SetModSettingsParams, UpdateModConfigParams,
};

/// Contract version reported by `WhCoreGetInfoJson` and `getCoreInfo`, asserted
/// by the TypeScript client at session creation. Must match `CONTRACT_VERSION`
/// in the front-end repository's `src/coreClient/contract.ts`.
pub const CONTRACT_VERSION: &str = "0.1.0";

/// Payload of `WhCoreGetInfoJson`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoreStaticInfo {
    pub contract_version: String,
    pub core_version: String,
}
