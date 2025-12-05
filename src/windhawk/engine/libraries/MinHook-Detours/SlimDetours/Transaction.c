/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Transaction APIs
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 *
 * Source base on Microsoft Detours:
 *
 * Microsoft Research Detours Package, Version 4.0.1
 * Copyright (c) Microsoft Corporation. All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

static _Interlocked_operand_ HANDLE volatile s_nPendingThreadId = NULL; // Thread owning pending transaction.
static PHANDLE s_phSuspendedThreads = NULL;
static ULONG s_ulSuspendedThreadCount = 0;
static PDETOUR_OPERATION s_pPendingOperations = NULL;

HRESULT
NTAPI
SlimDetoursTransactionBeginEx(
    _In_ PCDETOUR_TRANSACTION_OPTIONS pOptions)
{
    NTSTATUS Status;

    // Make sure only one thread can start a transaction.
    if (_InterlockedCompareExchangePointer(&s_nPendingThreadId, NtCurrentThreadId(), NULL) != NULL)
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    // Initialize memory management
    detour_memory_init();

    // Make sure the trampoline pages are writable.
    Status = detour_writable_trampoline_regions();
    if (!NT_SUCCESS(Status))
    {
        goto fail;
    }

    if (pOptions->fSuspendThreads)
    {
        Status = detour_thread_suspend(&s_phSuspendedThreads, &s_ulSuspendedThreadCount);
        if (!NT_SUCCESS(Status))
        {
            detour_runnable_trampoline_regions();
            goto fail;
        }
    } else
    {
        s_phSuspendedThreads = NULL;
        s_ulSuspendedThreadCount = 0;
    }

    s_pPendingOperations = NULL;
    return HRESULT_FROM_NT(STATUS_SUCCESS);

fail:
#ifdef _MSC_VER
#pragma warning(disable: __WARNING_INTERLOCKED_ACCESS)
#endif
    s_nPendingThreadId = NULL;
#ifdef _MSC_VER
#pragma warning(default: __WARNING_INTERLOCKED_ACCESS)
#endif
    return HRESULT_FROM_NT(Status);
}

HRESULT
NTAPI
SlimDetoursTransactionAbort(VOID)
{
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;
    BOOL freed = FALSE;

    if (s_nPendingThreadId != NtCurrentThreadId())
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    // Restore all of the page permissions.
    for (PDETOUR_OPERATION o = s_pPendingOperations; o != NULL;)
    {
        // We don't care if this fails, because the code is still accessible.
        pMem = o->pbTarget;
        sMem = o->pTrampoline->cbRestore;
        NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, o->dwPerm, &dwOld);
        if (o->dwOperation == DETOUR_OPERATION_ADD)
        {
            detour_free_trampoline(o->pTrampoline);
            o->pTrampoline = NULL;
            freed = TRUE;
        }

        PDETOUR_OPERATION n = o->pNext;
        detour_memory_free(o);
        o = n;
    }
    s_pPendingOperations = NULL;
    if (freed)
    {
        detour_free_unused_trampoline_regions();
    }

    // Make sure the trampoline pages are no longer writable.
    detour_runnable_trampoline_regions();

    // Resume any suspended threads.
    detour_thread_resume(s_phSuspendedThreads, s_ulSuspendedThreadCount);

    s_phSuspendedThreads = NULL;
    s_ulSuspendedThreadCount = 0;
    s_nPendingThreadId = NULL;
    return HRESULT_FROM_NT(STATUS_SUCCESS);
}

