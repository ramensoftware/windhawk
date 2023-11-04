#include "stdafx.h"

#include "mod.h"
#include "mods_api.h"

BOOL InternalWh_IsLogEnabled(void* mod) {
    return static_cast<LoadedMod*>(mod)->IsLogEnabled();
}

void InternalWh_Log(void* mod, PCWSTR format, va_list args) {
    static_cast<LoadedMod*>(mod)->Log(format, args);
}

int InternalWh_GetIntValue(void* mod, PCWSTR valueName, int defaultValue) {
    return static_cast<LoadedMod*>(mod)->GetIntValue(valueName, defaultValue);
}

BOOL InternalWh_SetIntValue(void* mod, PCWSTR valueName, int value) {
    return static_cast<LoadedMod*>(mod)->SetIntValue(valueName, value);
}

size_t InternalWh_GetStringValue(void* mod,
                                 PCWSTR valueName,
                                 PWSTR stringBuffer,
                                 size_t bufferChars) {
    return static_cast<LoadedMod*>(mod)->GetStringValue(valueName, stringBuffer,
                                                        bufferChars);
}

BOOL InternalWh_SetStringValue(void* mod, PCWSTR valueName, PCWSTR value) {
    return static_cast<LoadedMod*>(mod)->SetStringValue(valueName, value);
}

size_t InternalWh_GetBinaryValue(void* mod,
                                 PCWSTR valueName,
                                 void* buffer,
                                 size_t bufferSize) {
    return static_cast<LoadedMod*>(mod)->GetBinaryValue(valueName, buffer,
                                                        bufferSize);
}

BOOL InternalWh_SetBinaryValue(void* mod,
                               PCWSTR valueName,
                               const void* buffer,
                               size_t bufferSize) {
    return static_cast<LoadedMod*>(mod)->SetBinaryValue(valueName, buffer,
                                                        bufferSize);
}

int InternalWh_GetIntSetting(void* mod, PCWSTR valueName, va_list args) {
    return static_cast<LoadedMod*>(mod)->GetIntSetting(valueName, args);
}

PCWSTR InternalWh_GetStringSetting(void* mod, PCWSTR valueName, va_list args) {
    return static_cast<LoadedMod*>(mod)->GetStringSetting(valueName, args);
}

void InternalWh_FreeStringSetting(void* mod, PCWSTR string) {
    static_cast<LoadedMod*>(mod)->FreeStringSetting(string);
}

BOOL InternalWh_SetFunctionHook(void* mod,
                                void* targetFunction,
                                void* hookFunction,
                                void** originalFunction) {
    return static_cast<LoadedMod*>(mod)->SetFunctionHook(
        targetFunction, hookFunction, originalFunction);
}

BOOL InternalWh_RemoveFunctionHook(void* mod, void* targetFunction) {
    return static_cast<LoadedMod*>(mod)->RemoveFunctionHook(targetFunction);
}

BOOL InternalWh_ApplyHookOperations(void* mod) {
    return static_cast<LoadedMod*>(mod)->ApplyHookOperations();
}

HANDLE InternalWh_FindFirstSymbol(void* mod,
                                  HMODULE hModule,
                                  PCWSTR symbolServer,
                                  void* findData) {
    return static_cast<LoadedMod*>(mod)->FindFirstSymbol(hModule, symbolServer,
                                                         findData);
}

HANDLE InternalWh_FindFirstSymbol2(void* mod,
                                   HMODULE hModule,
                                   PCWSTR symbolServer,
                                   WH_FIND_SYMBOL* findData) {
    return static_cast<LoadedMod*>(mod)->FindFirstSymbol2(hModule, symbolServer,
                                                          findData);
}

HANDLE InternalWh_FindFirstSymbol3(void* mod,
                                   HMODULE hModule,
                                   const WH_FIND_SYMBOL_OPTIONS* options,
                                   WH_FIND_SYMBOL* findData) {
    return static_cast<LoadedMod*>(mod)->FindFirstSymbol3(hModule, options,
                                                          findData);
}

BOOL InternalWh_FindNextSymbol(void* mod, HANDLE symSearch, void* findData) {
    return static_cast<LoadedMod*>(mod)->FindNextSymbol(symSearch, findData);
}

BOOL InternalWh_FindNextSymbol2(void* mod,
                                HANDLE symSearch,
                                WH_FIND_SYMBOL* findData) {
    return static_cast<LoadedMod*>(mod)->FindNextSymbol2(symSearch, findData);
}

void InternalWh_FindCloseSymbol(void* mod, HANDLE symSearch) {
    static_cast<LoadedMod*>(mod)->FindCloseSymbol(symSearch);
}

BOOL InternalWh_Disasm(void* mod, void* address, WH_DISASM_RESULT* result) {
    return static_cast<LoadedMod*>(mod)->Disasm(address, result);
}
