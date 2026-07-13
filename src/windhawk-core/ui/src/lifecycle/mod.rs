//! Process lifetime + native app integration: the core session bring-up
//! (`session`), the startup mod-runtime seed (`mods_runtime`), and the launcher
//! contract / window helpers (`window`). `lib.rs` wires them together - it
//! brings up the session in `setup` (so only the single-instance primary
//! creates one), presents a startup failure via `window::show_fatal`, and
//! drives the bare-launch single-instance handshake.

pub mod mods_runtime;
pub mod session;
pub mod window;
pub mod window_state;

pub use session::{CoreHandles, start_core};
