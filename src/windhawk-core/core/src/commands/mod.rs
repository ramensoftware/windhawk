//! Command handlers, one module per functional area. `parseModSource` is
//! dispatch-direct into domain (no service state); the service modules proper
//! live under `crate::services`.

pub mod diag;
pub mod parse_mod_source;
