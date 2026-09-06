#include "stdafx.h"

#include "session_processes.h"

#include "logger.h"
#include "shared_functions.h"
#include "var_init_once.h"

// wtsapi32.dll, userenv.dll and user32.dll are loaded on demand rather than
// imported, so that a module which links this file without calling into it
// doesn't load them: the engine is injected into every process, sandboxed ones
// included.

namespace {

using CreateProcessInternalW_t =
    BOOL(WINAPI*)(HANDLE hUserToken,
                  LPCWSTR lpApplicationName,
                  LPWSTR lpCommandLine,
                  LPSECURITY_ATTRIBUTES lpProcessAttributes,
                  LPSECURITY_ATTRIBUTES lpThreadAttributes,
                  BOOL bInheritHandles,
                  DWORD dwCreationFlags,
                  LPVOID lpEnvironment,
                  LPCWSTR lpCurrentDirectory,
                  LPSTARTUPINFOW lpStartupInfo,
                  LPPROCESS_INFORMATION lpProcessInformation,
                  PHANDLE hRestrictedUserToken);

// Resolved the way the engine resolves the function it hooks, kernelbase.dll
// before kernel32.dll, so that both land on the same address. Null when neither
// exports it.
CreateProcessInternalW_t GetCreateProcessInternalW() {
    GET_PROC_ADDRESS_ONCE(CreateProcessInternalW_t, pKernelBase,
                          L"kernelbase.dll", "CreateProcessInternalW");
    if (pKernelBase) {
        return pKernelBase;
    }

    GET_PROC_ADDRESS_ONCE(CreateProcessInternalW_t, pKernel32, L"kernel32.dll",
                          "CreateProcessInternalW");
    return pKernel32;
}

// Creates a process by calling CreateProcessInternalW, which CreateProcessW and
// CreateProcessAsUserW are wrappers over, directly, so that the launch passes
// through the engine's hook on it: emulated on ARM64, a wrapper reaches the
// ARM64EC body without passing through the x64 fast-forward stub the hook sits
// on. Falls back to the wrapper when the undocumented export can't be resolved,
// which costs the injection rather than the launch.
BOOL CreateProcessInternal(HANDLE token,
                           PCWSTR applicationName,
                           PWSTR commandLine,
                           DWORD creationFlags,
                           void* environment,
                           LPSTARTUPINFOW startupInfo,
                           LPPROCESS_INFORMATION processInformation) {
    CreateProcessInternalW_t pCreateProcessInternalW =
        GetCreateProcessInternalW();
    if (pCreateProcessInternalW) {
        return pCreateProcessInternalW(token, applicationName, commandLine,
                                       nullptr, nullptr, FALSE, creationFlags,
                                       environment, nullptr, startupInfo,
                                       processInformation, nullptr);
    }

    if (token) {
        return CreateProcessAsUser(token, applicationName, commandLine, nullptr,
                                   nullptr, FALSE, creationFlags, environment,
                                   nullptr, startupInfo, processInformation);
    }

    return CreateProcess(applicationName, commandLine, nullptr, nullptr, FALSE,
                         creationFlags, environment, nullptr, startupInfo,
                         processInformation);
}

// The process which owns the shell window, to parent a new process to it and
// take the desktop user's token from it, or null when the caller isn't elevated
// and already runs as that user.
wil::unique_process_handle OpenShellProcessIfElevated() {
    if (!Functions::IsCurrentProcessElevated()) {
        return nullptr;
    }

    using GetShellWindow_t = decltype(&GetShellWindow);
    using GetWindowThreadProcessId_t = decltype(&GetWindowThreadProcessId);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        GetShellWindow_t, pGetShellWindow, L"user32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "GetShellWindow");
    THROW_HR_IF_NULL(E_UNEXPECTED, pGetShellWindow);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        GetWindowThreadProcessId_t, pGetWindowThreadProcessId, L"user32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "GetWindowThreadProcessId");
    THROW_HR_IF_NULL(E_UNEXPECTED, pGetWindowThreadProcessId);

    HWND shellWindow = pGetShellWindow();
    DWORD shellProcessId = 0;
    if (shellWindow) {
        pGetWindowThreadProcessId(shellWindow, &shellProcessId);
    }

    if (!shellProcessId) {
        // Creating the process with the elevated caller's own rights instead is
        // not what was asked for.
        throw std::runtime_error("No shell process to take the user from");
    }

    // PROCESS_CREATE_PROCESS to parent the new process, and
    // PROCESS_QUERY_LIMITED_INFORMATION to reach the token its environment is
    // built from.
    wil::unique_process_handle shellProcess(
        OpenProcess(PROCESS_CREATE_PROCESS | PROCESS_QUERY_LIMITED_INFORMATION,
                    FALSE, shellProcessId));
    THROW_LAST_ERROR_IF_NULL(shellProcess);

    return shellProcess;
}

using WTSFreeMemory_t = decltype(&WTSFreeMemory);

WTSFreeMemory_t GetWTSFreeMemory() {
    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        WTSFreeMemory_t, pWTSFreeMemory, L"wtsapi32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "WTSFreeMemory");
    THROW_HR_IF_NULL(E_UNEXPECTED, pWTSFreeMemory);

    return pWTSFreeMemory;
}

// The user token a session logon leaves behind, which is the interactive,
// filtered one.
wil::unique_handle QuerySessionUserToken(DWORD sessionId) {
    using WTSQueryUserToken_t = decltype(&WTSQueryUserToken);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        WTSQueryUserToken_t, pWTSQueryUserToken, L"wtsapi32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "WTSQueryUserToken");
    THROW_HR_IF_NULL(E_UNEXPECTED, pWTSQueryUserToken);

    wil::unique_handle token;
    THROW_IF_WIN32_BOOL_FALSE(pWTSQueryUserToken(sessionId, &token));
    return token;
}

void DestroyUserEnvironmentBlock(void* environment) {
    using DestroyEnvironmentBlock_t = decltype(&DestroyEnvironmentBlock);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        DestroyEnvironmentBlock_t, pDestroyEnvironmentBlock, L"userenv.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "DestroyEnvironmentBlock");
    if (pDestroyEnvironmentBlock) {
        pDestroyEnvironmentBlock(environment);
    }
}

using unique_environment_block =
    wil::unique_any<void*,
                    decltype(&DestroyUserEnvironmentBlock),
                    DestroyUserEnvironmentBlock>;

// The environment of a user, built from their token, in the form
// CREATE_UNICODE_ENVIRONMENT takes.
unique_environment_block CreateUserEnvironmentBlock(HANDLE token) {
    using CreateEnvironmentBlock_t = decltype(&CreateEnvironmentBlock);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        CreateEnvironmentBlock_t, pCreateEnvironmentBlock, L"userenv.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "CreateEnvironmentBlock");
    THROW_HR_IF_NULL(E_UNEXPECTED, pCreateEnvironmentBlock);

    void* environment = nullptr;
    THROW_IF_WIN32_BOOL_FALSE(
        pCreateEnvironmentBlock(&environment, token, FALSE));
    return unique_environment_block(environment);
}

void CreateProcessAsUserWithEnvironment(HANDLE token,
                                        PCWSTR applicationName,
                                        PWSTR commandLine) {
    unique_environment_block environment = CreateUserEnvironmentBlock(token);

    STARTUPINFO startupInfo = {
        .cb = sizeof(STARTUPINFO),
        .dwFlags = STARTF_FORCEOFFFEEDBACK,
    };
    wil::unique_process_information processInfo;

    THROW_IF_WIN32_BOOL_FALSE(CreateProcessInternal(
        token, applicationName, commandLine,
        NORMAL_PRIORITY_CLASS | CREATE_UNICODE_ENVIRONMENT, environment.get(),
        &startupInfo, &processInfo));
}

// Enables one privilege, which must be present in the token. previousState,
// when given, receives what to hand back to AdjustTokenPrivileges to put it
// back.
void EnableTokenPrivilege(HANDLE token,
                          PCWSTR privilegeName,
                          TOKEN_PRIVILEGES* previousState = nullptr) {
    LUID luid;
    THROW_IF_WIN32_BOOL_FALSE(
        LookupPrivilegeValue(nullptr, privilegeName, &luid));

    TOKEN_PRIVILEGES privileges = {
        .PrivilegeCount = 1,
        .Privileges = {{.Luid = luid, .Attributes = SE_PRIVILEGE_ENABLED}},
    };
    DWORD previousStateSize = 0;
    THROW_IF_WIN32_BOOL_FALSE(AdjustTokenPrivileges(
        token, FALSE, &privileges, previousState ? sizeof(*previousState) : 0,
        previousState, previousState ? &previousStateSize : nullptr));

    // AdjustTokenPrivileges reports a privilege it couldn't assign through the
    // last error while still returning success.
    THROW_LAST_ERROR_IF(GetLastError() == ERROR_NOT_ALL_ASSIGNED);
}

// Enables one privilege for as long as it lives, so that a call which borrows
// one doesn't leave the process holding it.
class ScopedTokenPrivilege {
   public:
    ScopedTokenPrivilege(HANDLE token, PCWSTR privilegeName) : m_token(token) {
        EnableTokenPrivilege(token, privilegeName, &m_previousState);
    }

