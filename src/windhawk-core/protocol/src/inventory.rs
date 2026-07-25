//! The command inventory. One entry per contract command; the dispatch table
//! must stay a subset of this list until every command is ported.
//!
//! This is frozen at contractVersion 0.1.0. The "Editor support" group's one
//! additive command, `getCompileFlags`, is purely additive - no existing shape
//! changes - so the contractVersion stays 0.1.0 for the migration window: the
//! front-end's session-create gate compares the version by exact equality and
//! is pinned at 0.1.0, so a bump would make the shipped client reject the DLL
//! outright and fall back fully in-process (a regression for the already-ported
//! commands). A formal version bump lands when the front-end adopts
//! `getCompileFlags`.

pub const COMMAND_INVENTORY: &[&str] = &[
    // Meta.
    "getCoreInfo",
    // Pure helpers.
    "parseModSource",
    "appendToModIdAndName",
    // Installed-mod queries and scoped writes.
    "listInstalledMods",
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
