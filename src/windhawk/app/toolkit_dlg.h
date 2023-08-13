#pragma once

#include "resource.h"

class CToolkitDlg : public CDialogImpl<CToolkitDlg> {
   public:
    enum { IDD = IDD_TOOLKIT };

    using DlgCallback = std::function<void(HWND)>;

    struct DialogOptions {
        bool createInactive = false;
        DlgCallback runButtonCallback;
        DlgCallback loadedModsButtonCallback;
        DlgCallback exitButtonCallback;
        DlgCallback safeModeButtonCallback;
        DlgCallback finalMessageCallback;
    };

    CToolkitDlg(DialogOptions dialogOptions);

    void LoadLanguageStrings();

    bool WasActive();
    void Close();

   private:
    BEGIN_MSG_MAP_EX(CToolkitDlg)
        MSG_WM_INITDIALOG(OnInitDialog)
        MSG_WM_ACTIVATE(OnActivate)
        COMMAND_ID_HANDLER_EX(IDOK, OnOK)
        COMMAND_ID_HANDLER_EX(IDC_TOOLKIT_LOADED_MODS, OnLoadedMods)
        COMMAND_ID_HANDLER_EX(IDC_TOOLKIT_EXIT, OnExit)
        COMMAND_ID_HANDLER_EX(IDC_TOOLKIT_SAFE_MODE, OnSafeMode)
        COMMAND_ID_HANDLER_EX(IDC_TOOLKIT_CLOSE, OnClose)
    END_MSG_MAP()

    BOOL OnInitDialog(CWindow wndFocus, LPARAM lInitParam);
    void OnActivate(UINT nState, BOOL bMinimized, CWindow wndOther);

    void OnOK(UINT uNotifyCode, int nID, CWindow wndCtl);
    void OnLoadedMods(UINT uNotifyCode, int nID, CWindow wndCtl);
    void OnExit(UINT uNotifyCode, int nID, CWindow wndCtl);
    void OnSafeMode(UINT uNotifyCode, int nID, CWindow wndCtl);
    void OnClose(UINT uNotifyCode, int nID, CWindow wndCtl);

    void OnFinalMessage(HWND hWnd) override;
    void PlaceWindowAtTrayArea();

    const DialogOptions m_dialogOptions;
    CIcon m_icon, m_smallIcon;
    bool m_wasActive = false;
};
