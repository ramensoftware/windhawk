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
    // engines of any integrity level write their entries, records the users of
    // the Windows sessions already signed in to, and prunes keys left by dead
    // sessions. Best-effort: logs and continues on failure, never throws.
    SessionMetadataStore() noexcept;

    // Deletes this session's keys. Best-effort; whatever is left vanishes with
    // the hive holding it, at reboot or at logoff.
    ~SessionMetadataStore();

    SessionMetadataStore(const SessionMetadataStore&) = delete;
    SessionMetadataStore& operator=(const SessionMetadataStore&) = delete;

    // Deletes this session's entries whose owning process has exited; volatile
    // values aren't removed on process exit.
    void SweepDeadEntries() noexcept;

    // Forgets the user of a Windows session they signed out of, and records
    // nobody for the session from then on, until it ends, nobody is signed in
    // to it, or it's logged on to again: a session can still name its user
    // while it signs out. Only a non-portable store keeps session users;
    // elsewhere this does nothing. Best-effort.
    void RemoveSessionUser(DWORD windowsSessionId) noexcept;

    // Records the users of every Windows session as it is now: the user of each
    // session signed in to, and nobody for a session which ended, which nobody
    // is signed in to, or which was signed out of (RemoveSessionUser). A
    // session whose user can't be read keeps what it had. Best-effort, like the
    // above.
    void RecordSessionUsers() noexcept;

    // Records the users like RecordSessionUsers, after a logon to a Windows
    // session, recording its new user even where an earlier session with the
    // same id was signed out of and not yet seen to end. Best-effort, like the
    // above.
    void RecordSessionLogon(DWORD windowsSessionId) noexcept;

   private:
    void EnsureKeys() noexcept;
    void EnsureSessionUsersKey(HKEY sessionKey, PCWSTR sddl) noexcept;
    void DeleteKeys() noexcept;

    // Records the user of a Windows session signed in to. A write wakes every
    // process that follows a session user, in every session, so a user who is
    // already recorded isn't written again.
    void SetSessionUser(DWORD windowsSessionId) noexcept;

    // Deletes the recorded user of a Windows session, if there is one.
    void DeleteSessionUser(DWORD windowsSessionId) noexcept;

    // The container this session's keys live in and every access goes through:
    // the key under HKEY_LOCAL_MACHINE, or the application hive's root. Empty
    // when the store couldn't be set up. Held for the length of the session,
    // which in the hive case is also what keeps the hive loaded.
    wil::unique_hkey m_container;

    // Whether m_container is the application hive's root, in which case the
    // file behind it is this session's to remove once the handle is gone.
    bool m_containerIsHive = false;

    // The session-users key. Empty when the store doesn't keep session users.
    wil::unique_hkey m_sessionUsersKey;

    // The Windows sessions signed out of which are left without a recorded
    // user while they end (RemoveSessionUser).
    std::unordered_set<DWORD> m_signedOutSessionIds;
};
