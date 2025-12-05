/*
 * Adapt to NDKs to access low-level Windows NT APIs
 * 
 * SlimDetours uses KNSoft.NDK by default, and also support other NDKs.
 * 
 * KNSoft.NDK
 *   Used when macro `_USE_KNSOFT_NDK` is defined, this is the default behavior on offical project.
 * 
 * ReactOS NDK
 *   Used when macro `__REACTOS__` is defined, can be built with ReactOS.
 * 
 * Other NDKs
 *   Include other NDKs (e.g. phnt) header before SlimDetours, they should provide what we need.
 */

#pragma once

#if defined(_USE_KNSOFT_NDK)

#include <KNSoft/NDK/NDK.h>

#elif defined(__REACTOS__)

#define WIN32_NO_STATUS
#include <windef.h>
#include <winbase.h>

#define NTOS_MODE_USER
#include <ndk/exfuncs.h>
#include <ndk/obfuncs.h>
#include <ndk/mmfuncs.h>
#include <ndk/kefuncs.h>
#include <ndk/psfuncs.h>
#include <ndk/cmfuncs.h>
#include <ndk/rtlfuncs.h>
#include <ndk/umfuncs.h>

#endif

/* Add KNSoft.NDK specific stuff */
#ifndef _USE_KNSOFT_NDK 

/* Use phnt */
#include <phnt/phnt_windows.h>
#include <phnt/phnt.h>
#undef NtCurrentProcessId
#undef NtCurrentThreadId

#define PAGE_SIZE 0x1000
#define MM_ALLOCATION_GRANULARITY 0x10000

#if defined(_X86_)
#define CONTEXT_PC Eip
#elif defined(_AMD64_)
#define CONTEXT_PC Rip
#elif defined(_ARM64_)
#define CONTEXT_PC Pc
#endif

#define Add2Ptr(P,I) ((PVOID)((PUCHAR)(P) + (I)))
#define PtrOffset(B,O) ((ULONG)((ULONG_PTR)(O) - (ULONG_PTR)(B)))

#define MM_LOWEST_USER_ADDRESS ((PVOID)(LONG_PTR)0x10000)

#if defined(_WIN64)

/* [0x00007FF7FFFF0000 ... 0x00007FFFFFFF0000), 32G */
#define MI_ASLR_BITMAP_SIZE 0x10000
#define MI_ASLR_LOWEST_SYSTEM_RANGE_ADDRESS ((PVOID)0x00007FF7FFFF0000ULL)
#define MI_ASLR_HIGHEST_SYSTEM_RANGE_ADDRESS ((PVOID)0x00007FFFFFFEFFFFULL)

#else

/* [0x50000000 ... 0x78000000), 640M */
#define MI_ASLR_BITMAP_SIZE 0x500
#define MI_ASLR_LOWEST_SYSTEM_RANGE_ADDRESS ((PVOID)0x50000000UL)
#define MI_ASLR_HIGHEST_SYSTEM_RANGE_ADDRESS ((PVOID)0x77FFFFFFUL)

#endif

C_ASSERT((ULONG_PTR)MI_ASLR_HIGHEST_SYSTEM_RANGE_ADDRESS - (ULONG_PTR)MI_ASLR_LOWEST_SYSTEM_RANGE_ADDRESS + 1 == (ULONG_PTR)MI_ASLR_BITMAP_SIZE * 8UL * (ULONG_PTR)MM_ALLOCATION_GRANULARITY);

#define NtCurrentProcessId() ((HANDLE)NtCurrentTeb()->ClientId.UniqueProcess)
#define NtCurrentThreadId() ((HANDLE)NtCurrentTeb()->ClientId.UniqueThread)
#define NtGetNtdllBase() (CONTAINING_RECORD(NtCurrentPeb()->Ldr->InInitializationOrderModuleList.Flink, LDR_DATA_TABLE_ENTRY, InInitializationOrderLinks)->DllBase)

#define RtlProcessHeap() (NtCurrentPeb()->ProcessHeap)

#if defined(_WIN64)
#define DECLSPEC_POINTERALIGN DECLSPEC_ALIGN(8)
#else
#define DECLSPEC_POINTERALIGN DECLSPEC_ALIGN(4)
#endif

#define DEFINE_ANYSIZE_STRUCT(varName, baseType, arrayType, arraySize) struct {\
    baseType BaseType;\
    arrayType Array[(arraySize) - 1];\
} varName

#define _1KB (1024ULL)
#define _1MB (1024ULL * _1KB)
#define _1GB (1024ULL * _1MB)

#define _KB(x) ((x) * _1KB)
#define _MB(x) ((x) * _1MB)
#define _GB(x) ((x) * _1GB)

#define _512KB  _KB(512)
#define _640MB  _MB(640)
#define _2GB    _GB(2)
#define _32GB   _GB(32)

#define MM_SHARED_USER_DATA_VA 0x7FFE0000

#define SharedUserData ((KUSER_SHARED_DATA * const)MM_SHARED_USER_DATA_VA)

#endif
