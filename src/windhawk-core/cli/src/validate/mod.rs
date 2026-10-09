//! Pure, I/O-free input validation: the int32 range check, the mod-settings
//! flat-key flatten + type-check, the whole-map check of a mod-settings
//! replace, and the AppSettings dotted-key schema + patch builder. Each is a
//! free function over plain values - no session, no storage - so the
//! interesting edge cases are unit-tested in isolation, away from dispatch and
//! the C ABI.

pub mod app_settings;
pub mod int_range;
pub mod mod_settings;
pub mod mod_settings_map;
