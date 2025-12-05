#include "stdafx.h"

#include "service.h"

#include "engine_control.h"
#include "functions.h"
#include "logger.h"
#include "service_common.h"
#include "storage_manager.h"
#include "version.h"

namespace {

HANDLE CreateServiceInfoFileMapping() {
    // Allow only FILE_MAP_READ (0x0004), only for medium integrity.
    PCWSTR pszStringSecurityDescriptor = L"D:(A;;0x0004;;;WD)S:(ML;;NW;;;ME)";

    wil::unique_hlocal secDesc;
    THROW_IF_WIN32_BOOL_FALSE(
        ConvertStringSecurityDescriptorToSecurityDescriptor(
            pszStringSecurityDescriptor, SDDL_REVISION_1, &secDesc, nullptr));

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    wil::unique_handle fileMapping(
        CreateFileMapping(INVALID_HANDLE_VALUE, &secAttr, PAGE_READWRITE, 0,
                          sizeof(ServiceCommon::ServiceInfo),
                          ServiceCommon::kInfoFileMappingName));
    THROW_LAST_ERROR_IF(!fileMapping || GetLastError() == ERROR_ALREADY_EXISTS);

    wil::unique_mapview_ptr<ServiceCommon::ServiceInfo> fileMappingView(
        reinterpret_cast<ServiceCommon::ServiceInfo*>(
            MapViewOfFile(fileMapping.get(), FILE_MAP_WRITE, 0, 0, 0)));
    THROW_LAST_ERROR_IF(!fileMappingView);

    FILETIME creationTime;
    FILETIME exitTime;
    FILETIME kernelTime;
    FILETIME userTime;
    THROW_IF_WIN32_BOOL_FALSE(GetProcessTimes(
        GetCurrentProcess(), &creationTime, &exitTime, &kernelTime, &userTime));

    fileMappingView->version = VER_FILE_VERSION_LONG;
    fileMappingView->processId = GetCurrentProcessId();
    fileMappingView->processCreationTime =
        wil::filetime::to_int64(creationTime);

    return fileMapping.release();
}

HANDLE CreateServiceMutex() {
    // Allow only SYNCHRONIZE (0x00100000), only for medium integrity.
    PCWSTR pszStringSecurityDescriptor =
        L"D:(A;;0x00100000;;;WD)S:(ML;;NW;;;ME)";

    wil::unique_hlocal secDesc;
    THROW_IF_WIN32_BOOL_FALSE(
        ConvertStringSecurityDescriptorToSecurityDescriptor(
            pszStringSecurityDescriptor, SDDL_REVISION_1, &secDesc, nullptr));

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    wil::unique_mutex_nothrow mutex(
        CreateMutex(&secAttr, TRUE, ServiceCommon::kMutexName));
    THROW_LAST_ERROR_IF(!mutex || GetLastError() == ERROR_ALREADY_EXISTS);

    return mutex.release();
}

void CreateProcessOnSessionId(DWORD dwSessionId,
                              const WCHAR* pszPath,
                              WCHAR* pszCommandLine) {
    wil::unique_handle token;
    THROW_IF_WIN32_BOOL_FALSE(WTSQueryUserToken(dwSessionId, &token));

    wil::unique_environment_block environment;
    THROW_IF_WIN32_BOOL_FALSE(
        CreateEnvironmentBlock(&environment, token.get(), FALSE));

    wil::unique_process_information processInfo;
    STARTUPINFO startupInfo = {sizeof(STARTUPINFO)};

    THROW_IF_WIN32_BOOL_FALSE(CreateProcessAsUser(
        token.get(), pszPath, pszCommandLine, nullptr, nullptr, FALSE,
        NORMAL_PRIORITY_CLASS | CREATE_UNICODE_ENVIRONMENT, environment.get(),
        nullptr, &startupInfo, &processInfo));
}

void CreateProcessOnAllSessions(const WCHAR* pszPath, WCHAR* pszCommandLine) {
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
            CreateProcessOnSessionId(sessionInfo[i].SessionId, pszPath,
                                     pszCommandLine);
        }
    }
}

}  // namespace

class ServiceInstance {
   public:
    VOID SvcMain(DWORD dwArgc, LPTSTR* lpszArgv);

