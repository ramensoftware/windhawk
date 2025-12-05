/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Thread management
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

static HANDLE s_Handles[32];

#if _WIN32_WINNT >= _WIN32_WINNT_WIN6

static BOOL g_CurrentThreadSkipped = FALSE;
static HANDLE g_PrevThreadHandle = NULL;

static
VOID
detour_suspend_next_thread_rest(VOID)
{
    g_PrevThreadHandle = NULL;
    g_CurrentThreadSkipped = FALSE;
}

static
NTSTATUS
detour_suspend_next_thread(
    _Out_ PHANDLE NextThreadHandle)
{
    NTSTATUS Status;
    THREAD_BASIC_INFORMATION TBI;
    BOOL ClosePrevThread = FALSE;

_Next_thread:
    /* Get next thread */
    Status = NtGetNextThread(NtCurrentProcess(),
                             g_PrevThreadHandle,
                             THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT,
                             0,
                             0,
                             NextThreadHandle);
    if (ClosePrevThread)
    {
        NtClose(g_PrevThreadHandle);
    }
    if (!NT_SUCCESS(Status))
    {
        if (Status == STATUS_NO_MORE_ENTRIES)
        {
            *NextThreadHandle = NULL;
            Status = STATUS_SUCCESS;
        }
        detour_suspend_next_thread_rest();
        return Status;
    }

    /* Skip current thread */
    if (g_CurrentThreadSkipped)
    {
        goto _Suspend_thread;
    }
    /* False positive warning, *NextThreadHandle should be already assigned */
#pragma warning(disable: __WARNING_USING_UNINIT_VAR)
    Status = NtQueryInformationThread(*NextThreadHandle, ThreadBasicInformation, &TBI, sizeof(TBI), NULL);
#pragma warning(default: __WARNING_USING_UNINIT_VAR)
    if (!NT_SUCCESS(Status))
    {
        goto _Step_next;
    }
    if (TBI.ClientId.UniqueThread == NtCurrentThreadId())
    {
        g_CurrentThreadSkipped = TRUE;
        goto _Step_next;
    }

_Suspend_thread:
    if (NT_SUCCESS(NtSuspendThread(*NextThreadHandle, NULL)))
    {
        g_PrevThreadHandle = *NextThreadHandle;
        return STATUS_SUCCESS;
    }
_Step_next:
    g_PrevThreadHandle = *NextThreadHandle;
    ClosePrevThread = TRUE;
    goto _Next_thread;
}

#else

static PSYSTEM_PROCESS_INFORMATION g_SPI = NULL;
static PSYSTEM_THREAD_INFORMATION g_STI;
static ULONG g_ThreadCount;
static OBJECT_ATTRIBUTES g_EmptyObjectAttributes = RTL_CONSTANT_OBJECT_ATTRIBUTES(NULL, 0);

static
VOID
detour_suspend_next_thread_rest(VOID)
{
    detour_memory_free(g_SPI);
    g_SPI = NULL;
}

static
NTSTATUS
detour_suspend_next_thread(
    _Out_ PHANDLE ThreadHandle)
{
    NTSTATUS Status;
    ULONG i;
    PSYSTEM_PROCESS_INFORMATION pSPI;
    PSYSTEM_THREAD_INFORMATION pSTI;

    if (g_SPI == NULL)
    {
        /* Get system process and thread information */
        i = _1MB;
_Try_alloc:
        g_SPI = (PSYSTEM_PROCESS_INFORMATION)detour_memory_alloc(i);
        if (g_SPI == NULL)
        {
            return STATUS_NO_MEMORY;
        }
        Status = NtQuerySystemInformation(SystemProcessInformation, g_SPI, i, &i);
        if (!NT_SUCCESS(Status))
        {
            detour_memory_free(g_SPI);
            if (Status == STATUS_INFO_LENGTH_MISMATCH)
            {
                goto _Try_alloc;
            }
            g_SPI = NULL;
            return Status;
        }

        /* Find current process and threads */
        pSPI = g_SPI;
        while (pSPI->UniqueProcessId != NtCurrentProcessId())
        {
            if (pSPI->NextEntryOffset == 0)
            {
                detour_suspend_next_thread_rest();
                return STATUS_NOT_FOUND;
            }
            pSPI = (PSYSTEM_PROCESS_INFORMATION)Add2Ptr(pSPI, pSPI->NextEntryOffset);
        }
        g_ThreadCount = pSPI->NumberOfThreads;
        pSTI = (PSYSTEM_THREAD_INFORMATION)Add2Ptr(pSPI, sizeof(*pSPI));
    } else
    {
        pSTI = g_STI + 1;
    }
    if (g_ThreadCount == 0)
    {
        goto _No_more_thread;
    }

_Suspend_thread:
    g_ThreadCount--;
    /* Open and suspend thread, skip current thread */
    if (pSTI->ClientId.UniqueThread != NtCurrentThreadId() &&
        NT_SUCCESS(NtOpenThread(ThreadHandle,
                                THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT,
                                &g_EmptyObjectAttributes,
                                &pSTI->ClientId)))
    {
        if (NT_SUCCESS(NtSuspendThread(*ThreadHandle, NULL)))
        {
            g_STI = pSTI;
            return STATUS_SUCCESS;
        } else
        {
            NtClose(*ThreadHandle);
        }
    }

    if (g_ThreadCount)
    {
        pSTI += 1;
        goto _Suspend_thread;
    }
_No_more_thread:
    *ThreadHandle = NULL;
    detour_suspend_next_thread_rest();
    return STATUS_SUCCESS;
}

#endif

NTSTATUS
detour_thread_suspend(
    _Outptr_result_maybenull_ PHANDLE* SuspendedHandles,
    _Out_ PULONG SuspendedHandleCount)
{
    NTSTATUS Status;
    PHANDLE Buffer = s_Handles;
    ULONG BufferCapacity = ARRAYSIZE(s_Handles);
    ULONG SuspendedCount = 0;
    HANDLE ThreadHandle;

    /* Suspend next thread and get handle */
_Suspend_next_thread:
    Status = detour_suspend_next_thread(&ThreadHandle);
    if (!NT_SUCCESS(Status))
    {
        goto _Fail;
    } else if (ThreadHandle == NULL)
    {
        goto _Exit;
    }

    /* Allocate buffer dynamically if static buffer is insufficient */
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
        } else
        {
            Status = STATUS_NO_MEMORY;
            NtResumeThread(ThreadHandle, NULL);
            NtClose(ThreadHandle);
            detour_suspend_next_thread_rest();
            goto _Fail;
        }
    }

    // Perform a synchronous operation to make sure the thread really is suspended.
    // https://devblogs.microsoft.com/oldnewthing/20150205-00/?p=44743
    CONTEXT cxt;
    cxt.ContextFlags = CONTEXT_CONTROL;
    NtGetContextThread(ThreadHandle, &cxt);

    Buffer[SuspendedCount++] = ThreadHandle;
    goto _Suspend_next_thread;

_Fail:
    for (ULONG i = 0; i < SuspendedCount; ++i)
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

_Exit:
    *SuspendedHandles = Buffer;
    *SuspendedHandleCount = SuspendedCount;

    return Status;
}

VOID
detour_thread_resume(
    _In_reads_(SuspendedHandleCount) _Frees_ptr_ PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount)
{
    for (ULONG i = 0; i < SuspendedHandleCount; i++)
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
    for (PDETOUR_OPERATION o = PendingOperations; o != NULL && !bUpdateContext; o = o->pNext)
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