    ~ScopedTokenPrivilege() {
        AdjustTokenPrivileges(m_token, FALSE, &m_previousState, 0, nullptr,
                              nullptr);
    }

    ScopedTokenPrivilege(const ScopedTokenPrivilege&) = delete;
    ScopedTokenPrivilege& operator=(const ScopedTokenPrivilege&) = delete;

   private:
    HANDLE m_token;
    TOKEN_PRIVILEGES m_previousState = {};
};

// A primary token duplicated from another, ready to launch with.
wil::unique_handle DuplicatePrimaryToken(HANDLE token) {
    wil::unique_handle primaryToken;
    THROW_IF_WIN32_BOOL_FALSE(DuplicateTokenEx(
        token, TOKEN_ALL_ACCESS, nullptr, SecurityImpersonation, TokenPrimary,
        &primaryToken));
    return primaryToken;
}

// The full, elevated token behind a user token: what the filtered token links
// to under UAC, the token itself when it is already the full one, null when the
// user has no elevated token, as a standard user hasn't.
wil::unique_handle TryGetUserFullToken(HANDLE userToken) {
    switch (wil::get_token_information<TOKEN_ELEVATION_TYPE>(userToken)) {
        case TokenElevationTypeFull:
            return DuplicatePrimaryToken(userToken);

        case TokenElevationTypeLimited: {
            auto linked = wil::get_linked_token_information(userToken);
            return DuplicatePrimaryToken(linked.LinkedToken);
        }

        default:
            // UAC isn't filtering this token, so it is the only one there is.
            if (wil::get_token_information<TOKEN_ELEVATION>(userToken)
                    .TokenIsElevated) {
                return DuplicatePrimaryToken(userToken);
            }
            return nullptr;
    }
}

wil::unique_handle GetUserFullToken(HANDLE userToken) {
    wil::unique_handle fullToken = TryGetUserFullToken(userToken);
    if (!fullToken) {
        throw std::runtime_error("The session user has no elevated token");
    }

    return fullToken;
}

// The integrity level in a token's mandatory label.
DWORD GetTokenIntegrityLevel(HANDLE token) {
    auto label = wil::get_token_information<TOKEN_MANDATORY_LABEL>(token);
    PSID sid = label->Label.Sid;
    return *GetSidSubAuthority(sid, *GetSidSubAuthorityCount(sid) - 1);
}

void SetTokenIntegrityLevel(HANDLE token, DWORD integrityLevel) {
    SID_IDENTIFIER_AUTHORITY labelAuthority =
        SECURITY_MANDATORY_LABEL_AUTHORITY;

    BYTE sidBuffer[SECURITY_MAX_SID_SIZE];
    PSID sid = sidBuffer;
    THROW_IF_WIN32_BOOL_FALSE(InitializeSid(sid, &labelAuthority, 1));
    *GetSidSubAuthority(sid, 0) = integrityLevel;

    TOKEN_MANDATORY_LABEL label = {
        .Label = {.Sid = sid, .Attributes = SE_GROUP_INTEGRITY},
    };
    THROW_IF_WIN32_BOOL_FALSE(SetTokenInformation(
        token, TokenIntegrityLevel, &label, sizeof(label) + GetLengthSid(sid)));
}

// The integrity level AppInfo puts a UIAccess process at: High for a user who
// has a full token, one step up for one who hasn't. UIPI gates a window on the
// integrity level of whoever reaches for it, so the user's own level leaves the
// process below the windows UIAccess is asked for in order to drive; High for a
// user who can't elevate would hand over any elevated process which owns one.
DWORD UiAccessIntegrityLevel(HANDLE userToken) {
    if (TryGetUserFullToken(userToken)) {
        return SECURITY_MANDATORY_HIGH_RID;
    }

    DWORD integrityLevel = GetTokenIntegrityLevel(userToken) + 0x10;
    if (integrityLevel > SECURITY_MANDATORY_HIGH_RID) {
        integrityLevel = SECURITY_MANDATORY_HIGH_RID;
    }

    return integrityLevel;
}

// A primary token duplicated from a user token with UIAccess granted, the way
// AppInfo grants it. The current effective token must hold SE_TCB_NAME, which
// setting the flag needs, and an integrity level no lower than the one assigned
// here.
wil::unique_handle MakeUiAccessToken(HANDLE userToken) {
    wil::unique_handle uiAccessToken = DuplicatePrimaryToken(userToken);

    ULONG enableUiAccess = 1;
    THROW_IF_WIN32_BOOL_FALSE(SetTokenInformation(
        uiAccessToken.get(), TokenUIAccess, &enableUiAccess,
        sizeof(enableUiAccess)));

    SetTokenIntegrityLevel(uiAccessToken.get(),
                           UiAccessIntegrityLevel(userToken));

    return uiAccessToken;
}

// Whether the token's user is the local system account.
bool IsLocalSystemToken(HANDLE token) {
    auto user = wil::get_token_information<TOKEN_USER>(token);

    BYTE systemSid[SECURITY_MAX_SID_SIZE];
    DWORD systemSidSize = sizeof(systemSid);
    THROW_IF_WIN32_BOOL_FALSE(
        CreateWellKnownSid(WinLocalSystemSid, nullptr, systemSid, &systemSidSize));

    return EqualSid(user->User.Sid, systemSid);
}

// An impersonation token of the local system account, with the privileges a
// UIAccess launch needs but an elevated administrator lacks enabled on it.
//
// The privileges the launch borrows, SE_TCB_NAME chief among them, are the
// system account's, so the token is lifted out of a process already running as
// it. Where the service is available a UIAccess launch runs as the account and
// takes none of this; a portable daemon has no service to defer to. Scanning
// processes to duplicate a system token, and enabling SE_DEBUG_NAME to reach
// them, is textbook token theft and expected to trip endpoint detection on the
// machines this runs on, a known cost of the portable launch.
wil::unique_handle OpenSystemImpersonationToken() {
    // Opening a system process takes SE_DEBUG_NAME.
    wil::unique_handle processToken;
    std::optional<ScopedTokenPrivilege> debugPrivilege;
    if (OpenProcessToken(GetCurrentProcess(),
                         TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                         &processToken)) {
        try {
            debugPrivilege.emplace(processToken.get(), SE_DEBUG_NAME);
        } catch (const std::exception&) {
            // A caller which hasn't got it is left with the processes it can
            // already open.
        }
    }

    HANDLE snapshotHandle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    THROW_LAST_ERROR_IF(snapshotHandle == INVALID_HANDLE_VALUE);
    wil::unique_handle snapshot(snapshotHandle);

    PROCESSENTRY32 processEntry = {.dwSize = sizeof(processEntry)};
    for (BOOL ok = Process32First(snapshot.get(), &processEntry); ok;
         ok = Process32Next(snapshot.get(), &processEntry)) {
        wil::unique_process_handle process(OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processEntry.th32ProcessID));
        if (!process) {
            continue;
        }

        wil::unique_handle token;
        if (!OpenProcessToken(process.get(), TOKEN_DUPLICATE | TOKEN_QUERY,
                              &token)) {
            continue;
        }

        try {
            if (!IsLocalSystemToken(token.get())) {
                continue;
            }

            wil::unique_handle impersonationToken;
            THROW_IF_WIN32_BOOL_FALSE(DuplicateTokenEx(
                token.get(),
                TOKEN_IMPERSONATE | TOKEN_QUERY | TOKEN_DUPLICATE |
                    TOKEN_ADJUST_PRIVILEGES,
                nullptr, SecurityImpersonation, TokenImpersonation,
                &impersonationToken));

            EnableTokenPrivilege(impersonationToken.get(), SE_TCB_NAME);
            EnableTokenPrivilege(impersonationToken.get(),
                                 SE_ASSIGNPRIMARYTOKEN_NAME);
            EnableTokenPrivilege(impersonationToken.get(),
                                 SE_INCREASE_QUOTA_NAME);
            return impersonationToken;
        } catch (const std::exception&) {
            // This system process won't do; try the next one.
            continue;
        }
    }

