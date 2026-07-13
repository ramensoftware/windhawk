//! `WhCoreGetInfoJson` payload. Static info: no session required.

use windhawk_core_protocol::{CONTRACT_VERSION, CoreStaticInfo};

pub fn core_info_json() -> String {
    let info = CoreStaticInfo {
        contract_version: CONTRACT_VERSION.to_owned(),
        core_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    serde_json::to_string(&info).unwrap_or_else(|_| {
        format!("{{\"contractVersion\":\"{CONTRACT_VERSION}\",\"coreVersion\":\"unknown\"}}")
    })
}
