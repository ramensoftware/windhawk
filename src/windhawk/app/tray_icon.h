#pragma once

class AppTrayIcon {
   public:
    enum class TrayAction {
        kNone,
        kDefault,
        kBalloon,
        kContextMenu,
    };

    enum class NotificationIcon {
        kNone,
        kAppUpdate,
        kModUpdate,
    };

    static inline constexpr size_t kMaxNotificationTooltipSize =
        ARRAYSIZE(NOTIFYICONDATA::szTip);
    static inline constexpr size_t kMaxNotificationMessageSize =
        ARRAYSIZE(NOTIFYICONDATA::szInfo);

    AppTrayIcon(HWND hWnd, UINT uCallbackMsg, bool hidden = false);

    void Create();
    void Modify();
    void UpdateIcons(HWND hWnd);
    void Hide(bool hidden);
    void SetNotificationIconAndTooltip(NotificationIcon icon, PCWSTR pText);
    void ShowNotificationMessage(PCWSTR pText);
    void Remove();
    TrayAction HandleMsg(WPARAM wParam, LPARAM lParam);

   private:
    void ReloadIcons(HWND hWnd);
    HICON CurrentIcon();

    CIcon m_trayIcon;
    CIcon m_balloonIcon;
    CIcon m_trayIconWithNotification;
    CIcon m_trayIconWithModNotification;
    NotificationIcon m_notificationIcon = NotificationIcon::kNone;
    NOTIFYICONDATA m_nid{};
    DWORD m_lastClickTickCount = 0;
};
