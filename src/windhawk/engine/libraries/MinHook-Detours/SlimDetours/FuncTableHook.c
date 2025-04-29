/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Function Table Hook Implementation
 *
 * Hook function address in a read-only table, used by COM/IAT/... hooking.
 *
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

/* Function table hook core implementation */

static
NTSTATUS
detour_hook_table_func(
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulOffset,
    _Out_opt_ PVOID* ppOldFunc,
    _In_ PVOID pNewFunc)
{
    NTSTATUS Status;
    PVOID Base, *Method;
    SIZE_T Size;
    ULONG OldProtect;

    Base = Method = Add2Ptr(pFuncTable, ulOffset);
    Size = sizeof(PVOID);
    Status = NtProtectVirtualMemory(NtCurrentProcess(), &Base, &Size, PAGE_READWRITE, &OldProtect);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }
    if (ppOldFunc != NULL)
    {
        *ppOldFunc = *Method;
    }
    *Method = pNewFunc;
    NtProtectVirtualMemory(NtCurrentProcess(), &Base, &Size, OldProtect, &OldProtect);

    return STATUS_SUCCESS;
}

static
NTSTATUS
detour_hook_table_funcs(
    _In_ BOOL bEnable,
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_FUNC_TABLE_HOOK pHooks)
{
    NTSTATUS Status;
    PVOID Base, *Method;
    SIZE_T Size;
    ULONG i, OldProtect, MaxOffset;

    MaxOffset = 0;
    for (i = 0; i < ulCount; i++)
    {
        if (pHooks[i].ulOffset > MaxOffset)
        {
            MaxOffset = pHooks[i].ulOffset;
        }
    }
    Base = pFuncTable;
    Size = MaxOffset + sizeof(PVOID);
    Status = NtProtectVirtualMemory(NtCurrentProcess(), &Base, &Size, PAGE_READWRITE, &OldProtect);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }

    for (i = 0; i < ulCount; i++)
    {
        Method = Add2Ptr(pFuncTable, pHooks[i].ulOffset);
        if (bEnable)
        {
            *pHooks[i].ppOldFunc = *Method;
            *Method = pHooks[i].pNewFunc;
        } else
        {
            *Method = *pHooks[i].ppOldFunc;
        }
    }

    NtProtectVirtualMemory(NtCurrentProcess(), &Base, &Size, OldProtect, &OldProtect);
    return STATUS_SUCCESS;
}

HRESULT
NTAPI
SlimDetoursFuncTableHook(
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulOffset,
    _Out_opt_ PVOID* ppOldFunc,
    _In_ PVOID pNewFunc)
{
    return HRESULT_FROM_NT(detour_hook_table_func(pFuncTable, ulOffset, ppOldFunc, pNewFunc));
}

HRESULT
NTAPI
SlimDetoursFuncTableHooks(
    _In_ BOOL bEnable,
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_FUNC_TABLE_HOOK pHooks)
{
    return HRESULT_FROM_NT(detour_hook_table_funcs(bEnable, pFuncTable, ulCount, pHooks));
}

/* COM Hook */

typedef
_Check_return_
HRESULT
STDAPICALLTYPE
FN_CoCreateInstanceEx(
    _In_ REFCLSID Clsid,
    _In_opt_ IUnknown* punkOuter,
    _In_ DWORD dwClsCtx,
    _In_opt_ COSERVERINFO* pServerInfo,
    _In_ DWORD dwCount,
    _Inout_updates_(dwCount) MULTI_QI* pResults);

static PVOID g_hComBase = NULL;

static CONST UNICODE_STRING g_usCombaseDllName = RTL_CONSTANT_STRING(L"combase.dll");
static CONST ANSI_STRING g_asCoCreateInstanceEx = RTL_CONSTANT_STRING("CoCreateInstanceEx");
static FN_CoCreateInstanceEx* g_pfnCoCreateInstanceEx = NULL;

static PS_RUNONCE g_stRunOnceCombaseInit = PS_RUNONCE_INIT;
static NTSTATUS g_lCombaseInitStatus = STATUS_UNSUCCESSFUL;

HRESULT
NTAPI
SlimDetoursCOMHooks(
    _In_ BOOL bEnable,
    _In_ REFCLSID rCLSID,
    _In_ REFCLSID rIID,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_FUNC_TABLE_HOOK pHooks)
{
    NTSTATUS Status;
    HRESULT hr;
    MULTI_QI MQI = { rIID };

    /* Initialize combase.dll */
    if (PS_RunOnceBegin(&g_stRunOnceCombaseInit))
    {
        Status = LdrLoadDll(NULL, NULL, (PUNICODE_STRING)&g_usCombaseDllName, &g_hComBase);
        if (!NT_SUCCESS(Status))
        {
            goto _Init_Exit;
        }
        Status = LdrGetProcedureAddress(g_hComBase, (PANSI_STRING)&g_asCoCreateInstanceEx, 0, (PVOID*)&g_pfnCoCreateInstanceEx);
        if (!NT_SUCCESS(Status))
        {
            LdrUnloadDll(g_hComBase);
            g_hComBase = NULL;
            goto _Init_Exit;
        }
        Status = STATUS_SUCCESS;
_Init_Exit:
        g_lCombaseInitStatus = Status;
        PS_RunOnceEnd(&g_stRunOnceCombaseInit, Status == STATUS_SUCCESS);
    }
    if (!NT_SUCCESS(g_lCombaseInitStatus))
    {
        return HRESULT_FROM_NT(g_lCombaseInitStatus);
    }

    /* Create COM Object and set VTable Hooks */
    hr = g_pfnCoCreateInstanceEx(rCLSID, NULL, CLSCTX_ALL, NULL, 1, &MQI);
    if (FAILED(hr))
    {
        goto _Create_Fail_0;
    }
    _Analysis_assume_(MQI.pItf != NULL);
    Status = detour_hook_table_funcs(bEnable, (PVOID*)MQI.pItf->lpVtbl, ulCount, pHooks);
    if (!NT_SUCCESS(Status))
    {
        hr = HRESULT_FROM_NT(Status);
        goto _Create_Fail_1;
    }
    hr = S_OK;

    /* Cleanup */
_Create_Fail_1:
    MQI.pItf->lpVtbl->Release(MQI.pItf);
_Create_Fail_0:
    return hr;
}
