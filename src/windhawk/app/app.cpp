#include "stdafx.h"

#include "functions.h"
#include "logger.h"
#include "main_window.h"
#include "resource.h"
#include "service.h"
#include "storage_manager.h"
#include "ui_control.h"

CAppModule _Module;

namespace {

enum class Action {
    kDefault,
    kService,
    kServiceStart,
    kServiceStop,
    kRunUI,
    kEnableSafeMode,
    kCheckForUpdates,
    kAppSettingsChanged,
    kExit,
    kRestart,
    kRestartBg,
};

void Initialize();
void Run(Action action);
void RunDaemon();
void CheckForUpdates();
void NotifyAppSettingsChanged();
void ExitApp(bool wait, DWORD timeout, DWORD excludeProcessId = 0);
void RestartApp(DWORD timeout, bool trayOnly);
void RestartAppBg(DWORD timeout);
void EnableSafeMode();
void StartServiceAndRunUI(bool trayOnly);
void WaitForRunningProcessesToTerminate(DWORD timeout,
                                        bool windhawkBgOnly = false,
                                        DWORD excludeProcessId = 0);
void RunAsNewProcess(PCWSTR parameters);
std::wstring DescriptionFromHresult(HRESULT hr);
bool RunElevatedStep(PCWSTR what, PCWSTR parameters);
bool PostCommandToRunningDaemon(CMainWindow::DaemonCommand command);
void SetNamedEventForAllSessions(PCWSTR eventNamePrefix);
bool SetNamedEvent(PCWSTR eventName);
bool DoesParamExist(PCWSTR param);
int GetIntParam(PCWSTR param);

}  // namespace

int WINAPI wWinMain(_In_ HINSTANCE hInstance,
                    _In_opt_ HINSTANCE hPrevInstance,
                    _In_ LPWSTR lpCmdLine,
                    _In_ int nShowCmd) {
    if (DoesParamExist(L"-tool-mod") || DoesParamExist(L"-windhawk-tool-mod")) {
        return 0;
    }

    HRESULT hRes = ::CoInitialize(nullptr);
    ATLASSERT(SUCCEEDED(hRes));

    hRes = _Module.Init(nullptr, hInstance);
    ATLASSERT(SUCCEEDED(hRes));

    // Disable exception suppression in timer callbacks, as suggested by MSDN
    // and Bruce Dawson.
    // https://randomascii.wordpress.com/2012/07/05/when-even-crashing-doesnt-work/
    BOOL insanity = FALSE;
    SetUserObjectInformation(GetCurrentProcess(),
                             UOI_TIMERPROC_EXCEPTION_SUPPRESSION, &insanity,
                             sizeof(insanity));

    SetCurrentProcessExplicitAppUserModelID(L"RamenSoftware.Windhawk");

    Functions::EnableDarkModeMenus();

    Action action = Action::kDefault;
    if (DoesParamExist(L"-service")) {
        action = Action::kService;
    } else if (DoesParamExist(L"-service-start")) {
        action = Action::kServiceStart;
    } else if (DoesParamExist(L"-service-stop")) {
        action = Action::kServiceStop;
    } else if (DoesParamExist(L"-run-ui")) {
        action = Action::kRunUI;
    } else if (DoesParamExist(L"-x-enable-safe-mode")) {
        action = Action::kEnableSafeMode;
    } else if (DoesParamExist(L"-check-for-updates")) {
        action = Action::kCheckForUpdates;
    } else if (DoesParamExist(L"-app-settings-changed")) {
        action = Action::kAppSettingsChanged;
    } else if (DoesParamExist(L"-exit")) {
        action = Action::kExit;
    } else if (DoesParamExist(L"-restart")) {
        action = Action::kRestart;
    } else if (DoesParamExist(L"-restart-bg")) {
        action = Action::kRestartBg;
    }
    // New flags should start with "-x-" for compatibility with tool mods.

    HRESULT hr = S_OK;

    try {
        Initialize();
        Run(action);
    } catch (const std::exception& e) {
        switch (action) {
            case Action::kDefault:
            case Action::kRunUI:
                ::MessageBoxA(nullptr, e.what(), "Windhawk error",
                              MB_ICONERROR);
                break;

            default:
                LOG(L"%S", e.what());
                break;
        }

        hr = wil::ResultFromCaughtException();
    }

    _Module.Term();
    ::CoUninitialize();

    return hr;
}

