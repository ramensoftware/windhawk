#pragma once

NTSTATUS
threadscan_thread_suspend(
    _Outptr_result_maybenull_ PHANDLE* SuspendedHandles,
    _Out_ PULONG SuspendedHandleCount,
    _In_ ULONG ThreadIdToSkip);

VOID
threadscan_thread_resume(
    _In_reads_(SuspendedHandleCount) PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount);

VOID
threadscan_thread_free(
    _In_reads_(SuspendedHandleCount) _Frees_ptr_ PHANDLE SuspendedHandles,
    _In_ ULONG SuspendedHandleCount);
