#include "MinHook.h"

#include "SlimDetours/SlimDetours.h"

// Initial capacity of the HOOK_ENTRY buffer.
#define INITIAL_HOOK_CAPACITY   32

// Special hook position values.
#define INVALID_HOOK_POS UINT_MAX

// Memory protection flags to check the executable address.
#define PAGE_EXECUTE_FLAGS \
    (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY)

typedef struct _HOOK_ENTRY
{
    ULONG_PTR hookIdent;
    LPVOID pTarget;
    LPVOID pDetour;
    LPVOID pTargetOrTrampoline;
    LPVOID *ppOriginal;
    LPVOID pTrampolineToFree;
    UINT8 isEnabled : 1;
    UINT8 queueEnable : 1;
    HRESULT bulkLastError;
} HOOK_ENTRY, *PHOOK_ENTRY;

static CRITICAL_SECTION g_criticalSection;

static BOOL g_initialized = FALSE;

// Thread freeze related variables.
static MH_THREAD_FREEZE_METHOD g_threadFreezeMethod = MH_FREEZE_METHOD_ORIGINAL;

// Bulk operation related variables.
static BOOL g_bulkContinueOnError = FALSE;
static MH_ERROR_CALLBACK g_bulkErrorCallback = NULL;

// Hook entries.
struct
{
    PHOOK_ENTRY pItems;     // Data heap
    UINT        capacity;   // Size of allocated data heap, items
    UINT        size;       // Actual number of data items
} g_hooks;

// Returns INVALID_HOOK_POS if not found.
static UINT FindHookEntry(ULONG_PTR hookIdent, LPVOID pTarget, UINT pos)
{
    UINT i;
    for (i = pos; i < g_hooks.size; ++i)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if ((hookIdent == MH_ALL_IDENTS || pHook->hookIdent == hookIdent) &&
            (pTarget == MH_ALL_HOOKS || (ULONG_PTR)pTarget == (ULONG_PTR)pHook->pTarget))
        {
            return i;
        }
    }

    return INVALID_HOOK_POS;
}

static UINT FindHookEntryEnabled(ULONG_PTR hookIdent, LPVOID pTarget, UINT pos, BOOL enabled)
{
    UINT i = FindHookEntry(hookIdent, pTarget, pos);
    while (i != INVALID_HOOK_POS)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if (pHook->isEnabled == enabled)
        {
            break;
        }

        i = FindHookEntry(hookIdent, pTarget, i + 1);
    }

    return i;
}

static UINT FindHookEntryQueued(ULONG_PTR hookIdent, LPVOID pTarget, UINT pos)
{
    UINT i = FindHookEntry(hookIdent, pTarget, pos);
    while (i != INVALID_HOOK_POS)
    {
        PHOOK_ENTRY pHook = &g_hooks.pItems[i];
        if (pHook->queueEnable != pHook->isEnabled)
        {
            break;
        }

        i = FindHookEntry(hookIdent, pTarget, i + 1);
    }

    return i;
}

static PHOOK_ENTRY AddHookEntry()
{
    if (g_hooks.pItems == NULL)
    {
        g_hooks.capacity = INITIAL_HOOK_CAPACITY;
        g_hooks.pItems = (PHOOK_ENTRY)HeapAlloc(
            GetProcessHeap(), 0, g_hooks.capacity * sizeof(HOOK_ENTRY));
        if (g_hooks.pItems == NULL)
            return NULL;
    }
    else if (g_hooks.size >= g_hooks.capacity)
    {
        PHOOK_ENTRY p = (PHOOK_ENTRY)HeapReAlloc(
            GetProcessHeap(), 0, g_hooks.pItems, (g_hooks.capacity * 2) * sizeof(HOOK_ENTRY));
        if (p == NULL)
            return NULL;

        g_hooks.capacity *= 2;
        g_hooks.pItems = p;
    }

    return &g_hooks.pItems[g_hooks.size++];
}

static VOID DeleteHookEntry(UINT pos)
{
    if (pos < g_hooks.size - 1)
        g_hooks.pItems[pos] = g_hooks.pItems[g_hooks.size - 1];

    g_hooks.size--;

    if (g_hooks.capacity / 2 >= INITIAL_HOOK_CAPACITY && g_hooks.capacity / 2 >= g_hooks.size)
    {
        PHOOK_ENTRY p = (PHOOK_ENTRY)HeapReAlloc(
            GetProcessHeap(), 0, g_hooks.pItems, (g_hooks.capacity / 2) * sizeof(HOOK_ENTRY));
        if (p == NULL)
            return;

        g_hooks.capacity /= 2;
        g_hooks.pItems = p;
    }
}

