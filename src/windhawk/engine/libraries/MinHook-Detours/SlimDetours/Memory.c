/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Memory Management
 * Copyright (c) KNSoft.org (https://github.com/KNSoft). All rights reserved.
 * Licensed under the MIT license.
 */

#include "SlimDetours.inl"

/*
 * Region reserved for system DLLs
 *
 * System reserved a region to make each system DLL was relocated only once
 * when loaded into every process as far as possible. Avoid using this region for trampoline.
 *
 * The region is [0x50000000 ... 0x78000000] (640MB) on 32-bit Windows;
 * and [0x00007FF7FFFF0000 ... 0x00007FFFFFFF0000] (32GB) on 64-bit Windows, which is too large to avoid.
 * In this case, avoiding 1GB range starting at Ntdll.dll is make sense.
 *
 * Use MI_ASLR_* provided by KNSoft.NDK instead of hard-coded.
 */

#define SYSTEM_RESERVED_REGION_HIGHEST ((ULONG_PTR)MI_ASLR_HIGHEST_SYSTEM_RANGE_ADDRESS - 1)
#define SYSTEM_RESERVED_REGION_SIZE (MI_ASLR_BITMAP_SIZE * (ULONG_PTR)CHAR_BIT * MM_ALLOCATION_GRANULARITY)
#define SYSTEM_RESERVED_REGION_LOWEST (SYSTEM_RESERVED_REGION_HIGHEST - SYSTEM_RESERVED_REGION_SIZE + 1)

#if defined(_WIN64)
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_HIGHEST + 1 == 0x00007FFFFFFF0000ULL);
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_SIZE == GB_TO_BYTES(32ULL));
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_LOWEST == 0x00007FF7FFFF0000ULL);

static ULONG_PTR s_ulSystemRegionHighLowerBound = MAXULONG_PTR;
static ULONG_PTR s_ulSystemRegionLowUpperBound = 0;
static ULONG_PTR s_ulSystemRegionLowLowerBound = 0;
#else
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_HIGHEST + 1 == 0x78000000UL);
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_SIZE == MB_TO_BYTES(640UL));
_STATIC_ASSERT(SYSTEM_RESERVED_REGION_LOWEST == 0x50000000UL);

static ULONG_PTR s_ulSystemRegionLowUpperBound = SYSTEM_RESERVED_REGION_HIGHEST;
static ULONG_PTR s_ulSystemRegionLowLowerBound = SYSTEM_RESERVED_REGION_LOWEST;
#endif

/*
 * System memory information with defaults
 *
 * Result from NtQuerySystemInformation(SystemBasicInformation, ...) is better,
 * and those default values are enough to work properly.
 */

static SYSTEM_BASIC_INFORMATION g_sbi = {
    .PageSize = PAGE_SIZE,
    .AllocationGranularity = MM_ALLOCATION_GRANULARITY,
    .MinimumUserModeAddress = (ULONG_PTR)MM_LOWEST_USER_ADDRESS,
#if defined(_WIN64)
    .MaximumUserModeAddress = 0x00007FFFFFFEFFFF,
#else
    .MaximumUserModeAddress = 0x7FFEFFFF,
#endif
};

static HANDLE _detour_memory_heap = NULL;

