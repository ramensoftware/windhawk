#pragma once

class EngineControl {
   public:
    EngineControl();
    ~EngineControl();

    EngineControl(const EngineControl&) = delete;
    EngineControl(EngineControl&&) = delete;
    EngineControl& operator=(const EngineControl&) = delete;
    EngineControl& operator=(EngineControl&&) = delete;

    BOOL HandleNewProcesses();

    // Tells the engine about a session which has just gained a user, so that
    // it launches the tool mod hosts the session is due.
    BOOL HandleNewLogonSession(DWORD sessionId);

   private:
    using GLOBAL_HOOK_SESSION_START = HANDLE (*)();
    using GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES = BOOL (*)(HANDLE hSession);
    using HANDLE_NEW_LOGON_SESSION = BOOL (*)(DWORD dwSessionId);
    using GLOBAL_HOOK_SESSION_END = BOOL (*)(HANDLE hSession);

    wil::unique_hmodule engineModule;
    GLOBAL_HOOK_SESSION_START pGlobalHookSessionStart;
    GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES
        pGlobalHookSessionHandleNewProcesses;
    HANDLE_NEW_LOGON_SESSION pHandleNewLogonSession;
    GLOBAL_HOOK_SESSION_END pGlobalHookSessionEnd;
    HANDLE hGlobalHookSession;
};
