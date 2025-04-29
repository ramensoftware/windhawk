#pragma once

#include <windows.h>

#ifndef WH_EDITING
#include "mods_api_internal.h"
#define WH_INTERNAL(x) (x)
#define WH_INTERNAL_OR(x, y) (x)
#else
#define WH_INTERNAL(x)
#define WH_INTERNAL_OR(x, y) (y)
#endif

typedef struct tagWH_FIND_SYMBOL_OPTIONS {
    // Must be set to `sizeof(WH_FIND_SYMBOL_OPTIONS)`.
    size_t optionsSize;
    // The symbol server to query. Set to `NULL` to query the Microsoft public
    // symbol server.
    PCWSTR symbolServer;
    // Set to `TRUE` to only retrieve decorated symbols, making the enumeration
    // faster. Can be especially useful for very large modules such as Chrome or
    // Firefox.
    BOOL noUndecoratedSymbols;
} WH_FIND_SYMBOL_OPTIONS;

typedef struct tagWH_FIND_SYMBOL {
    void* address;
    PCWSTR symbol;
    PCWSTR symbolDecorated;  // Since Windhawk v1.0
} WH_FIND_SYMBOL;

typedef struct tagWH_HOOK_SYMBOLS_OPTIONS {
    // Must be set to `sizeof(WH_HOOK_SYMBOLS_OPTIONS)`.
    size_t optionsSize;
    // Same as for `WH_FIND_SYMBOL_OPTIONS`.
    PCWSTR symbolServer;
    // Same as for `WH_FIND_SYMBOL_OPTIONS`.
    BOOL noUndecoratedSymbols;
    // The online cache URL that will be used before downloading the symbols.
    // Set to `NULL` to use the default online cache URL. Set to an empty string
    // to disable the online cache.
    PCWSTR onlineCacheUrl;
} WH_HOOK_SYMBOLS_OPTIONS;

typedef struct tagWH_DISASM_RESULT {
    // The length of the decoded instruction.
    size_t length;
    // The textual, human-readable representation of the instruction.
    char text[96];
} WH_DISASM_RESULT;

typedef struct tagWH_GET_URL_CONTENT_OPTIONS {
    // Must be set to `sizeof(WH_GET_URL_CONTENT_OPTIONS)`.
    size_t optionsSize;
    // The path to the file to which the content will be written. If set, the
    // data will be written to the file and the `data` field of the returned
    // struct will be `NULL`. If this field is `NULL`, the content will be
    // returned in the `data` field.
    PCWSTR targetFilePath;
} WH_GET_URL_CONTENT_OPTIONS;

typedef struct tagWH_URL_CONTENT {
    const char* data;
    size_t length;
    int statusCode;
} WH_URL_CONTENT;

// Definitions for mods.
#ifdef WH_MOD

// Placeholder values for the editor, will be defined when the mod is compiled.
#ifdef WH_EDITING
#define WH_MOD_ID L"mod-id-placeholder"
#define WH_MOD_VERSION L"1.0"
#endif

