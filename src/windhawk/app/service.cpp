#include "stdafx.h"

#include "service.h"

#include "engine_control.h"
#include "functions.h"
#include "logger.h"
#include "service_common.h"
#include "session_processes.h"
#include "storage_manager.h"
#include "version.h"

namespace {

// Ending the engine session unloads the engine from every process it's in,
// which takes a while, so every report of a pending stop carries an estimate
// that lets the SCM tell it apart from a stop which is stuck.
constexpr DWORD kStopWaitHint = 30000;

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

// Launches the Windhawk daemon on every logged-on session, each on that
// session's own user token. A daemon opens the UI unless started with
// -tray-only, so the session named by runUiSessionId gets one without it, which
// is how an elevated caller has the UI opened unelevated.
void CreateDaemonOnAllSessions(std::optional<DWORD> runUiSessionId) {
    auto modulePath = wil::GetModuleFileName<std::wstring>();

    for (DWORD sessionId : Functions::GetLoggedOnSessionIds()) {
        // A session which can't be launched into doesn't stop the others.
        try {
            std::wstring commandLine = L"\"" + modulePath + L"\"";
            if (runUiSessionId != sessionId) {
                commandLine += L" -tray-only";
            }

            Functions::CreateProcessOnSessionId(sessionId, modulePath.c_str(),
                                                commandLine.data());
        } catch (const std::exception& e) {
            LOG(L"Creating the daemon on session %u failed: %S", sessionId,
                e.what());
        }
    }
}

// The session id passed to StartService by a caller which wants the UI opened.
// A service start argument, not a command line flag: it reaches ServiceMain and
// never appears on this process's command line.
std::optional<DWORD> GetRunUiSessionId(DWORD dwArgc, LPTSTR* lpszArgv) {
    // lpszArgv[0] is the service name.
    for (DWORD i = 1; i + 1 < dwArgc; i++) {
        if (_wcsicmp(lpszArgv[i], L"-run-ui-session") == 0) {
            return static_cast<DWORD>(wcstoul(lpszArgv[i + 1], nullptr, 10));
        }
    }

    return std::nullopt;
}

}  // namespace

class ServiceInstance {
   public:
    VOID SvcMain(DWORD dwArgc, LPTSTR* lpszArgv);

   private:
    VOID SvcInit(DWORD dwArgc, LPTSTR* lpszArgv);
    VOID SvcRun(DWORD dwArgc, LPTSTR* lpszArgv);
    VOID SvcStop(DWORD dwWin32ExitCode);
    VOID ReportSvcStatus(DWORD dwCurrentState,
                         DWORD dwWin32ExitCode,
                         DWORD dwWaitHint);
    VOID SetControlEnabled(bool enabled);
    static DWORD WINAPI SvcCtrlHandlerExThunk(DWORD dwControl,
                                              DWORD dwEventType,
                                              LPVOID lpEventData,
                                              LPVOID lpContext);
    DWORD SvcCtrlHandlerEx(DWORD dwControl,
                           DWORD dwEventType,
                           LPVOID lpEventData);
    VOID QueueSessionChange(DWORD dwSessionId, bool loggedOn);
    VOID HandlePendingSessionChanges();
    VOID HandleSessionLogon(DWORD dwSessionId);
    VOID HandleSessionLogoff(DWORD dwSessionId);

    SERVICE_STATUS_HANDLE m_svcStatusHandle{};
    DWORD m_dwCheckPoint = 1;

    // The control handler runs on an SCM thread and stays registered for as
    // long as the process lives, so it can be entered before SvcInit created
    // the resources below and after they were released. It only touches them
    // while m_controlEnabled is set, which SvcMain switches under
    // m_controlLock, waiting there for a handler already in flight.
    wil::srwlock m_controlLock;
    bool m_controlEnabled = false;

    // Everything SvcInit brings up, kept as one object so that releasing it
    // takes no list of members to keep in sync: dropping it closes all of them,
    // in reverse order of creation.
    struct SvcResources {
        wil::unique_handle infoFileMapping;
        wil::unique_mutex mutex;
        wil::mutex_release_scope_exit mutexLock;
        wil::unique_event stopEvent;
        wil::unique_event scanForProcessesEvent;
        wil::unique_event emergencyStopEvent;
        wil::unique_event safeModeStopEvent;
        wil::unique_event sessionChangeEvent;
        std::optional<EngineControl> engineControl;

