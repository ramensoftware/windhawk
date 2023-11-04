#pragma once

#include <windows.h>

typedef struct tagWH_FIND_SYMBOL_OPTIONS WH_FIND_SYMBOL_OPTIONS;
typedef struct tagWH_FIND_SYMBOL WH_FIND_SYMBOL;
typedef struct tagWH_DISASM_RESULT WH_DISASM_RESULT;

// Internal functions, do not call directly.
#ifdef __cplusplus
extern "C" {
#endif

BOOL InternalWh_IsLogEnabled(void* mod);
void InternalWh_Log(void* mod, PCWSTR format, va_list args);

int InternalWh_GetIntValue(void* mod, PCWSTR valueName, int defaultValue);
BOOL InternalWh_SetIntValue(void* mod, PCWSTR valueName, int value);
size_t InternalWh_GetStringValue(void* mod,
                                 PCWSTR valueName,
                                 PWSTR stringBuffer,
                                 size_t bufferChars);
BOOL InternalWh_SetStringValue(void* mod, PCWSTR valueName, PCWSTR value);
size_t InternalWh_GetBinaryValue(void* mod,
                                 PCWSTR valueName,
                                 void* buffer,
                                 size_t bufferSize);
BOOL InternalWh_SetBinaryValue(void* mod,
                               PCWSTR valueName,
                               const void* buffer,
                               size_t bufferSize);

int InternalWh_GetIntSetting(void* mod, PCWSTR valueName, va_list args);
PCWSTR InternalWh_GetStringSetting(void* mod, PCWSTR valueName, va_list args);
void InternalWh_FreeStringSetting(void* mod, PCWSTR string);

BOOL InternalWh_SetFunctionHook(void* mod,
                                void* targetFunction,
                                void* hookFunction,
                                void** originalFunction);
BOOL InternalWh_RemoveFunctionHook(void* mod, void* targetFunction);
BOOL InternalWh_ApplyHookOperations(void* mod);

HANDLE InternalWh_FindFirstSymbol3(void* mod,
                                   HMODULE hModule,
                                   const WH_FIND_SYMBOL_OPTIONS* options,
                                   WH_FIND_SYMBOL* findData);
BOOL InternalWh_FindNextSymbol2(void* mod,
                                HANDLE symSearch,
                                WH_FIND_SYMBOL* findData);
void InternalWh_FindCloseSymbol(void* mod, HANDLE symSearch);

BOOL InternalWh_Disasm(void* mod, void* address, WH_DISASM_RESULT* result);

#ifdef __cplusplus
}
#endif

// Internal definitions for mods.
#ifdef WH_MOD

inline void* InternalWhModPtr;

inline void InternalWh_Log_Wrapper(PCWSTR format, ...) {
    va_list args;
    va_start(args, format);
    InternalWh_Log(InternalWhModPtr, format, args);
    va_end(args);
}

#endif  // WH_MOD