   private:
    VOID SvcInit(DWORD dwArgc, LPTSTR* lpszArgv);
    VOID SvcRun(DWORD dwArgc, LPTSTR* lpszArgv);
    VOID ReportSvcStatus(DWORD dwCurrentState,
                         DWORD dwWin32ExitCode,
                         DWORD dwWaitHint);
    static DWORD WINAPI SvcCtrlHandlerExThunk(DWORD dwControl,
                                              DWORD dwEventType,
                                              LPVOID lpEventData,
                                              LPVOID lpContext);
    DWORD SvcCtrlHandlerEx(DWORD dwControl,
                           DWORD dwEventType,
                           LPVOID lpEventData);

    SERVICE_STATUS_HANDLE m_svcStatusHandle{};
    DWORD m_dwCheckPoint = 1;
    wil::unique_handle m_svcInfoFileMapping;
    wil::unique_mutex m_svcMutex;
    wil::mutex_release_scope_exit m_svcMutexLock;
    wil::unique_event m_svcStopEvent;
    wil::unique_event m_svcScanForProcessesEvent;
    wil::unique_event m_svcEmergencyStopEvent;
    wil::unique_event m_svcSafeModeStopEvent;
    std::optional<EngineControl> m_engineControl;
};

//
// Purpose:
//   Entry point for the service
//
// Parameters:
//   dwArgc - Number of arguments in the lpszArgv array
//   lpszArgv - Array of strings. The first string is the name of
//     the service and subsequent strings are passed by the process
//     that called the StartService function to start the service.
//
// Return value:
//   None.
//
VOID ServiceInstance::SvcMain(DWORD dwArgc, LPTSTR* lpszArgv) {
    // Register the handler function for the service
    m_svcStatusHandle = RegisterServiceCtrlHandlerEx(
        ServiceCommon::kName, SvcCtrlHandlerExThunk,
        reinterpret_cast<LPVOID>(this));
    THROW_LAST_ERROR_IF_NULL(m_svcStatusHandle);

    // Report initial status to the SCM
    ReportSvcStatus(SERVICE_START_PENDING, NO_ERROR, 3000);

    // Perform service-specific initialization and work.
    try {
        VERBOSE(L"Running SvcInit");
        SvcInit(dwArgc, lpszArgv);
    } catch (const std::exception& e) {
        LOG(L"SvcInit failed: %S", e.what());
        ReportSvcStatus(SERVICE_STOPPED, wil::ResultFromCaughtException(), 0);
        return;
    }

    // Report running status when initialization is complete.
    ReportSvcStatus(SERVICE_RUNNING, NO_ERROR, 0);

    try {
        VERBOSE(L"Running SvcRun");
        SvcRun(dwArgc, lpszArgv);
    } catch (const std::exception& e) {
        LOG(L"SvcRun failed: %S", e.what());
        ReportSvcStatus(SERVICE_STOPPED, wil::ResultFromCaughtException(), 0);
        return;
    }

    VERBOSE(L"Reporting SERVICE_STOPPED");
    ReportSvcStatus(SERVICE_STOPPED, NO_ERROR, 0);
}

//
// Purpose:
//   The service code
//
// Parameters:
//   dwArgc - Number of arguments in the lpszArgv array
//   lpszArgv - Array of strings. The first string is the name of
//     the service and subsequent strings are passed by the process
//     that called the StartService function to start the service.
//
// Return value:
//   None
//
VOID ServiceInstance::SvcInit(DWORD dwArgc, LPTSTR* lpszArgv) {
    // TO_DO: Declare and set any required variables.
    //   Be sure to periodically call ReportSvcStatus() with
    //   SERVICE_START_PENDING. If initialization fails, call
    //   ReportSvcStatus with SERVICE_STOPPED.

    if (!Functions::SetDebugPrivilege(TRUE)) {
        LOG(L"SetDebugPrivilege failed with error %u", GetLastError());
    }

    m_svcInfoFileMapping.reset(CreateServiceInfoFileMapping());

    m_svcMutex.reset(CreateServiceMutex());

    m_svcMutexLock = m_svcMutex.ReleaseMutex_scope_exit();

    // Create an event. The control handler function, SvcCtrlHandler,
    // signals this event when it receives the stop control code.
    m_svcStopEvent.reset(CreateEvent(nullptr,    // default security attributes
                                     TRUE,       // manual reset event
                                     FALSE,      // not signaled
                                     nullptr));  // no name
    THROW_LAST_ERROR_IF_NULL(m_svcStopEvent);

    m_svcScanForProcessesEvent.reset(Functions::CreateEventForMediumIntegrity(
        ServiceCommon::kScanForProcessesEventName, FALSE));

    m_svcEmergencyStopEvent.reset(Functions::CreateEventForMediumIntegrity(
        ServiceCommon::kEmergencyStopEventName, TRUE));

    m_svcSafeModeStopEvent.reset(Functions::CreateEventForMediumIntegrity(
        ServiceCommon::kSafeModeStopEventName, TRUE));

    auto settings =
        StorageManager::GetInstance().GetAppConfig(L"Settings", false);

    if (!settings->GetInt(L"SafeMode").value_or(0)) {
        m_engineControl.emplace();
        m_engineControl->HandleNewProcesses();
    }
}