    throw std::runtime_error("No system process token available");
}

}  // namespace

namespace Functions {

bool IsSessionLoggedOn(DWORD sessionId) {
    using WTSQuerySessionInformation_t = decltype(&WTSQuerySessionInformationW);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        WTSQuerySessionInformation_t, pWTSQuerySessionInformation,
        L"wtsapi32.dll", LOAD_LIBRARY_SEARCH_SYSTEM32,
        "WTSQuerySessionInformationW");
    THROW_HR_IF_NULL(E_UNEXPECTED, pWTSQuerySessionInformation);

    WTSFreeMemory_t pWTSFreeMemory = GetWTSFreeMemory();

    WCHAR* userName;
    DWORD userNameLen;
    THROW_IF_WIN32_BOOL_FALSE(
        pWTSQuerySessionInformation(WTS_CURRENT_SERVER_HANDLE, sessionId,
                                    WTSUserName, &userName, &userNameLen));
    auto userNameCleanup = wil::scope_exit(
        [userName, pWTSFreeMemory] { pWTSFreeMemory(userName); });

    return *userName != L'\0';
}

std::vector<DWORD> GetLoggedOnSessionIds() {
    using WTSEnumerateSessions_t = decltype(&WTSEnumerateSessionsW);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        WTSEnumerateSessions_t, pWTSEnumerateSessions, L"wtsapi32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "WTSEnumerateSessionsW");
    THROW_HR_IF_NULL(E_UNEXPECTED, pWTSEnumerateSessions);

    WTSFreeMemory_t pWTSFreeMemory = GetWTSFreeMemory();

    WTS_SESSION_INFO* sessionInfo;
    DWORD sessionCount;
    THROW_IF_WIN32_BOOL_FALSE(pWTSEnumerateSessions(
        WTS_CURRENT_SERVER_HANDLE, 0, 1, &sessionInfo, &sessionCount));
    auto sessionInfoCleanup = wil::scope_exit(
        [sessionInfo, pWTSFreeMemory] { pWTSFreeMemory(sessionInfo); });

    std::vector<DWORD> sessionIds;

    for (DWORD i = 0; i < sessionCount; i++) {
        DWORD sessionId = sessionInfo[i].SessionId;

        try {
            if (IsSessionLoggedOn(sessionId)) {
                sessionIds.push_back(sessionId);
            }
        } catch (const std::exception& e) {
            // A session which can't be asked doesn't stop the others.
            LOG(L"Reading the user of session %u failed: %S", sessionId,
                e.what());
        }
    }

    return sessionIds;
}