HRESULT
NTAPI
SlimDetoursTransactionCommit(VOID)
{
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;

    // Common variables.
    PDETOUR_OPERATION o, n, m;
    PBYTE pbCode;
    BOOL freed = FALSE;
    ULONG i;

    if (s_nPendingThreadId != NtCurrentThreadId())
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    if (s_pPendingOperations == NULL)
    {
        goto _exit;
    }

    // Insert each of the detours.
    for (o = s_pPendingOperations; o != NULL; o = o->pNext)
    {
        if (o->dwOperation != DETOUR_OPERATION_ADD)
            continue;

        DETOUR_TRACE("detours: pbTramp =%p, pbRemain=%p, pbDetour=%p, cbRestore=%u\n",
            o->pTrampoline,
            o->pTrampoline->pbRemain,
            o->pTrampoline->pbDetour,
            o->pTrampoline->cbRestore);

        DETOUR_TRACE("detours: pbTarget=%p: "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x [before]\n",
            o->pbTarget,
            o->pbTarget[0], o->pbTarget[1], o->pbTarget[2], o->pbTarget[3],
            o->pbTarget[4], o->pbTarget[5], o->pbTarget[6], o->pbTarget[7],
            o->pbTarget[8], o->pbTarget[9], o->pbTarget[10], o->pbTarget[11]);

        m = NULL;
        if (!RtlEqualMemory(o->pbTarget, o->pTrampoline->rbRestore, o->pTrampoline->cbRestore))
        {
            DETOUR_TRACE("detours: target is modified\n");

            for (n = s_pPendingOperations; n != o; n = n->pNext)
            {
                if (n->dwOperation == DETOUR_OPERATION_ADD && n->pbTarget == o->pbTarget)
                {
                    m = n;
                }
            }
        }

        if (m != NULL)
        {
            DETOUR_TRACE("detours: chaining to last detour in the transaction\n");

#if defined(_X86_) || defined(_AMD64_)
            pbCode = detour_gen_jmp_indirect(o->pTrampoline->rbCode, &m->pTrampoline->pbDetour);
#elif defined(_ARM64_)
            pbCode = detour_gen_jmp_indirect(o->pTrampoline->rbCode, (ULONG64*)&(m->pTrampoline->pbDetour));
#endif
            o->pTrampoline->cbCode = 0;

            CopyMemory(o->pTrampoline->rbRestore, o->pbTarget, m->pTrampoline->cbRestore);
            o->pTrampoline->cbRestore = m->pTrampoline->cbRestore;

            RtlZeroMemory(o->pTrampoline->rAlign, sizeof(o->pTrampoline->rAlign));
            o->pTrampoline->pbRemain = o->pbTarget + o->pTrampoline->cbRestore;
        }

#if defined(_X86_) || defined(_AMD64_)
        pbCode = detour_gen_jmp_indirect(o->pTrampoline->rbCodeIn, &o->pTrampoline->pbDetour);
        NtFlushInstructionCache(NtCurrentProcess(), o->pTrampoline->rbCodeIn, pbCode - o->pTrampoline->rbCodeIn);
        pbCode = detour_gen_jmp_immediate(o->pbTarget, o->pTrampoline->rbCodeIn);
#elif defined(_ARM64_)
        pbCode = detour_gen_jmp_indirect(o->pbTarget, (ULONG64*)&(o->pTrampoline->pbDetour));
#endif
        pbCode = detour_gen_brk(pbCode, o->pTrampoline->pbRemain);
        NtFlushInstructionCache(NtCurrentProcess(), o->pbTarget, pbCode - o->pbTarget);

        *o->ppbPointer = o->pTrampoline->rbCode;

        DETOUR_TRACE("detours: pbTarget=%p: "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x [after]\n",
            o->pbTarget,
            o->pbTarget[0], o->pbTarget[1], o->pbTarget[2], o->pbTarget[3],
            o->pbTarget[4], o->pbTarget[5], o->pbTarget[6], o->pbTarget[7],
            o->pbTarget[8], o->pbTarget[9], o->pbTarget[10], o->pbTarget[11]);

        DETOUR_TRACE("detours: pbTramp =%p: "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x "
            "%02x %02x %02x %02x\n",
            o->pTrampoline,
            o->pTrampoline->rbCode[0], o->pTrampoline->rbCode[1],
            o->pTrampoline->rbCode[2], o->pTrampoline->rbCode[3],
            o->pTrampoline->rbCode[4], o->pTrampoline->rbCode[5],
            o->pTrampoline->rbCode[6], o->pTrampoline->rbCode[7],
            o->pTrampoline->rbCode[8], o->pTrampoline->rbCode[9],
            o->pTrampoline->rbCode[10], o->pTrampoline->rbCode[11]);
    }

    // Remove each of the detours.
    for (o = s_pPendingOperations; o != NULL; o = o->pNext)
    {
        if (o->dwOperation != DETOUR_OPERATION_REMOVE)
            continue;

        // Check if the jmps still points where we expect, otherwise someone might have hooked us.
        BOOL hookIsStillThere =
#if defined(_X86_) || defined(_AMD64_)
            detour_is_jmp_immediate_to(o->pbTarget, o->pTrampoline->rbCodeIn) &&
            detour_is_jmp_indirect_to(o->pTrampoline->rbCodeIn, &o->pTrampoline->pbDetour);
#elif defined(_ARM64_)
            detour_is_jmp_indirect_to(o->pbTarget, (ULONG64*)&(o->pTrampoline->pbDetour));
#endif

        if (hookIsStillThere)
        {
            RtlCopyMemory(o->pbTarget, o->pTrampoline->rbRestore, o->pTrampoline->cbRestore);
            NtFlushInstructionCache(NtCurrentProcess(), o->pbTarget, o->pTrampoline->cbRestore);
        } else
        {
            // Don't remove and leak trampoline in this case.
            o->dwOperation = DETOUR_OPERATION_NONE;
            DETOUR_TRACE("detours: Leaked hook on pbTarget=%p due to external hooking\n", o->pbTarget);
        }

        // Put hook in bypass mode.
        o->pTrampoline->pbDetour = o->pTrampoline->rbCode;

        *o->ppbPointer = o->pbTarget;
    }

    // Update any suspended threads.
    for (i = 0; i < s_ulSuspendedThreadCount; i++)
    {
        detour_thread_update(s_phSuspendedThreads[i], s_pPendingOperations);
    }

    // Restore all of the page permissions and free any trampoline regions that are now unused.
    for (o = s_pPendingOperations; o != NULL;)
    {
        // We don't care if this fails, because the code is still accessible.
        pMem = o->pbTarget;
        sMem = o->pTrampoline->cbRestore;
        NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, o->dwPerm, &dwOld);
        if (o->dwOperation == DETOUR_OPERATION_REMOVE)
        {
            if (!o->ppTrampolineToFreeManually)
            {
                detour_free_trampoline(o->pTrampoline);
                freed = TRUE;
            } else
            {
                // The caller is responsible for freeing the trampoline.
                *o->ppTrampolineToFreeManually = o->pTrampoline;
            }
            o->pTrampoline = NULL;
        }

        n = o->pNext;
        detour_memory_free(o);
        o = n;
    }
    s_pPendingOperations = NULL;
    if (freed)
    {
        detour_free_unused_trampoline_regions();
    }

