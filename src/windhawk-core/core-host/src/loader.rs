//! DLL path resolution and [`GatedCore`]: the loaded, ABI- and contract-gated
//! library. Exposes the session-free stateless invokes and the session
//! constructor.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use windhawk_core_client::{CoreLibrary, SessionCallbacks};

use crate::config::SessionConfig;
use crate::error::{HostError, parse_response};
use crate::gate::gate_contract;
use crate::request_envelope;
use crate::session::Session;

/// Locate `windhawk-core.dll`: the `WINDHAWK_DEBUG_CORE_DLL_PATH` override
/// (development/tests, debug build only) first, then the install layout (next to
/// the consumer's own exe - the production placement), then the bare name
/// resolved by the OS loader.
pub fn resolve_dll_path() -> String {
    if cfg!(debug_assertions)
        && let Ok(path) = std::env::var("WINDHAWK_DEBUG_CORE_DLL_PATH")
        && !path.is_empty()
    {
        return path;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("windhawk-core.dll");
        if candidate.exists() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    "windhawk-core.dll".to_owned()
}

/// A loaded `windhawk-core.dll` with the ABI integer (in `core-client`) and the
/// `contractVersion` (here) both gated. Owns the session-free path
/// (`invoke_stateless`) and the session constructor.
pub struct GatedCore {
    lib: CoreLibrary,
}

impl GatedCore {
    /// Load the DLL, hard-gate the ABI integer (in `core-client`), then enforce
    /// the contract version. A contract mismatch is fatal for the native
    /// consumers (DLL-only, no fallback).
    pub fn load(dll_path: &str) -> Result<GatedCore, HostError> {
        let lib = CoreLibrary::load(dll_path)?;
        gate_contract(&lib)?;
        Ok(GatedCore { lib })
    }

    /// Stateless synchronous invoke (`WhCoreInvokeStateless`) over a typed
    /// request DTO `P`: serves the pure session-free helpers (`parseModSource`)
    /// with no app root. Returns the success `result` value.
    pub fn invoke_stateless<P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<Value, HostError> {
        let request = request_envelope(command, params)?;
        let raw = self.lib.invoke_stateless(&request)?;
        parse_response(&raw)
    }

    /// Stateless synchronous invoke that decodes its success `result` into a
    /// typed wire DTO `T`: [`GatedCore::invoke_stateless`] plus the
    /// `serde_json::from_value` the call site would otherwise hand-roll, mapping a
    /// decode failure through the same `Decode` path.
    pub fn invoke_stateless_as<T: DeserializeOwned, P: Serialize>(
        &self,
        command: &str,
        params: &P,
    ) -> Result<T, HostError> {
        Ok(serde_json::from_value(
            self.invoke_stateless(command, params)?,
        )?)
    }

    /// Create a live session from a resolved [`SessionConfig`], wiring the
    /// consumer's own log/event callbacks straight to `core-client`. The host
    /// imposes no event transport: the callbacks fire on core-owned threads
    /// (the FFI re-entrancy rule), so the consumer owns the delivery strategy
    /// (the CLI's `mpsc` drain, the UI's channel-to-`emit` pump).
    pub fn create_session(
        &self,
        config: &SessionConfig,
        callbacks: SessionCallbacks,
    ) -> Result<Session, HostError> {
        let session = self
            .lib
            .create_session(&config.to_json().to_string(), callbacks)?;
        Ok(Session::new(session))
    }
}
