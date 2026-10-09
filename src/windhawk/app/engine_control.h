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

    // Tells the engine that nobody is signed in to a session any more, so that
    // the system processes of a later session given its id don't take the
    // last user's mod configurations.
    BOOL RemoveSessionUser(DWORD sessionId);

    // Tells the engine who is signed in to every session, for the system
    // processes of each to take that user's mod configurations.
    BOOL RecordSessionUsers();

    // Like RecordSessionUsers, after a logon to a session.
    BOOL RecordSessionLogon(DWORD sessionId);

   private:
    using GLOBAL_HOOK_SESSION_START = HANDLE (*)();
    using GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES = BOOL (*)(HANDLE hSession);
    using GLOBAL_HOOK_SESSION_SESSION_USER = BOOL (*)(HANDLE hSession,
                                                      DWORD dwSessionId);
    using GLOBAL_HOOK_SESSION_RECORD_SESSION_USERS = BOOL (*)(HANDLE hSession);
    using HANDLE_NEW_LOGON_SESSION = BOOL (*)(DWORD dwSessionId);
    using GLOBAL_HOOK_SESSION_END = BOOL (*)(HANDLE hSession);

    wil::unique_hmodule engineModule;
    GLOBAL_HOOK_SESSION_START pGlobalHookSessionStart;
    GLOBAL_HOOK_SESSION_HANDLE_NEW_PROCESSES
        pGlobalHookSessionHandleNewProcesses;
    GLOBAL_HOOK_SESSION_SESSION_USER pGlobalHookSessionRemoveSessionUser;
    GLOBAL_HOOK_SESSION_RECORD_SESSION_USERS
        pGlobalHookSessionRecordSessionUsers;
    GLOBAL_HOOK_SESSION_SESSION_USER pGlobalHookSessionRecordSessionLogon;
    HANDLE_NEW_LOGON_SESSION pHandleNewLogonSession;
    GLOBAL_HOOK_SESSION_END pGlobalHookSessionEnd;
    HANDLE hGlobalHookSession;
};
