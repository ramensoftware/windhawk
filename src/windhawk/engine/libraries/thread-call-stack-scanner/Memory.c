/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Memory Management
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include <phnt/phnt_windows.h>
#include <phnt/phnt.h>

#include "Memory.h"

static HANDLE _threadscan_memory_heap = NULL;

static
_Ret_notnull_
HANDLE
threadscan_memory_init(VOID)
{
    HANDLE hHeap;

    /* Initialize private heap */
    hHeap = RtlCreateHeap(HEAP_NO_SERIALIZE | HEAP_GROWABLE, NULL, 0, 0, NULL, NULL);
    if (hHeap == NULL)
    {
        //DETOUR_TRACE("RtlCreateHeap failed, fallback to use process default heap\n");
        hHeap = NtCurrentPeb()->ProcessHeap;
    }

    return hHeap;
}

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
threadscan_memory_alloc(
    _In_ SIZE_T Size)
{
    /*
     * threadscan_memory_alloc is called BEFORE any other threadscan_memory_* functions,
     * and only one thread that owning pending transaction could reach here,
     * so it's safe to do the initialzation here and not use a lock.
     */
    if (_threadscan_memory_heap == NULL)
    {
        _threadscan_memory_heap = threadscan_memory_init();
    }

    return RtlAllocateHeap(_threadscan_memory_heap, 0, Size);
}

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
threadscan_memory_realloc(
    _Frees_ptr_opt_ PVOID BaseAddress,
    _In_ SIZE_T Size)
{
    return RtlReAllocateHeap(_threadscan_memory_heap, 0, BaseAddress, Size);
}

BOOL
threadscan_memory_free(
    _Frees_ptr_ PVOID BaseAddress)
{
    return RtlFreeHeap(_threadscan_memory_heap, 0, BaseAddress);
}

BOOL
threadscan_memory_initialize(VOID)
{
    if (_threadscan_memory_heap == NULL)
    {
        _threadscan_memory_heap = threadscan_memory_init();
        return _threadscan_memory_heap != NULL;
    }

    return FALSE;
}

BOOL
threadscan_memory_uninitialize(VOID)
{
    if (_threadscan_memory_heap != NULL && _threadscan_memory_heap != NtCurrentPeb()->ProcessHeap)
    {
        _threadscan_memory_heap = RtlDestroyHeap(_threadscan_memory_heap);
        return _threadscan_memory_heap == NULL;
    }

    return TRUE;
}
