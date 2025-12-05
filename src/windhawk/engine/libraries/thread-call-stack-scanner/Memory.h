#pragma once

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
threadscan_memory_alloc(
    _In_ SIZE_T Size);

_Must_inspect_result_
_Ret_maybenull_
_Post_writable_byte_size_(Size)
PVOID
threadscan_memory_realloc(
    _Frees_ptr_opt_ PVOID BaseAddress,
    _In_ SIZE_T Size);

BOOL
threadscan_memory_free(
    _Frees_ptr_ PVOID BaseAddress);

BOOL
threadscan_memory_initialize(VOID);

BOOL
threadscan_memory_uninitialize(VOID);
