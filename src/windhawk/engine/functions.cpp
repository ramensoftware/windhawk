#include "stdafx.h"

#include "functions.h"
#include "var_init_once.h"

namespace Functions {

bool IsProcessBlockingNonMicrosoftBinaries(HANDLE hProcess) {
    PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY policy;
    if (!GetProcessMitigationPolicy(hProcess, ProcessSignaturePolicy, &policy,
                                    sizeof(policy))) {
        return false;
    }

    return policy.MicrosoftSignedOnly || policy.StoreSignedOnly ||
           policy.MitigationOptIn;
}

// Based on:
// http://securityxploded.com/ntcreatethreadex.php
// Another reference:
// https://github.com/winsiderss/systeminformer/blob/25846070780183848dc8d8f335a54fa6e636e281/phlib/basesup.c#L217
HANDLE MyCreateRemoteThread(HANDLE hProcess,
                            LPTHREAD_START_ROUTINE lpStartAddress,
                            LPVOID lpParameter,
                            ULONG createFlags) {
    using NtCreateThreadEx_t = NTSTATUS(WINAPI*)(
        _Out_ PHANDLE ThreadHandle, _In_ ACCESS_MASK DesiredAccess,
        _In_opt_ LPVOID ObjectAttributes,  // POBJECT_ATTRIBUTES
        _In_ HANDLE ProcessHandle,
        _In_ PVOID StartRoutine,  // PUSER_THREAD_START_ROUTINE
        _In_opt_ PVOID Argument,
        _In_ ULONG CreateFlags,  // THREAD_CREATE_FLAGS_*
        _In_ SIZE_T ZeroBits, _In_ SIZE_T StackSize,
        _In_ SIZE_T MaximumStackSize,
        _In_opt_ LPVOID AttributeList  // PPS_ATTRIBUTE_LIST
    );

    GET_PROC_ADDRESS_ONCE(NtCreateThreadEx_t, pNtCreateThreadEx, L"ntdll.dll",
                          "NtCreateThreadEx");

    if (!pNtCreateThreadEx) {
        SetLastError(ERROR_PROC_NOT_FOUND);
        return nullptr;
    }

    HANDLE hThread;
    NTSTATUS result = pNtCreateThreadEx(&hThread, THREAD_ALL_ACCESS, nullptr,
                                        hProcess, lpStartAddress, lpParameter,
                                        createFlags, 0, 0, 0, nullptr);
    if (result < 0) {
        SetLastError(LsaNtStatusToWinError(result));
        return nullptr;
    }

    return hThread;
}

}  // namespace Functions
