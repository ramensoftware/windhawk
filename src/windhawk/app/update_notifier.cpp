#include "stdafx.h"

#include "update_notifier.h"

#include "functions.h"
#include "resource.h"

namespace {

constexpr auto kNotificationPollInterval = 1000 * 2;  // 2sec
constexpr auto kUserInputIdleThreshold = 1000 * 60;   // 60sec

// Renders the availability as user-facing text, shared by the tooltip and the
// balloon so they can't disagree. Empty when nothing is available.
void GetNotificationText(PWSTR text,
                         size_t textSize,
                         bool appUpdateAvailable,
                         int modUpdatesAvailable) {
    text[0] = L'\0';

    if (appUpdateAvailable) {
        if (modUpdatesAvailable == 0) {
            wcsncpy_s(text, textSize,
                      Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP),
                      _TRUNCATE);
        } else if (modUpdatesAvailable == 1) {
            wcsncpy_s(
                text, textSize,
                Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP_MOD),
                _TRUNCATE);
        } else {
            _snwprintf_s(
                text, textSize, _TRUNCATE,
                Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP_MODS),
                modUpdatesAvailable);
        }
    } else {
        if (modUpdatesAvailable == 1) {
            wcsncpy_s(text, textSize,
                      Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_MOD),
                      _TRUNCATE);
        } else if (modUpdatesAvailable > 1) {
            _snwprintf_s(
                text, textSize, _TRUNCATE,
                Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_MODS),
                modUpdatesAvailable);
        }
    }
}

}  // namespace

UpdateNotifier::UpdateNotifier(HWND hWnd,
                               UINT_PTR pollTimerId,
                               AppTrayIcon& trayIcon)
    : m_hWnd(hWnd), m_pollTimerId(pollTimerId), m_trayIcon(trayIcon) {}

UpdateNotifier::~UpdateNotifier() {
    CancelPending();
}

void UpdateNotifier::SetStatus(const UserProfile::UpdateStatus& status,
                               Announce announce) {
    bool announceStatus = ShouldAnnounce(status, announce);

    m_status = status;

    RefreshTrayIndication();

    if (!AnyAvailable()) {
        // There's nothing to announce, and a queued balloon just lost its
        // subject.
        CancelPending();
        return;
    }

    if (!announceStatus) {
        // A pass which re-reports availability the user was already told about,
        // such as the one a read-triggered profile write raises, leaves a
        // queued balloon queued.
        return;
    }

    if (!CanShowNotification()) {
        QueueNotification();
        return;
    }

    CancelPending();
    ShowNotification();
}

void UpdateNotifier::Clear() {
    SetStatus(UserProfile::UpdateStatus{}, Announce::kAll);
}

void UpdateNotifier::OnPollTimer() {
    if (!m_pendingNotification || !CanShowNotification()) {
        return;
    }

    CancelPending();
    ShowNotification();
}

void UpdateNotifier::CancelPending() {
    if (!m_pendingNotification) {
        return;
    }

    KillTimer(m_hWnd, m_pollTimerId);
    m_pendingNotification = false;
}

bool UpdateNotifier::AppUpdateAvailable() const {
    return m_status && m_status->appUpdateAvailable;
}

bool UpdateNotifier::ShouldAnnounce(const UserProfile::UpdateStatus& status,
                                    Announce announce) const {
    switch (announce) {
        case Announce::kNewlyFound:
            return status.newUpdatesFound;

        case Announce::kIncrease:
            return m_status &&
                   ((status.appUpdateAvailable &&
                     !m_status->appUpdateAvailable) ||
                    status.modUpdatesAvailable > m_status->modUpdatesAvailable);

        case Announce::kAll:
            return true;
    }

    return false;
}

bool UpdateNotifier::AnyAvailable() const {
    return m_status &&
           (m_status->appUpdateAvailable || m_status->modUpdatesAvailable > 0);
}

// Whether a balloon shown right now would reach the user: the shell must be
// accepting notifications, and the machine must have been used recently enough
// for the user to be in front of it. A shell which can't be asked isn't taken
// to be objecting.
bool UpdateNotifier::CanShowNotification() {
    QUERY_USER_NOTIFICATION_STATE state{};
    if (SUCCEEDED(SHQueryUserNotificationState(&state)) &&
        state != QUNS_ACCEPTS_NOTIFICATIONS) {
        return false;
    }

    LASTINPUTINFO lii{sizeof(lii)};
    if (GetLastInputInfo(&lii) &&
        GetTickCount() - lii.dwTime >= kUserInputIdleThreshold) {
        return false;
    }

    return true;
}

void UpdateNotifier::RefreshTrayIndication() {
    AppTrayIcon::NotificationIcon icon;
    if (m_status->appUpdateAvailable) {
        icon = AppTrayIcon::NotificationIcon::kAppUpdate;
    } else if (m_status->modUpdatesAvailable > 0) {
        icon = AppTrayIcon::NotificationIcon::kModUpdate;
    } else {
        m_trayIcon.SetNotificationIconAndTooltip(
            AppTrayIcon::NotificationIcon::kNone, nullptr);
        return;
    }

    WCHAR tooltip[AppTrayIcon::kMaxNotificationTooltipSize] = L"";
    GetNotificationText(tooltip, ARRAYSIZE(tooltip),
                        m_status->appUpdateAvailable,
                        m_status->modUpdatesAvailable);

    m_trayIcon.SetNotificationIconAndTooltip(icon, tooltip);
}

void UpdateNotifier::ShowNotification() {
    WCHAR message[AppTrayIcon::kMaxNotificationMessageSize] = L"";
    GetNotificationText(message, ARRAYSIZE(message),
                        m_status->appUpdateAvailable,
                        m_status->modUpdatesAvailable);

    m_trayIcon.ShowNotificationMessage(message);
}

void UpdateNotifier::QueueNotification() {
    if (m_pendingNotification) {
        // Already waiting; keep the running poll's cadence.
        return;
    }

    SetTimer(m_hWnd, m_pollTimerId, kNotificationPollInterval, nullptr);
    m_pendingNotification = true;
}
