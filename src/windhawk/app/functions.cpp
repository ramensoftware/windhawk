#include "stdafx.h"

#include "functions.h"

namespace Functions {

namespace {

typedef struct _UNICODE_STRING {
    USHORT Length;
    USHORT MaximumLength;
    _Field_size_bytes_part_opt_(MaximumLength, Length) PWCH Buffer;
} UNICODE_STRING, *PUNICODE_STRING;

// wdm
typedef struct _COUNTED_REASON_CONTEXT {
    ULONG Version;
    ULONG Flags;
    union {
        struct {
            UNICODE_STRING ResourceFileName;
            USHORT ResourceReasonId;
            ULONG StringCount;
            _Field_size_(StringCount) PUNICODE_STRING ReasonStrings;
        };
        UNICODE_STRING SimpleString;
    };
} COUNTED_REASON_CONTEXT, *PCOUNTED_REASON_CONTEXT;

#ifndef _WIN64
#pragma pack(push, 8)
typedef struct _UNICODE_STRING64 {
    USHORT Length;
    USHORT MaximumLength;
    _Field_size_bytes_part_opt_(MaximumLength, Length) DWORD64 Buffer;
} UNICODE_STRING64, *PUNICODE_STRING64;

typedef struct _COUNTED_REASON_CONTEXT64 {
    ULONG Version;
    ULONG Flags;
    union {
        struct {
            UNICODE_STRING64 ResourceFileName;
            USHORT ResourceReasonId;
            ULONG StringCount;
            _Field_size_(StringCount) PUNICODE_STRING64 ReasonStrings;
        };
        UNICODE_STRING64 SimpleString;
    };
} COUNTED_REASON_CONTEXT64, *PCOUNTED_REASON_CONTEXT64;
#pragma pack(pop)
#endif

// POWER_REQUEST_TYPE
typedef enum _POWER_REQUEST_TYPE_INTERNAL {
    PowerRequestDisplayRequiredInternal,
    PowerRequestSystemRequiredInternal,
    PowerRequestAwayModeRequiredInternal,
    PowerRequestExecutionRequiredInternal,  // Windows 8+
    PowerRequestPerfBoostRequiredInternal,  // Windows 8+
    PowerRequestActiveLockScreenInternal,   // Windows 10 RS1+ (reserved on
                                            // Windows 8)
    // Values 6 and 7 are reserved for Windows 8 only
    PowerRequestInternalInvalid,
    PowerRequestInternalUnknown,
    PowerRequestFullScreenVideoRequired  // Windows 8 only
} POWER_REQUEST_TYPE_INTERNAL;

typedef struct _POWER_REQUEST_ACTION {
    HANDLE PowerRequestHandle;
    POWER_REQUEST_TYPE_INTERNAL RequestType;
    BOOLEAN SetAction;
    HANDLE ProcessHandle;  // Windows 8+ and only for requests created via
                           // PlmPowerRequestCreate
} POWER_REQUEST_ACTION, *PPOWER_REQUEST_ACTION;

#ifndef NT_SUCCESS
#define NT_SUCCESS(Status) (((NTSTATUS)(Status)) >= 0)
#endif

#define POWER_REQUEST_CONTEXT_NOT_SPECIFIED DIAGNOSTIC_REASON_NOT_SPECIFIED

NTSTATUS NtPowerInformation(_In_ POWER_INFORMATION_LEVEL InformationLevel,
                            _In_reads_bytes_opt_(InputBufferLength)
                                PVOID InputBuffer,
                            _In_ ULONG InputBufferLength,
                            _Out_writes_bytes_opt_(OutputBufferLength)
                                PVOID OutputBuffer,
                            _In_ ULONG OutputBufferLength) {
    using NtPowerInformation_t = NTSTATUS(WINAPI*)(
        _In_ POWER_INFORMATION_LEVEL InformationLevel,
        _In_reads_bytes_opt_(InputBufferLength) PVOID InputBuffer,
        _In_ ULONG InputBufferLength,
        _Out_writes_bytes_opt_(OutputBufferLength) PVOID OutputBuffer,
        _In_ ULONG OutputBufferLength);
    static NtPowerInformation_t pNtPowerInformation = []() {
        HMODULE hNtdll = GetModuleHandle(L"ntdll.dll");
        if (hNtdll) {
            return (NtPowerInformation_t)GetProcAddress(hNtdll,
                                                        "NtPowerInformation");
        }

        return (NtPowerInformation_t) nullptr;
    }();

    if (!pNtPowerInformation) {
        return STATUS_UNSUCCESSFUL;
    }

    return pNtPowerInformation(InformationLevel, InputBuffer, InputBufferLength,
                               OutputBuffer, OutputBufferLength);
}

}  // namespace

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
        s | std::views::split(delim) | std::views::transform([](auto&& rng) {
            return std::wstring_view(rng.data(), rng.size());
        });
    return std::vector<std::wstring>(view.begin(), view.end());
}

