#include "stdafx.h"

#include "functions.h"

namespace Functions {

// SetPrivilege enables/disables process token privilege.
// https://docs.microsoft.com/en-us/windows-hardware/drivers/debugger/debug-privilege
BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege) {
    LUID luid;
    BOOL bRet = FALSE;

    if (LookupPrivilegeValue(nullptr, lpszPrivilege, &luid)) {
        TOKEN_PRIVILEGES tp;

        tp.PrivilegeCount = 1;
        tp.Privileges[0].Luid = luid;
        tp.Privileges[0].Attributes =
            (bEnablePrivilege) ? SE_PRIVILEGE_ENABLED : 0;

        // Enable the privilege or disable all privileges.
        if (AdjustTokenPrivileges(hToken, FALSE, &tp, 0, nullptr, nullptr)) {
            // Check to see if you have proper access.
            // You may get "ERROR_NOT_ALL_ASSIGNED".
            bRet = (GetLastError() == ERROR_SUCCESS);
        }
    }

    return bRet;
}

BOOL SetDebugPrivilege(BOOL bEnablePrivilege) {
    wil::unique_handle token;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES,
                          &token)) {
        return FALSE;
    }

    return SetPrivilege(token.get(), SE_DEBUG_NAME, bEnablePrivilege);
}

HANDLE CreateEventForMediumIntegrity(PCWSTR eventName, BOOL manualReset) {
    // Allow only EVENT_MODIFY_STATE (0x0002), only for medium integrity.
    PCWSTR pszStringSecurityDescriptor = L"D:(A;;0x0002;;;WD)S:(ML;;NW;;;ME)";

    wil::unique_hlocal secDesc;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
            pszStringSecurityDescriptor, SDDL_REVISION_1, &secDesc, nullptr)) {
        return nullptr;
    }

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    return CreateEvent(&secAttr, manualReset, FALSE, eventName);
}

//
// FUNCTION: IsRunAsAdmin()
//
// PURPOSE: The function checks whether the current process is run as
// administrator. In other words, it dictates whether the primary access
// token of the process belongs to user account that is a member of the
// local Administrators group and it is elevated.
//
// RETURN VALUE: Returns TRUE if the primary access token of the process
// belongs to user account that is a member of the local Administrators
// group and it is elevated. Returns FALSE if the token does not. Returns
// FALSE on failure. To get extended error information, call GetLastError.
//
BOOL IsRunAsAdmin() {
    BOOL fIsRunAsAdmin = FALSE;
    DWORD dwError = ERROR_SUCCESS;
    PSID pAdministratorsGroup = nullptr;
    SID_IDENTIFIER_AUTHORITY NtAuthority = SECURITY_NT_AUTHORITY;

    // Allocate and initialize a SID of the administrators group.
    if (AllocateAndInitializeSid(&NtAuthority, 2, SECURITY_BUILTIN_DOMAIN_RID,
                                 DOMAIN_ALIAS_RID_ADMINS, 0, 0, 0, 0, 0, 0,
                                 &pAdministratorsGroup)) {
        // Determine whether the SID of administrators group is enabled in
        // the primary access token of the process.
        if (!CheckTokenMembership(nullptr, pAdministratorsGroup,
                                  &fIsRunAsAdmin)) {
            dwError = GetLastError();
        }

        FreeSid(pAdministratorsGroup);

        if (dwError != ERROR_SUCCESS) {
            SetLastError(dwError);
        }
    }

    return fIsRunAsAdmin;
}

PCWSTR LoadStrFromRsrc(UINT uStrId) {
    PCWSTR pStr;
    if (!LoadString(nullptr, uStrId, (WCHAR*)&pStr, 0)) {
        pStr = L"(Could not load resource)";
    }

    return pStr;
}

std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim) {
    // https://stackoverflow.com/a/48403210
    auto view =
        s | std::ranges::views::split(delim) |
        std::ranges::views::transform([](auto&& rng) {
            return std::wstring_view(&*rng.begin(), std::ranges::distance(rng));
        });
    return std::vector<std::wstring>(view.begin(), view.end());
}

UINT GetDpiForWindowWithFallback(HWND hWnd) {
    using GetDpiForWindow_t = UINT(WINAPI*)(HWND hwnd);
    static GetDpiForWindow_t pGetDpiForWindow = []() {
        HMODULE hUser32 = GetModuleHandle(L"user32.dll");
        if (hUser32) {
            return (GetDpiForWindow_t)GetProcAddress(hUser32,
                                                     "GetDpiForWindow");
        }

        return (GetDpiForWindow_t) nullptr;
    }();

    int iDpi = 96;
    if (pGetDpiForWindow) {
        iDpi = pGetDpiForWindow(hWnd);
    } else {
        CDC hdc = ::GetDC(NULL);
        if (hdc) {
            iDpi = hdc.GetDeviceCaps(LOGPIXELSX);
        }
    }

    return iDpi;
}