namespace {

void Initialize() {
    // Make sure we can get an instance.
    // If not, this call will throw an exception.
    StorageManager::GetInstance();
}

void Run(Action action) {
    switch (action) {
        case Action::kService:
            VERBOSE("Running service");
            Service::Run();
            break;

        case Action::kServiceStart:
            VERBOSE("Starting service");
            Service::Start();
            break;

        case Action::kServiceStop:
            VERBOSE("Stopping service");
            Service::Stop(DoesParamExist(L"-also-no-autostart"));
            break;

        case Action::kRunUI:
            VERBOSE("Running UI");
            UIControl::RunUI();
            break;

        case Action::kEnableSafeMode:
            VERBOSE("Enabling safe mode");
            // Shuts everything down and writes the flag, without starting the
            // UI: the action exists to be elevated, and the unelevated caller
            // opens the UI itself. That caller is a windhawk.exe under the
            // install directory, so -caller-pid excludes it from the shutdown
            // wait. A failure here is only logged, and reported by the caller.
            ExitApp(/*wait=*/true, /*timeout=*/30000,
                    /*excludeProcessId=*/GetIntParam(L"-caller-pid"));
            EnableSafeMode();
            break;

        case Action::kCheckForUpdates:
            VERBOSE("Checking for updates");
            CheckForUpdates();
            break;

        case Action::kAppSettingsChanged:
            VERBOSE("Notifying about app settings changed");
            NotifyAppSettingsChanged();
            break;

        case Action::kExit: {
            VERBOSE("Exiting app");
            DWORD timeout = GetIntParam(L"-timeout");
            if (timeout == 0) {
                timeout = INFINITE;
            }

            ExitApp(DoesParamExist(L"-wait"), timeout);
            break;
        }

        case Action::kRestart: {
            VERBOSE("Restarting app");
            DWORD timeout = GetIntParam(L"-timeout");
            if (timeout == 0) {
                timeout = INFINITE;
            }

            RestartApp(timeout, DoesParamExist(L"-tray-only"));
            break;
        }

        case Action::kRestartBg: {
            VERBOSE("Restarting service/daemon");
            DWORD timeout = GetIntParam(L"-timeout");
            if (timeout == 0) {
                timeout = INFINITE;
            }

            RestartAppBg(timeout);
            break;
        }

        default:
            VERBOSE("Running Windhawk daemon");
            RunDaemon();
            break;
    }
}

void RunDaemon() {
    if (DoesParamExist(L"-wait")) {
        DWORD timeout = GetIntParam(L"-timeout");
        if (timeout == 0) {
            timeout = INFINITE;
        }

        WaitForRunningProcessesToTerminate(timeout);
    }

    bool portable = StorageManager::GetInstance().IsPortable();

    if (DoesParamExist(L"-safe-mode") ||
        (GetSystemMetrics(SM_CLEANBOOT) != 0 &&
         MessageBox(nullptr,
                    Functions::LoadStrFromRsrc(IDS_SAFE_MODE_DETECTED_TEXT),
                    Functions::LoadStrFromRsrc(IDS_SAFE_MODE_DETECTED_TITLE),
                    MB_ICONWARNING | MB_YESNO) == IDYES)) {
        if (portable) {
            ExitApp(/*wait=*/true, /*timeout=*/30000);
            EnableSafeMode();
            UIControl::RunUI();
        } else {
            // Elevate for the shutdown and the flag only, then open the UI from
            // this process, which is still unelevated. -caller-pid keeps the
            // helper's shutdown wait from waiting for us.
            auto parameters = std::format(L"-x-enable-safe-mode -caller-pid {}",
                                          GetCurrentProcessId());
            if (RunElevatedStep(L"enable safe mode", parameters.c_str())) {
                UIControl::RunUIOrBringToFront(nullptr);
            }
        }
        return;
    }

    bool trayOnly = DoesParamExist(L"-tray-only");

    if (!portable && !Service::IsRunning(/*waitIfStarting=*/true)) {
        // Start the service, which will in turn launch a new instance.
        if (!Functions::IsRunAsAdmin()) {
            // Elevate for the service start only, and wait for it, so the UI is
            // opened from this process, which is still unelevated.
            if (RunElevatedStep(L"start the Windhawk service",
                                L"-service-start") &&
                !trayOnly) {
                UIControl::RunUIOrBringToFront(nullptr);
            }
        } else {
            // Already elevated, so there is no unelevated process here to open
            // the UI. The service has one: the per-session daemon it launches.
            StartServiceAndRunUI(trayOnly);
        }
        return;
    }

    wil::unique_mutex_nothrow mutex(
        ::CreateMutex(nullptr, TRUE, L"WindhawkDaemon"));
    THROW_LAST_ERROR_IF_NULL(mutex);

    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        if (!trayOnly) {
            UIControl::RunUIOrBringToFront(nullptr);
        }

        return;
    }

