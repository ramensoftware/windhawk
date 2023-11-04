#include "stdafx.h"

#include "toolkit_dlg.h"

#include "functions.h"

CToolkitDlg::CToolkitDlg(DialogOptions dialogOptions)
    : m_dialogOptions(std::move(dialogOptions)) {}

void CToolkitDlg::LoadLanguageStrings() {
    SetWindowText(Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_TITLE));

    SetDlgItemText(IDOK,
                   Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_BUTTON_OPEN));
    SetDlgItemText(
        IDC_TOOLKIT_LOADED_MODS,
        Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_BUTTON_LOADED_MODS));
    SetDlgItemText(IDC_TOOLKIT_EXIT,
                   Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_BUTTON_EXIT));
    SetDlgItemText(IDC_TOOLKIT_SAFE_MODE,
                   Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_BUTTON_SAFE_MODE));
    SetDlgItemText(IDC_TOOLKIT_CLOSE,
                   Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_BUTTON_CLOSE));
}

bool CToolkitDlg::WasActive() {
    return m_wasActive;
}

void CToolkitDlg::Close() {
    DestroyWindow();
}

BOOL CToolkitDlg::OnInitDialog(CWindow wndFocus, LPARAM lInitParam) {
    ReloadMainIcon();

    SetWindowPos(HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);

    // PlaceWindowAtTrayArea();
    CenterWindow();

    LoadLanguageStrings();

    return !m_dialogOptions.createInactive;
}

void CToolkitDlg::OnDestroy() {
    // From GDI handle checks, not all icons are freed automatically.
    ::DestroyIcon(SetIcon(nullptr, TRUE));
    ::DestroyIcon(SetIcon(nullptr, FALSE));
}

void CToolkitDlg::OnActivate(UINT nState, BOOL bMinimized, CWindow wndOther) {
    switch (nState) {
        case WA_ACTIVE:
        case WA_CLICKACTIVE:
            m_wasActive = true;
            break;
    }
}

void CToolkitDlg::OnDpiChanged(UINT nDpiX, UINT nDpiY, PRECT pRect) {
    ReloadMainIcon();
}

void CToolkitDlg::OnOK(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.runButtonCallback) {
        m_dialogOptions.runButtonCallback(m_hWnd);
    }
}

void CToolkitDlg::OnLoadedMods(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.loadedModsButtonCallback) {
        m_dialogOptions.loadedModsButtonCallback(m_hWnd);
    }
}

void CToolkitDlg::OnExit(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.exitButtonCallback) {
        m_dialogOptions.exitButtonCallback(m_hWnd);
    }
}

void CToolkitDlg::OnSafeMode(UINT uNotifyCode, int nID, CWindow wndCtl) {
    if (m_dialogOptions.safeModeButtonCallback) {
        m_dialogOptions.safeModeButtonCallback(m_hWnd);
    }
}

void CToolkitDlg::OnClose(UINT uNotifyCode, int nID, CWindow wndCtl) {
    Close();
}

void CToolkitDlg::OnFinalMessage(HWND hWnd) {
    if (m_dialogOptions.finalMessageCallback) {
        m_dialogOptions.finalMessageCallback(m_hWnd);
    }
}

void CToolkitDlg::ReloadMainIcon() {
    UINT dpi = Functions::GetDpiForWindowWithFallback(m_hWnd);

    CIconHandle mainIcon;
    mainIcon.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYICON, dpi));
    CIcon prevMainIcon = SetIcon(mainIcon, TRUE);

    CIconHandle mainIconSmall;
    mainIconSmall.LoadIconWithScaleDown(
        IDR_MAINFRAME,
        Functions::GetSystemMetricsForDpiWithFallback(SM_CXSMICON, dpi),
        Functions::GetSystemMetricsForDpiWithFallback(SM_CYSMICON, dpi));
    CIcon prevMainIconSmall = SetIcon(mainIconSmall, FALSE);
}

void CToolkitDlg::PlaceWindowAtTrayArea() {
    CRect windowRect;
    GetWindowRect(&windowRect);

    CRect workAreaRect;
    ::SystemParametersInfo(SPI_GETWORKAREA, 0, &workAreaRect, 0);

    int margin = 8;

    windowRect.MoveToXY(workAreaRect.right - windowRect.Width() - margin,
                        workAreaRect.bottom - windowRect.Height() - margin);

    SetWindowPos(nullptr, windowRect,
                 SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}
