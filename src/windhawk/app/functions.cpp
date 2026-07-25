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
    // Grant EVENT_MODIFY_STATE (0x0002) only to interactive users (IU) and
    // SYSTEM (SY), and require at least medium integrity via the label (which
    // blocks lower-integrity, sandboxed callers). The only legitimate signalers
    // are the per-user Windhawk apps running in interactive sessions and the
    // SYSTEM service that broadcasts to them; a broader grant such as the World
    // SID would also expose these service-control events (stop, safe mode) to
    // non-interactive, network, batch, and anonymous callers.
    PCWSTR pszStringSecurityDescriptor =
        L"D:(A;;0x0002;;;IU)(A;;0x0002;;;SY)S:(ML;;NW;;;ME)";

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
    bool modified = wnd.ModifyStyleEx(isLayoutRtl ? 0 : WS_EX_LAYOUTRTL,
                                      isLayoutRtl ? WS_EX_LAYOUTRTL : 0);
    if (!modified) {
        // No change, so no need to update child controls.
        return;
    }

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

// Undocumented uxtheme.dll dark mode controls, resolved by ordinal.
// https://github.com/ysc3839/win32-darkmode
void EnableDarkModeMenus() {
    // Note: Before 1903, `BOOL __stdcall AllowDarkModeForApp(BOOL)` (same
    // ordinal) only accepts TRUE or FALSE. TRUE means dark mode is allowed and
    // vice versa. After 1903, `PreferredMode __stdcall
    // SetPreferredAppMode(PreferredMode)` accepts 4 valid values. Calling it
    // with TRUE (1) is valid in both cases.
    enum PreferredAppMode {
        PreferredAppModeDefault,
        PreferredAppModeAllowDark,
        PreferredAppModeForceDark,
        PreferredAppModeForceLight,
        PreferredAppModeMax,
    };

    using SetPreferredAppMode_t =
        PreferredAppMode(WINAPI*)(PreferredAppMode appMode);
    static SetPreferredAppMode_t pSetPreferredAppMode = []() {
        // The ordinal only holds this function starting with Windows 10 1809,
        // the first version with dark mode support. On older versions it may
        // resolve to an unrelated export with a different signature.
        if (!IsWindowsVersionOrGreaterWithBuildNumber(10, 0, 17763)) {
            return (SetPreferredAppMode_t) nullptr;
        }

        HMODULE hUxtheme = LoadLibraryEx(L"uxtheme.dll", nullptr,
                                         LOAD_LIBRARY_SEARCH_SYSTEM32);
        if (hUxtheme) {
            return (SetPreferredAppMode_t)GetProcAddress(hUxtheme,
                                                         MAKEINTRESOURCEA(135));
        }

        return (SetPreferredAppMode_t) nullptr;
    }();

    if (pSetPreferredAppMode) {
        pSetPreferredAppMode(PreferredAppModeAllowDark);
    }
}

bool WriteFileContentAtomically(const std::filesystem::path& path,
                                std::string_view content) {
    // Unique temp name per writer, so no two concurrent writers ever touch the
    // same temp file - not two threads here, not another Windhawk process, and
    // not the core, all of which derive the temp name the same way. The process
    // and thread ids suffice: a thread runs only one write at a time, so the
    // pair is unique across every writer live at once. The single shared step
    // is the atomic MoveFileEx below, which resolves to a clean
    // last-writer-wins without torn files.
    std::filesystem::path tempPath = path;
    tempPath += L"." + std::to_wstring(GetCurrentProcessId()) + L"." +
                std::to_wstring(GetCurrentThreadId()) + L".tmp";

    {
        // Open exclusively (no sharing) so nothing can touch our private temp
        // file between creation and the rename.
        wil::unique_hfile tempFile(CreateFile(tempPath.c_str(), GENERIC_WRITE,
                                              0, nullptr, CREATE_ALWAYS,
                                              FILE_ATTRIBUTE_NORMAL, nullptr));
        if (!tempFile) {
            return false;
        }

        DWORD bytesWritten;
        bool succeeded = WriteFile(tempFile.get(), content.data(),
                                   wil::safe_cast<DWORD>(content.size()),
                                   &bytesWritten, nullptr) &&
                         bytesWritten == content.size() &&
                         FlushFileBuffers(tempFile.get());
        if (!succeeded) {
            tempFile.reset();
            DeleteFile(tempPath.c_str());
            return false;
        }
    }

    if (!MoveFileEx(tempPath.c_str(), path.c_str(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
        DeleteFile(tempPath.c_str());
        return false;
    }

    return true;
}

}  // namespace Functions