        struct SessionChange {
            DWORD sessionId;
            bool loggedOn;
        };

        // The logons and logoffs which have yet to be served, in the order
        // they happened. Filled by the control handler, drained by the main
        // loop, hence the lock.
        wil::srwlock pendingSessionChangesLock;
        std::vector<SessionChange> pendingSessionChanges;
    };

    std::optional<SvcResources> m_svcResources;
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
        SvcStop(wil::ResultFromCaughtException());
        return;
    }

    // Everything the control handler uses exists from here until the matching
    // call below.
    SetControlEnabled(true);

    // Report running status when initialization is complete.
    ReportSvcStatus(SERVICE_RUNNING, NO_ERROR, 0);

    DWORD exitCode = NO_ERROR;
    try {
        VERBOSE(L"Running SvcRun");
        SvcRun(dwArgc, lpszArgv);
    } catch (const std::exception& e) {
        LOG(L"SvcRun failed: %S", e.what());
        exitCode = wil::ResultFromCaughtException();
    }

    SetControlEnabled(false);

    SvcStop(exitCode);
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

    auto& resources = m_svcResources.emplace();

    resources.infoFileMapping.reset(CreateServiceInfoFileMapping());

    resources.mutex.reset(CreateServiceMutex());

    resources.mutexLock = resources.mutex.ReleaseMutex_scope_exit();

    // Create an event. The control handler function, SvcCtrlHandler,
    // signals this event when it receives the stop control code.
    resources.stopEvent.reset(
        CreateEvent(nullptr,    // default security attributes
                    TRUE,       // manual reset event
                    FALSE,      // not signaled
                    nullptr));  // no name
    THROW_LAST_ERROR_IF_NULL(resources.stopEvent);

    resources.scanForProcessesEvent.reset(
        Functions::CreateEventForMediumIntegrity(
            ServiceCommon::kScanForProcessesEventName, FALSE));
    THROW_LAST_ERROR_IF_NULL(resources.scanForProcessesEvent);

    resources.emergencyStopEvent.reset(Functions::CreateEventForMediumIntegrity(
        ServiceCommon::kEmergencyStopEventName, TRUE));
    THROW_LAST_ERROR_IF_NULL(resources.emergencyStopEvent);

    resources.safeModeStopEvent.reset(Functions::CreateEventForMediumIntegrity(
        ServiceCommon::kSafeModeStopEventName, TRUE));
    THROW_LAST_ERROR_IF_NULL(resources.safeModeStopEvent);

    // Wakes the main loop when a logon or a logoff is queued, so that it
    // doesn't wait out its timeout first.
    resources.sessionChangeEvent.reset(
        CreateEvent(nullptr, FALSE, FALSE, nullptr));
    THROW_LAST_ERROR_IF_NULL(resources.sessionChangeEvent);

    auto settings =
        StorageManager::GetInstance().GetAppConfig(L"Settings", false);

    if (!settings->GetInt(L"SafeMode").value_or(0)) {
        resources.engineControl.emplace();
        resources.engineControl->HandleNewProcesses();
    }
}

//
// Purpose:
//   The service code
//
VOID ServiceInstance::SvcRun(DWORD dwArgc, LPTSTR* lpszArgv) {
    // TO_DO: Perform work until service stops.

    try {
        CreateDaemonOnAllSessions(GetRunUiSessionId(dwArgc, lpszArgv));
    } catch (const std::exception& e) {
        LOG(L"CreateDaemonOnAllSessions failed: %S", e.what());
    }

    auto& resources = *m_svcResources;

    // The engine recorded the session users as it started, and session changes
    // only reach a running service, so a logon or a logoff in between has no
    // notification to come.
    if (resources.engineControl) {
        resources.engineControl->RecordSessionUsers();
    }

    HANDLE events[] = {
        resources.stopEvent.get(),
        resources.scanForProcessesEvent.get(),
        resources.emergencyStopEvent.get(),
        resources.safeModeStopEvent.get(),
        resources.sessionChangeEvent.get(),
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

            case WAIT_OBJECT_0 + 4:
                VERBOSE(L"Received session change event");

                keepLooping = true;
                break;

            default:
                LOG(L"Received unknown event %u", dwWaitResult);
                break;
        }

        if (!keepLooping) {
            break;
        }

        HandlePendingSessionChanges();

        if (resources.engineControl) {
            resources.engineControl->HandleNewProcesses();
        }
    }
}

