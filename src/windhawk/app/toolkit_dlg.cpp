#include "stdafx.h"

#include "toolkit_dlg.h"

#include "functions.h"

namespace {

int AutoSizeStaticHeight(CStatic stat) {
    CRect rc;
    stat.GetWindowRect(&rc);

    CString str;
    stat.GetWindowText(str);

    CRect rcNew(rc);
    {
        CDC dc = stat.GetDC();
        CFontHandle oldFont(dc.SelectFont(stat.GetFont()));
        dc.DrawText(str, str.GetLength(), &rcNew,
                    DT_WORDBREAK | DT_EXPANDTABS | DT_NOCLIP | DT_CALCRECT);
        dc.SelectFont(oldFont);
    }

    if (rcNew.Height() == rc.Height()) {
        return 0;
    }

    stat.SetWindowPos(NULL, 0, 0, rc.Width(), rcNew.Height(),
                      SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOZORDER);
    return rcNew.Height() - rc.Height();
}

}  // namespace

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

    if (m_dialogOptions.showTaskbarCrashExplanation) {
        UINT windowDpi = Functions::GetDpiForWindowWithFallback(m_hWnd);
        const int extraWidth = MulDiv(100, windowDpi, 96);
        CRect rc;

        CStatic explanationStatic{GetDlgItem(IDC_TOOLKIT_EXPLANATION)};

        explanationStatic.GetWindowRect(&rc);
        explanationStatic.SetWindowPos(
            nullptr, 0, 0, rc.Width() + extraWidth, rc.Height(),
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);

        explanationStatic.SetWindowText(
            Functions::LoadStrFromRsrc(IDS_TOOLKITDLG_EXPLANATION_CRASH));
        AutoSizeStaticHeight(explanationStatic);
        explanationStatic.ShowWindow(SW_SHOW);
        explanationStatic.GetWindowRect(&rc);
        int offsetY = rc.Height() + MulDiv(12, windowDpi, 96);

        for (int controlId : {
                 IDOK,
                 IDC_TOOLKIT_LOADED_MODS,
                 IDC_TOOLKIT_EXIT,
                 IDC_TOOLKIT_SAFE_MODE,
                 IDC_TOOLKIT_CLOSE,
             }) {
            CWindow control = GetDlgItem(controlId);
            control.GetWindowRect(&rc);
            ::MapWindowPoints(nullptr, m_hWnd, (POINT*)&rc, 2);
            CPoint ptMove = rc.TopLeft();
            ptMove.Offset(extraWidth / 2, offsetY);
            control.SetWindowPos(nullptr, ptMove.x, ptMove.y, 0, 0,
                                 SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }

        GetWindowRect(&rc);
        rc.top -= offsetY / 2;
        rc.bottom += offsetY / 2 + offsetY % 2;
        rc.left -= extraWidth / 2;
        rc.right += extraWidth / 2 + extraWidth % 2;
        SetWindowPos(nullptr, rc, SWP_NOZORDER | SWP_NOACTIVATE);
    }

    bool languageRightToLeft =
        Functions::IsRightToLeftLanguage(GetThreadUILanguage());
    Functions::ApplyDialogLayoutRtl(*this, languageRightToLeft);
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
