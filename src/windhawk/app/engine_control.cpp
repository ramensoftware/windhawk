#include "stdafx.h"

#include "engine_control.h"

#include "storage_manager.h"

EngineControl::EngineControl(bool skipCriticalProcesses) {
    auto engineLibraryPath =
        StorageManager::GetInstance().GetEnginePath() / L"windhawk.dll";

    engineModule.reset(LoadLibrary(engineLibraryPath.c_str()));
    THROW_LAST_ERROR_IF_NULL(engineModule);

    pGlobalHookSessionStart = reinterpret_cast<GLOBAL_HOOK_SESSION_START>(
        GetProcAddress(engineModule.get(), "GlobalHookSessionStart"));
    THROW_LAST_ERROR_IF_NULL(pGlobalHookSessionStart);

    pGlobalHookSessionHandleNewProcesses =
        reinterpret_cast<GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES>(
            GetProcAddress(engineModule.get(),
                           "GlobalHookSessionHandleNewProcesses"));
    THROW_LAST_ERROR_IF_NULL(pGlobalHookSessionHandleNewProcesses);

    pGlobalHookSessionEnd = reinterpret_cast<GLOBAL_HOOK_SESSION_END>(
        GetProcAddress(engineModule.get(), "GlobalHookSessionEnd"));
    THROW_LAST_ERROR_IF_NULL(pGlobalHookSessionEnd);

    hGlobalHookSession = pGlobalHookSessionStart(skipCriticalProcesses);
    if (!hGlobalHookSession) {
        throw std::runtime_error("Failed to start the global hooking session");
    }
}

EngineControl::~EngineControl() {
    pGlobalHookSessionEnd(hGlobalHookSession);
}

BOOL EngineControl::HandleNewProcesses() {
    return pGlobalHookSessionHandleNewProcesses(hGlobalHookSession);
}