void CreateProcessOnSessionId(DWORD sessionId,
                              PCWSTR applicationName,
                              PWSTR commandLine) {
    wil::unique_handle token = QuerySessionUserToken(sessionId);
    CreateProcessAsUserWithEnvironment(token.get(), applicationName,
                                       commandLine);
}

void CreateProcessOnSessionIdElevated(DWORD sessionId,
                                      PCWSTR applicationName,
                                      PWSTR commandLine) {
    wil::unique_handle token = QuerySessionUserToken(sessionId);
    wil::unique_handle fullToken = GetUserFullToken(token.get());
    CreateProcessAsUserWithEnvironment(fullToken.get(), applicationName,
                                       commandLine);
}

bool CanCreateProcessOnSessionIdElevated(DWORD sessionId) {
    wil::unique_handle token = QuerySessionUserToken(sessionId);
    return !!TryGetUserFullToken(token.get());
}

void CreateProcessOnSessionIdWithUiAccess(DWORD sessionId,
                                          PCWSTR applicationName,
                                          PWSTR commandLine) {
    wil::unique_handle processToken;
    THROW_IF_WIN32_BOOL_FALSE(
        OpenProcessToken(GetCurrentProcess(),
                         TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &processToken));

    // Setting UIAccess on the token is only allowed with SE_TCB_NAME, which
    // the system account holds.
    ScopedTokenPrivilege tcbPrivilege(processToken.get(), SE_TCB_NAME);

    wil::unique_handle token = QuerySessionUserToken(sessionId);
    wil::unique_handle uiAccessToken = MakeUiAccessToken(token.get());
    CreateProcessAsUserWithEnvironment(uiAccessToken.get(), applicationName,
                                       commandLine);
}

