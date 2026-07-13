//! The service layer: one module per functional area, each owning its commands.
//! It provides storage resolution + `getCoreInfo`, the app-settings commands,
//! and the mod config/settings commands; the mod-source file I/O and
//! `listInstalledMods` (in `mods`) and the user-profile commands (`profile`);
//! the repository client (`repo`) and the update download (`update`) over the
//! `Http` port; the compiler (`compiler`), the install/compile orchestration
//! (`install`, serving `compileInstalledMod` and `installMod`), and the tray
//! (`tray`).

pub mod app_settings;
pub mod compiler;
pub mod install;
pub mod mods;
pub mod net;
pub mod profile;
pub mod repo;
pub mod settings_io;
pub mod storage;
pub mod tray;
pub mod update;
pub mod wire;

pub use profile::ProfileState;
pub use storage::Storage;