static BOOL IsExecutableAddress(LPVOID pAddress)
{
    MEMORY_BASIC_INFORMATION mi;
    VirtualQuery(pAddress, &mi, sizeof(mi));

    return (mi.State == MEM_COMMIT && (mi.Protect & PAGE_EXECUTE_FLAGS));
}

static void FreeHookTrampolineIfNeeded(PHOOK_ENTRY pHook)
{
    if (pHook->pTrampolineToFree)
    {
        SlimDetoursFreeTrampoline(pHook->pTrampolineToFree);
        pHook->pTrampolineToFree = NULL;
    }
}

static HRESULT MHDetoursTransactionBegin()
{
    DETOUR_TRANSACTION_OPTIONS options = {
        .fSuspendThreads = g_threadFreezeMethod != MH_FREEZE_METHOD_NONE_UNSAFE,
    };
    return SlimDetoursTransactionBeginEx(&options);
}

static HRESULT MHDetoursAttach(PHOOK_ENTRY pHook)
{
    FreeHookTrampolineIfNeeded(pHook);
    return SlimDetoursAttach(pHook->ppOriginal, pHook->pDetour);
}

static HRESULT MHDetoursDetach(PHOOK_ENTRY pHook)
{
    DETOUR_DETACH_OPTIONS options = {
        .ppTrampolineToFreeManually = &pHook->pTrampolineToFree,
    };
    return SlimDetoursDetachEx(pHook->ppOriginal, pHook->pDetour, &options);
}

static MH_STATUS CreateHook(ULONG_PTR hookIdent, LPVOID pTarget, LPVOID pDetour, LPVOID *ppOriginal)
{
    MH_STATUS status = MH_OK;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        status = MH_ERROR_UNSUPPORTED_FUNCTION;
    }
    else if (FindHookEntry(hookIdent, pTarget, 0) != INVALID_HOOK_POS)
    {
        status = MH_ERROR_ALREADY_CREATED;
    }
    else if (!IsExecutableAddress(pTarget) || !IsExecutableAddress(pDetour))
    {
        status = MH_ERROR_NOT_EXECUTABLE;
    }
    else
    {
        PHOOK_ENTRY pHook = AddHookEntry();
        if (pHook != NULL)
        {
            pHook->hookIdent = hookIdent;
            pHook->pTarget = pTarget;
            pHook->pDetour = pDetour;

            if (ppOriginal)
            {
                // Check if the ppOriginal pointer was already specified for
                // other hooks. If so, modify them to use pTargetOrTrampoline.
                // This fixes a problem with the following questionable code:
                //
                // MH_CreateHook(pTarget1, pDetour, &ppOriginal);
                // // ...
                // MH_CreateHook(pTarget2, pDetour, &ppOriginal);
                //
                // While it's unsupported to have the same ppOriginal pointer
                // specified more than once, it worked in MinHook, and some
                // Windhawk mods which call HandleLoadedExplorerPatcher rely on
                // it.
                for (UINT i = 0; i < g_hooks.size - 1; ++i)
                {
                    PHOOK_ENTRY pHookIter = &g_hooks.pItems[i];
                    if (pHookIter->ppOriginal == ppOriginal)
                    {
                        pHookIter->pTargetOrTrampoline = *pHookIter->ppOriginal;
                        pHookIter->ppOriginal = &pHookIter->pTargetOrTrampoline;
                    }
                }

                pHook->pTargetOrTrampoline = NULL;
                pHook->ppOriginal = ppOriginal;
                *ppOriginal = pTarget;
            }
            else
            {
                pHook->pTargetOrTrampoline = pTarget;
                pHook->ppOriginal = &pHook->pTargetOrTrampoline;
            }

            pHook->pTrampolineToFree = NULL;
            pHook->isEnabled = FALSE;
            pHook->queueEnable = FALSE;
            pHook->bulkLastError = S_OK;
        }
        else
        {
            status = MH_ERROR_MEMORY_ALLOC;
        }
    }

    return status;
}

