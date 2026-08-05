#pragma once

// Maintains this session's <session-id> subtree of the store container, which
// injected engines write and the app reads. Meant to be used from the
// session-manager (daemon) process. See shared/session_metadata.h.
//
// One instance per session, outliving everything that writes to the store: the
// keys have to exist before the first injection and may only be removed once
// the last one has stopped.
class SessionMetadataStore {
   public:
    // Creates the session's volatile keys, with permissions that let injected
    // engines of any integrity level write their entries, and prunes keys left
    // by dead sessions. Best-effort: logs and continues on failure, never
    // throws.
    SessionMetadataStore() noexcept;

    // Deletes this session's keys. Best-effort; whatever is left vanishes with
    // the hive holding it, at reboot or at logoff.
    ~SessionMetadataStore();

    SessionMetadataStore(const SessionMetadataStore&) = delete;
    SessionMetadataStore& operator=(const SessionMetadataStore&) = delete;

    // Deletes this session's entries whose owning process has exited; volatile
    // values aren't removed on process exit.
    void SweepDeadEntries() noexcept;

   private:
    void EnsureKeys() noexcept;
    void DeleteKeys() noexcept;

    // The container this session's keys live in and every access goes through:
    // the key under HKEY_LOCAL_MACHINE, or the application hive's root. Empty
    // when the store couldn't be set up. Held for the length of the session,
    // which in the hive case is also what keeps the hive loaded.
    wil::unique_hkey m_container;

    // Whether m_container is the application hive's root, in which case the
    // file behind it is this session's to remove once the handle is gone.
    bool m_containerIsHive = false;
};
