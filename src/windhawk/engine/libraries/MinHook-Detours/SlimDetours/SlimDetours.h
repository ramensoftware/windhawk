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

#ifdef __cplusplus
}
#endif