static MH_STATUS EnableHook(ULONG_PTR hookIdent, LPVOID pTarget, BOOL enable)
{
    MH_STATUS status = MH_OK;
    HRESULT hr;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        UINT pos = FindHookEntryEnabled(hookIdent, pTarget, 0, !enable);
        if (pos != INVALID_HOOK_POS)
        {
            hr = MHDetoursTransactionBegin();
            if (SUCCEEDED(hr))
            {
                do
                {
                    PHOOK_ENTRY pHook = &g_hooks.pItems[pos];

                    if (enable)
                    {
                        hr = MHDetoursAttach(pHook);
                    }
                    else
                    {
                        hr = MHDetoursDetach(pHook);
                    }

                    pHook->bulkLastError = hr;

                    if (g_bulkContinueOnError)
                    {
                        hr = S_OK;
                    }
                    else if (FAILED(hr))
                    {
                        break;
                    }

                    pos = FindHookEntryEnabled(hookIdent, pTarget, pos + 1, !enable);
                } while (pos != INVALID_HOOK_POS);

                if (SUCCEEDED(hr))
                {
                    hr = SlimDetoursTransactionCommit();
                    if (SUCCEEDED(hr))
                    {
                        UINT pos = FindHookEntryEnabled(hookIdent, pTarget, 0, !enable);
                        while (pos != INVALID_HOOK_POS)
                        {
                            PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
                            if (SUCCEEDED(pHook->bulkLastError))
                            {
                                pHook->isEnabled = enable;
                                pHook->queueEnable = enable;
                            }
                            else if (g_bulkErrorCallback)
                            {
                                g_bulkErrorCallback(pHook->pTarget, pHook->bulkLastError);
                            }
                            pos = FindHookEntryEnabled(hookIdent, pTarget, pos + 1, !enable);
                        }
                    }
                    else
                    {
                        status = MH_ERROR_DETOURS_TRANSACTION_COMMIT;
                    }
                }
                else
                {
                    status = MH_ERROR_UNSUPPORTED_FUNCTION;
                    SlimDetoursTransactionAbort();
                }
            }
            else
            {
                status = MH_ERROR_DETOURS_TRANSACTION_BEGIN;
            }
        }
    }
    else
    {
        UINT pos = FindHookEntry(hookIdent, pTarget, 0);
        if (pos != INVALID_HOOK_POS)
        {
            PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
            if (pHook->isEnabled != enable)
            {
                hr = MHDetoursTransactionBegin();
                if (SUCCEEDED(hr))
                {
                    if (enable)
                    {
                        hr = MHDetoursAttach(pHook);
                    }
                    else
                    {
                        hr = MHDetoursDetach(pHook);
                    }

                    if (SUCCEEDED(hr))
                    {
                        hr = SlimDetoursTransactionCommit();
                        if (SUCCEEDED(hr))
                        {
                            pHook->isEnabled = enable;
                            pHook->queueEnable = enable;
                        }
                        else
                        {
                            status = MH_ERROR_DETOURS_TRANSACTION_COMMIT;
                        }
                    }
                    else
                    {
                        status = MH_ERROR_UNSUPPORTED_FUNCTION;
                        SlimDetoursTransactionAbort();
                    }
                }
                else
                {
                    status = MH_ERROR_DETOURS_TRANSACTION_BEGIN;
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

    return status;
}

static void RemoveDisabledHooks(ULONG_PTR hookIdent, LPVOID pTarget)
{
    UINT pos = FindHookEntryEnabled(hookIdent, pTarget, 0, FALSE);
    while (pos != INVALID_HOOK_POS)
    {
        FreeHookTrampolineIfNeeded(&g_hooks.pItems[pos]);
        DeleteHookEntry(pos);
        pos = FindHookEntryEnabled(hookIdent, pTarget, pos, FALSE);
    }
}

static MH_STATUS QueueHook(ULONG_PTR hookIdent, LPVOID pTarget, BOOL queueEnable)
{
    MH_STATUS status = MH_OK;

    if (hookIdent == MH_ALL_IDENTS || pTarget == MH_ALL_HOOKS)
    {
        UINT pos = FindHookEntry(hookIdent, pTarget, 0);
        while (pos != INVALID_HOOK_POS)
        {
            PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
            pHook->queueEnable = queueEnable;
            pos = FindHookEntry(hookIdent, pTarget, pos + 1);
        }
    }
    else
    {
        UINT pos = FindHookEntry(hookIdent, pTarget, 0);
        if (pos != INVALID_HOOK_POS)
        {
            PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
            pHook->queueEnable = queueEnable;
        }
        else
        {
            status = MH_ERROR_NOT_CREATED;
        }
    }

    return status;
}

static MH_STATUS ApplyQueued(ULONG_PTR hookIdent)
{
    MH_STATUS status = MH_OK;
    HRESULT hr;

    UINT pos = FindHookEntryQueued(hookIdent, MH_ALL_HOOKS, 0);
    if (pos != INVALID_HOOK_POS)
    {
        hr = MHDetoursTransactionBegin();
        if (SUCCEEDED(hr))
        {
            do
            {
                PHOOK_ENTRY pHook = &g_hooks.pItems[pos];

                if (pHook->queueEnable)
                {
                    hr = MHDetoursAttach(pHook);
                }
                else
                {
                    hr = MHDetoursDetach(pHook);
                }

                pHook->bulkLastError = hr;

                if (g_bulkContinueOnError)
                {
                    hr = S_OK;
                }
                else if (FAILED(hr))
                {
                    break;
                }

                pos = FindHookEntryQueued(hookIdent, MH_ALL_HOOKS, pos + 1);
            } while (pos != INVALID_HOOK_POS);

            if (SUCCEEDED(hr))
            {
                hr = SlimDetoursTransactionCommit();
                if (SUCCEEDED(hr))
                {
                    UINT pos = FindHookEntryQueued(hookIdent, MH_ALL_HOOKS, 0);
                    while (pos != INVALID_HOOK_POS)
                    {
                        PHOOK_ENTRY pHook = &g_hooks.pItems[pos];
                        if (SUCCEEDED(pHook->bulkLastError))
                        {
                            pHook->isEnabled = pHook->queueEnable;
                        }
                        else if (g_bulkErrorCallback)
                        {
                            g_bulkErrorCallback(pHook->pTarget, pHook->bulkLastError);
                        }
                        pos = FindHookEntryQueued(hookIdent, MH_ALL_HOOKS, pos + 1);
                    }
                }
                else
                {
                    status = MH_ERROR_DETOURS_TRANSACTION_COMMIT;
                }
            }
            else
            {
                status = MH_ERROR_UNSUPPORTED_FUNCTION;
                SlimDetoursTransactionAbort();
            }
        }
        else
        {
            status = MH_ERROR_DETOURS_TRANSACTION_BEGIN;
        }
    }

    return status;
}

MH_STATUS WINAPI MH_Initialize(VOID)
{
    if (g_initialized)
        return MH_ERROR_ALREADY_INITIALIZED;

    InitializeCriticalSection(&g_criticalSection);

    g_initialized = TRUE;
    return MH_OK;
}

MH_STATUS WINAPI MH_Uninitialize(VOID)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = EnableHook(MH_ALL_IDENTS, MH_ALL_HOOKS, FALSE);
    RemoveDisabledHooks(MH_ALL_IDENTS, MH_ALL_HOOKS);

    if (status == MH_OK && g_hooks.size > 0)
        status = MH_ERROR_UNABLE_TO_UNINITIALIZE;

    if (status != MH_OK)
    {
        LeaveCriticalSection(&g_criticalSection);
        return status;
    }

    SlimDetoursUninitialize();

    HeapFree(GetProcessHeap(), 0, g_hooks.pItems);

    g_hooks.pItems = NULL;
    g_hooks.capacity = 0;
    g_hooks.size = 0;

    g_initialized = FALSE;

    LeaveCriticalSection(&g_criticalSection);
    DeleteCriticalSection(&g_criticalSection);

    return MH_OK;
}

MH_STATUS WINAPI MH_SetThreadFreezeMethod(MH_THREAD_FREEZE_METHOD method)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    g_threadFreezeMethod = method;

    LeaveCriticalSection(&g_criticalSection);

    return MH_OK;
}

MH_STATUS WINAPI MH_SetBulkOperationMode(BOOL continueOnError, MH_ERROR_CALLBACK errorCallback)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    g_bulkContinueOnError = continueOnError;
    g_bulkErrorCallback = errorCallback;

    LeaveCriticalSection(&g_criticalSection);

    return MH_OK;
}