//
// Purpose:
//   Releases everything SvcInit brought up, then reports SERVICE_STOPPED.
//
// Parameters:
//   dwWin32ExitCode - The error the service stops with, NO_ERROR for a clean
//     stop
//
// Return value:
//   None
//
VOID ServiceInstance::SvcStop(DWORD dwWin32ExitCode) {
    // The first report of a pending stop on the paths which stop without a stop
    // control, such as the emergency stop event.
    ReportSvcStatus(SERVICE_STOP_PENDING, NO_ERROR, kStopWaitHint);

    // Nothing may outlive the stop below: the SCM lets the next start create a
    // service process as soon as the service is reported as stopped, and that
    // process fails while this one still holds the named mutex and the info
    // file mapping.
    m_svcResources.reset();

    VERBOSE(L"Reporting SERVICE_STOPPED");
    ReportSvcStatus(SERVICE_STOPPED, dwWin32ExitCode, 0);
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

    if (dwCurrentState != SERVICE_START_PENDING)
        SvcStatus.dwControlsAccepted |= SERVICE_ACCEPT_STOP;

    // Session changes are only of use while running: a logon reported earlier
    // or later has no engine to be handed to, and the daemon it launches would
    // find no service mutex and info file mapping to attach to.
    if (dwCurrentState == SERVICE_RUNNING)
        SvcStatus.dwControlsAccepted |= SERVICE_ACCEPT_SESSIONCHANGE;

    if (dwCurrentState == SERVICE_RUNNING || dwCurrentState == SERVICE_STOPPED)
        SvcStatus.dwCheckPoint = 0;
    else
        SvcStatus.dwCheckPoint = m_dwCheckPoint++;

    // Report the status of the service to the SCM.
    SetServiceStatus(m_svcStatusHandle, &SvcStatus);
}

//
// Purpose:
//   Opens and closes the window in which the control handler may use the
//   members SvcInit created. Waits for a handler already inside it.
//
VOID ServiceInstance::SetControlEnabled(bool enabled) {
    auto lock = m_controlLock.lock_exclusive();
    m_controlEnabled = enabled;
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
        case SERVICE_CONTROL_STOP: {
            VERBOSE("Handling SERVICE_CONTROL_STOP");

            auto lock = m_controlLock.lock_exclusive();
            if (m_controlEnabled) {
                ReportSvcStatus(SERVICE_STOP_PENDING, NO_ERROR, kStopWaitHint);

                // Signal the service to stop.
                SetEvent(m_svcResources->stopEvent.get());
            }
            return NO_ERROR;
        }

        case SERVICE_CONTROL_SESSIONCHANGE: {
            bool loggedOn;
            if (dwEventType == WTS_SESSION_LOGON) {
                VERBOSE("Handling WTS_SESSION_LOGON");
                loggedOn = true;
            } else if (dwEventType == WTS_SESSION_LOGOFF) {
                VERBOSE("Handling WTS_SESSION_LOGOFF");
                loggedOn = false;
            } else {
                return NO_ERROR;
            }

            auto sessionNotification =
                reinterpret_cast<const WTSSESSION_NOTIFICATION*>(lpEventData);

            try {
                QueueSessionChange(sessionNotification->dwSessionId, loggedOn);
            } catch (const std::exception& e) {
                LOG(L"Session change handler failed: %S", e.what());
            }
            return NO_ERROR;
        }

        case SERVICE_CONTROL_INTERROGATE:
            return NO_ERROR;

        default:
            return ERROR_CALL_NOT_IMPLEMENTED;
    }
}

