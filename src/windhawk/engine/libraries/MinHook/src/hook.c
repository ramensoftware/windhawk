/*
 *  MinHook - The Minimalistic API Hooking Library for x64/x86
 *  Copyright (C) 2009-2017 Tsuda Kageyu.
 *  All rights reserved.
 *
 *  Redistribution and use in source and binary forms, with or without
 *  modification, are permitted provided that the following conditions
 *  are met:
 *
 *   1. Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *   2. Redistributions in binary form must reproduce the above copyright
 *      notice, this list of conditions and the following disclaimer in the
 *      documentation and/or other materials provided with the distribution.
 *
 *  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 *  "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 *  TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 *  PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER
 *  OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 *  EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 *  PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 *  PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 *  LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 *  NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 *  SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

#include <windows.h>
#include <tlhelp32.h>
#include <limits.h>

#include "../include/MinHook.h"
#include "buffer.h"
#include "trampoline.h"

#ifndef ARRAYSIZE
    #define ARRAYSIZE(A) (sizeof(A)/sizeof((A)[0]))
#endif

// Initial capacity of the HOOK_ENTRY buffer.
#define INITIAL_HOOK_CAPACITY   32

// Initial capacity of the thread IDs buffer.
#define INITIAL_THREAD_CAPACITY 128

// Special hook position values.
#define INVALID_HOOK_POS UINT_MAX

// Thread access rights for suspending/resuming threads.
#define THREAD_ACCESS \
    (THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION | THREAD_SET_CONTEXT)

// Suspended threads for Freeze()/Unfreeze().
typedef struct _FROZEN_THREADS
{
    LPHANDLE pItems;         // Data heap
    UINT     capacity;       // Size of allocated data heap, items
    UINT     size;           // Actual number of data items
} FROZEN_THREADS, *PFROZEN_THREADS;

// Thread freeze related definitions.
typedef NTSTATUS(NTAPI *NtGetNextThread_t)(
    _In_ HANDLE ProcessHandle,
    _In_opt_ HANDLE ThreadHandle,
    _In_ ACCESS_MASK DesiredAccess,
    _In_ ULONG HandleAttributes,
    _In_ ULONG Flags,
    _Out_ PHANDLE NewThreadHandle
    );

#ifndef STATUS_NO_MORE_ENTRIES
    #define STATUS_NO_MORE_ENTRIES ((NTSTATUS)0x8000001AL)
#endif

#ifndef NT_SUCCESS
    #define NT_SUCCESS(Status) ((NTSTATUS)(Status) >= 0)
#endif

// Function and function pointer declarations.
typedef MH_STATUS(WINAPI *ENABLE_HOOK_LL_PROC)(UINT pos, BOOL enable, PFROZEN_THREADS pThreads);
typedef MH_STATUS(WINAPI *DISABLE_HOOK_CHAIN_PROC)(ULONG_PTR hookIdent, LPVOID pTarget, UINT parentPos, ENABLE_HOOK_LL_PROC ParentEnableHookLL, PFROZEN_THREADS pThreads);

static MH_STATUS WINAPI DisableHookChain(ULONG_PTR hookIdent, LPVOID pTarget, UINT parentPos, ENABLE_HOOK_LL_PROC ParentEnableHookLL, PFROZEN_THREADS pThreads);

// Executable buffer of a hook.
typedef struct _EXEC_BUFFER
{
    DISABLE_HOOK_CHAIN_PROC pDisableHookChain;
    ULONG_PTR hookIdent;
    JMP_RELAY jmpRelay;
    UINT8     trampoline[1]; // Uses the rest of the MEMORY_SLOT_SIZE bytes.
} EXEC_BUFFER, *PEXEC_BUFFER;

// Hook information.
typedef struct _HOOK_ENTRY
{
    ULONG_PTR hookIdent;        // Hook identifier, allows to hook the same function multiple times with different identifiers.

    LPVOID pTarget;             // Address of the target function.
    LPVOID pDetour;             // Address of the detour function.
    PEXEC_BUFFER pExecBuffer;   // Address of the executable buffer for relay and trampoline.
    UINT8  backup[8];           // Original prologue of the target function.

    UINT8  patchAbove  : 1;     // Uses the hot patch area.
    UINT8  isEnabled   : 1;     // Enabled.
    UINT8  queueEnable : 1;     // Queued for enabling/disabling when != isEnabled.

    UINT   nIP : 4;             // Count of the instruction boundaries.
    UINT8  oldIPs[8];           // Instruction boundaries of the target function.
    UINT8  newIPs[8];           // Instruction boundaries of the trampoline function.
} HOOK_ENTRY, *PHOOK_ENTRY;

//-------------------------------------------------------------------------
// Global Variables:
//-------------------------------------------------------------------------

// Mutex. If not NULL, this library is initialized.
static HANDLE g_hMutex = NULL;

// Private heap handle.
static HANDLE g_hHeap;

// Thread freeze related variables.
static MH_THREAD_FREEZE_METHOD g_threadFreezeMethod = MH_FREEZE_METHOD_ORIGINAL;

static NtGetNextThread_t pNtGetNextThread;

// Hook entries.
static struct
{
    PHOOK_ENTRY pItems;     // Data heap
    UINT        capacity;   // Size of allocated data heap, items
    UINT        size;       // Actual number of data items
} g_hooks;

//-------------------------------------------------------------------------
// Returns INVALID_HOOK_POS if not found.
static UINT FindHookEntry(ULONG_PTR hookIdent, LPVOID pTarget)
{
    UINT i;
    for (i = 0; i < g_hooks.size; ++i)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if (hookIdent == pHook->hookIdent && (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget)
            return i;
    }

    return INVALID_HOOK_POS;
}

//-------------------------------------------------------------------------
static PHOOK_ENTRY AddHookEntry()
{
    if (g_hooks.pItems == NULL)
    {
        g_hooks.capacity = INITIAL_HOOK_CAPACITY;
        g_hooks.pItems = (PHOOK_ENTRY)HeapAlloc(
            g_hHeap, 0, g_hooks.capacity * sizeof(HOOK_ENTRY));
        if (g_hooks.pItems == NULL)
            return NULL;
    }
    else if (g_hooks.size >= g_hooks.capacity)
    {
        PHOOK_ENTRY p = (PHOOK_ENTRY)HeapReAlloc(
            g_hHeap, 0, g_hooks.pItems, (g_hooks.capacity * 2) * sizeof(HOOK_ENTRY));
        if (p == NULL)
            return NULL;

        g_hooks.capacity *= 2;
        g_hooks.pItems = p;
    }

    return &g_hooks.pItems[g_hooks.size++];
}

//-------------------------------------------------------------------------
static VOID DeleteHookEntry(UINT pos)
{
    if (pos < g_hooks.size - 1)
        g_hooks.pItems[pos] = g_hooks.pItems[g_hooks.size - 1];

    g_hooks.size--;

    if (g_hooks.capacity / 2 >= INITIAL_HOOK_CAPACITY && g_hooks.capacity / 2 >= g_hooks.size)
    {
        PHOOK_ENTRY p = (PHOOK_ENTRY)HeapReAlloc(
            g_hHeap, 0, g_hooks.pItems, (g_hooks.capacity / 2) * sizeof(HOOK_ENTRY));
        if (p == NULL)
            return;

        g_hooks.capacity /= 2;
        g_hooks.pItems = p;
    }
}

//-------------------------------------------------------------------------
static DWORD_PTR FindOldIP(PHOOK_ENTRY pHook, DWORD_PTR ip)
{
    // In any of the jump locations:
    // Target -> Hotpatch jump (if patchAbove) -> Relay jump
    // Restore IP to the detour. This is required for consistent behavior
    // as a part of a DisableHookChain call, otherwise, if IP is restored
    // to the target, hooks that should be called may be skipped.

    if (ip == (DWORD_PTR)pHook->pTarget)
        return (DWORD_PTR)pHook->pDetour;

    if (pHook->patchAbove && ip == ((DWORD_PTR)pHook->pTarget - sizeof(JMP_REL)))
        return (DWORD_PTR)pHook->pDetour;

    if (ip == (DWORD_PTR)&pHook->pExecBuffer->jmpRelay)
        return (DWORD_PTR)pHook->pDetour;

    UINT i;
    for (i = 0; i < pHook->nIP; ++i)
    {
        if (ip == ((DWORD_PTR)pHook->pExecBuffer->trampoline + pHook->newIPs[i]))
            return (DWORD_PTR)pHook->pTarget + pHook->oldIPs[i];
    }

    return 0;
}

//-------------------------------------------------------------------------
static DWORD_PTR FindNewIP(PHOOK_ENTRY pHook, DWORD_PTR ip)
{
    UINT i;
    for (i = 0; i < pHook->nIP; ++i)
    {
        if (ip == ((DWORD_PTR)pHook->pTarget + pHook->oldIPs[i]))
            return (DWORD_PTR)pHook->pExecBuffer->trampoline + pHook->newIPs[i];
    }

    return 0;
}

//-------------------------------------------------------------------------
static VOID ProcessThreadIPs(HANDLE hThread, UINT pos, BOOL enable)
{
    // If the thread suspended in the overwritten area,
    // move IP to the proper address.

    CONTEXT     c;
#if defined(_M_X64) || defined(__x86_64__)
    DWORD64     *pIP = &c.Rip;
#else
    DWORD       *pIP = &c.Eip;
#endif
    PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
    DWORD_PTR   ip;

    c.ContextFlags = CONTEXT_CONTROL;
    if (!GetThreadContext(hThread, &c))
        return;

    if (enable)
        ip = FindNewIP(pHook, *pIP);
    else
        ip = FindOldIP(pHook, *pIP);

    if (ip != 0)
    {
        *pIP = ip;
        SetThreadContext(hThread, &c);
    }
}

//-------------------------------------------------------------------------
static BOOL EnumerateAndSuspendThreads(PFROZEN_THREADS pThreads)
{
    BOOL succeeded = FALSE;

    HANDLE hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if (hSnapshot != INVALID_HANDLE_VALUE)
    {
        THREADENTRY32 te;
        te.dwSize = sizeof(THREADENTRY32);
        if (Thread32First(hSnapshot, &te))
        {
            succeeded = TRUE;
            do
            {
                if (te.dwSize >= (FIELD_OFFSET(THREADENTRY32, th32OwnerProcessID) + sizeof(DWORD))
                    && te.th32OwnerProcessID == GetCurrentProcessId()
                    && te.th32ThreadID != GetCurrentThreadId())
                {
                    HANDLE hThread = OpenThread(THREAD_ACCESS, FALSE, te.th32ThreadID);

                    if (hThread != NULL && SuspendThread(hThread) == (DWORD)-1)
                    {
                        CloseHandle(hThread);
                        hThread = NULL;
                    }

                    if (hThread != NULL)
                    {
                        if (pThreads->pItems == NULL)
                        {
                            pThreads->capacity = INITIAL_THREAD_CAPACITY;
                            pThreads->pItems
                                = (LPHANDLE)HeapAlloc(g_hHeap, 0, pThreads->capacity * sizeof(HANDLE));
                            if (pThreads->pItems == NULL)
                                succeeded = FALSE;
                        }
                        else if (pThreads->size >= pThreads->capacity)
                        {
                            LPHANDLE p;
                            pThreads->capacity *= 2;
                            p = (LPHANDLE)HeapReAlloc(
                                g_hHeap, 0, pThreads->pItems, pThreads->capacity * sizeof(HANDLE));
                            if (p)
                                pThreads->pItems = p;
                            else
                                succeeded = FALSE;
                        }

                        if (!succeeded)
                        {
                            ResumeThread(hThread);
                            CloseHandle(hThread);
                            break;
                        }

                        pThreads->pItems[pThreads->size++] = hThread;
                    }
                }

                te.dwSize = sizeof(THREADENTRY32);
            } while (Thread32Next(hSnapshot, &te));

            if (succeeded && GetLastError() != ERROR_NO_MORE_FILES)
                succeeded = FALSE;

            if (!succeeded && pThreads->pItems != NULL)
            {
                UINT i;
                for (i = 0; i < pThreads->size; ++i)
                {
                    ResumeThread(pThreads->pItems[i]);
                    CloseHandle(pThreads->pItems[i]);
                }

                HeapFree(g_hHeap, 0, pThreads->pItems);
                pThreads->pItems = NULL;
            }
        }
        CloseHandle(hSnapshot);
    }

    return succeeded;
}

//-------------------------------------------------------------------------
static BOOL EnumerateAndSuspendThreadsFast(PFROZEN_THREADS pThreads)
{
    BOOL succeeded = TRUE;

    HANDLE hThread = NULL;
    BOOL bClosePrevThread = FALSE;
    while (1)
    {
        HANDLE hNextThread;
        NTSTATUS status = pNtGetNextThread(GetCurrentProcess(), hThread, THREAD_ACCESS, 0, 0, &hNextThread);
        if (bClosePrevThread)
            CloseHandle(hThread);

        if (!NT_SUCCESS(status))
        {
            if (status != STATUS_NO_MORE_ENTRIES)
                succeeded = FALSE;
            break;
        }

        hThread = hNextThread;
        bClosePrevThread = TRUE;

        if (GetThreadId(hThread) == GetCurrentThreadId())
            continue;

        if (SuspendThread(hThread) == (DWORD)-1)
            continue;

        bClosePrevThread = FALSE;

        if (pThreads->pItems == NULL)
        {
            pThreads->capacity = INITIAL_THREAD_CAPACITY;
            pThreads->pItems
                = (LPHANDLE)HeapAlloc(g_hHeap, 0, pThreads->capacity * sizeof(HANDLE));
            if (pThreads->pItems == NULL)
                succeeded = FALSE;
        }
        else if (pThreads->size >= pThreads->capacity)
        {
            pThreads->capacity *= 2;
            LPHANDLE p = (LPHANDLE)HeapReAlloc(
                g_hHeap, 0, pThreads->pItems, pThreads->capacity * sizeof(HANDLE));
            if (p)
                pThreads->pItems = p;
            else
                succeeded = FALSE;
        }

        if (!succeeded)
        {
            ResumeThread(hThread);
            CloseHandle(hThread);
            break;
        }

        // Perform a synchronous operation to make sure the thread really is suspended.
        // https://devblogs.microsoft.com/oldnewthing/20150205-00/?p=44743
        CONTEXT c;
        c.ContextFlags = CONTEXT_CONTROL;
        GetThreadContext(hThread, &c);

        pThreads->pItems[pThreads->size++] = hThread;
    }

    if (!succeeded && pThreads->pItems != NULL)
    {
        UINT i;
        for (i = 0; i < pThreads->size; ++i)
        {
            ResumeThread(pThreads->pItems[i]);
            CloseHandle(pThreads->pItems[i]);
        }

        HeapFree(g_hHeap, 0, pThreads->pItems);
        pThreads->pItems = NULL;
    }

    return succeeded;
}

//-------------------------------------------------------------------------
static VOID ProcessFrozenThreads(PFROZEN_THREADS pThreads, UINT pos, BOOL enable)
{
    if (pThreads->pItems != NULL)
    {
        UINT i;
        for (i = 0; i < pThreads->size; ++i)
        {
            ProcessThreadIPs(pThreads->pItems[i], pos, enable);
        }
    }
}

//-------------------------------------------------------------------------
static MH_STATUS Freeze(PFROZEN_THREADS pThreads)
{
    MH_STATUS status = MH_OK;

    pThreads->pItems   = NULL;
    pThreads->capacity = 0;
    pThreads->size     = 0;

    switch (g_threadFreezeMethod)
    {
    case MH_FREEZE_METHOD_ORIGINAL:
        if (!EnumerateAndSuspendThreads(pThreads))
            status = MH_ERROR_MEMORY_ALLOC;
        break;

    case MH_FREEZE_METHOD_FAST_UNDOCUMENTED:
        if (!EnumerateAndSuspendThreadsFast(pThreads))
            status = MH_ERROR_MEMORY_ALLOC;
        break;

    case MH_FREEZE_METHOD_NONE_UNSAFE:
        // Nothing to do.
        break;
    }

    return status;
}

//-------------------------------------------------------------------------
static VOID Unfreeze(PFROZEN_THREADS pThreads)
{
    if (pThreads->pItems != NULL)
    {
        UINT i;
        for (i = 0; i < pThreads->size; ++i)
        {
            ResumeThread(pThreads->pItems[i]);
            CloseHandle(pThreads->pItems[i]);
        }

        HeapFree(g_hHeap, 0, pThreads->pItems);
    }
}

//-------------------------------------------------------------------------
static MH_STATUS CreateHookTrampoline(UINT pos)
{
    PHOOK_ENTRY pHook = &g_hooks.pItems[pos];

    TRAMPOLINE ct;
    ct.pTarget = pHook->pTarget;
    ct.pTrampoline = pHook->pExecBuffer->trampoline;
    ct.trampolineSize = MEMORY_SLOT_SIZE - offsetof(EXEC_BUFFER, trampoline);
    if (!CreateTrampolineFunction(&ct))
    {
        return MH_ERROR_UNSUPPORTED_FUNCTION;
    }

    // Back up the target function.
    if (ct.patchAbove)
    {
        memcpy(
            pHook->backup,
            (LPBYTE)pHook->pTarget - sizeof(JMP_REL),
            sizeof(JMP_REL) + sizeof(JMP_REL_SHORT));
    }
    else
    {
        memcpy(pHook->backup, pHook->pTarget, sizeof(JMP_REL));
    }

    pHook->patchAbove = ct.patchAbove;
    pHook->nIP = ct.nIP;
    memcpy(pHook->oldIPs, ct.oldIPs, ARRAYSIZE(ct.oldIPs));
    memcpy(pHook->newIPs, ct.newIPs, ARRAYSIZE(ct.newIPs));

    return MH_OK;
}

//-------------------------------------------------------------------------
static MH_STATUS WINAPI EnableHookLL(UINT pos, BOOL enable, PFROZEN_THREADS pThreads)
{
    PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
    DWORD  oldProtect;
    SIZE_T patchSize    = sizeof(JMP_REL);
    LPBYTE pPatchTarget = (LPBYTE)pHook->pTarget;

    if (enable)
    {
        MH_STATUS status = CreateHookTrampoline(pos);
        if (status != MH_OK)
            return status;
    }

    if (pHook->patchAbove)
    {
        pPatchTarget -= sizeof(JMP_REL);
        patchSize    += sizeof(JMP_REL_SHORT);
    }

    if (!enable)
    {
        PJMP_REL pJmp = (PJMP_REL)pPatchTarget;
        if (pJmp->opcode == 0xE9)
        {
            PJMP_RELAY pJmpRelay = (PJMP_RELAY)(((LPBYTE)pJmp + sizeof(JMP_REL)) + (INT32)pJmp->operand);
            if (&pHook->pExecBuffer->jmpRelay != pJmpRelay)
            {
                PEXEC_BUFFER pOtherExecBuffer = (PEXEC_BUFFER)((LPBYTE)pJmpRelay - offsetof(EXEC_BUFFER, jmpRelay));
                return pOtherExecBuffer->pDisableHookChain(pOtherExecBuffer->hookIdent, pHook->pTarget, pos, EnableHookLL, pThreads);
            }
        }
    }

    if (!VirtualProtect(pPatchTarget, patchSize, PAGE_EXECUTE_READWRITE, &oldProtect))
        return MH_ERROR_MEMORY_PROTECT;

    if (enable)
    {
        PJMP_REL pJmp = (PJMP_REL)pPatchTarget;
        pJmp->opcode = 0xE9;
        pJmp->operand = (UINT32)((LPBYTE)&pHook->pExecBuffer->jmpRelay - (pPatchTarget + sizeof(JMP_REL)));

        if (pHook->patchAbove)
        {
            PJMP_REL_SHORT pShortJmp = (PJMP_REL_SHORT)pHook->pTarget;
            pShortJmp->opcode = 0xEB;
            pShortJmp->operand = (UINT8)(0 - (sizeof(JMP_REL_SHORT) + sizeof(JMP_REL)));
        }
    }
    else
    {
        if (pHook->patchAbove)
            memcpy(pPatchTarget, pHook->backup, sizeof(JMP_REL) + sizeof(JMP_REL_SHORT));
        else
            memcpy(pPatchTarget, pHook->backup, sizeof(JMP_REL));
    }

    VirtualProtect(pPatchTarget, patchSize, oldProtect, &oldProtect);

    // Just-in-case measure.
    FlushInstructionCache(GetCurrentProcess(), pPatchTarget, patchSize);

    ProcessFrozenThreads(pThreads, pos, enable);

    pHook->isEnabled   = enable;
    pHook->queueEnable = enable;

    return MH_OK;
}

//-------------------------------------------------------------------------
static MH_STATUS EnableHooksLL(ULONG_PTR hookIdent, LPVOID pTarget, BOOL enable)
{
    MH_STATUS status = MH_OK;
    UINT i, first = INVALID_HOOK_POS;

    for (i = 0; i < g_hooks.size; ++i)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if (pHook->isEnabled != enable &&
            (hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
            (pTarget == MH_ALL_HOOKS || (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget))
        {
            first = i;
            break;
        }
    }

    if (first != INVALID_HOOK_POS)
    {
        FROZEN_THREADS threads;
        status = Freeze(&threads);
        if (status == MH_OK)
        {
            for (i = first; i < g_hooks.size; ++i)
            {
                PHOOK_ENTRY pHook = &g_hooks.pItems[i];
                if (pHook->isEnabled != enable &&
                    (hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
                    (pTarget == MH_ALL_HOOKS || (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget))
                {
                    MH_STATUS enable_status = EnableHookLL(i, enable, &threads);

                    // Instead of stopping on the first error, we enable as much
                    // hooks as we can, and return the last error, if any.
                    if (enable_status != MH_OK)
                        status = enable_status;
                }
            }

            Unfreeze(&threads);
        }
    }

    return status;
}

//-------------------------------------------------------------------------
static HANDLE CreateProcessMutex(VOID)
{
    TCHAR szMutexName[sizeof("minhook_multihook_12345678")] = TEXT("minhook_multihook_");
    UINT mutexNameLen = sizeof("minhook_multihook_") - 1;
    DWORD dw = GetCurrentProcessId();
    UINT i;

    // Build szMutexName in the following format:
    // printf("minhook_multihook_%08X", GetCurrentProcessId());

    for (i = 0; i < 8; i++)
    {
        TCHAR ch;
        BYTE b = dw >> (32 - 4);

        if (b < 0x0A)
            ch = b + TEXT('0');
        else
            ch = b - 0x0A + TEXT('A');

        szMutexName[mutexNameLen++] = ch;
        dw <<= 4;
    }

    szMutexName[mutexNameLen] = TEXT('\0');

    return CreateMutex(NULL, FALSE, szMutexName);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_Initialize(VOID)
{
    if (g_hMutex != NULL)
        return MH_ERROR_ALREADY_INITIALIZED;

    g_hMutex = CreateProcessMutex();
    if (g_hMutex == NULL)
        return MH_ERROR_MUTEX_FAILURE;

    g_hHeap = HeapCreate(0, 0, 0);
    if (g_hHeap == NULL)
    {
        CloseHandle(g_hMutex);
        g_hMutex = NULL;
        return MH_ERROR_MEMORY_ALLOC;
    }

    // Initialize the internal function buffer.
    InitializeBuffer();

    return MH_OK;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_Uninitialize(VOID)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = EnableHooksLL(MH_ALL_IDENTS, MH_ALL_HOOKS, FALSE);

    ReleaseMutex(g_hMutex);

    if (status != MH_OK)
        return status;

    // Free the internal function buffer.
    // HeapFree is actually not required, but some tools detect a false
    // memory leak without HeapFree.
    UninitializeBuffer();
    HeapFree(g_hHeap, 0, g_hooks.pItems);
    HeapDestroy(g_hHeap);
    g_hHeap = NULL;

    g_hooks.pItems = NULL;
    g_hooks.capacity = 0;
    g_hooks.size = 0;

    CloseHandle(g_hMutex);
    g_hMutex = NULL;

    return MH_OK;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_SetThreadFreezeMethod(MH_THREAD_FREEZE_METHOD method)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    if (method == MH_FREEZE_METHOD_FAST_UNDOCUMENTED && !pNtGetNextThread)
    {
        HMODULE hNtdll = GetModuleHandle(L"ntdll.dll");
        if (hNtdll)
            pNtGetNextThread = (NtGetNextThread_t)GetProcAddress(hNtdll, "NtGetNextThread");

        if (!pNtGetNextThread)
        {
            // Fall back to the original method.
            method = MH_FREEZE_METHOD_ORIGINAL;
        }
    }

    g_threadFreezeMethod = method;

    ReleaseMutex(g_hMutex);

    return MH_OK;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_CreateHookEx(ULONG_PTR hookIdent, LPVOID pTarget, LPVOID pDetour, LPVOID *ppOriginal)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;

    if (IsExecutableAddress(pTarget) && IsExecutableAddress(pDetour))
    {
        UINT pos = FindHookEntry(hookIdent, pTarget);
        if (pos == INVALID_HOOK_POS)
        {
            PEXEC_BUFFER pBuffer = (PEXEC_BUFFER)AllocateBuffer(pTarget);
            if (pBuffer != NULL)
            {
                PHOOK_ENTRY pHook = AddHookEntry();
                if (pHook != NULL)
                {
                    pBuffer->hookIdent = hookIdent;
                    pBuffer->pDisableHookChain = DisableHookChain;
                    CreateRelayFunction(&pBuffer->jmpRelay, pDetour);

                    pHook->hookIdent = hookIdent;
                    pHook->pTarget = pTarget;
                    pHook->pDetour = pDetour;
                    pHook->pExecBuffer = pBuffer;
                    pHook->isEnabled = FALSE;
                    pHook->queueEnable = FALSE;

                    if (ppOriginal != NULL)
                        *ppOriginal = pBuffer->trampoline;
                }
                else
                {
                    status = MH_ERROR_MEMORY_ALLOC;
                }

                if (status != MH_OK)
                {
                    FreeBuffer(pBuffer);
                }
            }
            else
            {
                status = MH_ERROR_MEMORY_ALLOC;
            }
        }
        else
        {
            status = MH_ERROR_ALREADY_CREATED;
        }
    }
    else
    {
        status = MH_ERROR_NOT_EXECUTABLE;
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_CreateHook(LPVOID pTarget, LPVOID pDetour, LPVOID *ppOriginal)
{
    return MH_CreateHookEx(MH_DEFAULT_IDENT, pTarget, pDetour, ppOriginal);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_RemoveHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        status = EnableHooksLL(hookIdent, pTarget, FALSE);
        if (status == MH_OK)
        {
            UINT i = 0;
            while (i < g_hooks.size)
            {
                PHOOK_ENTRY pHook = &g_hooks.pItems[i];
                if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
                    (pTarget == MH_ALL_HOOKS || (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget))
                {
                    FreeBuffer(pHook->pExecBuffer);
                    DeleteHookEntry(i);
                }
                else
                {
                    ++i;
                }
            }
        }
    }
    else
    {
        UINT pos = FindHookEntry(hookIdent, pTarget);
        if (pos != INVALID_HOOK_POS)
        {
            if (g_hooks.pItems[pos].isEnabled)
            {
                FROZEN_THREADS threads;
                status = Freeze(&threads);
                if (status == MH_OK)
                {
                    status = EnableHookLL(pos, FALSE, &threads);

                    Unfreeze(&threads);
                }
            }

            if (status == MH_OK)
            {
                FreeBuffer(g_hooks.pItems[pos].pExecBuffer);
                DeleteHookEntry(pos);
            }
        }
        else
        {
            status = MH_ERROR_NOT_CREATED;
        }
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_RemoveHook(LPVOID pTarget)
{
    return MH_RemoveHookEx(MH_DEFAULT_IDENT, pTarget);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_RemoveDisabledHooksEx(ULONG_PTR hookIdent)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;

    UINT i = 0;
    while (i < g_hooks.size)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
            !pHook->isEnabled)
        {
            FreeBuffer(pHook->pExecBuffer);
            DeleteHookEntry(i);
        }
        else
        {
            ++i;
        }
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_RemoveDisabledHooks()
{
    return MH_RemoveDisabledHooksEx(MH_DEFAULT_IDENT);
}

//-------------------------------------------------------------------------
static MH_STATUS WINAPI DisableHookChain(ULONG_PTR hookIdent, LPVOID pTarget, UINT parentPos, ENABLE_HOOK_LL_PROC ParentEnableHookLL, PFROZEN_THREADS pThreads)
{
    UINT pos = FindHookEntry(hookIdent, pTarget);
    if (pos == INVALID_HOOK_POS)
        return MH_ERROR_NOT_CREATED;

    if (!g_hooks.pItems[pos].isEnabled)
        return MH_ERROR_DISABLED;

    // We're not Freeze()-ing the threads here, because we assume that the function
    // was called from a different MinHook module, which already suspended all threads.

    MH_STATUS status = EnableHookLL(pos, FALSE, pThreads);
    if (status != MH_OK)
        return status;

    status = ParentEnableHookLL(parentPos, FALSE, pThreads);
    if (status != MH_OK)
        return status;

    return EnableHookLL(pos, TRUE, pThreads);
}

//-------------------------------------------------------------------------
static MH_STATUS EnableHook(ULONG_PTR hookIdent, LPVOID pTarget, BOOL enable)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        status = EnableHooksLL(hookIdent, pTarget, enable);
    }
    else
    {
        UINT pos = FindHookEntry(hookIdent, pTarget);
        if (pos != INVALID_HOOK_POS)
        {
            if (g_hooks.pItems[pos].isEnabled != enable)
            {
                FROZEN_THREADS threads;
                status = Freeze(&threads);
                if (status == MH_OK)
                {
                    status = EnableHookLL(pos, enable, &threads);

                    Unfreeze(&threads);
                }
            }
            else
            {
                status = enable ? MH_ERROR_ENABLED : MH_ERROR_DISABLED;
            }
        }
        else
        {
            status = MH_ERROR_NOT_CREATED;
        }
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_EnableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    return EnableHook(hookIdent, pTarget, TRUE);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_EnableHook(LPVOID pTarget)
{
    return MH_EnableHookEx(MH_DEFAULT_IDENT, pTarget);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_DisableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    return EnableHook(hookIdent, pTarget, FALSE);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_DisableHook(LPVOID pTarget)
{
    return MH_DisableHookEx(MH_DEFAULT_IDENT, pTarget);
}

//-------------------------------------------------------------------------
static MH_STATUS QueueHook(ULONG_PTR hookIdent, LPVOID pTarget, BOOL queueEnable)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        UINT i;
        for (i = 0; i < g_hooks.size; ++i)
        {
            PHOOK_ENTRY pHook = &g_hooks.pItems[i];
            if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
                (pTarget == MH_ALL_HOOKS || (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget))
            {
                pHook->queueEnable = queueEnable;
            }
        }
    }
    else
    {
        UINT pos = FindHookEntry(hookIdent, pTarget);
        if (pos != INVALID_HOOK_POS)
        {
            g_hooks.pItems[pos].queueEnable = queueEnable;
        }
        else
        {
            status = MH_ERROR_NOT_CREATED;
        }
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_QueueEnableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    return QueueHook(hookIdent, pTarget, TRUE);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_QueueEnableHook(LPVOID pTarget)
{
    return MH_QueueEnableHookEx(MH_DEFAULT_IDENT, pTarget);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_QueueDisableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    return QueueHook(hookIdent, pTarget, FALSE);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_QueueDisableHook(LPVOID pTarget)
{
    return MH_QueueDisableHookEx(MH_DEFAULT_IDENT, pTarget);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_ApplyQueuedEx(ULONG_PTR hookIdent)
{
    if (g_hMutex == NULL)
        return MH_ERROR_NOT_INITIALIZED;

    if (WaitForSingleObject(g_hMutex, INFINITE) != WAIT_OBJECT_0)
        return MH_ERROR_MUTEX_FAILURE;

    MH_STATUS status = MH_OK;
    UINT i, first = INVALID_HOOK_POS;

    for (i = 0; i < g_hooks.size; ++i)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
            pHook->isEnabled != pHook->queueEnable)
        {
            first = i;
            break;
        }
    }

    if (first != INVALID_HOOK_POS)
    {
        FROZEN_THREADS threads;
        status = Freeze(&threads);
        if (status == MH_OK)
        {
            for (i = first; i < g_hooks.size; ++i)
            {
                PHOOK_ENTRY pHook = &g_hooks.pItems[i];
                if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
                    pHook->isEnabled != pHook->queueEnable)
                {
                    MH_STATUS enable_status = EnableHookLL(i, pHook->queueEnable, &threads);

                    // Instead of stopping on the first error, we apply as much
                    // hooks as we can, and return the last error, if any.
                    if (enable_status != MH_OK)
                        status = enable_status;
                }
            }

            Unfreeze(&threads);
        }
    }

    ReleaseMutex(g_hMutex);

    return status;
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_ApplyQueued(VOID)
{
    return MH_ApplyQueuedEx(MH_DEFAULT_IDENT);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_CreateHookApiEx(
    LPCWSTR pszModule, LPCSTR pszProcName, LPVOID pDetour,
    LPVOID *ppOriginal, LPVOID *ppTarget)
{
    HMODULE hModule;
    LPVOID  pTarget;

    hModule = GetModuleHandleW(pszModule);
    if (hModule == NULL)
        return MH_ERROR_MODULE_NOT_FOUND;

    pTarget = (LPVOID)GetProcAddress(hModule, pszProcName);
    if (pTarget == NULL)
        return MH_ERROR_FUNCTION_NOT_FOUND;

    if (ppTarget != NULL)
        *ppTarget = pTarget;

    return MH_CreateHook(pTarget, pDetour, ppOriginal);
}

//-------------------------------------------------------------------------
MH_STATUS WINAPI MH_CreateHookApi(
    LPCWSTR pszModule, LPCSTR pszProcName, LPVOID pDetour, LPVOID *ppOriginal)
{
    return MH_CreateHookApiEx(pszModule, pszProcName, pDetour, ppOriginal, NULL);
}

//-------------------------------------------------------------------------
const char *WINAPI MH_StatusToString(MH_STATUS status)
{
#define MH_ST2STR(x)    \
    case x:             \
        return #x;

    switch (status) {
        MH_ST2STR(MH_UNKNOWN)
        MH_ST2STR(MH_OK)
        MH_ST2STR(MH_ERROR_ALREADY_INITIALIZED)
        MH_ST2STR(MH_ERROR_NOT_INITIALIZED)
        MH_ST2STR(MH_ERROR_ALREADY_CREATED)
        MH_ST2STR(MH_ERROR_NOT_CREATED)
        MH_ST2STR(MH_ERROR_ENABLED)
        MH_ST2STR(MH_ERROR_DISABLED)
        MH_ST2STR(MH_ERROR_NOT_EXECUTABLE)
        MH_ST2STR(MH_ERROR_UNSUPPORTED_FUNCTION)
        MH_ST2STR(MH_ERROR_MEMORY_ALLOC)
        MH_ST2STR(MH_ERROR_MEMORY_PROTECT)
        MH_ST2STR(MH_ERROR_MODULE_NOT_FOUND)
        MH_ST2STR(MH_ERROR_FUNCTION_NOT_FOUND)
        MH_ST2STR(MH_ERROR_MUTEX_FAILURE)
    }

#undef MH_ST2STR

    return "(unknown)";
}