int GetSystemMetricsForDpiWithFallback(int nIndex, UINT dpi) {
    using GetSystemMetricsForDpi_t = int(WINAPI*)(int nIndex, UINT dpi);
    static GetSystemMetricsForDpi_t pGetSystemMetricsForDpi = []() {
        HMODULE hUser32 = GetModuleHandle(L"user32.dll");
        if (hUser32) {
            return (GetSystemMetricsForDpi_t)GetProcAddress(
                hUser32, "GetSystemMetricsForDpi");
        }

        return (GetSystemMetricsForDpi_t) nullptr;
    }();

    if (pGetSystemMetricsForDpi) {
        return pGetSystemMetricsForDpi(nIndex, dpi);
    } else {
        return GetSystemMetrics(nIndex);
    }
}

int GetSystemMetricsForWindow(HWND hWnd, int nIndex) {
    return GetSystemMetricsForDpiWithFallback(
        nIndex, GetDpiForWindowWithFallback(hWnd));
}

bool IsProcessFrozen(HANDLE hProcess) {
    // https://github.com/winsiderss/systeminformer/blob/044957137e1d7200431926130ea7cd6bf9d8a11f/phnt/include/ntpsapi.h#L303-L334
    typedef struct _PROCESS_BASIC_INFORMATION {
        NTSTATUS ExitStatus;
        /*PPEB*/ LPVOID PebBaseAddress;
        ULONG_PTR AffinityMask;
        /*KPRIORITY*/ LONG BasePriority;
        HANDLE UniqueProcessId;
        HANDLE InheritedFromUniqueProcessId;
    } PROCESS_BASIC_INFORMATION, *PPROCESS_BASIC_INFORMATION;

    typedef struct _PROCESS_EXTENDED_BASIC_INFORMATION {
        SIZE_T Size;  // set to sizeof structure on input
        PROCESS_BASIC_INFORMATION BasicInfo;
        union {
            ULONG Flags;
            struct {
                ULONG IsProtectedProcess : 1;
                ULONG IsWow64Process : 1;
                ULONG IsProcessDeleting : 1;
                ULONG IsCrossSessionCreate : 1;
                ULONG IsFrozen : 1;
                ULONG IsBackground : 1;
                ULONG IsStronglyNamed : 1;
                ULONG IsSecureProcess : 1;
                ULONG IsSubsystemProcess : 1;
                ULONG SpareBits : 23;
            };
        };
    } PROCESS_EXTENDED_BASIC_INFORMATION, *PPROCESS_EXTENDED_BASIC_INFORMATION;

    using NtQueryInformationProcess_t = NTSTATUS(WINAPI*)(
        _In_ HANDLE ProcessHandle,
        _In_ /*PROCESSINFOCLASS*/ DWORD ProcessInformationClass,
        _Out_writes_bytes_(ProcessInformationLength) PVOID ProcessInformation,
        _In_ ULONG ProcessInformationLength, _Out_opt_ PULONG ReturnLength);
    static NtQueryInformationProcess_t pNtQueryInformationProcess = []() {
        HMODULE hNtdll = LoadLibrary(L"ntdll.dll");
        if (hNtdll) {
            return (NtQueryInformationProcess_t)GetProcAddress(
                hNtdll, "NtQueryInformationProcess");
        }

        return (NtQueryInformationProcess_t) nullptr;
    }();

    if (!pNtQueryInformationProcess) {
        return false;
    }

    PROCESS_EXTENDED_BASIC_INFORMATION pebi;
    if (0 <= pNtQueryInformationProcess(hProcess, /*ProcessBasicInformation*/ 0,
                                        &pebi, sizeof(pebi), 0) &&
        pebi.Size >= sizeof(pebi)) {
        return pebi.IsFrozen != 0;
    }

    return false;
}

void GetNtVersionNumbers(ULONG* pNtMajorVersion,
                         ULONG* pNtMinorVersion,
                         ULONG* pNtBuildNumber) {
    using RtlGetNtVersionNumbers_t =
        void(WINAPI*)(ULONG * pNtMajorVersion, ULONG * pNtMinorVersion,
                      ULONG * pNtBuildNumber);
    static RtlGetNtVersionNumbers_t pRtlGetNtVersionNumbers = []() {
        HMODULE hNtdll = GetModuleHandle(L"ntdll.dll");
        if (hNtdll) {
            return (RtlGetNtVersionNumbers_t)GetProcAddress(
                hNtdll, "RtlGetNtVersionNumbers");
        }

        return (RtlGetNtVersionNumbers_t) nullptr;
    }();

    if (pRtlGetNtVersionNumbers) {
        pRtlGetNtVersionNumbers(pNtMajorVersion, pNtMinorVersion,
                                pNtBuildNumber);
        // The upper 4 bits are reserved for the type of the OS build.
        // https://dennisbabkin.com/blog/?t=how-to-tell-the-real-version-of-windows-your-app-is-running-on
        *pNtBuildNumber &= ~0xF0000000;
        return;
    }

    // Use GetVersionEx as a fallback.
#pragma warning(push)
#pragma warning(disable : 4996)  // disable deprecation message
    OSVERSIONINFO versionInfo = {sizeof(OSVERSIONINFO)};
    if (GetVersionEx(&versionInfo)) {
        *pNtMajorVersion = versionInfo.dwMajorVersion;
        *pNtMinorVersion = versionInfo.dwMinorVersion;
        *pNtBuildNumber = versionInfo.dwBuildNumber;
        return;
    }
#pragma warning(pop)

    *pNtMajorVersion = 0;
    *pNtMinorVersion = 0;
    *pNtBuildNumber = 0;
}

}  // namespace Functions