void CreateProcessAsDesktopUser(PCWSTR applicationName, PWSTR commandLine) {
    wil::unique_process_information processInfo;

    wil::unique_process_handle shellProcess = OpenShellProcessIfElevated();
    if (!shellProcess) {
        STARTUPINFO startupInfo = {
            .cb = sizeof(STARTUPINFO),
            .dwFlags = STARTF_FORCEOFFFEEDBACK,
        };

        THROW_IF_WIN32_BOOL_FALSE(CreateProcessInternal(
            nullptr, applicationName, commandLine, NORMAL_PRIORITY_CLASS,
            nullptr, &startupInfo, &processInfo));
        return;
    }

    // A child is given its creator's environment, so the desktop user's has to
    // be built and handed over alongside the parent.
    wil::unique_handle shellToken;
    THROW_IF_WIN32_BOOL_FALSE(OpenProcessToken(
        shellProcess.get(), TOKEN_QUERY | TOKEN_DUPLICATE, &shellToken));

    unique_environment_block environment =
        CreateUserEnvironmentBlock(shellToken.get());

    SIZE_T attributeListSize = 0;
    if (!InitializeProcThreadAttributeList(nullptr, 1, 0, &attributeListSize) &&
        GetLastError() != ERROR_INSUFFICIENT_BUFFER) {
        THROW_LAST_ERROR();
    }

    std::vector<BYTE> attributeListBuffer(attributeListSize);
    auto* attributeList = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(
        attributeListBuffer.data());
    THROW_IF_WIN32_BOOL_FALSE(InitializeProcThreadAttributeList(
        attributeList, 1, 0, &attributeListSize));
    auto attributeListCleanup = wil::scope_exit(
        [attributeList] { DeleteProcThreadAttributeList(attributeList); });

    HANDLE parentProcess = shellProcess.get();
    THROW_IF_WIN32_BOOL_FALSE(UpdateProcThreadAttribute(
        attributeList, 0, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, &parentProcess,
        sizeof(parentProcess), nullptr, nullptr));

    STARTUPINFOEX startupInfoEx = {
        .StartupInfo =
            {
                .cb = sizeof(STARTUPINFOEX),
                .dwFlags = STARTF_FORCEOFFFEEDBACK,
            },
        .lpAttributeList = attributeList,
    };

    THROW_IF_WIN32_BOOL_FALSE(CreateProcessInternal(
        nullptr, applicationName, commandLine,
        NORMAL_PRIORITY_CLASS | CREATE_UNICODE_ENVIRONMENT |
            EXTENDED_STARTUPINFO_PRESENT,
        environment.get(), &startupInfoEx.StartupInfo, &processInfo));
}

