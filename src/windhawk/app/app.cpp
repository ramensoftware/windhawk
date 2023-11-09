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
    kRunUIAsAdmin,
    kServiceStartAndRunUI,
    kCheckForUpdates,
    kNewUpdatesFound,
    kAppSettingsChanged,
    kExit,
    kRestart,
};

void Initialize();
void Run(Action action);
void RunDaemon();
void CheckForUpdates();
void NotifyNewUpdatesFound();
void NotifyAppSettingsChanged();
void ExitApp();
void RestartApp();
void WaitForRunningProcessesToTerminate(DWORD timeout);
void RunAsNewProcess(PCWSTR parameters);
bool RunAsAdmin(PCWSTR parameters);
bool PostCommandToRunningDaemon(CMainWindow::AppCommand command);
void SetNamedEventForAllSessions(PCWSTR eventNamePrefix);
bool SetNamedEvent(PCWSTR eventName);
bool DoesParamExist(PCWSTR param);
int GetIntParam(PCWSTR param);

}  // namespace

int WINAPI wWinMain(_In_ HINSTANCE hInstance,
                    _In_opt_ HINSTANCE hPrevInstance,
                    _In_ LPWSTR lpCmdLine,
                    _In_ int nShowCmd) {
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

    Action action = Action::kDefault;
    if (DoesParamExist(L"-service")) {
        action = Action::kService;
    } else if (DoesParamExist(L"-service-start")) {
        action = Action::kServiceStart;
    } else if (DoesParamExist(L"-service-stop")) {
        action = Action::kServiceStop;
    } else if (DoesParamExist(L"-run-ui")) {
        action = Action::kRunUI;
    } else if (DoesParamExist(L"-run-ui-as-admin")) {
        action = Action::kRunUIAsAdmin;
    } else if (DoesParamExist(L"-service-start-and-run-ui")) {
        action = Action::kServiceStartAndRunUI;
    } else if (DoesParamExist(L"-check-for-updates")) {
        action = Action::kCheckForUpdates;
    } else if (DoesParamExist(L"-new-updates-found")) {
        action = Action::kNewUpdatesFound;
    } else if (DoesParamExist(L"-app-settings-changed")) {
        action = Action::kAppSettingsChanged;
    } else if (DoesParamExist(L"-exit")) {
        action = Action::kExit;
    } else if (DoesParamExist(L"-restart")) {
        action = Action::kRestart;
    }

    HRESULT hr = S_OK;

    try {
        Initialize();
        Run(action);
    } catch (const std::exception& e) {
        switch (action) {
            case Action::kDefault:
            case Action::kRunUI:
            case Action::kRunUIAsAdmin:
            case Action::kServiceStartAndRunUI:
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

        case Action::kRunUIAsAdmin:
            VERBOSE("Running UI as admin");
            if (!Functions::IsRunAsAdmin()) {
                RunAsAdmin(L"-run-ui");
                break;
            }
            [[fallthrough]];
        case Action::kRunUI:
            VERBOSE("Running UI");
            UIControl::RunUI();
            break;

        case Action::kServiceStartAndRunUI:
            VERBOSE("Starting service and running UI");
            Service::Start();
            UIControl::RunUI();
            break;

        case Action::kCheckForUpdates:
            VERBOSE("Checking for updates");
            CheckForUpdates();
            break;

        case Action::kNewUpdatesFound:
            VERBOSE("Notifying about new updates found");
            NotifyNewUpdatesFound();
            break;

        case Action::kAppSettingsChanged:
            VERBOSE("Notifying about app settings changed");
            NotifyAppSettingsChanged();
            break;

        case Action::kExit:
            VERBOSE("Exiting app");
            ExitApp();
            break;

        case Action::kRestart:
            VERBOSE("Restarting app");
            RestartApp();
            break;

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

    bool trayOnly = DoesParamExist(L"-tray-only");
    bool portable = StorageManager::GetInstance().IsPortable();

    if (!portable && !Service::IsRunning()) {
        // Start the service, which will in turn launch a new instance.
        if (!Functions::IsRunAsAdmin()) {
            RunAsAdmin(trayOnly ? L"-service-start"
                                : L"-service-start-and-run-ui");
        } else {
            Service::Start();
            if (!trayOnly) {
                UIControl::RunUI();
            }
        }
        return;
    }

    wil::unique_mutex_nothrow mutex(
        ::CreateMutex(nullptr, TRUE, L"WindhawkDaemon"));
    THROW_LAST_ERROR_IF_NULL(mutex);

    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        if (!trayOnly) {
            UIControl::RunUIOrBringToFront(
                nullptr, !portable && !Functions::IsRunAsAdmin());
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

    if (result.updateStatus.newUpdatesFound) {
        NotifyNewUpdatesFound();
    }
}

void NotifyNewUpdatesFound() {
    SetNamedEventForAllSessions(
        L"Global\\WindhawkNewUpdatesFoundEvent-daemon-session=");
}

void NotifyAppSettingsChanged() {
    if (StorageManager::GetInstance().IsPortable()) {
        SetNamedEvent(L"WindhawkAppSettingsChangedEvent-daemon");
        return;
    }

    SetNamedEventForAllSessions(
        L"Global\\WindhawkAppSettingsChangedEvent-daemon-session=");
}

void ExitApp() {
    if (StorageManager::GetInstance().IsPortable()) {
        PostCommandToRunningDaemon(CMainWindow::AppCommand::kExit);
    } else {
        Service::Stop(false);
    }

    if (DoesParamExist(L"-wait")) {
        DWORD timeout = GetIntParam(L"-timeout");
        if (timeout == 0) {
            timeout = INFINITE;
        }

        WaitForRunningProcessesToTerminate(timeout);
    }
}

void RestartApp() {
    bool trayOnly = DoesParamExist(L"-tray-only");
    bool portable = StorageManager::GetInstance().IsPortable();

    if (portable) {
        PostCommandToRunningDaemon(CMainWindow::AppCommand::kExit);
    } else {
        Service::Stop(false);
    }

    DWORD timeout = GetIntParam(L"-timeout");
    if (timeout == 0) {
        timeout = INFINITE;
    }

    WaitForRunningProcessesToTerminate(timeout);

    if (portable) {
        RunAsNewProcess(trayOnly ? L"-tray-only" : nullptr);
    } else {
        Service::Start();
        if (!trayOnly) {
            UIControl::RunUI();
        }
    }
}

void WaitForRunningProcessesToTerminate(DWORD timeout) {
    DWORD startTickCount = GetTickCount();

    HRESULT hr;

    // Use QueryFullProcessImageName instead of GetModuleFileName because the
    // latter can return a path with a different case depending on how the
    // process was launched. QueryFullProcessImageName seems to be consistent
    // in this regard.
    std::filesystem::path modulePath =
        wil::QueryFullProcessImageName<std::wstring>();
    auto folderPath = modulePath.parent_path();

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
            } else if (pe.th32ProcessID == GetCurrentProcessId()) {
                // Skipping current process.
            } else {
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
                            L"QueryFullProcessImageName for %u (%s) failed "
                            L"with error 0x%08X",
                            pe.th32ProcessID, pe.szExeFile, hr);
                    }
                } else {
                    VERBOSE(L"OpenProcess for %u (%s) failed with error %u",
                            pe.th32ProcessID, pe.szExeFile, GetLastError());
                }
            }

            pe.dwSize = sizeof(PROCESSENTRY32);
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

bool RunAsAdmin(PCWSTR parameters) {
    auto modulePath = wil::GetModuleFileName<std::wstring>();

    if ((int)(UINT_PTR)ShellExecute(nullptr, L"runas", modulePath.c_str(),
                                    parameters, nullptr, SW_SHOWNORMAL) > 32) {
        return true;
    }

    THROW_LAST_ERROR_IF(GetLastError() != ERROR_CANCELLED);
    return false;
}

bool PostCommandToRunningDaemon(CMainWindow::AppCommand command) {
    CWindow hDaemonWnd(FindWindow(L"WindhawkDaemon", nullptr));
    if (!hDaemonWnd) {
        return false;
    }

    ::AllowSetForegroundWindow(hDaemonWnd.GetWindowProcessID());

    THROW_IF_WIN32_BOOL_FALSE(
        hDaemonWnd.PostMessage(CMainWindow::UWM_APP_COMMAND, (WPARAM)command));

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
