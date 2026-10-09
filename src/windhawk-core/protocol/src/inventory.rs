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
    // User-data export/import (additive): reference-only inspect and export, and
    // the async importUserData. Additive like getCompileFlags, so contractVersion
    // stays 0.1.0.
    "exportUserData",
    "inspectUserData",
    "importUserData",
    // Mod settings data types (additive). One read of the options a mod writes
    // at runtime for its `$dynamicSelect` settings. Additive like
    // getCompileFlags, so contractVersion stays 0.1.0.
    "getModDynamicSelectOptions",
    // Host query (additive): the installed font families for a fontFamily
    // setting's completion. Additive like getCompileFlags, so contractVersion
    // stays 0.1.0.
    "listFontFamilies",
    // Host capture (additive): one keyboard chord recorded behind a hotkey
    // setting's badge, async with the held modifiers as progress. Additive like
    // getCompileFlags, so contractVersion stays 0.1.0.
    "captureHotkey",
    // Review votes (additive): the profile write behind a review's upvote, the
    // one that takes it back, and the read of what a mod has been voted on.
    // Additive like getCompileFlags, so contractVersion stays 0.1.0.
    "voteModReview",
    "retractModReviewVote",
    "getModReviewVotes",
    // User-scoped mod configuration (additive): switch the session user's own
    // configuration of a mod on or off, creating it from the global one the
    // first time, and delete it. Additive like getCompileFlags, so
    // contractVersion stays 0.1.0.
    "setModUserScope",
    "deleteModUserConfig",
    // Mod settings defaults (additive): the settings a fresh install of a mod's
    // stored source writes, for a settings editor's reset. Additive like
    // getCompileFlags, so contractVersion stays 0.1.0.
    "getModDefaultSettings",
];