void CreateProcessInOwnSessionElevated(PCWSTR applicationName,
                                       PWSTR commandLine) {
    STARTUPINFO startupInfo = {
        .cb = sizeof(STARTUPINFO),
        .dwFlags = STARTF_FORCEOFFFEEDBACK,
    };
    wil::unique_process_information processInfo;

    THROW_IF_WIN32_BOOL_FALSE(CreateProcessInternal(
        nullptr, applicationName, commandLine, NORMAL_PRIORITY_CLASS, nullptr,
        &startupInfo, &processInfo));
}

void CreateProcessAsDesktopUserWithUiAccess(PCWSTR applicationName,
                                            PWSTR commandLine) {
    wil::unique_process_handle shellProcess = OpenShellProcessIfElevated();
    // Without an elevated caller there is no separate desktop user token to
    // grant UIAccess on.
    THROW_HR_IF_NULL(E_UNEXPECTED, shellProcess);

    wil::unique_handle shellToken;
    THROW_IF_WIN32_BOOL_FALSE(OpenProcessToken(
        shellProcess.get(), TOKEN_QUERY | TOKEN_DUPLICATE, &shellToken));

    // Borrow the privileges the launch needs by impersonating the system
    // account.
    wil::unique_handle systemToken = OpenSystemImpersonationToken();
    auto revert = wil::impersonate_token(systemToken.get());

    wil::unique_handle uiAccessToken = MakeUiAccessToken(shellToken.get());
    CreateProcessAsUserWithEnvironment(uiAccessToken.get(), applicationName,
                                       commandLine);
}

}  // namespace Functions
