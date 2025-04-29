/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours)
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 *
 * Source base on Microsoft Detours:
 *
 * Microsoft Research Detours Package, Version 4.0.1
 * Copyright (c) Microsoft Corporation. All rights reserved.
 * Licensed under the MIT license.
 */

#pragma once

#include <Windows.h>

#if !defined(_X86_) && !defined(_AMD64_) && !defined(_ARM64_)
#error Unsupported architecture (x86, amd64, arm64)
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Improved original Detours API */

#define DETOUR_INSTRUCTION_TARGET_NONE ((PVOID)0)
#define DETOUR_INSTRUCTION_TARGET_DYNAMIC ((PVOID)(LONG_PTR)-1)

typedef struct _DETOUR_TRANSACTION_OPTIONS
{
    BOOL fSuspendThreads;
} DETOUR_TRANSACTION_OPTIONS, *PDETOUR_TRANSACTION_OPTIONS;

typedef const DETOUR_TRANSACTION_OPTIONS* PCDETOUR_TRANSACTION_OPTIONS;

HRESULT
NTAPI
SlimDetoursTransactionBeginEx(
    _In_ PCDETOUR_TRANSACTION_OPTIONS pOptions);

FORCEINLINE
HRESULT
SlimDetoursTransactionBegin(VOID)
{
    DETOUR_TRANSACTION_OPTIONS Options;
    Options.fSuspendThreads = TRUE;
    return SlimDetoursTransactionBeginEx(&Options);
}

HRESULT
NTAPI
SlimDetoursTransactionAbort(VOID);

HRESULT
NTAPI
SlimDetoursTransactionCommit(VOID);

HRESULT
NTAPI
SlimDetoursAttach(
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour);

typedef struct _DETOUR_DETACH_OPTIONS
{
    PVOID *ppTrampolineToFreeManually;
} DETOUR_DETACH_OPTIONS, *PDETOUR_DETACH_OPTIONS;

typedef const DETOUR_DETACH_OPTIONS* PCDETOUR_DETACH_OPTIONS;

HRESULT
NTAPI
SlimDetoursDetachEx(
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour,
    _In_ PCDETOUR_DETACH_OPTIONS pOptions);

FORCEINLINE
HRESULT
SlimDetoursDetach(
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour)
{
    DETOUR_DETACH_OPTIONS Options;
    Options.ppTrampolineToFreeManually = NULL;
    return SlimDetoursDetachEx(ppPointer, pDetour, &Options);
}

HRESULT
NTAPI
SlimDetoursFreeTrampoline(
    _In_ PVOID pTrampoline);

PVOID
NTAPI
SlimDetoursCodeFromPointer(
    _In_ PVOID pPointer);

PVOID
NTAPI
SlimDetoursCopyInstruction(
    _In_opt_ PVOID pDst,
    _In_ PVOID pSrc,
    _Out_opt_ PVOID* ppTarget,
    _Out_opt_ LONG* plExtra);

HRESULT
NTAPI
SlimDetoursUninitialize(VOID);

/* Inline Hook, base on Detours */

/// <summary>
/// Set or unset a single inline hook
/// </summary>
/// <param name="bEnable">Set to TRUE to hook, or FALSE to unhook.</param>
/// <param name="ppPointer">See also SlimDetoursAttach or SlimDetoursDetach.</param>
/// <param name="pDetour">See also SlimDetoursAttach or SlimDetoursDetach.</param>
/// <returns>Returns HRESULT</returns>
/// <seealso cref="SlimDetoursAttach"/>
/// <seealso cref="SlimDetoursDetach"/>
HRESULT
NTAPI
SlimDetoursInlineHook(
    _In_ BOOL bEnable,
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour);

typedef struct _DETOUR_INLINE_HOOK
{
    PCSTR pszFuncName;  // Can be an ordinal
    PVOID* ppPointer;   // Pointer to a variable contains the original function address before hooking,
                        // and will be replaced with trampoline address after hooking
    PVOID pDetour;      // Address of detour function
} DETOUR_INLINE_HOOK, *PDETOUR_INLINE_HOOK;

