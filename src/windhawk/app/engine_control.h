#pragma once

class EngineControl {
   public:
    EngineControl(bool skipCriticalProcesses);
    ~EngineControl();

    EngineControl(const EngineControl&) = delete;
    EngineControl(EngineControl&&) = delete;
    EngineControl& operator=(const EngineControl&) = delete;
    EngineControl& operator=(EngineControl&&) = delete;

    BOOL HandleNewProcesses();

   private:
    using GLOBAL_HOOK_SESSION_START = HANDLE (*)(bool skipCriticalProcesses);
    using GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES = BOOL (*)(HANDLE hSession);
    using GLOBAL_HOOK_SESSION_END = BOOL (*)(HANDLE hSession);

    wil::unique_hmodule engineModule;
    GLOBAL_HOOK_SESSION_START pGlobalHookSessionStart;
    GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES
        pGlobalHookSessionHandleNewProcesses;
    GLOBAL_HOOK_SESSION_END pGlobalHookSessionEnd;
    HANDLE hGlobalHookSession;
};
