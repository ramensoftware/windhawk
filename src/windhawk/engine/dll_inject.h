#pragma once

namespace DllInject {

constexpr ACCESS_MASK kProcessAccess = PROCESS_CREATE_THREAD |
                                       PROCESS_VM_OPERATION | PROCESS_VM_READ |
                                       PROCESS_VM_WRITE | PROCESS_DUP_HANDLE |
                                       PROCESS_QUERY_INFORMATION | SYNCHRONIZE;

constexpr ACCESS_MASK kApcThreadsAccess = THREAD_SET_CONTEXT;

struct LOAD_LIBRARY_REMOTE_DATA {
    INT32 nLogVerbosity;
    BOOL bRunningFromAPC;
    BOOL bThreadAttachExempt;
    union {
        HANDLE hSessionManagerProcess;
        // Make sure 32-bit/64-bit layouts are the same.
        DWORD64 dw64SessionManagerProcess;
    };
    union {
        HANDLE hSessionMutex;
        // Make sure 32-bit/64-bit layouts are the same.
        DWORD64 dw64SessionMutex;
    };
    union {
        void* pThreadShellcodeAddress;
        // Make sure 32-bit/64-bit layouts are the same.
        DWORD64 dw64ThreadShellcodeAddress;
    };
    union {
        void* pAPCShellcodeAddress;
        // Make sure 32-bit/64-bit layouts are the same.
        DWORD64 dw64APCShellcodeAddress;
    };
    WCHAR szDllName[1];  // flexible array member
};

// Pins the 32-bit/64-bit layouts to match; catches accidental drift from
// reordering or resizing fields above.
static_assert(offsetof(LOAD_LIBRARY_REMOTE_DATA, szDllName) == 48);
static_assert(alignof(LOAD_LIBRARY_REMOTE_DATA) == 8);

void DllInject(HANDLE hProcess,
               HANDLE hThreadForAPC,
               HANDLE hSessionManagerProcess,
               HANDLE hSessionMutex,
               bool threadAttachExempt);

}  // namespace DllInject
