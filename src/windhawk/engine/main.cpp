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

    // Acquire handles to make sure we'll close them and not the caller, since
    // we return TRUE from now on.
    wil::unique_process_handle sessionManagerProcess(
        pInjData->hSessionManagerProcess);
    wil::unique_mutex_nothrow sessionMutex(pInjData->hSessionMutex);

    try {
        CustomizationSession::Start(
            pInjData->bRunningFromAPC, pInjData->bThreadAttachExempt,
            std::move(sessionManagerProcess), std::move(sessionMutex));
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }

    return TRUE;
}

// Exported
HANDLE GlobalHookSessionStart() {
// Only used by the x86 background process.
#ifdef _M_IX86
    if (!LazyInitialize()) {
        return nullptr;
    }

    VERBOSE(L"Running GlobalHookSessionStart");

    try {
        return static_cast<HANDLE>(new AllProcessesInjector());
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }
#endif  // _M_IX86

    return nullptr;
}

// Exported
BOOL GlobalHookSessionHandleNewProcesses(HANDLE hSession) {
#ifdef _M_IX86
    if (!LazyInitialize()) {
        return FALSE;
    }

    // VERBOSE(L"Running GlobalHookSessionHandleNewProcesses");

    auto allProcessInjector = static_cast<AllProcessesInjector*>(hSession);
    allProcessInjector->InjectIntoNewProcesses();
    return TRUE;
#else
	return FALSE;
#endif  // _M_IX86
}

// Exported
BOOL GlobalHookSessionEnd(HANDLE hSession) {
#ifdef _M_IX86
    if (!LazyInitialize()) {
        return FALSE;
    }

    VERBOSE(L"Running GlobalHookSessionEnd");

    auto allProcessInjector = static_cast<AllProcessesInjector*>(hSession);
    delete allProcessInjector;

    return TRUE;
#else
    return FALSE;
#endif  // _M_IX86
}