//
// Purpose:
//   Notes a logon or a logoff for the main loop to serve and wakes it. Serving
//   a logon means launching processes, which takes longer than a control
//   handler may spend before returning.
//
VOID ServiceInstance::QueueSessionChange(DWORD dwSessionId, bool loggedOn) {
    auto lock = m_controlLock.lock_exclusive();
    if (!m_controlEnabled) {
        return;
    }

    auto& resources = *m_svcResources;

    {
        auto pendingLock = resources.pendingSessionChangesLock.lock_exclusive();
        resources.pendingSessionChanges.push_back({dwSessionId, loggedOn});
    }

    resources.sessionChangeEvent.SetEvent();
}

//
// Purpose:
//   Serves the logons and logoffs queued since the last sweep, in order. One
//   which can't be served doesn't stop the others, and isn't queued again.
//
VOID ServiceInstance::HandlePendingSessionChanges() {
    auto& resources = *m_svcResources;

    std::vector<SvcResources::SessionChange> changes;

    {
        auto lock = resources.pendingSessionChangesLock.lock_exclusive();
        changes.swap(resources.pendingSessionChanges);
    }

    for (const auto& change : changes) {
        try {
            if (change.loggedOn) {
                HandleSessionLogon(change.sessionId);
            } else {
                HandleSessionLogoff(change.sessionId);
            }
        } catch (const std::exception& e) {
            LOG(L"Handling the %s of session %u failed: %S",
                change.loggedOn ? L"logon" : L"logoff", change.sessionId,
                e.what());
        }
    }
}

//
// Purpose:
//   Gives a session which has just gained a user what every logged-on session
//   gets at startup: its user recorded for its system processes to take the
//   user's mod configurations, a daemon, and the tool mod hosts the engine
//   launches. The users of the other sessions are recorded again with it,
//   which catches up on a sign-in or a sign-out that sent no notification.
//
VOID ServiceInstance::HandleSessionLogon(DWORD dwSessionId) {
    auto& resources = *m_svcResources;

    if (resources.engineControl) {
        resources.engineControl->RecordSessionLogon(dwSessionId);
    }

    if (!Functions::IsSessionLoggedOn(dwSessionId)) {
        return;
    }

    // A daemon which can't be launched doesn't hold back the hosts.
    try {
        auto modulePath = wil::GetModuleFileName<std::wstring>();
        auto commandLine = L"\"" + modulePath + L"\" -tray-only";
        Functions::CreateProcessOnSessionId(dwSessionId, modulePath.c_str(),
                                            commandLine.data());
    } catch (const std::exception& e) {
        LOG(L"Creating the daemon on session %u failed: %S", dwSessionId,
            e.what());
    }

    if (resources.engineControl) {
        resources.engineControl->HandleNewLogonSession(dwSessionId);
    }
}

