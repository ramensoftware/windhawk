#include "stdafx.h"

#include "all_processes_injector.h"
#include "customization_session.h"
#include "dll_inject.h"
#include "logger.h"
#include "no_destructor.h"
#include "storage_manager.h"

HINSTANCE g_hDllInst;

BOOL APIENTRY DllMain(HINSTANCE hinstDLL, DWORD fdwReason, LPVOID lpvReserved) {
    switch (fdwReason) {
        case DLL_PROCESS_ATTACH:
            g_hDllInst = hinstDLL;

#ifndef _WIN64
            Wow64ExtInitialize();
#endif  // _WIN64
            break;

        case DLL_THREAD_ATTACH:
        case DLL_THREAD_DETACH:
            break;

        case DLL_PROCESS_DETACH:
            // From the documentation: If the process is terminating (the
            // lpvReserved parameter is non-NULL), all threads in the process
            // except the current thread either have exited already or have been
            // explicitly terminated by a call to the ExitProcess function.
            if (lpvReserved) {
                NoDestructorIfTerminatingBase::SetProcessTerminating();
            }
            break;
    }

    return TRUE;
}

bool LazyInitialize() {
    try {
        // Make sure we can get an instance.
        // If not, this call will throw an exception.
        StorageManager::GetInstance();
        return true;
    } catch (const std::exception& e) {
        LOG(L"Initialization failed: %S", e.what());
        return false;
    }
}

// Exported
BOOL InjectInit(const DllInject::LOAD_LIBRARY_REMOTE_DATA* pInjData) {
    if (!LazyInitialize()) {
        return FALSE;
    }

    VERBOSE(L"Running InjectInit");

    if (WaitForSingleObject(pInjData->hSessionManagerProcess, 0) ==
        WAIT_OBJECT_0) {
        VERBOSE(L"Session manager process is no longer running");
        return FALSE;
    }

    try {
        CustomizationSession::Start(
            pInjData->bRunningFromAPC, pInjData->bThreadAttachExempt,
            pInjData->hSessionManagerProcess, pInjData->hSessionMutex);
        return TRUE;
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }

    return FALSE;
}

// Exported
HANDLE GlobalHookSessionStart(bool skipCriticalProcesses) {
    if (!LazyInitialize()) {
        return nullptr;
    }

    VERBOSE(L"Running GlobalHookSessionStart");

    try {
        return static_cast<HANDLE>(
            new AllProcessesInjector(skipCriticalProcesses));
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }

    return nullptr;
}

// Exported
BOOL GlobalHookSessionHandleNewProcesses(HANDLE hSession) {
    if (!LazyInitialize()) {
        return FALSE;
    }

    // VERBOSE(L"Running GlobalHookSessionHandleNewProcesses");

    auto allProcessInjector = static_cast<AllProcessesInjector*>(hSession);
    allProcessInjector->InjectIntoNewProcesses();
    return TRUE;
}

// Exported
BOOL GlobalHookSessionEnd(HANDLE hSession) {
    if (!LazyInitialize()) {
        return FALSE;
    }

    VERBOSE(L"Running GlobalHookSessionEnd");

    auto allProcessInjector = static_cast<AllProcessesInjector*>(hSession);
    delete allProcessInjector;

    return TRUE;
}