    auto mutexLock = mutex.ReleaseMutex_scope_exit();

    if (portable) {
        if (!Functions::SetDebugPrivilege(TRUE)) {
            LOG(L"SetDebugPrivilege failed with error %u", GetLastError());
        }
    }

    // We need a custom CMessageLoop class to be able to wait
    // for objects in OnIdle correctly.
    class CMessageLoopAlwaysRunOnIdle : public CMessageLoop {
       public:
        BOOL OnIdle(int nIdleCount) override {
            CMessageLoop::OnIdle(nIdleCount);
            return TRUE;  // continue
        }
    };

    CMessageLoopAlwaysRunOnIdle loop;
    _Module.AddMessageLoop(&loop);

    CMainWindow wnd(trayOnly, portable);
    wnd.Create(nullptr);
    // wnd.ShowWindow(SW_SHOW);

    loop.Run();

    _Module.RemoveMessageLoop();
}

void CheckForUpdates() {
    bool portable = StorageManager::GetInstance().IsPortable();

    UpdateChecker m_updateChecker(portable ? UpdateChecker::kFlagPortable : 0,
                                  nullptr);
    UpdateChecker::Result result = m_updateChecker.HandleResponse();
    THROW_IF_FAILED(result.hrError);

    // The write to userprofile.json performed by the check above is observed by
    // the running daemon's file watcher, which refreshes the tray. No explicit
    // notification is needed.
}

void NotifyAppSettingsChanged() {
    if (StorageManager::GetInstance().IsPortable()) {
        SetNamedEvent(L"WindhawkAppSettingsChangedEvent-daemon");
        return;
    }

    SetNamedEventForAllSessions(
        L"Global\\WindhawkAppSettingsChangedEvent-daemon-session=");
}

void ExitApp(bool wait, DWORD timeout, DWORD excludeProcessId) {
    if (StorageManager::GetInstance().IsPortable()) {
        PostCommandToRunningDaemon(CMainWindow::DaemonCommand::kExit);
    } else {
        Service::Stop(false);
    }

    if (wait) {
        WaitForRunningProcessesToTerminate(timeout, /*windhawkBgOnly=*/false,
                                           excludeProcessId);
    }
}

void RestartApp(DWORD timeout, bool trayOnly) {
    bool portable = StorageManager::GetInstance().IsPortable();

    if (portable) {
        PostCommandToRunningDaemon(CMainWindow::DaemonCommand::kExit);
    } else {
        Service::Stop(false);
    }

    WaitForRunningProcessesToTerminate(timeout);

    if (portable) {
        RunAsNewProcess(trayOnly ? L"-tray-only" : nullptr);
    } else {
        StartServiceAndRunUI(trayOnly);
    }
}

void RestartAppBg(DWORD timeout) {
    auto uiWindows = UIControl::GetOpenUIWindows();

    // Disable UI windows to prevent them from being closed by the daemon. Not a
    // perfect solution but it works.
    for (HWND hWnd : uiWindows) {
        EnableWindow(hWnd, false);
    }

    bool portable = StorageManager::GetInstance().IsPortable();

    if (portable) {
        PostCommandToRunningDaemon(CMainWindow::DaemonCommand::kExit);
    } else {
        Service::Stop(false);
    }

    WaitForRunningProcessesToTerminate(timeout, /*windhawkBgOnly=*/true);

    for (HWND hWnd : uiWindows) {
        EnableWindow(hWnd, true);
    }

    if (portable) {
        RunAsNewProcess(L"-tray-only");
    } else {
        Service::Start();
    }
}

