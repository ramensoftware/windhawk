#pragma once

#include "mods_api.h"

class LoadedMod {
   public:
    LoadedMod(PCWSTR modName,
              PCWSTR modInstanceId,
              PCWSTR libraryPath,
              bool loadedOnStartup,
              bool loggingEnabled,
              bool debugLoggingEnabled);
    ~LoadedMod();

    // Disallow copy and move - we assume that the pointer of the class won't
    // change.
    LoadedMod(const LoadedMod&) = delete;
    LoadedMod& operator=(const LoadedMod&) = delete;

    bool Initialize();
    void AfterInit();
    void BeforeUninit();
    void Uninitialize();
    void EnableLogging(bool enable);
    void EnableDebugLogging(bool enable);
    bool SettingsChanged(bool* reload);

    PCWSTR GetModName();
    HMODULE GetModModuleHandle();

    BOOL IsLogEnabled();
    void Log(PCWSTR format, va_list args);

    int GetIntValue(PCWSTR valueName, int defaultValue);
    BOOL SetIntValue(PCWSTR valueName, int value);
    size_t GetStringValue(PCWSTR valueName,
                          PWSTR stringBuffer,
                          size_t bufferChars);
    BOOL SetStringValue(PCWSTR valueName, PCWSTR value);
    size_t GetBinaryValue(PCWSTR valueName, void* buffer, size_t bufferSize);
    BOOL SetBinaryValue(PCWSTR valueName,
                        const void* buffer,
                        size_t bufferSize);
    BOOL DeleteValue(PCWSTR valueName);

    size_t GetModStoragePath(PWSTR pathBuffer, size_t bufferChars);

    int GetIntSetting(PCWSTR valueName, va_list args);
    PCWSTR GetStringSetting(PCWSTR valueName, va_list args);
    void FreeStringSetting(PCWSTR string);

    BOOL SetFunctionHook(void* targetFunction,
                         void* hookFunction,
                         void** originalFunction);
    BOOL RemoveFunctionHook(void* targetFunction);
    BOOL ApplyHookOperations();

    HANDLE FindFirstSymbol(HMODULE hModule,
                           PCWSTR symbolServer,
                           BYTE* findData);
    HANDLE FindFirstSymbol2(HMODULE hModule,
                            PCWSTR symbolServer,
                            WH_FIND_SYMBOL* findData);
    HANDLE FindFirstSymbol3(HMODULE hModule,
                            const BYTE* options,
                            WH_FIND_SYMBOL* findData);
    HANDLE FindFirstSymbol4(HMODULE hModule,
                            const WH_FIND_SYMBOL_OPTIONS* options,
                            WH_FIND_SYMBOL* findData);
    BOOL FindNextSymbol(HANDLE symSearch, BYTE* findData);
    BOOL FindNextSymbol2(HANDLE symSearch, WH_FIND_SYMBOL* findData);
    void FindCloseSymbol(HANDLE symSearch);

    BOOL HookSymbols(HMODULE module,
                     const WH_SYMBOL_HOOK* symbolHooks,
                     size_t symbolHooksCount,
                     const WH_HOOK_SYMBOLS_OPTIONS* options);

    BOOL Disasm(void* address, WH_DISASM_RESULT* result);

    const WH_URL_CONTENT* GetUrlContent(
        PCWSTR url,
        const WH_GET_URL_CONTENT_OPTIONS* options);
    void FreeUrlContent(const WH_URL_CONTENT* content);

   private:
    std::optional<std::wstring> HookSymbolsGetOnlineCache(
        PCWSTR onlineCacheBaseUrl,
        std::wstring_view cacheStrKey);

    void SetTask(PCWSTR task);
    void LogFunctionError(const std::exception& e);

    std::wstring m_modName;
    std::wstring m_modInstanceId;
    wil::unique_hfile m_modTaskFile;
    bool m_loadedOnStartup;
    std::atomic<bool> m_loggingEnabled = false;
    std::atomic<bool> m_debugLoggingEnabled = false;
    std::atomic<bool> m_initialized = false;
    std::atomic<bool> m_uninitializing = false;

    // Temporary compatibility flag.
    const bool m_compatDemangling = false;

    // Temporary compatibility shim library.
    wil::unique_hmodule m_modShimLibrary;

    wil::unique_hmodule m_modModule;
};

class Mod {
   public:
    Mod(PCWSTR modName);

    bool Load(bool loadedOnStartup);
    void AfterInit();
    void BeforeUninit();
    void Uninitialize();
    bool ApplyChangedSettings(bool* reload);
    void Unload();

    HMODULE GetLoadedModModuleHandle();

    static bool ShouldLoadInRunningProcess(PCWSTR modName);

   private:
    void SetStatus(PCWSTR status);

    std::wstring m_modName;
    std::wstring m_modInstanceId;
    wil::unique_hfile m_modStatusFile;
    std::wstring m_libraryFileName;
    int m_settingsChangeTime = 0;
    std::unique_ptr<LoadedMod> m_loadedMod;
};