static
_Ret_notnull_
HANDLE
detour_memory_init(VOID)
{
    HANDLE hHeap;

    /* Initialize memory management information */
    NtQuerySystemInformation(SystemBasicInformation, &g_sbi, sizeof(g_sbi), NULL);
    if (NtCurrentPeb()->OSMajorVersion >= 6)
    {
#if defined(_WIN64)
        PLDR_DATA_TABLE_ENTRY NtdllLdrEntry;

        NtdllLdrEntry = CONTAINING_RECORD(NtCurrentPeb()->Ldr->InInitializationOrderModuleList.Flink,
                                          LDR_DATA_TABLE_ENTRY,
                                          InInitializationOrderLinks);
        s_ulSystemRegionLowUpperBound = (ULONG_PTR)NtdllLdrEntry->DllBase + NtdllLdrEntry->SizeOfImage - 1;
        s_ulSystemRegionLowLowerBound = s_ulSystemRegionLowUpperBound - _1GB + 1;
        if (s_ulSystemRegionLowLowerBound < SYSTEM_RESERVED_REGION_LOWEST)
        {
            s_ulSystemRegionHighLowerBound = s_ulSystemRegionLowLowerBound + SYSTEM_RESERVED_REGION_SIZE;
            s_ulSystemRegionLowLowerBound = SYSTEM_RESERVED_REGION_LOWEST;
        }
#endif
    } else
    {
        /* TODO: What if NT5 x64? Let's keep the original Detours behavior. */
        s_ulSystemRegionLowUpperBound = 0x80000000;
        s_ulSystemRegionLowLowerBound = 0x70000000;
    }

    /* Initialize private heap */
    hHeap = RtlCreateHeap(HEAP_NO_SERIALIZE | HEAP_GROWABLE, NULL, 0, 0, NULL, NULL);
    if (hHeap == NULL)
    {
        DETOUR_TRACE("RtlCreateHeap failed, fallback to use process default heap\n");
        hHeap = NtGetProcessHeap();
    }

    return hHeap;
}

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
detour_memory_alloc(
    _In_ SIZE_T Size)
{
    /*
     * detour_memory_alloc is called BEFORE any other detour_memory_* functions,
     * and only one thread that owning pending transaction could reach here,
     * so it's safe to do the initialzation here and not use a lock.
     */
    if (_detour_memory_heap == NULL)
    {
        _detour_memory_heap = detour_memory_init();
    }

    return RtlAllocateHeap(_detour_memory_heap, 0, Size);
}

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
detour_memory_realloc(
    _Frees_ptr_opt_ PVOID BaseAddress,
    _In_ SIZE_T Size)
{
    return RtlReAllocateHeap(_detour_memory_heap, 0, BaseAddress, Size);
}

BOOL
detour_memory_free(
    _Frees_ptr_ PVOID BaseAddress)
{
    return RtlFreeHeap(_detour_memory_heap, 0, BaseAddress);
}

BOOL
detour_memory_uninitialize(VOID)
{
    if (_detour_memory_heap != NULL && _detour_memory_heap != NtGetProcessHeap())
    {
        _detour_memory_heap = RtlDestroyHeap(_detour_memory_heap);
        return _detour_memory_heap == NULL;
    }

    return TRUE;
}

BOOL
detour_memory_is_system_reserved(
    _In_ PVOID Address)
{
    return
        ((ULONG_PTR)Address >= s_ulSystemRegionLowLowerBound && (ULONG_PTR)Address <= s_ulSystemRegionLowUpperBound)
#if defined(_WIN64)
        || ((ULONG_PTR)Address >= s_ulSystemRegionHighLowerBound &&
            (ULONG_PTR)Address <= SYSTEM_RESERVED_REGION_HIGHEST)
#endif
        ;
}

_Ret_notnull_
PVOID
detour_memory_2gb_below(
    _In_ PVOID Address)
{
    return (ULONG_PTR)Address > g_sbi.MinimumUserModeAddress + _2GB ?
        (PBYTE)Address - (_2GB - _512KB) :
        (PVOID)(g_sbi.MinimumUserModeAddress + _512KB);
}

_Ret_notnull_
PVOID
detour_memory_2gb_above(
    _In_ PVOID Address)
{
    return (
#if !defined(_WIN64)
        g_sbi.MaximumUserModeAddress >= _2GB &&
#endif
        (ULONG_PTR)Address <= g_sbi.MaximumUserModeAddress - _2GB) ?
        (PBYTE)Address + (_2GB - _512KB) :
        (PVOID)(g_sbi.MaximumUserModeAddress - _512KB);
}
