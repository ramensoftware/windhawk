#pragma once

// The user whose mod configurations the current process takes: the process's
// own user, except in a process of a signed-in session which runs as a system
// account, such as dwm.exe, fontdrvhost.exe, winlogon.exe, logonui.exe or
// consent.exe. Those take the configurations of whoever is signed in to their
// session, as the session manager records them, and the global ones until
// somebody is. Once a user was recorded, the process keeps theirs while nobody
// is: the session manager forgets a user who signs out, and every user as it
// stops, both while the session's processes still run.
namespace ModConfigUser {

// Whether the process takes its session's user's configurations rather than
// its own user's.
bool FollowsSessionUser();

// The SID string of the user whose configurations the process takes, or
// nothing for the global configurations alone.
std::optional<std::wstring> GetSid();

// Signaled when the user of the session changes, for a process which follows
// its session's user. Re-armed with ContinueMonitoring once it fired, from any
// thread.
class ChangeNotification {
   public:
    // Throws when the session users can't be watched.
    ChangeNotification();

    ChangeNotification(const ChangeNotification&) = delete;
    ChangeNotification& operator=(const ChangeNotification&) = delete;

    HANDLE GetHandle() const;
    void ContinueMonitoring();

   private:
    wil::unique_hkey m_key;
    wil::unique_event_nothrow m_event;
};

}  // namespace ModConfigUser
