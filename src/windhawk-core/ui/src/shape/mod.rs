//! Pure reply shapers: a leaf both `commands/` and `pump/` depend on, split by
//! domain. Each function maps already- fetched core values into a front-end
//! reply `data` object with no session and no DLL, so they are unit-tested
//! directly.

pub mod app_ui;
pub mod catalog;
pub mod installed;
pub mod source;