#ifndef WH_EDITING
#define Wh_Log(message, ...)                                       \
    do {                                                           \
        if (InternalWh_IsLogEnabled(InternalWhModPtr)) {           \
            InternalWh_Log_Wrapper(L"[%d:%S]: " message, __LINE__, \
                                   __FUNCTION__, ##__VA_ARGS__);   \
        }                                                          \
    } while (0)
#else
/**
 * @brief Logs a message. If logging is enabled, the message can be viewed in
 *     the editor log output window. The arguments are only evaluated if logging
 *     is enabled.
 * @param message The message to be logged. It can optionally contain embedded
 *     printf-style format specifiers that are replaced by the values specified
 *     in subsequent additional arguments and formatted as requested.
 * @return None.
 */
inline void Wh_Log(PCWSTR message, ...) {}
#endif

/**
 * @brief Retrieves an integer value from the mod's local storage.
 * @param valueName The name of the value to retrieve.
 * @param defaultValue The default value to be returned as a fallback.
 * @return The retrieved integer value. If the value doesn't exist or in case of
 *     an error, the provided default value is returned.
 */
inline int Wh_GetIntValue(PCWSTR valueName, int defaultValue) {
    return WH_INTERNAL_OR(
        InternalWh_GetIntValue(InternalWhModPtr, valueName, defaultValue), 0);
}

/**
 * @brief Stores an integer value in the mod's local storage.
 * @param valueName The name of the value to store.
 * @param value The value to store.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_SetIntValue(PCWSTR valueName, int value) {
    return WH_INTERNAL_OR(
        InternalWh_SetIntValue(InternalWhModPtr, valueName, value), FALSE);
}

/**
 * @brief Retrieves a string value from the mod's local storage.
 * @param valueName The name of the value to retrieve.
 * @param stringBuffer The buffer that will receive the text, terminated with a
 *     null character.
 * @param bufferChars The length of `stringBuffer`, in characters. The buffer
 *     must be large enough to include the terminating null character.
 * @return The number of characters copied to the buffer, not including the
 *     terminating null character. If the value doesn't exist, if the buffer is
 *     not large enough, or in case of an error, an empty string is returned.
 */
inline size_t Wh_GetStringValue(PCWSTR valueName,
                                PWSTR stringBuffer,
                                size_t bufferChars) {
    return WH_INTERNAL_OR(InternalWh_GetStringValue(InternalWhModPtr, valueName,
                                                    stringBuffer, bufferChars),
                          0);
}

/**
 * @brief Stores a string value in the mod's local storage.
 * @param valueName The name of the value to store.
 * @param value A null-terminated string containing the value to store.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_SetStringValue(PCWSTR valueName, PCWSTR value) {
    return WH_INTERNAL_OR(
        InternalWh_SetStringValue(InternalWhModPtr, valueName, value), FALSE);
}

/**
 * @brief Retrieves a binary value (raw bytes) from the mod's local storage.
 * @param valueName The name of the value to retrieve.
 * @param buffer The buffer that will receive the value.
 * @param bufferSize The length of the buffer, in bytes.
 * @return The number of bytes copied to the buffer. If the value doesn't exist,
 *     if the buffer is not large enough, or in case of an error, no data is
 *     copied and the return value is zero.
 */
inline size_t Wh_GetBinaryValue(PCWSTR valueName,
                                void* buffer,
                                size_t bufferSize) {
    return WH_INTERNAL_OR(InternalWh_GetBinaryValue(InternalWhModPtr, valueName,
                                                    buffer, bufferSize),
                          0);
}

/**
 * @brief Stores a binary value (raw bytes) in the mod's local storage.
 * @param valueName The name of the value to store.
 * @param buffer An array of bytes containing the value to store.
 * @param bufferSize The size of the array of bytes.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_SetBinaryValue(PCWSTR valueName,
                              const void* buffer,
                              size_t bufferSize) {
    return WH_INTERNAL_OR(InternalWh_SetBinaryValue(InternalWhModPtr, valueName,
                                                    buffer, bufferSize),
                          FALSE);
}

/**
 * @brief Deletes a value from the mod's local storage.
 * @since Windhawk v1.5
 * @param valueName The name of the value to delete.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_DeleteValue(PCWSTR valueName) {
    return WH_INTERNAL_OR(InternalWh_DeleteValue(InternalWhModPtr, valueName),
                          FALSE);
}

/**
 * @brief Retrieves the mod's storage directory path. The directory can be used
 *     by the mod to store any necessary files. The directory will be removed
 *     when the mod is removed.
 * @param pathBuffer The buffer that will receive the path, terminated with a
 *     null character.
 * @param bufferChars The length of `pathBuffer`, in characters. The buffer must
 *     be large enough to include the terminating null character.
 * @return The number of characters copied to the buffer, not including the
 *     terminating null character. If the buffer is not large enough or in case
 *     of an error, an empty string is returned.
 */
inline size_t Wh_GetModStoragePath(PWSTR pathBuffer, size_t bufferChars) {
    return WH_INTERNAL_OR(
        InternalWh_GetModStoragePath(InternalWhModPtr, pathBuffer, bufferChars),
        0);
}

/**
 * @brief Retrieves an integer value from the mod's user settings.
 * @param valueName The name of the value to retrieve. It can optionally contain
 *     embedded printf-style format specifiers that are replaced by the values
 *     specified in subsequent additional arguments and formatted as requested.
 * @return The retrieved integer value. If the value doesn't exist or in case of
 *     an error, the return value is zero.
 */
inline int Wh_GetIntSetting(PCWSTR valueName, ...) {
    va_list args;
    va_start(args, valueName);
    int result = WH_INTERNAL_OR(
        InternalWh_GetIntSetting(InternalWhModPtr, valueName, args), 0);
    va_end(args);
    return result;
}

/**
 * @brief Retrieves a string value from the mod's user settings. When no longer
 *     needed, free the memory with `Wh_FreeStringSetting`.
 * @param valueName The name of the value to retrieve. It can optionally contain
 *     embedded printf-style format specifiers that are replaced by the values
 *     specified in subsequent additional arguments and formatted as requested.
 * @return The retrieved string value. If the value doesn't exist or in case of
 *     an error, an empty string is returned.
 */
inline PCWSTR Wh_GetStringSetting(PCWSTR valueName, ...) {
    va_list args;
    va_start(args, valueName);
    PCWSTR result = WH_INTERNAL_OR(
        InternalWh_GetStringSetting(InternalWhModPtr, valueName, args), L"");
    va_end(args);
    return result;
}

/**
 * @brief Frees a string returned by `Wh_GetStringSetting`.
 * @param string The string to free.
 * @return None.
 */
inline void Wh_FreeStringSetting(PCWSTR string) {
    WH_INTERNAL(InternalWh_FreeStringSetting(InternalWhModPtr, string));
}

/**
 * @brief Registers a hook for the specified target function. Can't be called
 *     after `Wh_ModBeforeUninit` returns. Registered hook operations can be
 *     applied with `Wh_ApplyHookOperations`.
 * @param targetFunction A pointer to the target function, which will be
 *     overridden by the detour function.
 * @param hookFunction A pointer to the detour function, which will override the
 *     target function.
 * @param originalFunction A pointer to the trampoline function, which will be
 *     used to call the original target function. Can be `NULL`.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_SetFunctionHook(void* targetFunction,
                               void* hookFunction,
                               void** originalFunction) {
    return WH_INTERNAL_OR(
        InternalWh_SetFunctionHook(InternalWhModPtr, targetFunction,
                                   hookFunction, originalFunction),
        FALSE);
}

/**
 * @brief Registers a hook to be removed for the specified target function.
 *     Can't be called before `Wh_ModInit` returns or after `Wh_ModBeforeUninit`
 *     returns. Registered hook operations can be applied with
 *     `Wh_ApplyHookOperations`.
 * @since Windhawk v1.0
 * @param targetFunction A pointer to the target function, for which the hook
 *     will be removed.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_RemoveFunctionHook(void* targetFunction) {
    return WH_INTERNAL_OR(
        InternalWh_RemoveFunctionHook(InternalWhModPtr, targetFunction), FALSE);
}

/**
 * @brief Applies hook operations registered by `Wh_SetFunctionHook` and
 *     `Wh_RemoveFunctionHook`. Called automatically by Windhawk after
 *     `Wh_ModInit`. Can't be called before `Wh_ModInit` returns or after
 *     `Wh_ModBeforeUninit` returns. Note: This function is very slow, avoid
 *     using it if possible. Ideally, all hooks should be set in `Wh_ModInit`
 *     and this function should never be used.
 * @since Windhawk v1.0
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_ApplyHookOperations() {
    return WH_INTERNAL_OR(InternalWh_ApplyHookOperations(InternalWhModPtr),
                          FALSE);
}

/**
 * @brief Returns information about the first symbol for the specified module
 *     handle.
 * @since `options` param since v1.4
 * @param hModule A handle to the loaded module whose information is being
 *     requested. If this parameter is `NULL`, the module of the current process
 *     (.exe file) is used.
 * @param options Can be used to customize the symbol enumeration. Pass `NULL`
 *     to use the default options.
 * @param findData A pointer to a structure to receive the symbol information.
 * @return A search handle used in a subsequent call to `Wh_FindNextSymbol` or
 *     `Wh_FindCloseSymbol`. If no symbols are found or in case of an error, the
 *     return value is `NULL`.
 */
inline HANDLE Wh_FindFirstSymbol(HMODULE hModule,
                                 const WH_FIND_SYMBOL_OPTIONS* options,
                                 WH_FIND_SYMBOL* findData) {
    return WH_INTERNAL_OR(InternalWh_FindFirstSymbol4(InternalWhModPtr, hModule,
                                                      options, findData),
                          NULL);
}

/**
 * @brief Returns information about the next symbol for the specified search
 *     handle, continuing an enumeration from a previous call to
 *     `Wh_FindFirstSymbol`.
 * @param symSearch A search handle returned by a previous call to
 *     `Wh_FindFirstSymbol`.
 * @param findData A pointer to a structure to receive the symbol information.
 * @return A boolean value indicating whether symbol information was retrieved.
 *     If no more symbols are found or in case of an error, the return value is
 *     `FALSE`.
 */
inline BOOL Wh_FindNextSymbol(HANDLE symSearch, WH_FIND_SYMBOL* findData) {
    return WH_INTERNAL_OR(
        InternalWh_FindNextSymbol2(InternalWhModPtr, symSearch, findData),
        FALSE);
}

/**
 * @brief Closes a file search handle opened by `Wh_FindFirstSymbol`.
 * @param symSearch The search handle. If symSearch is `NULL`, the function does
 *     nothing.
 * @return None.
 */
inline void Wh_FindCloseSymbol(HANDLE symSearch) {
    WH_INTERNAL(InternalWh_FindCloseSymbol(InternalWhModPtr, symSearch));
}

/**
 * @brief Disassembles an instruction and formats it to human-readable text.
 * @since Windhawk v1.2
 * @param address The address of the instruction to disassemble.
 * @param result A pointer to a structure to receive the disassembly
 *     information.
 * @return A boolean value indicating whether the function succeeded.
 */
inline BOOL Wh_Disasm(void* address, WH_DISASM_RESULT* result) {
    return WH_INTERNAL_OR(InternalWh_Disasm(InternalWhModPtr, address, result),
                          FALSE);
}

/**
 * @brief Retrieves the content of a URL. When no longer needed, call
 *     `Wh_FreeUrlContent` to free the content.
 * @since Windhawk v1.5
 * @param url The URL to retrieve.
 * @param options The options for the URL content retrieval. Pass `NULL` to use
 *     the default options.
 * @return The retrieved content. In case of an error, `NULL` is returned.
 */
inline const WH_URL_CONTENT* Wh_GetUrlContent(
    PCWSTR url,
    const WH_GET_URL_CONTENT_OPTIONS* options) {
    return WH_INTERNAL_OR(
        InternalWh_GetUrlContent(InternalWhModPtr, url, options), NULL);
}

/**
 * @brief Frees the content of a URL returned by `Wh_GetUrlContent`.
 * @since Windhawk v1.5
 * @param content The content to free. If `NULL`, the function does nothing.
 * @return None.
 */
inline void Wh_FreeUrlContent(const WH_URL_CONTENT* content) {
    WH_INTERNAL(InternalWh_FreeUrlContent(InternalWhModPtr, content));
}

#undef WH_INTERNAL
#undef WH_INTERNAL_OR

#endif  // WH_MOD