/// <summary>
/// Initialize an inline hook array
/// </summary>
/// <param name="hModule">Handle to the module exported those functions.</param>
/// <param name="ppPointer">See also <c>SlimDetoursAttach</c> or <c>SlimDetoursDetach</c>.</param>
/// <param name="pDetour">See also <c>SlimDetoursAttach</c> or <c>SlimDetoursDetach</c>.</param>
/// <returns>Return HRESULT</returns>
/// <remarks>
/// Get function address by <c>pszFuncName</c> and store in <c>*ppPointer</c>
/// for each <c>DETOUR_INLINE_HOOK</c> element.
/// </remarks>
/// <seealso cref="SlimDetoursAttach"/>
/// <seealso cref="SlimDetoursDetach"/>
HRESULT
NTAPI
SlimDetoursInitInlineHooks(
    _In_ HMODULE hModule,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_INLINE_HOOK pHooks);

/// <summary>
/// Set or unset multiple inline hooks in a same target module
/// </summary>
/// <param name="bEnable">Set to TRUE to hook, or FALSE to unhook.</param>
/// <param name="ulCount">Number of elements in <c>pHooks</c> array.</param>
/// <param name="pHooks">A pointer point to an <c>DETOUR_INLINE_HOOK</c> array.</param>
/// <returns>Return HRESULT</returns>
/// <remarks>
/// <c>*ppPointer</c> and <c>pDetour</c> in <c>DETOUR_INLINE_HOOK</c>
/// should be initialized before calling this function.
/// </remarks>
/// <seealso cref="SlimDetoursInitInlineHooks"/>
/// <seealso cref="SlimDetoursInlineHook"/>
HRESULT
NTAPI
SlimDetoursInlineHooks(
    _In_ BOOL bEnable,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_INLINE_HOOK pHooks);

/* Function Table Hook, by SlimDetours */

/// <summary>
/// Set or unset a single function hook in a function address table.
/// </summary>
/// <param name="pFuncTable">Pointer to the target function address table.</param>
/// <param name="ulOffset">Offset to the target function address in the table.</param>
/// <param name="ppOldFunc">Optional, pointer to a variable to save the old function address.</param>
/// <param name="pNewFunc">Address of the new function to overwrite.</param>
/// <returns>Return HRESULT</returns>
/// <remarks>
/// Function address table should be an array that each element is a function address.
/// SlimDetours overwrite corresponding function address in the table to implement the hooking.
/// This is useful to do COM/IAT/... hook, and automatically adjust write-protection on the table when writing it.
/// </remarks>
HRESULT
NTAPI
SlimDetoursFuncTableHook(
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulOffset,
    _Out_opt_ PVOID* ppOldFunc,
    _In_ PVOID pNewFunc);

typedef struct _DETOUR_FUNC_TABLE_HOOK
{
    ULONG ulOffset;     // Offset to the target function address in the table
    PVOID* ppOldFunc;   // Pointer to a variable contains the old function address
    PVOID pNewFunc;     // Address of new function
} DETOUR_FUNC_TABLE_HOOK, *PDETOUR_FUNC_TABLE_HOOK;

/// <summary>
/// Set or unset multiple function hooks in a same function address table.
/// </summary>
/// <param name="bEnable">Set to TRUE to hook, or FALSE to unhook.</param>
/// <param name="pFuncTable">Pointer to the target function address table.</param>
/// <param name="ulCount">Number of elements in <c>pHooks</c> array.</param>
/// <param name="pHooks">A pointer point to an <c>DETOUR_FUNC_TABLE_HOOK</c> array.</param>
/// <returns>Return HRESULT</returns>
/// <seealso cref="SlimDetoursFuncTableHook"/>
HRESULT
NTAPI
SlimDetoursFuncTableHooks(
    _In_ BOOL bEnable,
    _In_ PVOID* pFuncTable,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_FUNC_TABLE_HOOK pHooks);