std::vector<std::wstring_view> SplitStringToViews(std::wstring_view s,
                                                  WCHAR delim) {
    // https://stackoverflow.com/a/48403210
    auto view =
        s | std::views::split(delim) | std::views::transform([](auto&& rng) {
            return std::wstring_view(rng.data(), rng.size());
        });
    return std::vector<std::wstring_view>(view.begin(), view.end());
}

// https://stackoverflow.com/a/29752943
std::wstring ReplaceAll(std::wstring_view source,
                        std::wstring_view from,
                        std::wstring_view to,
                        bool ignoreCase) {
    auto findString = [ignoreCase](std::wstring_view haystack,
                                   std::wstring_view needle,
                                   size_t pos) -> size_t {
        if (!ignoreCase) {
            return haystack.find(needle, pos);
        }

        auto it = std::search(
            haystack.begin() + pos, haystack.end(), needle.begin(),
            needle.end(), [](WCHAR ch1, WCHAR ch2) {
                LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &ch1,
                              1, &ch1, 1, nullptr, nullptr, 0);
                LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &ch2,
                              1, &ch2, 1, nullptr, nullptr, 0);
                return ch1 == ch2;
            });
        if (it == haystack.end()) {
            return haystack.npos;
        }

        return std::distance(haystack.begin(), it);
    };

    std::wstring newString;

    size_t lastPos = 0;
    size_t findPos;

    while ((findPos = findString(source, from, lastPos)) != source.npos) {
        newString.append(source, lastPos, findPos - lastPos);
        newString += to;
        lastPos = findPos + from.length();
    }

    // Care for the rest after last occurrence.
    newString += source.substr(lastPos);

    return newString;
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
        CDC hdc = ::GetDC(nullptr);
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

