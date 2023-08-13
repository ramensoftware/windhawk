#pragma once

namespace DllInject {

constexpr ACCESS_MASK kProcessAccess = PROCESS_CREATE_THREAD |
                                       PROCESS_VM_OPERATION | PROCESS_VM_READ |
                                       PROCESS_VM_WRITE | PROCESS_DUP_HANDLE |
                                       PROCESS_QUERY_INFORMATION | SYNCHRONIZE;

struct LOAD_LIBRARY_REMOTE_DATA {
    INT32 nLogVerbosity;
    BOOL bRunningFromAPC;
    BOOL bThreadAttachExempt;
    union {
        HANDLE hSessionManagerProcess;
        DWORD64 dw64SessionManagerProcess;  // make sure 32-bit/64-bit layouts
                                            // are the same
    };
    union {
        HANDLE hSessionMutex;
        DWORD64
            dw64SessionMutex;  // make sure 32-bit/64-bit layouts are the same
    };
    WCHAR szDllName[1];  // flexible array member
};

void DllInject(HANDLE hProcess,
               HANDLE hThreadForAPC,
               HANDLE hSessionManagerProcess,
               HANDLE hSessionMutex,
               bool threadAttachExempt);

}  // namespace DllInject