/// <summary>
/// Set or unset multiple function hooks in a same COM interface.
/// </summary>
/// <param name="bEnable">Set to TRUE to hook, or FALSE to unhook.</param>
/// <param name="rCLSID">See also <c>rclsid</c> parameter of <c>CoCreateInstance</c>.</param>
/// <param name="rIID">See also <c>riid</c> parameter of <c>CoCreateInstance</c>.</param>
/// <param name="ulCount">Number of elements in <c>pHooks</c> array.</param>
/// <param name="pHooks">A pointer point to an <c>DETOUR_FUNC_TABLE_HOOK</c> array.</param>
/// <returns>Return HRESULT</returns>
/// <remarks>
/// COM should be initialized before calling this function,
/// a wrong COM context (or apartment type) may cause hooks ineffective and even crash.
/// <c>DETOUR_FUNC_TABLE_HOOK.ulOffset</c> should be the offset to a function address in the vtable of COM object.
/// e.g. <c>FIELD_OFFSET(IOpenControlPanelVtbl, GetPath)</c>.
/// </remarks>
/// <seealso cref="CoInitialize"/>
/// <seealso cref="CoCreateInstance"/>
/// <seealso cref="SlimDetoursFuncTableHooks"/>
HRESULT
NTAPI
SlimDetoursCOMHooks(
    _In_ BOOL bEnable,
    _In_ REFCLSID rCLSID,
    _In_ REFCLSID rIID,
    _In_ ULONG ulCount,
    _Inout_updates_(ulCount) PDETOUR_FUNC_TABLE_HOOK pHooks);

/// <summary>
/// Set or unset a singe function hook in a COM interface.
/// </summary>
/// <seealso cref="SlimDetoursCOMHooks"/>
FORCEINLINE
HRESULT
SlimDetoursCOMHook(
    _In_ REFCLSID rCLSID,
    _In_ REFCLSID rIID,
    _In_ ULONG ulOffset,
    _Out_opt_ PVOID* ppOldFunc,
    _In_ PVOID pNewFunc)
{
    DETOUR_FUNC_TABLE_HOOK Hook;

    Hook.ulOffset = ulOffset;
    if (ppOldFunc != NULL)
    {
        Hook.ppOldFunc = ppOldFunc;
        Hook.pNewFunc = pNewFunc;
    } else
    {
        Hook.ppOldFunc = &pNewFunc;
    }

    return SlimDetoursCOMHooks(ppOldFunc != NULL, rCLSID, rIID, 1, &Hook);
}

/* Delay Hook, by SlimDetours */

#if (NTDDI_VERSION >= NTDDI_WIN6)

typedef
_Function_class_(DETOUR_DELAY_ATTACH_CALLBACK_FN)
VOID
CALLBACK
DETOUR_DELAY_ATTACH_CALLBACK_FN(
    _In_ HRESULT Result,
    _In_ PVOID* ppPointer,
    _In_ PCWSTR DllName,
    _In_ PCSTR Function,
    _In_opt_ PVOID Context);
typedef DETOUR_DELAY_ATTACH_CALLBACK_FN *PDETOUR_DELAY_ATTACH_CALLBACK_FN;

/// <summary>
/// Delay hook, set hooks automatically when target DLL loaded.
/// </summary>
/// <param name="ppPointer">Variable to receive the address of the trampoline when hook apply.</param>
/// <param name="pDetour">Pointer to the detour function.</param>
/// <param name="DllName">Name of target DLL.</param>
/// <param name="Function">Name of target function.</param>
/// <param name="Callback">Optional. Callback to receive delay hook notification.</param>
/// <param name="Context">Optional. A parameter to be passed to the callback function.</param>
/// <returns>
/// Returns HRESULT.
/// HRESULT_FROM_NT(STATUS_PENDING): Delay hook register successfully.
/// Other success status: Hook is succeeded right now, delay hook won't execute.
/// Otherwise, returns an error HRESULT from NTSTATUS.
/// </returns>
HRESULT
NTAPI
SlimDetoursDelayAttach(
    _In_ PVOID* ppPointer,
    _In_ PVOID pDetour,
    _In_ PCWSTR DllName,
    _In_ PCSTR Function,
    _In_opt_ __callback PDETOUR_DELAY_ATTACH_CALLBACK_FN Callback,
    _In_opt_ PVOID Context);

#endif /* (NTDDI_VERSION >= NTDDI_WIN6) */

#ifdef __cplusplus
}
#endif
