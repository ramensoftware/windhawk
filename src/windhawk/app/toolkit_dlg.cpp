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
    m_icon = AtlLoadIconImage(IDR_MAINFRAME, LR_DEFAULTCOLOR,
                              ::GetSystemMetrics(SM_CXICON),
                              ::GetSystemMetrics(SM_CYICON));
    SetIcon(m_icon, TRUE);
    m_smallIcon = AtlLoadIconImage(IDR_MAINFRAME, LR_DEFAULTCOLOR,
                                   ::GetSystemMetrics(SM_CXSMICON),
                                   ::GetSystemMetrics(SM_CYSMICON));
    SetIcon(m_smallIcon, FALSE);

    SetWindowPos(HWND_TOPMOST, 0, 0, 0, 0,
                 SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);

    //PlaceWindowAtTrayArea();
    CenterWindow();

    LoadLanguageStrings();

    return !m_dialogOptions.createInactive;
}

void CToolkitDlg::OnActivate(UINT nState, BOOL bMinimized, CWindow wndOther) {
    switch (nState) {
        case WA_ACTIVE:
        case WA_CLICKACTIVE:
            m_wasActive = true;
            break;
    }
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
