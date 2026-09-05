//! The command inventory. One entry per contract command; the dispatch table
//! must stay a subset of this list until every command is ported.
//!
//! This is frozen at contractVersion 0.1.0. Two commands have been added since:
//! the "Editor support" group's `getCompileFlags`, and `getInstalledModDetails`.
//! Both are purely additive - no existing shape changes - so the contractVersion
//! stays 0.1.0 for the migration window: the front-end's session-create gate
//! compares the version by exact equality and is pinned at 0.1.0, so a bump
//! would make the shipped client reject the DLL outright and fall back fully
//! in-process (a regression for the already-ported commands). A formal version
//! bump lands when the front-end adopts them.
//!
//! An additive command is not free of ordering: a client calling one against an
//! older DLL gets an unknown-command error at the call, not at session create.
//! Both callers of `getInstalledModDetails` ship the DLL alongside the client,
//! so the pair moves together.
//!
//! ONE change in the window was not additive: `listInstalledMods` entries no
//! longer carry `updateAvailable` (the entry carries the terms and every
//! consumer reaches the answer -
//! [`InstalledModListEntry::is_update_available`](crate::InstalledModListEntry::is_update_available)).
//! It stayed at 0.1.0 for the reason above, and it is safe for the same reason
//! the additive commands are: the only client of this contract ships the DLL
//! beside itself, and it dropped the field per entry rather than reading it, so
//! nothing it does changed. Do not read this as licence for the next removal -
//! a shape change that a client would NOTICE needs the version bump, and so
//! needs the front-end off the pinned gate first.

pub const COMMAND_INVENTORY: &[&str] = &[
    // Meta.
    "getCoreInfo",
    // Pure helpers.
    "parseModSource",
    "appendToModIdAndName",
    // Installed-mod queries and scoped writes.
    "listInstalledMods",
    "getInstalledModDetails",
    "getModSource",
    "doesModExist",
    "getModConfig",
    "updateModConfig",
    "getModSettings",
    "setModSettings",
    "setModLoggingEnabled",
    "setModRating",
    // Use-case operations.
    "installMod",
    "compileInstalledMod",
    "setModEnabled",
    "removeMod",
    "applyAppSettings",
    "previewAppSettingsEffects",
    "syncCatalogToProfile",
    // App settings.
    "getAppSettings",
    // Repository (network).
    "fetchCatalog",
    "fetchRepoModSource",
    "fetchModVersions",
    // User profile auxiliary.
    "getAppUpdateStatus",
    "getProfileWatchInfo",
    // Tray.
    "notifyTray",
    // Update.
    "startUpdate",
    "startInstallDevTools",
    // Editor support (additive).
    "getCompileFlags",
    // User-data export/import (additive). Reference-only inspect and export land
    // in phase 1; the async importUserData follows in phase 2. Additive like
    // getCompileFlags, so contractVersion stays 0.1.0.
    "exportUserData",
    "inspectUserData",
    "importUserData",
];