_exit:
    // Make sure the trampoline pages are no longer writable.
    detour_runnable_trampoline_regions();

    // Resume any suspended threads.
    detour_thread_resume(s_phSuspendedThreads, s_ulSuspendedThreadCount);
    s_phSuspendedThreads = NULL;
    s_ulSuspendedThreadCount = 0;
    s_nPendingThreadId = NULL;

    return HRESULT_FROM_NT(STATUS_SUCCESS);
}

HRESULT
NTAPI
SlimDetoursAttach(
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour)
{
    NTSTATUS Status;
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;

    if (s_nPendingThreadId != NtCurrentThreadId())
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    PBYTE pbTarget = (PBYTE)*ppPointer;
    PDETOUR_TRAMPOLINE pTrampoline = NULL;
    PDETOUR_OPERATION o = NULL;

    pbTarget = (PBYTE)detour_skip_jmp(pbTarget);
    pDetour = detour_skip_jmp((PBYTE)pDetour);

    // Don't follow a jump if its destination is the target function.
    // This happens when the detour does nothing other than call the target.
    if (pDetour == (PVOID)pbTarget)
    {
        Status = STATUS_INVALID_PARAMETER;
        DETOUR_BREAK();
        goto fail;
    }

    o = detour_memory_alloc(sizeof(DETOUR_OPERATION));
    if (o == NULL)
    {
        Status = STATUS_NO_MEMORY;
fail:
        DETOUR_BREAK();
        if (pTrampoline != NULL)
        {
            detour_free_trampoline(pTrampoline);
            detour_free_trampoline_region_if_unused(pTrampoline);
            pTrampoline = NULL;
        }
        if (o != NULL)
        {
            detour_memory_free(o);
        }
        return HRESULT_FROM_NT(Status);
    }

    pTrampoline = detour_alloc_trampoline(pbTarget);
    if (pTrampoline == NULL)
    {
        Status = STATUS_NO_MEMORY;
        DETOUR_BREAK();
        goto fail;
    }

    DETOUR_TRACE("detours: pbTramp=%p, pDetour=%p\n", pTrampoline, pDetour);

    RtlZeroMemory(pTrampoline->rAlign, sizeof(pTrampoline->rAlign));

    // Determine the number of movable target instructions.
    PBYTE pbSrc = pbTarget;
    PBYTE pbTrampoline = pTrampoline->rbCode;
    PBYTE pbPool = pbTrampoline + sizeof(pTrampoline->rbCode);
    ULONG cbTarget = 0;
    ULONG cbJump = SIZE_OF_JMP;
    ULONG nAlign = 0;

    while (cbTarget < cbJump)
    {
        PBYTE pbOp = pbSrc;
        LONG lExtra = 0;

        DETOUR_TRACE(" SlimDetoursCopyInstruction(%p,%p)\n", pbTrampoline, pbSrc);
        pbSrc = (PBYTE)SlimDetoursCopyInstruction(pbTrampoline, pbSrc, NULL, &lExtra);
        if (pbSrc == NULL)
        {
            Status = STATUS_ILLEGAL_INSTRUCTION;
            DETOUR_BREAK();
            goto fail;
        }

        DETOUR_TRACE(" SlimDetoursCopyInstruction() = %p (%d bytes)\n", pbSrc, (int)(pbSrc - pbOp));
        pbTrampoline += (pbSrc - pbOp) + lExtra;
        cbTarget = PtrOffset(pbTarget, pbSrc);
        pTrampoline->rAlign[nAlign].obTarget = (BYTE)cbTarget;
        pTrampoline->rAlign[nAlign].obTrampoline = (BYTE)(pbTrampoline - pTrampoline->rbCode);
        nAlign++;

        if (nAlign >= ARRAYSIZE(pTrampoline->rAlign))
        {
            break;
        }

        if (detour_does_code_end_function(pbOp))
        {
            break;
        }
    }

    // Consume, but don't duplicate padding if it is needed and available.
    while (cbTarget < cbJump)
    {
        LONG cFiller = detour_is_code_filler(pbSrc);
        if (cFiller == 0)
        {
            break;
        }

        pbSrc += cFiller;
        cbTarget = PtrOffset(pbTarget, pbSrc);
    }

#if _DEBUG
    {
        DETOUR_TRACE(" detours: rAlign [");
        LONG n = 0;
        for (n = 0; n < ARRAYSIZE(pTrampoline->rAlign); n++)
        {
            if (pTrampoline->rAlign[n].obTarget == 0 && pTrampoline->rAlign[n].obTrampoline == 0)
            {
                break;
            }
            DETOUR_TRACE(" %u/%u", pTrampoline->rAlign[n].obTarget, pTrampoline->rAlign[n].obTrampoline);

        }
        DETOUR_TRACE(" ]\n");
    }
#endif

    if (cbTarget < cbJump || nAlign > ARRAYSIZE(pTrampoline->rAlign))
    {
        // Too few instructions.
        Status = STATUS_INVALID_BLOCK_LENGTH;
        DETOUR_BREAK();
        goto fail;
    }

    if (pbTrampoline > pbPool)
    {
        __debugbreak();
    }

    pTrampoline->cbCode = (BYTE)(pbTrampoline - pTrampoline->rbCode);
    pTrampoline->cbRestore = (BYTE)cbTarget;
    RtlCopyMemory(pTrampoline->rbRestore, pbTarget, cbTarget);

    if (cbTarget > sizeof(pTrampoline->rbCode) - cbJump)
    {
        // Too many instructions.
        Status = STATUS_INVALID_HANDLE;
        DETOUR_BREAK();
        goto fail;
    }

    pTrampoline->pbRemain = pbTarget + cbTarget;
    pTrampoline->pbDetour = (PBYTE)pDetour;

    pbTrampoline = pTrampoline->rbCode + pTrampoline->cbCode;
#if defined(_AMD64_)
    pbTrampoline = detour_gen_jmp_indirect(pbTrampoline, &pTrampoline->pbRemain);
#elif defined(_X86_)
    pbTrampoline = detour_gen_jmp_immediate(pbTrampoline, pTrampoline->pbRemain);
#elif defined(_ARM64_)
    pbTrampoline = detour_gen_jmp_immediate(pbTrampoline, &pbPool, pTrampoline->pbRemain);
#endif
    pbTrampoline = detour_gen_brk(pbTrampoline, pbPool);
    UNREFERENCED_PARAMETER(pbTrampoline);

    pMem = pbTarget;
    sMem = cbTarget;
    Status = NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, PAGE_EXECUTE_READWRITE, &dwOld);
    if (!NT_SUCCESS(Status))
    {
        DETOUR_BREAK();
        goto fail;
    }

    DETOUR_TRACE("detours: pbTarget=%p: "
                 "%02x %02x %02x %02x "
                 "%02x %02x %02x %02x "
                 "%02x %02x %02x %02x\n",
                 pbTarget,
                 pbTarget[0], pbTarget[1], pbTarget[2], pbTarget[3],
                 pbTarget[4], pbTarget[5], pbTarget[6], pbTarget[7],
                 pbTarget[8], pbTarget[9], pbTarget[10], pbTarget[11]);
    DETOUR_TRACE("detours: pbTramp =%p: "
                 "%02x %02x %02x %02x "
                 "%02x %02x %02x %02x "
                 "%02x %02x %02x %02x\n",
                 pTrampoline,
                 pTrampoline->rbCode[0], pTrampoline->rbCode[1],
                 pTrampoline->rbCode[2], pTrampoline->rbCode[3],
                 pTrampoline->rbCode[4], pTrampoline->rbCode[5],
                 pTrampoline->rbCode[6], pTrampoline->rbCode[7],
                 pTrampoline->rbCode[8], pTrampoline->rbCode[9],
                 pTrampoline->rbCode[10], pTrampoline->rbCode[11]);

    o->dwOperation = DETOUR_OPERATION_ADD;
    o->ppbPointer = (PBYTE*)ppPointer;
    o->pTrampoline = pTrampoline;
    o->pbTarget = pbTarget;
    o->dwPerm = dwOld;
    o->pNext = s_pPendingOperations;
    s_pPendingOperations = o;

    return HRESULT_FROM_NT(STATUS_SUCCESS);
}