//
// Purpose:
//   Forgets the user of a session they signed out of, so that a later session
//   given the same id doesn't take their mod configurations before its own
//   user signs in. The users of the other sessions are recorded again, as on a
//   logon.
//
VOID ServiceInstance::HandleSessionLogoff(DWORD dwSessionId) {
    auto& resources = *m_svcResources;

    if (resources.engineControl) {
        // A session can still name its user while it signs out, so it's
        // forgotten after the recording.
        resources.engineControl->RecordSessionUsers();
        resources.engineControl->RemoveSessionUser(dwSessionId);
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

namespace {

SERVICE_STATUS_PROCESS QueryStatus(SC_HANDLE service) {
    SERVICE_STATUS_PROCESS ssp;
    DWORD dwBytesNeeded;

    THROW_IF_WIN32_BOOL_FALSE(QueryServiceStatusEx(
        service,                        // handle to service
        SC_STATUS_PROCESS_INFO,         // info level
        reinterpret_cast<BYTE*>(&ssp),  // address of structure
        sizeof(ssp),                    // size of structure
        &dwBytesNeeded));               // if buffer too small

    return ssp;
}

// Polls a service which is starting until it settles on another state, telling
// a start which makes progress from one which is stuck by the checkpoint and
// the wait hint it reports. Returns the state it settled on, which is still
// SERVICE_START_PENDING if it ran out of time. A service which isn't starting
// is reported as it is, without waiting.
DWORD WaitWhileStartPending(SC_HANDLE service, SERVICE_STATUS_PROCESS ssp) {
    constexpr DWORD kStartTimeout = 30000;

    // Save the tick count and initial checkpoint.
    DWORD dwStartTickCount = GetTickCount();
    DWORD dwOldCheckPoint = ssp.dwCheckPoint;

    while (ssp.dwCurrentState == SERVICE_START_PENDING) {
        if (ssp.dwCheckPoint > dwOldCheckPoint) {
            // Continue to wait and check
            dwStartTickCount = GetTickCount();
            dwOldCheckPoint = ssp.dwCheckPoint;
        } else {
            if (GetTickCount() - dwStartTickCount > kStartTimeout) {
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
        ssp = QueryStatus(service);
    }

    return ssp.dwCurrentState;
}

}  // namespace

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

    SERVICE_STATUS_PROCESS ssp = QueryStatus(service.get());

    if (!waitIfStarting) {
        return ssp.dwCurrentState == SERVICE_RUNNING;
    }

    return WaitWhileStartPending(service.get(), ssp) == SERVICE_RUNNING;
}

bool Start(std::optional<DWORD> runUiSessionId) {
    wil::unique_schandle scManager(
        OpenSCManager(nullptr,  // local computer
                      nullptr,  // ServicesActive database
                      0));
    THROW_LAST_ERROR_IF_NULL(scManager);

    wil::unique_schandle service(
        OpenService(scManager.get(), ServiceCommon::kName,
                    SERVICE_START | SERVICE_CHANGE_CONFIG));
    THROW_LAST_ERROR_IF_NULL(service);

    std::wstring sessionId;
    LPCWSTR serviceArgs[2];
    DWORD serviceArgsCount = 0;
    if (runUiSessionId) {
        sessionId = std::to_wstring(*runUiSessionId);
        serviceArgs[serviceArgsCount++] = L"-run-ui-session";
        serviceArgs[serviceArgsCount++] = sessionId.c_str();
    }

    bool started = true;
    if (!StartService(service.get(), serviceArgsCount,
                      serviceArgsCount ? serviceArgs : nullptr)) {
        THROW_LAST_ERROR_IF(GetLastError() != ERROR_SERVICE_ALREADY_RUNNING);

        // The arguments only reach a start which actually happens, so tell a
        // caller which asked for the UI that nothing will open it.
        started = false;
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

    return started;
}

void Stop(bool disableAutoStart) {
    wil::unique_schandle scManager(
        OpenSCManager(nullptr,  // local computer
                      nullptr,  // ServicesActive database
                      0));
    THROW_LAST_ERROR_IF_NULL(scManager);

    wil::unique_schandle service(OpenService(
        scManager.get(), ServiceCommon::kName,
        SERVICE_STOP | SERVICE_QUERY_STATUS | SERVICE_CHANGE_CONFIG));
    THROW_LAST_ERROR_IF_NULL(service);

    // A service which is still starting rejects the stop control, and is left
    // running once the start completes, so let the start finish first.
    DWORD state =
        WaitWhileStartPending(service.get(), QueryStatus(service.get()));

    // A start which is stuck leaves nothing that can be stopped.
    THROW_WIN32_IF(ERROR_SERVICE_CANNOT_ACCEPT_CTRL,
                   state == SERVICE_START_PENDING);

    if (state != SERVICE_STOPPED && state != SERVICE_STOP_PENDING) {
        SERVICE_STATUS serviceStatus;
        if (!ControlService(service.get(), SERVICE_CONTROL_STOP,
                            &serviceStatus)) {
            DWORD error = GetLastError();
            // The service can have stopped, or begun stopping, since the state
            // above was queried.
            THROW_WIN32_IF(error,
                           error != ERROR_SERVICE_NOT_ACTIVE &&
                               error != ERROR_SERVICE_CANNOT_ACCEPT_CTRL);
        }
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