void EnableSafeMode() {
    StorageManager::GetInstance()
        .GetAppConfig(L"Settings", true)
        ->SetInt(L"SafeMode", 1);
}

// Starts the service and has it open the UI in this logon session, for elevated
// callers which can't launch the UI without passing their token on to it. The
// service launches a daemon per session on the session's own token, and a
// daemon without -tray-only opens the UI.
void StartServiceAndRunUI(bool trayOnly) {
    DWORD sessionId = 0;
    if (!trayOnly && !ProcessIdToSessionId(GetCurrentProcessId(), &sessionId)) {
        LOG(L"ProcessIdToSessionId failed with error %u", GetLastError());
        trayOnly = true;
    }

    // The session id only reaches the service on a start it actually performs.
    bool started =
        Service::Start(trayOnly ? std::nullopt : std::optional(sessionId));
    if (trayOnly || started) {
        return;
    }

    // The service was already running, so the start arguments were dropped and
    // it won't open anything. It has already launched this session's daemon on
    // the session's own token, though, which is the unelevated process this one
    // doesn't have, so hand the request to that. A service which only just
    // started may not have gotten to this session yet, hence the wait.
    VERBOSE(
        L"The service was already running, asking this session's daemon to "
        L"open the UI");

    constexpr DWORD kDaemonWaitTimeout = 10000;
    DWORD startTickCount = GetTickCount();

    while (!PostCommandToRunningDaemon(CMainWindow::DaemonCommand::kRunUI)) {
        if (GetTickCount() - startTickCount >= kDaemonWaitTimeout) {
            // Nothing in this session to hand it to, so open the UI from here,
            // elevation and all.
            LOG(L"No daemon in this session, running the UI from here");
            UIControl::RunUI();
            return;
        }

        Sleep(200);
    }
}

void WaitForRunningProcessesToTerminate(DWORD timeout,
                                        bool windhawkBgOnly,
                                        DWORD excludeProcessId) {
    DWORD startTickCount = GetTickCount();

    HRESULT hr;

    // Use QueryFullProcessImageName instead of GetModuleFileName because the
    // latter can return a path with a different case depending on how the
    // process was launched. QueryFullProcessImageName seems to be consistent
    // in this regard.
    std::filesystem::path modulePath =
        wil::QueryFullProcessImageName<std::wstring>();
    std::wstring folderPath = modulePath.parent_path().wstring() + L'\\';

    while (true) {
        HANDLE handlesRawArray[MAXIMUM_WAIT_OBJECTS];
        wil::unique_process_handle handles[MAXIMUM_WAIT_OBJECTS];
        DWORD handlesCount = 0;

        wil::unique_tool_help_snapshot snapshot(
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        THROW_LAST_ERROR_IF(!snapshot);

        PROCESSENTRY32 pe;
        pe.dwSize = sizeof(PROCESSENTRY32);
        THROW_IF_WIN32_BOOL_FALSE(Process32First(snapshot.get(), &pe));

        do {
            if (pe.th32ProcessID == 0) {
                // Skipping System Idle Process.
                continue;
            }

            if (pe.th32ProcessID == GetCurrentProcessId()) {
                // Skipping current process.
                continue;
            }

            if (excludeProcessId != 0 && pe.th32ProcessID == excludeProcessId) {
                // Skipping the caller which is waiting for this process.
                continue;
            }

            if (windhawkBgOnly) {
                if (_wcsicmp(pe.szExeFile, L"windhawk.exe") != 0) {
                    continue;
                }

                WCHAR toolModMutexName[sizeof(
                    "Global\\windhawk-tool-mod-pid=1234567890")];
                swprintf_s(toolModMutexName,
                           L"Global\\windhawk-tool-mod-pid=%u",
                           pe.th32ProcessID);
                if (wil::unique_mutex_nothrow(
                        ::OpenMutex(SYNCHRONIZE, FALSE, toolModMutexName))) {
                    // Skip tool-mod process.
                    continue;
                }
            } else {
                if (_wcsicmp(pe.szExeFile, L"uninstall.exe") == 0) {
                    // Skip uninstaller, which may be running but is not part of
                    // the app.
                    continue;
                }
            }

            wil::unique_process_handle process(
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                            FALSE, pe.th32ProcessID));
            if (process) {
                std::wstring fullProcessImageName;
                hr = wil::QueryFullProcessImageName<std::wstring>(
                    process.get(), 0, fullProcessImageName);
                if (SUCCEEDED(hr)) {
                    // Is path inside folder:
                    // https://stackoverflow.com/a/40441240
                    if (fullProcessImageName.rfind(folderPath, 0) == 0) {
                        VERBOSE(L"Waiting for %u (%s)", pe.th32ProcessID,
                                pe.szExeFile);
                        handlesRawArray[handlesCount] = process.get();
                        handles[handlesCount] = std::move(process);
                        handlesCount++;
                    }
                } else {
                    VERBOSE(
                        L"QueryFullProcessImageName for %u (%s) failed with "
                        L"error 0x%08X",
                        pe.th32ProcessID, pe.szExeFile, hr);
                }
            } else {
                VERBOSE(L"OpenProcess for %u (%s) failed with error %u",
                        pe.th32ProcessID, pe.szExeFile, GetLastError());
            }
        } while (handlesCount < _countof(handles) &&
                 Process32Next(snapshot.get(), &pe));

        if (handlesCount < _countof(handles)) {
            THROW_LAST_ERROR_IF(GetLastError() != ERROR_NO_MORE_FILES);
        }

        if (handlesCount > 0) {
            DWORD iterationTimeout = timeout;
            if (iterationTimeout != INFINITE) {
                DWORD timePassed = GetTickCount() - startTickCount;
                if (timePassed >= iterationTimeout) {
                    THROW_WIN32(ERROR_TIMEOUT);
                }

                iterationTimeout -= timePassed;
            }

            VERBOSE(L"Waiting for %u processes", handlesCount);

            switch (WaitForMultipleObjects(handlesCount, handlesRawArray, TRUE,
                                           iterationTimeout)) {
                case WAIT_TIMEOUT:
                    THROW_WIN32(ERROR_TIMEOUT);

                case WAIT_FAILED:
                    THROW_LAST_ERROR();
            }
        }

        if (handlesCount < _countof(handles)) {
            break;
        }
    }
}

