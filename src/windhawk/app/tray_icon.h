#pragma once

class AppTrayIcon {
   public:
    enum class TrayAction {
        kNone,
        kDefault,
        kBalloon,
        kContextMenu,
    };

    static inline constexpr size_t kMaxNotificationTooltipSize =
        ARRAYSIZE(NOTIFYICONDATA::szInfo);

    AppTrayIcon(HWND hWnd, UINT uCallbackMsg, bool hidden = false);

    void Create();
    void Modify();
    void UpdateIcons(HWND hWnd);
    void Hide(bool hidden);
    void SetNotificationIconAndTooltip(PCWSTR pText);
    void ShowNotificationMessage(PCWSTR pText);
    void Remove();
    TrayAction HandleMsg(WPARAM wParam, LPARAM lParam);

   private:
    void ReloadIcons(HWND hWnd);

    CIcon m_trayIcon;
    CIcon m_balloonIcon;
    CIcon m_trayIconWithNotification;
    NOTIFYICONDATA m_nid{};
    DWORD m_lastClickTickCount = 0;
};
