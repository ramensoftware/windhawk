#pragma once

// Maintains this session's volatile registry store of transient mod status and
// task metadata (under HKLM\SOFTWARE\WindhawkSessions\<session-id>), which
// injected engines write and the app reads. Meant to be used from the elevated
// session-manager (daemon) process. See shared/session_metadata.h for the path
// and value format.

// Creates the session's volatile keys with permissions that let injected
// engines of any integrity level write their entries, and prunes keys left by
// dead sessions. Best-effort: logs and continues on failure, never throws. Run
// once per session, before any injection.
void EnsureSessionKeys() noexcept;

// Deletes this session's entries whose owning process has exited. Volatile
// registry values aren't removed on process exit the way the delete-on-close
// temp files were, so the session manager sweeps them.
void SweepDeadSessionMetadata() noexcept;

// Deletes this session's keys (the <session-id> subtree under WindhawkSessions).
// Best-effort. Run once from the session manager when the session ends; the
// volatile keys also vanish on reboot if this doesn't run.
void DeleteSessionKeys() noexcept;
