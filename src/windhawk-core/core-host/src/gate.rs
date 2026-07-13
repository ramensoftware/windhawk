//! The `contractVersion` gate. `core-client` hard-gates only the ABI integer;
//! the contract check is consumer policy, so it lives here and is shared by
//! both consumers. A mismatch is fatal for the native consumers (DLL-only, no
//! graceful fallback - unlike the bridge, whose TS client handles it), so the
//! gate refuses to run.

use windhawk_core_client::CoreLibrary;
use windhawk_core_protocol::{CONTRACT_VERSION, CoreStaticInfo};

use crate::error::HostError;

/// Read `getCoreInfo` off the loaded library and enforce the contract version.
/// The ABI integer was already hard-gated by `core-client` at load. The mismatch
/// wording is consumer-neutral ("this build expects ...") so both the CLI and the
/// UI surface the same text.
pub(crate) fn gate_contract(lib: &CoreLibrary) -> Result<(), HostError> {
    let info_json = lib.get_info_json()?;
    let info: CoreStaticInfo = serde_json::from_str(&info_json)
        .map_err(|e| HostError::gate(format!("parsing getCoreInfo: {e}")))?;
    if info.contract_version != CONTRACT_VERSION {
        return Err(HostError::gate(format!(
            "windhawk-core contract version mismatch: DLL has {}, this build expects {CONTRACT_VERSION}",
            info.contract_version
        )));
    }
    Ok(())
}
