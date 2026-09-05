#include "stdafx.h"

#include "process_state.h"

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

}  // namespace Functions
