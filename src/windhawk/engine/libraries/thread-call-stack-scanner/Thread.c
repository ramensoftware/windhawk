/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Thread management
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include <phnt/phnt_windows.h>
#include <phnt/phnt.h>

#include "Thread.h"

#include "Memory.h"

#define THREAD_ACCESS (THREAD_QUERY_LIMITED_INFORMATION | THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT)

static HANDLE s_Handles[32];

NTSTATUS
threadscan_thread_suspend(
    _Outptr_result_maybenull_ PHANDLE* SuspendedHandles,
    _Out_ PULONG SuspendedHandleCount,
    _In_ ULONG ThreadIdToSkip)
{
    NTSTATUS Status;
    PHANDLE Buffer = s_Handles;
    ULONG BufferCapacity = ARRAYSIZE(s_Handles);
    ULONG SuspendedCount = 0;
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

        THREAD_BASIC_INFORMATION BasicInformation;
        if (!NT_SUCCESS(NtQueryInformationThread(ThreadHandle,
            ThreadBasicInformation,
            &BasicInformation,
            sizeof(BasicInformation),
            NULL)))
        {
            continue;
        }

        if (BasicInformation.ClientId.UniqueThread == CurrentTID ||
            BasicInformation.ClientId.UniqueThread == (HANDLE)(ULONG_PTR)ThreadIdToSkip)
        {
            continue;
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
                p = (PHANDLE)threadscan_memory_alloc(BufferCapacity * sizeof(HANDLE));
                if (p)
                {
                    RtlCopyMemory(p, Buffer, SuspendedCount * sizeof(HANDLE));
                }
            } else
            {
                p = (PHANDLE)threadscan_memory_realloc(Buffer, BufferCapacity * sizeof(HANDLE));
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
            threadscan_memory_free(Buffer);
        }

        Buffer = NULL;
        SuspendedCount = 0;
    }

    *SuspendedHandles = Buffer;
    *SuspendedHandleCount = SuspendedCount;

    return Status;
}

VOID
threadscan_thread_resume(
    _In_reads_(SuspendedHandleCount) PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount)
{
    ULONG i;

    for (i = 0; i < SuspendedHandleCount; i++)
    {
        NtResumeThread(SuspendedHandles[i], NULL);
    }
}

VOID
threadscan_thread_free(
    _In_reads_(SuspendedHandleCount) _Frees_ptr_ PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount)
{
    ULONG i;

    for (i = 0; i < SuspendedHandleCount; i++)
    {
        NtClose(SuspendedHandles[i]);
    }

    if (SuspendedHandles != s_Handles)
    {
        threadscan_memory_free(SuspendedHandles);
    }
}