HRESULT
NTAPI
SlimDetoursDetachEx(
    _Inout_ PVOID* ppPointer,
    _In_ PVOID pDetour,
    _In_ PCDETOUR_DETACH_OPTIONS pOptions)
{
    NTSTATUS Status;
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;

    if (s_nPendingThreadId != NtCurrentThreadId())
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    PDETOUR_OPERATION o = detour_memory_alloc(sizeof(DETOUR_OPERATION));
    if (o == NULL)
    {
        Status = STATUS_NO_MEMORY;
fail:
        DETOUR_BREAK();
        if (o != NULL)
        {
            detour_memory_free(o);
        }
        return HRESULT_FROM_NT(Status);
    }

    PDETOUR_TRAMPOLINE pTrampoline = (PDETOUR_TRAMPOLINE)*ppPointer;
    pDetour = detour_skip_jmp((PBYTE)pDetour);

    ////////////////////////////////////// Verify that Trampoline is in place.
    //
    LONG cbTarget = pTrampoline->cbRestore;
    PBYTE pbTarget = pTrampoline->pbRemain - cbTarget;
    if (cbTarget == 0 || cbTarget > sizeof(pTrampoline->rbCode) || pTrampoline->pbDetour != pDetour)
    {
        Status = STATUS_INVALID_BLOCK_LENGTH;
        DETOUR_BREAK();
        goto fail;
    }

    pMem = pbTarget;
    sMem = cbTarget;
    Status = NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, PAGE_EXECUTE_READWRITE, &dwOld);
    if (!NT_SUCCESS(Status))
    {
        DETOUR_BREAK();
        goto fail;
    }

    o->dwOperation = DETOUR_OPERATION_REMOVE;
    o->ppbPointer = (PBYTE*)ppPointer;
    o->pTrampoline = pTrampoline;
    o->pbTarget = pbTarget;
    o->dwPerm = dwOld;
    o->ppTrampolineToFreeManually = pOptions->ppTrampolineToFreeManually;
    o->pNext = s_pPendingOperations;
    s_pPendingOperations = o;

    return HRESULT_FROM_NT(STATUS_SUCCESS);
}

