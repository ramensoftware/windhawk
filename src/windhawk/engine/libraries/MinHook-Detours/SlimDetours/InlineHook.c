/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Inline Hook Wrappers
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

HRESULT
NTAPI
SlimDetoursInlineHook(
    _In_ BOOL bEnable,
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour)
{
    HRESULT hr;

    hr = SlimDetoursTransactionBegin();
    if (FAILED(hr))
    {
        return hr;
    }
    hr = bEnable ? SlimDetoursAttach(ppPointer, pDetour) : SlimDetoursDetach(ppPointer, pDetour);
    if (FAILED(hr))
    {
        SlimDetoursTransactionAbort();
        return hr;
    }
    return SlimDetoursTransactionCommit();
}

HRESULT
NTAPI
SlimDetoursInitInlineHooks(
    _In_ HMODULE hModule,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_INLINE_HOOK pHooks)
{
    NTSTATUS Status;
    ULONG i, uOridinal;
    ANSI_STRING FuncName, *pFuncName;

    for (i = 0; i < ulCount; i++)
    {
        if ((ULONG_PTR)pHooks[i].pszFuncName > MAXWORD)
        {
            Status = RtlInitAnsiStringEx(&FuncName, pHooks[i].pszFuncName);
            if (!NT_SUCCESS(Status))
            {
                return HRESULT_FROM_NT(Status);
            }
            pFuncName = &FuncName;
            uOridinal = 0;
        } else
        {
            pFuncName = NULL;
            uOridinal = (ULONG)(ULONG_PTR)pHooks[i].pszFuncName;
        }
        Status = LdrGetProcedureAddress(hModule, pFuncName, uOridinal, pHooks[i].ppPointer);
        if (!NT_SUCCESS(Status))
        {
            return HRESULT_FROM_NT(Status);
        }
    }

    return HRESULT_FROM_NT(STATUS_SUCCESS);
}

HRESULT
NTAPI
SlimDetoursInlineHooks(
    _In_ BOOL bEnable,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_INLINE_HOOK pHooks)
{
    HRESULT hr;
    ULONG i;

    hr = SlimDetoursTransactionBegin();
    if (FAILED(hr))
    {
        return hr;
    }
    for (i = 0; i < ulCount; i++)
    {
        hr = bEnable ?
            SlimDetoursAttach(pHooks[i].ppPointer, pHooks[i].pDetour) :
            SlimDetoursDetach(pHooks[i].ppPointer, pHooks[i].pDetour);
        if (FAILED(hr))
        {
            SlimDetoursTransactionAbort();
            return hr;
        }
    }
    return SlimDetoursTransactionCommit();
}