//
// Purpose:
//   The service code
//
VOID ServiceInstance::SvcRun(DWORD dwArgc, LPTSTR* lpszArgv) {
    // TO_DO: Perform work until service stops.

    try {
        auto modulePath = wil::GetModuleFileName<std::wstring>();
        auto commandLine = L"\"" + modulePath + L"\" -tray-only";
        CreateProcessOnAllSessions(modulePath.c_str(), commandLine.data());
    } catch (const std::exception& e) {
        LOG(L"CreateProcessOnAllSessions failed: %S", e.what());
    }

    HANDLE events[] = {
        m_svcStopEvent.get(),
        m_svcScanForProcessesEvent.get(),
        m_svcEmergencyStopEvent.get(),
        m_svcSafeModeStopEvent.get(),
    };

    while (true) {
        bool keepLooping = false;

        DWORD dwWaitResult = WaitForMultipleObjectsEx(ARRAYSIZE(events), events,
                                                      FALSE, 1000, FALSE);
        switch (dwWaitResult) {
            case WAIT_FAILED:
                THROW_LAST_ERROR();
                break;

            case WAIT_TIMEOUT:
				keepLooping = true;
                break;

            case WAIT_OBJECT_0:
                VERBOSE(L"Received stop event");
                break;

            case WAIT_OBJECT_0 + 1:
                VERBOSE(L"Received scan for processes event");

                keepLooping = true;
                break;

            case WAIT_OBJECT_0 + 2:
                LOG(L"Received emergency stop event");
                break;

            case WAIT_OBJECT_0 + 3: {
                LOG(L"Received safe mode stop event");

                auto settings = StorageManager::GetInstance().GetAppConfig(
                    L"Settings", true);
                settings->SetInt(L"SafeMode", 1);

                // Flush the settings to ensure they are saved, in case
                // unloading will cause a BSOD.
                StorageManager::GetInstance().FlushAppConfig(L"Settings");
                break;
            }

            default:
                LOG(L"Received unknown event %u", dwWaitResult);
                break;
        }

        if (!keepLooping) {
            break;
        }

        if (m_engineControl) {
            m_engineControl->HandleNewProcesses();
        }
    }
}

//
// Purpose:
//   Sets the current service status and reports it to the SCM.
//
// Parameters:
//   dwCurrentState - The current state (see SERVICE_STATUS)
//   dwWin32ExitCode - The system error code
//   dwWaitHint - Estimated time for pending operation,
//     in milliseconds
//
// Return value:
//   None
//
VOID ServiceInstance::ReportSvcStatus(DWORD dwCurrentState,
                                      DWORD dwWin32ExitCode,
                                      DWORD dwWaitHint) {
    SERVICE_STATUS SvcStatus{};

    // These SERVICE_STATUS members remain as set here.
    SvcStatus.dwServiceType = SERVICE_WIN32_OWN_PROCESS;
    SvcStatus.dwServiceSpecificExitCode = 0;

    // Fill in the SERVICE_STATUS structure.
    SvcStatus.dwCurrentState = dwCurrentState;
    SvcStatus.dwWin32ExitCode = dwWin32ExitCode;
    SvcStatus.dwWaitHint = dwWaitHint;

    SvcStatus.dwControlsAccepted = SERVICE_ACCEPT_SESSIONCHANGE;
    if (dwCurrentState != SERVICE_START_PENDING)
        SvcStatus.dwControlsAccepted |= SERVICE_ACCEPT_STOP;

    if (dwCurrentState == SERVICE_RUNNING || dwCurrentState == SERVICE_STOPPED)
        SvcStatus.dwCheckPoint = 0;
    else
        SvcStatus.dwCheckPoint = m_dwCheckPoint++;

    // Report the status of the service to the SCM.
    SetServiceStatus(m_svcStatusHandle, &SvcStatus);
}