void RunAsNewProcess(PCWSTR parameters) {
    auto modulePath = wil::GetModuleFileName<std::wstring>();

    std::wstring commandLine = L"\"" + modulePath + L"\"";
    if (parameters && *parameters != L'\0') {
        commandLine += L' ';
        commandLine += parameters;
    }

    STARTUPINFO si = {sizeof(STARTUPINFO)};
    wil::unique_process_information process;

    THROW_IF_WIN32_BOOL_FALSE(CreateProcess(
        modulePath.c_str(), commandLine.data(), nullptr, nullptr, FALSE,
        NORMAL_PRIORITY_CLASS, nullptr, nullptr, &si, &process));
}

// The system's description of an error code, empty if it has none. A code
// which wraps a Win32 error is looked up as that error, which is where the
// descriptions worth showing live.
std::wstring DescriptionFromHresult(HRESULT hr) {
    DWORD code = HRESULT_FACILITY(hr) == FACILITY_WIN32
                     ? static_cast<DWORD>(HRESULT_CODE(hr))
                     : static_cast<DWORD>(hr);

    wil::unique_hlocal_string buffer;
    DWORD length = FormatMessage(
        FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM |
            FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr, code, 0, reinterpret_cast<PWSTR>(buffer.addressof()), 0,
        nullptr);
    if (length == 0) {
        return {};
    }

    std::wstring description(buffer.get(), length);
    description.erase(description.find_last_not_of(L" \t\r\n") + 1);
    return description;
}

