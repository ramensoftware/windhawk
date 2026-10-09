//! The service layer: one module per functional area, each owning its commands.
//! It provides storage resolution + `getCoreInfo`, the app-settings commands,
//! and the mod config/settings commands; the mod-source file I/O and
//! `listInstalledMods` (in `mods`) and the user-profile commands (`profile`);
//! the repository client (`repo`) and the update download (`update`) over the
//! `Http` port; the compiler (`compiler`), the install/compile orchestration
//! (`install`, serving `compileInstalledMod` and `installMod`), the tray
//! (`tray`), the installed font families (`fonts`) over the `Fonts` port, the
//! one-chord keyboard capture (`hotkey`) over the `HotkeyCapture` port, the
//! review votes kept in the profile (`reviews`), the commands over a user's
//! own configuration of a mod (`user_scope`), and the mod catalog's `Nuked`
//! value (`nuke`).

pub mod app_settings;
pub mod compiler;
pub mod fonts;
pub mod hotkey;
pub mod install;
pub mod mods;
pub mod net;
pub mod nuke;
pub mod profile;
pub mod repo;
pub mod reviews;
pub mod settings_io;
pub mod storage;
pub mod tray;
pub mod update;
pub mod user_data;
pub mod user_scope;
pub mod wire;

pub use hotkey::HotkeyCaptureSlot;
pub use profile::ProfileState;
pub use repo::CatalogCache;
pub use storage::Storage;
