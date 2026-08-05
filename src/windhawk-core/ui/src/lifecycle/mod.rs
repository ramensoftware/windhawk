//! Process lifetime + native app integration: the core session bring-up
//! (`session`), the startup mod-runtime seed (`mods_runtime`), where the main
//! window's data goes (`ui_data`), the launcher contract / window helpers
//! (`window`), what keeps a wedged Explorer out of the window build
//! (`taskbar_list`), and what a fatal window failure can be told about itself
//! (`diagnostics`). `lib.rs` wires them together - it brings up the session in
//! `setup` (so only the single-instance primary creates one), presents a startup
//! failure via `window::show_fatal` with the detail `diagnostics` collected, and
//! drives the bare-launch single-instance handshake.
//!
//! `mods_runtime` is the one the BROKER process also runs, so it reaches for
//! neither `window` nor Tauri.

pub mod diagnostics;
pub mod mods_runtime;
pub mod session;
pub mod taskbar_list;
pub mod ui_data;
pub mod window;
pub mod window_state;

pub use session::{CoreHandles, discover_app_root, start_core};