// static
DWORD WINAPI ServiceInstance::SvcCtrlHandlerExThunk(DWORD dwControl,
                                                    DWORD dwEventType,
                                                    LPVOID lpEventData,
                                                    LPVOID lpContext) {
    auto serviceInstance = reinterpret_cast<ServiceInstance*>(lpContext);
    return serviceInstance->SvcCtrlHandlerEx(dwControl, dwEventType,
                                             lpEventData);
}

//
// Purpose:
//   Called by SCM whenever a control code is sent to the service
//   using the ControlService function.
//
DWORD ServiceInstance::SvcCtrlHandlerEx(DWORD dwControl,
                                        DWORD dwEventType,
                                        LPVOID lpEventData) {
    // Handle the requested control code.

    switch (dwControl) {
        case SERVICE_CONTROL_STOP:
            VERBOSE("Handling SERVICE_CONTROL_STOP");

            ReportSvcStatus(SERVICE_STOP_PENDING, NO_ERROR, 0);

            // Signal the service to stop.
            SetEvent(m_svcStopEvent.get());
            return NO_ERROR;

        case SERVICE_CONTROL_SESSIONCHANGE:
            if (dwEventType == WTS_SESSION_LOGON) {
                VERBOSE("Handling WTS_SESSION_LOGON");

                try {
                    auto sessionNotification =
                        reinterpret_cast<const WTSSESSION_NOTIFICATION*>(
                            lpEventData);
                    WCHAR* pszUserName;
                    DWORD dwUserNameLen;

                    THROW_IF_WIN32_BOOL_FALSE(WTSQuerySessionInformation(
                        WTS_CURRENT_SERVER_HANDLE,
                        sessionNotification->dwSessionId, WTSUserName,
                        &pszUserName, &dwUserNameLen));
                    wil::unique_wtsmem_ptr<WCHAR> scopedUserName(pszUserName);

                    if (*pszUserName != L'\0') {
                        auto modulePath =
                            wil::GetModuleFileName<std::wstring>();
                        auto commandLine =
                            L"\"" + modulePath + L"\" -tray-only";
                        CreateProcessOnSessionId(
                            sessionNotification->dwSessionId,
                            modulePath.c_str(), commandLine.data());
                    }
                } catch (const std::exception& e) {
                    LOG(L"WTS_SESSION_LOGON handler failed: %S", e.what());
                }
            }
            return NO_ERROR;

        case SERVICE_CONTROL_INTERROGATE:
            return NO_ERROR;

        default:
            return ERROR_CALL_NOT_IMPLEMENTED;
    }
}

VOID WINAPI SvcMain(DWORD dwArgc, LPTSTR* lpszArgv) {
    try {
        ServiceInstance serviceInstance;
        serviceInstance.SvcMain(dwArgc, lpszArgv);
    } catch (const std::exception& e) {
        LOG(L"SvcMain failed: %S", e.what());
    }
}

