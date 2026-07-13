//! Application layer of windhawk-core: `Session`, the command dispatch table,
//! the operation registry, and the callback dispatcher. This is the only crate
//! that knows the command inventory.
//!
//! It provides the session/dispatch/event/log plumbing and the contract
//! commands, plus the `_diagEmitEvents` internal diagnostic command that lets
//! the ABI and bridge tests exercise the async event plumbing. `_`-prefixed
//! commands are not part of the frozen inventory and may change or disappear at
//! any time; clients must not call them.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod callbacks;
mod commands;
mod config;
mod convert;
mod dispatch;
mod error;
mod gate;
mod info;
mod locks;
mod pending;
mod runtime;
mod services;
mod session;
mod stateless;

pub use callbacks::{HostCallbacks, LogLevel};
pub use config::{DebugOverrides, SessionConfig};
pub use dispatch::{CommandKind, CommandSpec, command_specs};
pub use error::{CoreError, CoreErrorKind, error_envelope_json};
pub use info::core_info_json;
pub use session::{Deps, Session};
pub use stateless::invoke_stateless;
