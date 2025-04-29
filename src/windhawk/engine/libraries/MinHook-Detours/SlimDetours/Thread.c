/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Thread management
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

#define THREAD_ACCESS (THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT)

static HANDLE s_Handles[32];

NTSTATUS
detour_thread_suspend(
    _Outptr_result_maybenull_ PHANDLE* SuspendedHandles,
    _Out_ PULONG SuspendedHandleCount)
{
    NTSTATUS Status;
    PHANDLE Buffer = s_Handles;
    ULONG BufferCapacity = ARRAYSIZE(s_Handles);
    ULONG SuspendedCount = 0;
    BOOL CurrentThreadSkipped = FALSE;
    HANDLE CurrentTID = (HANDLE)(ULONG_PTR)NtCurrentThreadId();
    BOOL ClosePrevThread = FALSE;
    HANDLE ThreadHandle = NULL;

    while (TRUE)
    {
        HANDLE NextThreadHandle;
        Status = NtGetNextThread(NtCurrentProcess(), ThreadHandle, THREAD_ACCESS, 0, 0, &NextThreadHandle);
        if (ClosePrevThread)
        {
            NtClose(ThreadHandle);
        }

        if (!NT_SUCCESS(Status))
        {
            if (Status == STATUS_NO_MORE_ENTRIES)
            {
                Status = STATUS_SUCCESS;
            }
            break;
        }

        ThreadHandle = NextThreadHandle;
        ClosePrevThread = TRUE;

        /* Skip the current thread */
        if (!CurrentThreadSkipped)
        {
            THREAD_BASIC_INFORMATION BasicInformation;
            if (!NT_SUCCESS(NtQueryInformationThread(ThreadHandle,
                                                     ThreadBasicInformation,
                                                     &BasicInformation,
                                                     sizeof(BasicInformation),
                                                     NULL)))
            {
                continue;
            }
            if (BasicInformation.ClientId.UniqueThread == CurrentTID)
            {
                CurrentThreadSkipped = TRUE;
                continue;
            }
        }

        if (!NT_SUCCESS(NtSuspendThread(ThreadHandle, NULL)))
        {
            continue;
        }

        ClosePrevThread = FALSE;

        Status = STATUS_SUCCESS;
        if (SuspendedCount >= BufferCapacity)
        {
            BufferCapacity *= 2;

            PHANDLE p;
            if (Buffer == s_Handles)
            {
                p = (PHANDLE)detour_memory_alloc(BufferCapacity * sizeof(HANDLE));
                if (p)
                {
                    RtlCopyMemory(p, Buffer, SuspendedCount * sizeof(HANDLE));
                }
            } else
            {
                p = (PHANDLE)detour_memory_realloc(Buffer, BufferCapacity * sizeof(HANDLE));
            }

            if (p)
            {
                Buffer = p;
            }
            else
            {
                Status = STATUS_NO_MEMORY;
            }
        }

        if (!NT_SUCCESS(Status))
        {
            NtResumeThread(ThreadHandle, NULL);
            NtClose(ThreadHandle);
            break;
        }

        // Perform a synchronous operation to make sure the thread really is suspended.
        // https://devblogs.microsoft.com/oldnewthing/20150205-00/?p=44743
        CONTEXT cxt;
        cxt.ContextFlags = CONTEXT_CONTROL;
        NtGetContextThread(ThreadHandle, &cxt);

        Buffer[SuspendedCount++] = ThreadHandle;
    }

    if (!NT_SUCCESS(Status))
    {
        for (UINT i = 0; i < SuspendedCount; ++i)
        {
            NtResumeThread(Buffer[i], NULL);
            NtClose(Buffer[i]);
        }

        if (Buffer != s_Handles)
        {
            detour_memory_free(Buffer);
        }

        Buffer = NULL;
        SuspendedCount = 0;
    }

    *SuspendedHandles = Buffer;
    *SuspendedHandleCount = SuspendedCount;

    return Status;
}

VOID
detour_thread_resume(
    _In_reads_(SuspendedHandleCount) _Frees_ptr_ PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount)
{
    ULONG i;

    for (i = 0; i < SuspendedHandleCount; i++)
    {
        NtResumeThread(SuspendedHandles[i], NULL);
        NtClose(SuspendedHandles[i]);
    }

    if (SuspendedHandles != s_Handles)
    {
        detour_memory_free(SuspendedHandles);
    }
}

NTSTATUS
detour_thread_update(
    _In_ HANDLE ThreadHandle,
    _In_ PDETOUR_OPERATION PendingOperations)
{
    NTSTATUS Status;
    PDETOUR_OPERATION o;
    CONTEXT cxt;
    BOOL bUpdateContext;

    /*
     * Work-around an issue in Arm64 (and Arm64EC) in which LR and FP registers may become zeroed
     * when CONTEXT_CONTROL is used without CONTEXT_INTEGER.
     * 
     * See also: https://github.com/microsoft/Detours/pull/313
     */
#if defined(_AMD64_) || defined(_ARM64_)
    cxt.ContextFlags = CONTEXT_CONTROL | CONTEXT_INTEGER;
#else
    cxt.ContextFlags = CONTEXT_CONTROL;
#endif

    Status = NtGetContextThread(ThreadHandle, &cxt);
    if (!NT_SUCCESS(Status))
    {
        return Status;
    }

    bUpdateContext = FALSE;
    for (o = PendingOperations; o != NULL && !bUpdateContext; o = o->pNext)
    {
        if (o->dwOperation == DETOUR_OPERATION_REMOVE)
        {
            if (cxt.CONTEXT_PC >= (ULONG_PTR)o->pTrampoline->rbCode &&
                cxt.CONTEXT_PC < ((ULONG_PTR)o->pTrampoline->rbCode + RTL_FIELD_SIZE(DETOUR_TRAMPOLINE, rbCode)))
            {
                cxt.CONTEXT_PC = (ULONG_PTR)o->pbTarget +
                    detour_align_from_trampoline(o->pTrampoline, (BYTE)(cxt.CONTEXT_PC - (ULONG_PTR)o->pTrampoline));
                bUpdateContext = TRUE;
            }
#if defined(_X86_) || defined(_AMD64_)
            else if (cxt.CONTEXT_PC == (ULONG_PTR)o->pTrampoline->rbCodeIn)
            {
                cxt.CONTEXT_PC = (ULONG_PTR)o->pbTarget;
                bUpdateContext = TRUE;
            }
#endif
        } else if (o->dwOperation == DETOUR_OPERATION_ADD)
        {
            if (cxt.CONTEXT_PC >= (ULONG_PTR)o->pbTarget &&
                cxt.CONTEXT_PC < ((ULONG_PTR)o->pbTarget + o->pTrampoline->cbRestore))
            {
                cxt.CONTEXT_PC = (ULONG_PTR)o->pTrampoline +
                    detour_align_from_target(o->pTrampoline, (BYTE)(cxt.CONTEXT_PC - (ULONG_PTR)o->pbTarget));
                bUpdateContext = TRUE;
            }
        }
    }

    if (bUpdateContext)
    {
        Status = NtSetContextThread(ThreadHandle, &cxt);
    }

    return Status;
}
