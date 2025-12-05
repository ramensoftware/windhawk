/*
 * KNSoft.SlimDetours (https://github.com/KNSoft/KNSoft.SlimDetours) Trampoline management
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

typedef struct _DETOUR_REGION DETOUR_REGION, *PDETOUR_REGION;

struct _DETOUR_REGION
{
    ULONGLONG ullSignature;
    PDETOUR_REGION pNext;       // Next region in list of regions.
    PDETOUR_TRAMPOLINE pFree;   // List of free trampolines in this region.
};

#define DETOUR_REGION_SIGNATURE ((ULONGLONG)'lSNK' << 32 | 'srtD')
#define DETOUR_REGION_SIZE 0x10000UL
#define DETOUR_TRAMPOLINES_PER_REGION ((DETOUR_REGION_SIZE / sizeof(DETOUR_TRAMPOLINE)) - 1)
static PDETOUR_REGION s_pRegions = NULL; // List of all regions.
static PDETOUR_REGION s_pRegion = NULL; // Default region.

NTSTATUS
detour_writable_trampoline_regions(VOID)
{
    NTSTATUS Status;
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;

    // Mark all of the regions as writable.
    sMem = DETOUR_REGION_SIZE;
    for (PDETOUR_REGION pRegion = s_pRegions; pRegion != NULL; pRegion = pRegion->pNext)
    {
        pMem = pRegion;
        Status = NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, PAGE_EXECUTE_READWRITE, &dwOld);
        if (!NT_SUCCESS(Status))
        {
            return Status;
        }
    }
    return STATUS_SUCCESS;
}

VOID
detour_runnable_trampoline_regions(VOID)
{
    PVOID pMem;
    SIZE_T sMem;
    DWORD dwOld;

    // Mark all of the regions as executable.
    sMem = DETOUR_REGION_SIZE;
    for (PDETOUR_REGION pRegion = s_pRegions; pRegion != NULL; pRegion = pRegion->pNext)
    {
        pMem = pRegion;
        NtProtectVirtualMemory(NtCurrentProcess(), &pMem, &sMem, PAGE_EXECUTE_READ, &dwOld);
        NtFlushInstructionCache(NtCurrentProcess(), pRegion, DETOUR_REGION_SIZE);
    }
}

static
PBYTE
detour_alloc_round_down_to_region(
    PBYTE pbTry)
{
    // WinXP64 returns free areas that aren't REGION aligned to 32-bit applications.
    ULONG_PTR extra = ((ULONG_PTR)pbTry) & (DETOUR_REGION_SIZE - 1);
    if (extra != 0)
    {
        pbTry -= extra;
    }
    return pbTry;
}

static
PBYTE
detour_alloc_round_up_to_region(
    PBYTE pbTry)
{
    // WinXP64 returns free areas that aren't REGION aligned to 32-bit applications.
    ULONG_PTR extra = ((ULONG_PTR)pbTry) & (DETOUR_REGION_SIZE - 1);
    if (extra != 0)
    {
        ULONG_PTR adjust = DETOUR_REGION_SIZE - extra;
        pbTry += adjust;
    }
    return pbTry;
}

// Starting at pbLo, try to allocate a memory region, continue until pbHi.

static
PVOID
detour_alloc_region_from_lo(
    PBYTE pbLo,
    PBYTE pbHi)
{
    NTSTATUS Status;
    PVOID pMem;
    SIZE_T sMem;
    MEMORY_BASIC_INFORMATION mbi;

    PBYTE pbTry = detour_alloc_round_up_to_region(pbLo);

    DETOUR_TRACE(" Looking for free region in %p..%p from %p:\n", pbLo, pbHi, pbTry);

    while (pbTry < pbHi)
    {
        if (detour_memory_is_system_reserved(pbTry))
        {
            // Skip region reserved for system DLLs, but preserve address space entropy.
            pbTry += 0x08000000;
            continue;
        }

        if (!NT_SUCCESS(NtQueryVirtualMemory(NtCurrentProcess(),
                                             pbTry,
                                             MemoryBasicInformation,
                                             &mbi,
                                             sizeof(mbi),
                                             NULL)))
        {
            break;
        }

        DETOUR_TRACE("  Try %p => %p..%p %6lx\n",
                     pbTry,
                     mbi.BaseAddress,
                     Add2Ptr(mbi.BaseAddress, mbi.RegionSize - 1),
                     mbi.State);

        if (mbi.State == MEM_FREE && mbi.RegionSize >= DETOUR_REGION_SIZE)
        {
            pMem = pbTry;
            sMem = DETOUR_REGION_SIZE;
            Status = NtAllocateVirtualMemory(NtCurrentProcess(),
                                             &pMem,
                                             0,
                                             &sMem,
                                             MEM_COMMIT | MEM_RESERVE,
                                             PAGE_EXECUTE_READWRITE);
            if (NT_SUCCESS(Status))
            {
                return pMem;
            } else if (Status == STATUS_DYNAMIC_CODE_BLOCKED)
            {
                return NULL;
            }
            pbTry += DETOUR_REGION_SIZE;
        } else
        {
            pbTry = detour_alloc_round_up_to_region((PBYTE)mbi.BaseAddress + mbi.RegionSize);
        }
    }
    return NULL;
}

// Starting at pbHi, try to allocate a memory region, continue until pbLo.

static
PVOID
detour_alloc_region_from_hi(
    PBYTE pbLo,
    PBYTE pbHi)
{
    NTSTATUS Status;
    PVOID pMem;
    SIZE_T sMem;
    MEMORY_BASIC_INFORMATION mbi;

    PBYTE pbTry = detour_alloc_round_down_to_region(pbHi - DETOUR_REGION_SIZE);

    DETOUR_TRACE(" Looking for free region in %p..%p from %p:\n", pbLo, pbHi, pbTry);

    while (pbTry > pbLo)
    {
        DETOUR_TRACE("  Try %p\n", pbTry);
        if (detour_memory_is_system_reserved(pbTry))
        {
            // Skip region reserved for system DLLs, but preserve address space entropy.
            pbTry -= 0x08000000;
            continue;
        }

        if (!NT_SUCCESS(NtQueryVirtualMemory(NtCurrentProcess(),
                                             pbTry,
                                             MemoryBasicInformation,
                                             &mbi,
                                             sizeof(mbi),
                                             NULL)))
        {
            break;
        }

        DETOUR_TRACE("  Try %p => %p..%p %6lx\n",
                     pbTry,
                     mbi.BaseAddress,
                     Add2Ptr(mbi.BaseAddress, mbi.RegionSize - 1),
                     mbi.State);

        if (mbi.State == MEM_FREE && mbi.RegionSize >= DETOUR_REGION_SIZE)
        {
            pMem = pbTry;
            sMem = DETOUR_REGION_SIZE;
            Status = NtAllocateVirtualMemory(NtCurrentProcess(),
                                             &pMem,
                                             0,
                                             &sMem,
                                             MEM_COMMIT | MEM_RESERVE,
                                             PAGE_EXECUTE_READWRITE);
            if (NT_SUCCESS(Status))
            {
                return pMem;
            } else if (Status == STATUS_DYNAMIC_CODE_BLOCKED)
            {
                return NULL;
            }
            pbTry -= DETOUR_REGION_SIZE;
        } else
        {
            pbTry = detour_alloc_round_down_to_region((PBYTE)mbi.AllocationBase - DETOUR_REGION_SIZE);
        }
    }
    return NULL;
}

static
PVOID
detour_alloc_trampoline_allocate_new(
    PBYTE pbTarget,
    PDETOUR_TRAMPOLINE pLo,
    PDETOUR_TRAMPOLINE pHi)
{
    PVOID pbTry = NULL;

    // NB: We must always also start the search at an offset from pbTarget
    //     in order to maintain ASLR entropy.

#if defined(_WIN64)
    // Try looking 1GB below or lower.
    if (pbTry == NULL && pbTarget > (PBYTE)0x40000000)
    {
        pbTry = detour_alloc_region_from_hi((PBYTE)pLo, pbTarget - 0x40000000);
    }
    // Try looking 1GB above or higher.
    if (pbTry == NULL && pbTarget < (PBYTE)0xffffffff40000000)
    {
        pbTry = detour_alloc_region_from_lo(pbTarget + 0x40000000, (PBYTE)pHi);
    }
    // Try looking 1GB below or higher.
    if (pbTry == NULL && pbTarget > (PBYTE)0x40000000)
    {
        pbTry = detour_alloc_region_from_lo(pbTarget - 0x40000000, pbTarget);
    }
    // Try looking 1GB above or lower.
    if (pbTry == NULL && pbTarget < (PBYTE)0xffffffff40000000)
    {
        pbTry = detour_alloc_region_from_hi(pbTarget, pbTarget + 0x40000000);
    }
#endif

    // Try anything below.
    if (pbTry == NULL)
    {
        pbTry = detour_alloc_region_from_hi((PBYTE)pLo, pbTarget);
    }
    // Try anything above.
    if (pbTry == NULL)
    {
        pbTry = detour_alloc_region_from_lo(pbTarget, (PBYTE)pHi);
    }

    return pbTry;
}

_Ret_maybenull_
PDETOUR_TRAMPOLINE
detour_alloc_trampoline(
    _In_ PBYTE pbTarget)
{
    // We have to place trampolines within +/- 2GB of target.

    PDETOUR_TRAMPOLINE pLo;
    PDETOUR_TRAMPOLINE pHi;

    detour_find_jmp_bounds(pbTarget, (PVOID*)&pLo, (PVOID*)&pHi);

    PDETOUR_TRAMPOLINE pTrampoline = NULL;

    // Insure that there is a default region.
    if (s_pRegion == NULL && s_pRegions != NULL)
    {
        s_pRegion = s_pRegions;
    }

    // First check the default region for an valid free block.
    if (s_pRegion != NULL && s_pRegion->pFree != NULL &&
        s_pRegion->pFree >= pLo && s_pRegion->pFree <= pHi)
    {

found_region:
        pTrampoline = s_pRegion->pFree;
        // do a last sanity check on region.
        if (pTrampoline < pLo || pTrampoline > pHi)
        {
            return NULL;
        }
        s_pRegion->pFree = (PDETOUR_TRAMPOLINE)pTrampoline->pbRemain;
        RtlFillMemory(pTrampoline, sizeof(*pTrampoline), 0xcc);
        return pTrampoline;
    }

    // Then check the existing regions for a valid free block.
    for (s_pRegion = s_pRegions; s_pRegion != NULL; s_pRegion = s_pRegion->pNext)
    {
        if (s_pRegion != NULL && s_pRegion->pFree != NULL && s_pRegion->pFree >= pLo && s_pRegion->pFree <= pHi)
        {
            goto found_region;
        }
    }

    // We need to allocate a new region.

    // Round pbTarget down to 64KB block.
    // /RTCc RuntimeChecks breaks PtrToUlong.
    pbTarget = pbTarget - (ULONG)((ULONG_PTR)pbTarget & 0xffff);

    PVOID pbNewlyAllocated = detour_alloc_trampoline_allocate_new(pbTarget, pLo, pHi);
    if (pbNewlyAllocated != NULL)
    {
        s_pRegion = (DETOUR_REGION*)pbNewlyAllocated;
        s_pRegion->ullSignature = DETOUR_REGION_SIGNATURE;
        s_pRegion->pFree = NULL;
        s_pRegion->pNext = s_pRegions;
        s_pRegions = s_pRegion;
        DETOUR_TRACE("  Allocated region %p..%p\n\n", s_pRegion, Add2Ptr(s_pRegion, DETOUR_REGION_SIZE - 1));

        // Put everything but the first trampoline on the free list.
        PBYTE pFree = NULL;
        pTrampoline = ((PDETOUR_TRAMPOLINE)s_pRegion) + 1;
        for (int i = DETOUR_TRAMPOLINES_PER_REGION - 1; i > 1; i--)
        {
            pTrampoline[i].pbRemain = pFree;
            pFree = (PBYTE)&pTrampoline[i];
        }
        s_pRegion->pFree = (PDETOUR_TRAMPOLINE)pFree;
        goto found_region;
    }

    DETOUR_TRACE("Couldn't find available memory region!\n");
    return NULL;
}

VOID
detour_free_trampoline(
    _In_ PDETOUR_TRAMPOLINE pTrampoline)
{
    PDETOUR_REGION pRegion = (PDETOUR_REGION)((ULONG_PTR)pTrampoline & ~(ULONG_PTR)0xffff);

    RtlZeroMemory(pTrampoline, sizeof(*pTrampoline));
    pTrampoline->pbRemain = (PBYTE)pRegion->pFree;
    pRegion->pFree = pTrampoline;
}

static
BOOL
detour_is_region_empty(
    PDETOUR_REGION pRegion)
{
    // Stop if the region isn't a region (this would be bad).
    if (pRegion->ullSignature != DETOUR_REGION_SIGNATURE)
    {
        return FALSE;
    }

    PBYTE pbRegionBeg = (PBYTE)pRegion;
    PBYTE pbRegionLim = pbRegionBeg + DETOUR_REGION_SIZE;

    // Stop if any of the trampolines aren't free.
    PDETOUR_TRAMPOLINE pTrampoline = ((PDETOUR_TRAMPOLINE)pRegion) + 1;
    for (int i = 0; i < DETOUR_TRAMPOLINES_PER_REGION; i++)
    {
        if (pTrampoline[i].pbRemain != NULL &&
            (pTrampoline[i].pbRemain < pbRegionBeg ||
             pTrampoline[i].pbRemain >= pbRegionLim))
        {
            return FALSE;
        }
    }

    // OK, the region is empty.
    return TRUE;
}

static
VOID
detour_free_region(
    _Out_ PDETOUR_REGION* ppRegionBase,
    _In_ PDETOUR_REGION pRegion)
{
    *ppRegionBase = pRegion->pNext;
    PVOID pMem = pRegion;
    SIZE_T sMem = 0;
    NtFreeVirtualMemory(NtCurrentProcess(), &pMem, &sMem, MEM_RELEASE);
}

VOID
detour_free_unused_trampoline_regions(VOID)
{
    PDETOUR_REGION* ppRegionBase = &s_pRegions;
    PDETOUR_REGION pRegion = s_pRegions;

    while (pRegion != NULL)
    {
        if (detour_is_region_empty(pRegion))
        {
            detour_free_region(ppRegionBase, pRegion);
            s_pRegion = NULL;
        } else
        {
            ppRegionBase = &pRegion->pNext;
        }
        pRegion = *ppRegionBase;
    }
}

VOID
detour_free_trampoline_region_if_unused(
    _In_ PDETOUR_TRAMPOLINE pTrampoline)
{
    PDETOUR_REGION pTargetRegion = (PDETOUR_REGION)((ULONG_PTR)pTrampoline & ~(ULONG_PTR)0xffff);

    PDETOUR_REGION* ppRegionBase = &s_pRegions;
    PDETOUR_REGION pRegion = s_pRegions;

    while (pRegion != NULL)
    {
        if (pRegion == pTargetRegion)
        {
            if (detour_is_region_empty(pRegion))
            {
                detour_free_region(ppRegionBase, pRegion);
                s_pRegion = NULL;
            }
            break;
        }

        ppRegionBase = &pRegion->pNext;
        pRegion = *ppRegionBase;
    }
}

BYTE
detour_align_from_trampoline(
    _In_ PDETOUR_TRAMPOLINE pTrampoline,
    BYTE obTrampoline)
{
    for (ULONG n = 0; n < ARRAYSIZE(pTrampoline->rAlign); n++)
    {
        if (pTrampoline->rAlign[n].obTrampoline == obTrampoline)
        {
            return pTrampoline->rAlign[n].obTarget;
        }
    }
    return 0;
}

BYTE
detour_align_from_target(
    _In_ PDETOUR_TRAMPOLINE pTrampoline,
    BYTE obTarget)
{
    for (ULONG n = 0; n < ARRAYSIZE(pTrampoline->rAlign); n++)
    {
        if (pTrampoline->rAlign[n].obTarget == obTarget)
        {
            return pTrampoline->rAlign[n].obTrampoline;
        }
    }
    return 0;
}
