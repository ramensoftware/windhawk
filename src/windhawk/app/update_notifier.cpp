#include "stdafx.h"

#include "update_notifier.h"

#include "resource.h"
#include "ui_functions.h"

namespace {

constexpr auto kNotificationPollInterval = 1000 * 2;  // 2sec
constexpr auto kUserInputIdleThreshold = 1000 * 60;   // 60sec

// Substitutes the count into the first "%d" of a localized string, copying the
// rest verbatim. A translator-supplied string is never handed to a printf-style
// formatter, where a mistyped or extra conversion would read arguments which
// were never passed.
std::wstring FormatCount(PCWSTR localizedText, int count) {
    std::wstring text = localizedText;

    size_t placeholder = text.find(L"%d");
    if (placeholder != std::wstring::npos) {
        text.replace(placeholder, 2, std::to_wstring(count));
    }

    return text;
}

// Renders the availability as user-facing text, shared by the tooltip and the
// balloon so they can't disagree. Empty when nothing is available.
std::wstring GetNotificationText(bool appUpdateAvailable,
                                 int modUpdatesAvailable) {
    if (appUpdateAvailable) {
        if (modUpdatesAvailable == 0) {
            return Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP);
        } else if (modUpdatesAvailable == 1) {
            return Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP_MOD);
        } else {
            return FormatCount(
                Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_APP_MODS),
                modUpdatesAvailable);
        }
    } else {
        if (modUpdatesAvailable == 1) {
            return Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_MOD);
        } else if (modUpdatesAvailable > 1) {
            return FormatCount(
                Functions::LoadStrFromRsrc(IDS_NOTIFICATION_UPDATE_MODS),
                modUpdatesAvailable);
        }
    }

    return {};
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

    std::wstring tooltip = GetNotificationText(m_status->appUpdateAvailable,
                                               m_status->modUpdatesAvailable);

    m_trayIcon.SetNotificationIconAndTooltip(icon, tooltip.c_str());
}

void UpdateNotifier::ShowNotification() {
    std::wstring message = GetNotificationText(m_status->appUpdateAvailable,
                                               m_status->modUpdatesAvailable);

    m_trayIcon.ShowNotificationMessage(message.c_str());
}

void UpdateNotifier::QueueNotification() {
    if (m_pendingNotification) {
        // Already waiting; keep the running poll's cadence.
        return;
    }

    SetTimer(m_hWnd, m_pollTimerId, kNotificationPollInterval, nullptr);
    m_pendingNotification = true;
}