HRESULT SetThreadDescriptionIfAvailable(HANDLE hThread,
                                        PCWSTR lpThreadDescription) {
    using SetThreadDescription_t = decltype(&SetThreadDescription);
    static SetThreadDescription_t pSetThreadDescription = []() {
        HMODULE hKernel32 = GetModuleHandle(L"kernel32.dll");
        if (hKernel32) {
            return (SetThreadDescription_t)GetProcAddress(
                hKernel32, "SetThreadDescription");
        }

        return (SetThreadDescription_t) nullptr;
    }();

    if (!pSetThreadDescription) {
        return E_NOTIMPL;
    }

    return pSetThreadDescription(hThread, lpThreadDescription);
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

bool IsWindowsVersionOrGreaterWithBuildNumber(WORD wMajorVersion,
                                              WORD wMinorVersion,
                                              WORD wBuildNumber) {
    ULONG majorVersion = 0;
    ULONG minorVersion = 0;
    ULONG buildNumber = 0;
    Functions::GetNtVersionNumbers(&majorVersion, &minorVersion, &buildNumber);

    if (majorVersion != wMajorVersion) {
        return majorVersion > wMajorVersion;
    }

    if (minorVersion != wMinorVersion) {
        return minorVersion > wMinorVersion;
    }

    return buildNumber >= wBuildNumber;
}

// Based on:
// https://github.com/winsiderss/systeminformer/blob/fc2a978e924f0f72f59928e74a5cfccbb48dfd10/phlib/native.c#L16472
//
// rev from RtlpCreateExecutionRequiredRequest (dmex)
/**
 * Creates a PLM execution request. This is mandatory on Windows 8 and above to
 * prevent processes freezing while querying process information and deadlocking
 * the calling process.
 *
 * \param ProcessHandle A handle to the process for which the power request is
 * to be created. \param PowerRequestHandle A pointer to a variable that
 * receives a handle to the new power request.
 *
 * \return Successful or errant status.
 */
NTSTATUS CreateExecutionRequiredRequest(_In_ HANDLE ProcessHandle,
                                        _Out_ PHANDLE PowerRequestHandle) {
    NTSTATUS status;

    HANDLE powerRequestHandle = nullptr;

    // On WoW64, NtPowerInformation only handles 4 info classes:
    // PowerRequestCreate, PowerRequestAction, EnergyTrackerCreate,
    // EnergyTrackerQuery. The rest are forwarded as-is to the native x86-64
    // implementation.
#ifndef _WIN64
    BOOL isWow64;
    if (IsWow64Process(GetCurrentProcess(), &isWow64) && isWow64) {
        COUNTED_REASON_CONTEXT64 powerRequestReason64;
        memset(&powerRequestReason64, 0, sizeof(COUNTED_REASON_CONTEXT64));
        powerRequestReason64.Version = POWER_REQUEST_CONTEXT_VERSION;
        powerRequestReason64.Flags = POWER_REQUEST_CONTEXT_NOT_SPECIFIED;

        DWORD64 powerRequestHandle64 = 0;
        status =
            NtPowerInformation(PlmPowerRequestCreate, &powerRequestReason64,
                               sizeof(COUNTED_REASON_CONTEXT64),
                               &powerRequestHandle64, sizeof(DWORD64));

        powerRequestHandle = (HANDLE)powerRequestHandle64;
    } else {
#endif
        COUNTED_REASON_CONTEXT powerRequestReason;
        memset(&powerRequestReason, 0, sizeof(COUNTED_REASON_CONTEXT));
        powerRequestReason.Version = POWER_REQUEST_CONTEXT_VERSION;
        powerRequestReason.Flags = POWER_REQUEST_CONTEXT_NOT_SPECIFIED;

        status = NtPowerInformation(PlmPowerRequestCreate, &powerRequestReason,
                                    sizeof(COUNTED_REASON_CONTEXT),
                                    &powerRequestHandle, sizeof(HANDLE));
#ifndef _WIN64
    }
#endif

    if (!NT_SUCCESS(status))
        return status;

    POWER_REQUEST_ACTION powerRequestAction;
    memset(&powerRequestAction, 0, sizeof(POWER_REQUEST_ACTION));
    powerRequestAction.PowerRequestHandle = powerRequestHandle;
    powerRequestAction.RequestType = PowerRequestExecutionRequiredInternal;
    powerRequestAction.SetAction = TRUE;
    powerRequestAction.ProcessHandle = ProcessHandle;

    status = NtPowerInformation(PowerRequestAction, &powerRequestAction,
                                sizeof(POWER_REQUEST_ACTION), nullptr, 0);

    if (NT_SUCCESS(status)) {
        *PowerRequestHandle = powerRequestHandle;
    } else {
        CloseHandle(powerRequestHandle);
    }

    return status;
}

bool IsRightToLeftLanguage(LANGID langId) {
    switch (PRIMARYLANGID(langId)) {
        case LANG_ARABIC:
        case LANG_FARSI:
        case LANG_HEBREW:
        case LANG_URDU:
            return true;

        default:
            return false;
    }
}

void ApplyDialogLayoutRtl(CWindow wnd, bool isLayoutRtl) {
    wnd.ModifyStyleEx(isLayoutRtl ? 0 : WS_EX_LAYOUTRTL,
                      isLayoutRtl ? WS_EX_LAYOUTRTL : 0);

    ::EnumChildWindows(
        wnd,
        [](HWND hWnd, LPARAM lParam) {
            bool isLayoutRtl = lParam != 0;

            CWindow control(hWnd);
            CWindow parent = control.GetParent();

            CRect rcParent;
            parent.GetClientRect(rcParent);

            CRect rcControl;
            control.GetWindowRect(rcControl);
            ::MapWindowPoints(NULL, parent, (POINT*)&rcControl, 2);

            rcControl.MoveToX(rcParent.Width() - rcControl.right);

            control.SetWindowPos(NULL, rcControl, SWP_NOZORDER);

            if (isLayoutRtl) {
                control.ModifyStyleEx(0, WS_EX_LAYOUTRTL);
            } else {
                // Sometimes (e.g. for Edit controls), when setting
                // WS_EX_LAYOUTRTL, the flag is not actually set. Other flags
                // are being set instead (e.g. WS_EX_RTLREADING). Below, we try
                // to handle such cases.

                DWORD dwExStyle = control.GetExStyle();
                if (dwExStyle & WS_EX_LAYOUTRTL) {
                    control.ModifyStyleEx(WS_EX_LAYOUTRTL, 0);
                } else if (dwExStyle & (WS_EX_RTLREADING | WS_EX_RIGHT |
                                        WS_EX_LEFTSCROLLBAR)) {
                    control.ModifyStyleEx(
                        WS_EX_RTLREADING | WS_EX_RIGHT | WS_EX_LEFTSCROLLBAR,
                        0);

                    WCHAR szClassName[64];
                    if (::GetClassName(control, szClassName,
                                       _countof(szClassName))) {
                        if (_wcsicmp(szClassName, L"Edit") == 0)
                            control.ModifyStyle(ES_RIGHT, 0);
                    }
                }
            }

            control.InvalidateRect(NULL);

            return TRUE;
        },
        isLayoutRtl);

    wnd.InvalidateRect(NULL);
}

}  // namespace Functions
