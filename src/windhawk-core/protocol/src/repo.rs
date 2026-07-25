//! DTOs of the network commands (the repository client), mirroring
//! `fetchCatalog` / `fetchRepoModSource` / `fetchModVersions` in
//! `windhawk-vscode`'s `src/coreClient/contract.ts` 1:1. camelCase field names
//! match the TS property names so the client does no mapping.
//!
//! `fetchCatalog`'s result is the catalog JSON passed through verbatim (the TS
//! `response.json() as Catalog` does no reshaping), so it has no typed result
//! DTO here - the core returns the parsed `serde_json::Value` unchanged,
//! preserving every catalog field. Only `fetchModVersions` reshapes
//! (versions.json -> the normalized `ModVersionInfo` list), so it gets a DTO.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FetchCatalogParams {
    pub language: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FetchRepoModSourceParams {
    pub mod_id: String,
    /// The latest version when omitted (the TS optional `version`). Serialized
    /// even when `None` (as explicit `null`), byte-identical to the caller's old
    /// `json!({ modId, version })` - so it keeps NO `skip_serializing_if`.
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FetchModVersionsParams {
    pub mod_id: String,
}

/// One normalized entry of a mod's versions.json (the TS `ModVersionInfo`).
/// `isPreRelease` is derived from the version string (`version.includes('-')`);
/// `timestamp` is a JSON number passed through, so an integer capture
/// round-trips as an integer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionInfo {
    pub version: String,
    pub timestamp: serde_json::Number,
    pub is_pre_release: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mod_version_info_round_trips() {
        let v = json!({"version": "2.0-beta.1", "timestamp": 1700000000, "isPreRelease": true});
        let dto: ModVersionInfo = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&dto).unwrap(), v);
    }
}