HRESULT
NTAPI
SlimDetoursFreeTrampoline(
    _In_ PVOID pTrampoline)
{
    if (pTrampoline == NULL)
    {
        return HRESULT_FROM_NT(STATUS_SUCCESS);
    }

    // This function can be called as part of a transaction or outside of a transaction.
    HANDLE nPrevPendingThreadId = _InterlockedCompareExchangePointer(&s_nPendingThreadId, NtCurrentThreadId(), NULL);
    BOOL bInTransaction = nPrevPendingThreadId != NULL;
    if (bInTransaction && nPrevPendingThreadId != NtCurrentThreadId())
    {
        return HRESULT_FROM_NT(STATUS_TRANSACTIONAL_CONFLICT);
    }

    NTSTATUS Status;

    if (!bInTransaction)
    {
        // Make sure the trampoline pages are writable.
        Status = detour_writable_trampoline_regions();
        if (!NT_SUCCESS(Status))
        {
            goto fail;
        }
    }

    detour_free_trampoline((PDETOUR_TRAMPOLINE)pTrampoline);
    detour_free_trampoline_region_if_unused((PDETOUR_TRAMPOLINE)pTrampoline);

    if (!bInTransaction)
    {
        detour_runnable_trampoline_regions();
    }

    Status = STATUS_SUCCESS;

fail:
    if (!bInTransaction)
    {
#ifdef _MSC_VER
#pragma warning(disable: __WARNING_INTERLOCKED_ACCESS)
#endif
        s_nPendingThreadId = NULL;
#ifdef _MSC_VER
#pragma warning(default: __WARNING_INTERLOCKED_ACCESS)
#endif
    }
    return HRESULT_FROM_NT(Status);
}

HRESULT
NTAPI
SlimDetoursUninitialize(VOID)
{
    NTSTATUS Status = STATUS_SUCCESS;

    if (!detour_memory_uninitialize())
    {
        Status = STATUS_INVALID_HANDLE;
    }

    return HRESULT_FROM_NT(Status);
}