MH_STATUS WINAPI MH_CreateHook(LPVOID pTarget, LPVOID pDetour, LPVOID *ppOriginal)
{
    return MH_CreateHookEx(MH_DEFAULT_IDENT, pTarget, pDetour, ppOriginal);
}
MH_STATUS WINAPI MH_CreateHookEx(ULONG_PTR hookIdent, LPVOID pTarget, LPVOID pDetour, LPVOID *ppOriginal)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = CreateHook(hookIdent, pTarget, pDetour, ppOriginal);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_CreateHookApi(
    LPCWSTR pszModule, LPCSTR pszProcName, LPVOID pDetour, LPVOID *ppOriginal)
{
    return MH_CreateHookApiEx(pszModule, pszProcName, pDetour, ppOriginal, NULL);
}

MH_STATUS WINAPI MH_CreateHookApiEx(
    LPCWSTR pszModule, LPCSTR pszProcName, LPVOID pDetour, LPVOID *ppOriginal, LPVOID *ppTarget)
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

MH_STATUS WINAPI MH_RemoveHook(LPVOID pTarget)
{
    return MH_RemoveHookEx(MH_DEFAULT_IDENT, pTarget);
}
MH_STATUS WINAPI MH_RemoveHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = EnableHook(hookIdent, pTarget, FALSE);
    if (status == MH_ERROR_DISABLED)
        status = MH_OK;

    RemoveDisabledHooks(hookIdent, pTarget);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_RemoveDisabledHooks()
{
    return MH_RemoveDisabledHooksEx(MH_DEFAULT_IDENT);
}
MH_STATUS WINAPI MH_RemoveDisabledHooksEx(ULONG_PTR hookIdent)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    RemoveDisabledHooks(hookIdent, MH_ALL_HOOKS);

    LeaveCriticalSection(&g_criticalSection);

    return MH_OK;
}

