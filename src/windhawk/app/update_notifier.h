#pragma once

#include "tray_icon.h"
#include "userprofile.h"

// Owns everything involved in telling the user that updates are available: the
// last known availability, the tray icon and tooltip which mirror it, and the
// notification balloon which announces it.
//
// The tray indication is ambient and is refreshed on every status change. The
// balloon is an event, and can't always be shown at the moment availability
// changes: the shell may be suppressing notifications, or the user may be away
// from the machine and would miss it. It is then queued, and a poll timer
// retries until showing it is worthwhile:
//
//   state    event                                       next state
//   ------------------------------------------------------------------------
//   idle     status worth announcing, can show now       balloon, idle
//   idle     status worth announcing, can't show now     queued
//   queued   poll, still can't show                      queued
//   queued   poll, can show now                          balloon, idle
//   queued   status with nothing available anymore       idle
//   queued   any other status                            queued
//
// A queued balloon carries no payload: its text is rendered from the
// availability at the moment it is shown, so waiting can't make it stale. It is
// dropped only once its subject is gone, since a status which merely re-reports
// the same availability must not cancel it.
class UpdateNotifier {
   public:
    // Which change in availability, if any, is worth a balloon.
    enum class Announce {
        // Only what the status itself reports as newly found, which an online
        // check derives from the versions it previously recorded.
        kNewlyFound,
        // Any growth over the last known availability, for a source which
        // reports what is currently available without saying what is new, such
        // as a re-read of the local profile. With no last known availability to
        // compare against, the status is adopted silently.
        kIncrease,
        // Everything available, however long it has been available.
        kAll,
    };

    // The poll timer is set on hWnd under pollTimerId, which the owner routes
    // back through OnPollTimer.
    UpdateNotifier(HWND hWnd, UINT_PTR pollTimerId, AppTrayIcon& trayIcon);
    ~UpdateNotifier();

    UpdateNotifier(const UpdateNotifier&) = delete;
    UpdateNotifier& operator=(const UpdateNotifier&) = delete;

    // Records the availability and announces it per the policy.
    void SetStatus(const UserProfile::UpdateStatus& status, Announce announce);

    // Records that nothing is available, dropping any queued balloon.
    void Clear();

    void OnPollTimer();

    // Gives up on a queued balloon regardless of availability.
    void CancelPending();

    bool AppUpdateAvailable() const;

   private:
    bool ShouldAnnounce(const UserProfile::UpdateStatus& status,
                        Announce announce) const;
    bool AnyAvailable() const;
    static bool CanShowNotification();
    void RefreshTrayIndication();
    void ShowNotification();
    void QueueNotification();

    HWND m_hWnd;
    UINT_PTR m_pollTimerId;
    AppTrayIcon& m_trayIcon;
    std::optional<UserProfile::UpdateStatus> m_status;
    // Set iff the poll timer is running.
    bool m_pendingNotification = false;
};