namespace Service {

void Run() {
    auto serviceName{std::to_array(ServiceCommon::kName)};

    SERVICE_TABLE_ENTRY DispatchTable[] = {{serviceName.data(), SvcMain},
                                           {nullptr, nullptr}};

    THROW_IF_WIN32_BOOL_FALSE(StartServiceCtrlDispatcher(DispatchTable));
}

bool IsRunning(bool waitIfStarting) {
    wil::unique_schandle scManager(
        OpenSCManager(nullptr,  // local computer
                      nullptr,  // ServicesActive database
                      0));
    THROW_LAST_ERROR_IF_NULL(scManager);

    wil::unique_schandle service(OpenService(
        scManager.get(), ServiceCommon::kName, SERVICE_QUERY_STATUS));
    THROW_LAST_ERROR_IF_NULL(service);

    SERVICE_STATUS_PROCESS ssp;
    DWORD dwBytesNeeded;

    THROW_IF_WIN32_BOOL_FALSE(QueryServiceStatusEx(
        service.get(), SC_STATUS_PROCESS_INFO, reinterpret_cast<BYTE*>(&ssp),
        sizeof(ssp), &dwBytesNeeded));

    switch (ssp.dwCurrentState) {
        case SERVICE_RUNNING:
            return true;

        case SERVICE_START_PENDING:
            if (!waitIfStarting) {
                return false;
            }
            break;

        default:
            return false;
    }

    constexpr DWORD kStartStopTimeout = 30000;

    // Save the tick count and initial checkpoint.
    DWORD dwStartTickCount = GetTickCount();
    DWORD dwOldCheckPoint = ssp.dwCheckPoint;

    while (ssp.dwCurrentState == SERVICE_START_PENDING) {
        if (ssp.dwCheckPoint > dwOldCheckPoint) {
            // Continue to wait and check
            dwStartTickCount = GetTickCount();
            dwOldCheckPoint = ssp.dwCheckPoint;
        } else {
            if (GetTickCount() - dwStartTickCount > kStartStopTimeout) {
                // Timeout.
                break;
            }
        }

        // Do not wait longer than the wait hint. A good interval is
        // one-tenth the wait hint, but no less than 1 second and no
        // more than 10 seconds.

        DWORD dwWaitTime = ssp.dwWaitHint / 10;

        // if (dwWaitTime < 1000)
        //     dwWaitTime = 1000;
        // else if (dwWaitTime > 10000)
        //     dwWaitTime = 10000;

        // 200-1000 ms for better responsiveness.
        if (dwWaitTime < 200)
            dwWaitTime = 200;
        else if (dwWaitTime > 1000)
            dwWaitTime = 1000;

        Sleep(dwWaitTime);

        // Check the status again.
        THROW_IF_WIN32_BOOL_FALSE(QueryServiceStatusEx(
            service.get(),                  // handle to service
            SC_STATUS_PROCESS_INFO,         // info level
            reinterpret_cast<BYTE*>(&ssp),  // address of structure
            sizeof(ssp),                    // size of structure
            &dwBytesNeeded));               // if buffer too small
    }

    return ssp.dwCurrentState == SERVICE_RUNNING;
}

void Start() {
    wil::unique_schandle scManager(
        OpenSCManager(nullptr,  // local computer
                      nullptr,  // ServicesActive database
                      0));
    THROW_LAST_ERROR_IF_NULL(scManager);

    wil::unique_schandle service(
        OpenService(scManager.get(), ServiceCommon::kName,
                    SERVICE_START | SERVICE_CHANGE_CONFIG));
    THROW_LAST_ERROR_IF_NULL(service);

    if (!StartService(service.get(), 0, nullptr)) {
        THROW_LAST_ERROR_IF(GetLastError() != ERROR_SERVICE_ALREADY_RUNNING);
    }

    // Change start type to autostart.
    THROW_IF_WIN32_BOOL_FALSE(
        ChangeServiceConfig(service.get(),
                            SERVICE_NO_CHANGE,   // service type
                            SERVICE_AUTO_START,  // start type
                            SERVICE_NO_CHANGE,   // error control type
                            nullptr,             // path to service's binary
                            nullptr,             // no load ordering group
                            nullptr,             // no tag identifier
                            nullptr,             // no dependencies
                            nullptr,             // LocalSystem account
                            nullptr,             // no password
                            nullptr));           // service name to display
}

void Stop(bool disableAutoStart) {
    wil::unique_schandle scManager(
        OpenSCManager(nullptr,  // local computer
                      nullptr,  // ServicesActive database
                      0));
    THROW_LAST_ERROR_IF_NULL(scManager);

    wil::unique_schandle service(
        OpenService(scManager.get(), ServiceCommon::kName,
                    SERVICE_STOP | SERVICE_CHANGE_CONFIG));
    THROW_LAST_ERROR_IF_NULL(service);

    SERVICE_STATUS serviceStatus;
    if (!ControlService(service.get(), SERVICE_CONTROL_STOP, &serviceStatus)) {
        THROW_LAST_ERROR_IF(GetLastError() != ERROR_SERVICE_NOT_ACTIVE);
    }

    // Change start type.
    if (disableAutoStart) {
        THROW_IF_WIN32_BOOL_FALSE(
            ChangeServiceConfig(service.get(),
                                SERVICE_NO_CHANGE,     // service type
                                SERVICE_DEMAND_START,  // start type
                                SERVICE_NO_CHANGE,     // error control type
                                nullptr,    // path to service's binary
                                nullptr,    // no load ordering group
                                nullptr,    // no tag identifier
                                nullptr,    // no dependencies
                                nullptr,    // LocalSystem account
                                nullptr,    // no password
                                nullptr));  // service name to display
    }
}

}  // namespace Service