MH_STATUS WINAPI MH_EnableHook(LPVOID pTarget)
{
    return MH_EnableHookEx(MH_DEFAULT_IDENT, pTarget);
}
MH_STATUS WINAPI MH_EnableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = EnableHook(hookIdent, pTarget, TRUE);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_DisableHook(LPVOID pTarget)
{
    return MH_DisableHookEx(MH_DEFAULT_IDENT, pTarget);
}
MH_STATUS WINAPI MH_DisableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = EnableHook(hookIdent, pTarget, FALSE);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_QueueEnableHook(LPVOID pTarget)
{
    return MH_QueueEnableHookEx(MH_DEFAULT_IDENT, pTarget);
}
MH_STATUS WINAPI MH_QueueEnableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = QueueHook(hookIdent, pTarget, TRUE);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_QueueDisableHook(LPVOID pTarget)
{
    return MH_QueueDisableHookEx(MH_DEFAULT_IDENT, pTarget);
}
MH_STATUS WINAPI MH_QueueDisableHookEx(ULONG_PTR hookIdent, LPVOID pTarget)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = QueueHook(hookIdent, pTarget, FALSE);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

MH_STATUS WINAPI MH_ApplyQueued(VOID)
{
    return MH_ApplyQueuedEx(MH_DEFAULT_IDENT);
}
MH_STATUS WINAPI MH_ApplyQueuedEx(ULONG_PTR hookIdent)
{
    if (!g_initialized)
        return MH_ERROR_NOT_INITIALIZED;

    EnterCriticalSection(&g_criticalSection);

    MH_STATUS status = ApplyQueued(hookIdent);

    LeaveCriticalSection(&g_criticalSection);

    return status;
}

const char *WINAPI MH_StatusToString(MH_STATUS status)
{
#define MH_ST2STR(x)    \
    case x:             \
        return #x

    switch (status)
    {
        MH_ST2STR(MH_OK);
        MH_ST2STR(MH_ERROR_ALREADY_INITIALIZED);
        MH_ST2STR(MH_ERROR_NOT_INITIALIZED);
        MH_ST2STR(MH_ERROR_ALREADY_CREATED);
        MH_ST2STR(MH_ERROR_NOT_CREATED);
        MH_ST2STR(MH_ERROR_ENABLED);
        MH_ST2STR(MH_ERROR_DISABLED);
        MH_ST2STR(MH_ERROR_NOT_EXECUTABLE);
        MH_ST2STR(MH_ERROR_DETOURS_TRANSACTION_BEGIN);
        MH_ST2STR(MH_ERROR_UNSUPPORTED_FUNCTION);
        MH_ST2STR(MH_ERROR_MEMORY_ALLOC);
        MH_ST2STR(MH_ERROR_MODULE_NOT_FOUND);
        MH_ST2STR(MH_ERROR_FUNCTION_NOT_FOUND);
    }

#undef MH_ST2STR

    return "(unknown)";
}
