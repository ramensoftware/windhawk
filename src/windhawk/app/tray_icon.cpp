#include "stdafx.h"

#include "tray_icon.h"

#include "functions.h"
#include "resource.h"

AppTrayIcon::AppTrayIcon(HWND hWnd,
                         UINT uCallbackMsg,
                         bool hidden /*= false*/) {
    ReloadIcons(hWnd);

    m_nid.cbSize = sizeof(NOTIFYICONDATA);
    m_nid.hWnd = hWnd;
    m_nid.uID = 1;
    m_nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_STATE | NIF_SHOWTIP;
    m_nid.uCallbackMessage = uCallbackMsg;
    m_nid.hIcon = m_trayIcon;
    m_nid.uVersion = NOTIFYICON_VERSION_4;
    wcscpy_s(m_nid.szTip, L"Windhawk");
    m_nid.dwState = hidden ? NIS_HIDDEN : 0;
    m_nid.dwStateMask = NIS_HIDDEN;
    m_nid.hBalloonIcon = m_balloonIcon;
}

void AppTrayIcon::Create() {
    Shell_NotifyIcon(NIM_ADD, &m_nid);
    Shell_NotifyIcon(NIM_SETVERSION, &m_nid);
}

void AppTrayIcon::Modify() {
    Shell_NotifyIcon(NIM_MODIFY, &m_nid);
}

void AppTrayIcon::UpdateIcons(HWND hWnd) {
    bool usingNotificationIcon = m_nid.hIcon == m_trayIconWithNotification;

    ReloadIcons(hWnd);

    if (usingNotificationIcon) {
        m_nid.hIcon = m_trayIconWithNotification;
    } else {
        m_nid.hIcon = m_trayIcon;
    }

    m_nid.hBalloonIcon = m_balloonIcon;
}

void AppTrayIcon::Hide(bool hidden) {
    if (hidden) {
        m_nid.dwState |= NIS_HIDDEN;
    } else {
        m_nid.dwState &= ~NIS_HIDDEN;
    }

    Shell_NotifyIcon(NIM_MODIFY, &m_nid);
}

void AppTrayIcon::SetNotificationIconAndTooltip(PCWSTR pText) {
    if (pText) {
        m_nid.hIcon = m_trayIconWithNotification;
        _snwprintf_s(m_nid.szTip, _TRUNCATE, L"%s - Windhawk", pText);
    } else {
        m_nid.hIcon = m_trayIcon;
        wcscpy_s(m_nid.szTip, L"Windhawk");
    }

    Shell_NotifyIcon(NIM_MODIFY, &m_nid);
}

void AppTrayIcon::ShowNotificationMessage(PCWSTR pText) {
    m_nid.uFlags |= NIF_INFO;
    wcsncpy_s(m_nid.szInfo, pText, _TRUNCATE);
    wcscpy_s(m_nid.szInfoTitle, L"Windhawk");
    m_nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;

    Shell_NotifyIcon(NIM_MODIFY, &m_nid);

    m_nid.uFlags &= ~NIF_INFO;
}

void AppTrayIcon::Remove() {
    Shell_NotifyIcon(NIM_DELETE, &m_nid);
}

AppTrayIcon::TrayAction AppTrayIcon::HandleMsg(WPARAM wParam, LPARAM lParam) {
    DWORD tickCount;
    WORD notificationEvent = LOWORD(lParam);
    switch (notificationEvent) {
        case NIN_SELECT:
        case NIN_KEYSELECT:
            // Prevent multiple actions for accidental double clicks.
            tickCount = GetTickCount();
            if (tickCount - m_lastClickTickCount <= 400) {
                return TrayAction::kNone;
            }

            m_lastClickTickCount = tickCount;
            return TrayAction::kDefault;

        case NIN_BALLOONUSERCLICK:
            return TrayAction::kBalloon;

        case WM_CONTEXTMENU:
            return TrayAction::kContextMenu;
    }

    return TrayAction::kNone;
}

void AppTrayIcon::ReloadIcons(HWND hWnd) {
    HWND hTaskbarWnd = FindWindow(L"Shell_TrayWnd", nullptr);
    UINT dpi = Functions::GetDpiForWindowWithFallback(hTaskbarWnd ? hTaskbarWnd
                                                                  : hWnd);

    m_trayIcon = nullptr;
    m_trayIcon.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXSMICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYSMICON, dpi));

    m_balloonIcon = nullptr;
    m_balloonIcon.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYICON, dpi));

    m_trayIconWithNotification = nullptr;
    m_trayIconWithNotification.LoadIconWithScaleDown(
        IDI_NOTIFICATION,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXSMICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYSMICON, dpi));
}
