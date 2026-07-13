#pragma once

// Ensures the storage file and registry permissions match what the installer
// sets, so the engine and mods stay accessible from every target process,
// including sandboxed ones. Idempotent: makes no changes when the permissions
// are already in place. Best-effort: logs and continues on per-object failure
// and never throws. Meant to run once per session from the elevated background
// process.
void EnsureStoragePermissions() noexcept;
