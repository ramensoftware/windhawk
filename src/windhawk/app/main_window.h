#pragma once

#include "engine_control.h"
#include "event_viewer_crash_monitor.h"
#include "service_common.h"
#include "session_metadata_reader.h"
#include "storage_manager.h"
#include "task_manager_dlg.h"
#include "toolkit_dlg.h"
#include "tray_icon.h"
#include "update_checker.h"
#include "update_notifier.h"
#include "user_profile_watcher.h"
#include "userprofile.h"

class CMainWindow : public CWindowImpl<CMainWindow, CWindow, CNullTraits>,
                    public CMessageFilter,
                    public CIdleHandler {
   public:
    DECLARE_WND_CLASS(L"WindhawkDaemon")

    // Custom messages.
    enum {
        UWM_DAEMON_COMMAND = WM_APP,
        UWM_TRAYICON,
        UWM_UPDATE_CHECKED,
    };

    // Requests another windhawk.exe instance can hand to the daemon of its
    // session. kExit is only honored by a portable daemon, which is the whole
    // app; a non-portable one exits with the service.
    enum class DaemonCommand {
        kRunUI = 1,
        kExit,
    };

    CMainWindow(bool trayOnly, bool portable);

   private:
    enum class Timer {
        kHandleNewProcesses = 1,
        kUpdateCheck,
        kReloadTrayIcons,
        kModTasksDlgCreate,
        kPendingUpdateNotification,
    };

    enum class Hotkey {
        kToolkit = 1,
    };

    BOOL PreTranslateMessage(MSG* pMsg) override;
    BOOL OnIdle() override;

    BEGIN_MSG_MAP_EX(CMainWindow)
        MSG_WM_CREATE(OnCreate)
        MSG_WM_DESTROY(OnDestroy)
        MSG_WM_HOTKEY(OnHotKey)
        MSG_WM_TIMER(OnTimer)
        MSG_WM_POWERBROADCAST(OnPowerBroadcast)
        MSG_WM_DPICHANGED(OnDpiChanged)
        MSG_WM_DISPLAYCHANGE(OnDisplayChange)
        MESSAGE_HANDLER_EX(UWM_DAEMON_COMMAND, OnDaemonCommand)
        MESSAGE_HANDLER_EX(UWM_TRAYICON, OnTrayIcon)
        MESSAGE_HANDLER_EX(UWM_UPDATE_CHECKED, OnUpdateChecked)
        MESSAGE_HANDLER_EX(m_taskbarCreatedMsg, OnTaskbarCreated)
    END_MSG_MAP()

    int OnCreate(LPCREATESTRUCT lpCreateStruct);
    void OnDestroy();
    void OnHotKey(int nHotKeyID, UINT uModifiers, UINT uVirtKey);
    void OnTimer(UINT_PTR nIDEvent);
    BOOL OnPowerBroadcast(DWORD dwPowerEvent, DWORD_PTR dwData);
    void OnDpiChanged(UINT nDpiX, UINT nDpiY, PRECT pRect);
    void OnDisplayChange(UINT uBitsPerPixel, CSize sizeScreen);
    LRESULT OnDaemonCommand(UINT uMsg, WPARAM wParam, LPARAM lParam);
    LRESULT OnTrayIcon(UINT uMsg, WPARAM wParam, LPARAM lParam);
    LRESULT OnUpdateChecked(UINT uMsg, WPARAM wParam, LPARAM lParam);
    LRESULT OnTaskbarCreated(UINT uMsg, WPARAM wParam, LPARAM lParam);

    UINT_PTR SetTimer(Timer nIDEvent,
                      UINT nElapse,
                      TIMERPROC lpfnTimer = nullptr);
    BOOL KillTimer(Timer nIDEvent);
    void InitForPortableVersion();
    void InitForNonPortableVersion();
    void LoadSettings();
    void Exit();
    void StopService(HWND hWnd = nullptr);
    void RunUI(HWND hWnd = nullptr);
    void CloseUI();
    UINT GetNextUpdateDelay(ULONGLONG lastUpdateCheck);
    void SetLastUpdateTime();
    void ResetLastUpdateTime();
    void OpenUpdatePage();
    void ShowLoadedModsDialog();
    void ShowToolkitDialog(bool triggeredBySystemInstability = false);
    void SwitchToSafeMode();
    void HandleExplorerCrash(int explorerCrashCount);

    bool m_trayOnly;
    bool m_portable;
    UINT m_taskbarCreatedMsg;
    wil::unique_mutex_nothrow m_serviceMutex;
    wil::unique_event_nothrow m_appSettingsChangedEvent;
    std::optional<AppTrayIcon> m_trayIcon;
    ServiceCommon::ServiceInfo m_serviceInfo{};
    std::optional<EngineControl> m_engineControl;
    std::unique_ptr<UpdateChecker> m_updateChecker;
    bool m_exitWhenUpdateCheckDone = false;
    std::optional<UpdateNotifier> m_updateNotifier;
    bool m_toolkitHotkeyRegistered = false;

    // Watches userprofile.json so the tray update icon/tooltip tracks the
    // update availability recorded there, whichever process wrote it. The guard
    // holds a hash of the profile's last processed content, filtering sibling
    // temp-file churn and signals for content which was already handled.
    std::optional<UserProfileChangeNotification>
        m_userProfileChangeNotification;
    std::optional<ULONGLONG> m_lastProfileContentHash;

    // Settings.
    LANGID m_languageId = 0;
    bool m_hideTrayIcon = true;
    bool m_disableUpdateCheck = true;
    bool m_checkForUpdates = false;  // portable version only
    bool m_dontAutoShowToolkit = true;
    bool m_disableToolkitHotkey = false;
    int m_modTasksDlgDelay = CTaskManagerDlg::kAutonomousModeShowDelayDefault;

    // Shown automatically when mods are doing tasks such as initializing or
    // loading symbols.
    std::optional<CTaskManagerDlg> m_modTasksDlg;
    std::optional<ModMetadataChangeNotification> m_modTasksChangeNotification;

    // Opened by the user.
    std::optional<CTaskManagerDlg> m_modStatusesDlg;
    std::optional<ModMetadataChangeNotification>
        m_modStatusesChangeNotification;

    // Opened from the tray icon, with a hotkey, or when explorer isn't running.
    std::optional<CToolkitDlg> m_toolkitDlg;

    // Explorer instability monitoring. Instability is detected when explorer
    // terminates more than once in a short period of time.
    constexpr static UINT kExplorerSecondCrashMaxPeriod = 1000 * 60;
    std::optional<EventViewerCrashMonitor> m_explorerCrashMonitor;
    ULONGLONG m_explorerLastTerminatedTickCount = 0;
};