// Runs an elevated instance to perform one step and waits for it. Returns
// whether the caller's follow-up work should go ahead: a declined consent
// dialog is an answer rather than a failure, so it is silent, while a failed
// instance is reported from here, the process the user is dealing with. What
// names the step in that report, as in "Could not <what>".
bool RunElevatedStep(PCWSTR what, PCWSTR parameters) {
    auto modulePath = wil::GetModuleFileName<std::wstring>();

    SHELLEXECUTEINFO executeInfo = {sizeof(SHELLEXECUTEINFO)};
    executeInfo.fMask = SEE_MASK_NOCLOSEPROCESS;
    executeInfo.lpVerb = L"runas";
    executeInfo.lpFile = modulePath.c_str();
    executeInfo.lpParameters = parameters;
    executeInfo.nShow = SW_SHOWNORMAL;

    if (!ShellExecuteEx(&executeInfo)) {
        THROW_LAST_ERROR_IF(GetLastError() != ERROR_CANCELLED);
        return false;
    }

    // SEE_MASK_NOCLOSEPROCESS asks for the handle, but a request satisfied
    // without starting a process (a DDE conversation) succeeds without one,
    // and there is nothing to wait on then.
    wil::unique_process_handle process(executeInfo.hProcess);
    THROW_HR_IF_NULL(E_UNEXPECTED, process);

    THROW_LAST_ERROR_IF(WaitForSingleObject(process.get(), INFINITE) ==
                        WAIT_FAILED);

    DWORD exitCode;
    THROW_IF_WIN32_BOOL_FALSE(GetExitCodeProcess(process.get(), &exitCode));
    if (exitCode != 0) {
        // wWinMain returns the HRESULT it caught, so the code is all that
        // crosses the process boundary; the instance logged the message.
        LOG(L"Could not %s, the elevated instance returned 0x%08X", what,
            exitCode);

        auto message =
            std::format(L"Could not {} (error 0x{:08X}).", what, exitCode);

        // The code is what the user has to go on, so spell out what it means
        // when the system can.
        auto description = DescriptionFromHresult(exitCode);
        if (!description.empty()) {
            message += std::format(L"\n\n{}", description);
        }

        ::MessageBox(nullptr, message.c_str(), L"Windhawk error", MB_ICONERROR);
        return false;
    }

    return true;
}

bool PostCommandToRunningDaemon(CMainWindow::DaemonCommand command) {
    // Window stations are per session, so this only ever finds the daemon of
    // this logon session.
    CWindow hDaemonWnd(FindWindow(L"WindhawkDaemon", nullptr));
    if (!hDaemonWnd) {
        return false;
    }

    ::AllowSetForegroundWindow(hDaemonWnd.GetWindowProcessID());

    THROW_IF_WIN32_BOOL_FALSE(hDaemonWnd.PostMessage(
        CMainWindow::UWM_DAEMON_COMMAND, (WPARAM)command));

    return true;
}

void SetNamedEventForAllSessions(PCWSTR eventNamePrefix) {
    WTS_SESSION_INFO* sessionInfo;
    DWORD dwCount;

    THROW_IF_WIN32_BOOL_FALSE(WTSEnumerateSessions(WTS_CURRENT_SERVER_HANDLE, 0,
                                                   1, &sessionInfo, &dwCount));
    wil::unique_wtsmem_ptr<WTS_SESSION_INFO> scopedSessionInfo(sessionInfo);

    for (DWORD i = 0; i < dwCount; i++) {
        WCHAR* pszUserName;
        DWORD dwUserNameLen;

        THROW_IF_WIN32_BOOL_FALSE(WTSQuerySessionInformation(
            WTS_CURRENT_SERVER_HANDLE, sessionInfo[i].SessionId, WTSUserName,
            &pszUserName, &dwUserNameLen));
        wil::unique_wtsmem_ptr<WCHAR> scopedUserName(pszUserName);

        if (*pszUserName != L'\0') {
            auto eventName =
                eventNamePrefix + std::to_wstring(sessionInfo[i].SessionId);
            SetNamedEvent(eventName.c_str());
        }
    }
}

bool SetNamedEvent(PCWSTR eventName) {
    wil::unique_event namedEvent(
        OpenEvent(EVENT_MODIFY_STATE, FALSE, eventName));
    if (!namedEvent) {
        THROW_LAST_ERROR_IF(GetLastError() != ERROR_FILE_NOT_FOUND);
        return false;
    }

    namedEvent.SetEvent();
    return true;
}

bool DoesParamExist(PCWSTR param) {
    for (int i = 1; i < __argc; i++) {
        if (_wcsicmp(__wargv[i], param) == 0) {
            return true;
        }
    }

    return false;
}

int GetIntParam(PCWSTR param) {
    for (int i = 1; i < __argc - 1; i++) {
        if (_wcsicmp(__wargv[i], param) == 0) {
            return _wtoi(__wargv[i + 1]);
        }
    }

    return 0;
}

}  // namespace
