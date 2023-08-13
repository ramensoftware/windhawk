#pragma once

#include "mods_api.h"

class LoadedMod {
   public:
    LoadedMod(PCWSTR modName,
              PCWSTR modInstanceId,
              PCWSTR libraryPath,
              bool loggingEnabled,
              bool debugLoggingEnabled);
    ~LoadedMod();

    // Disallow copy and move - we assume that the pointer of the class won't
    // change.
    LoadedMod(const LoadedMod&) = delete;
    LoadedMod(LoadedMod&&) noexcept = delete;
    LoadedMod& operator=(const LoadedMod&) = delete;
    LoadedMod& operator=(LoadedMod&&) noexcept = delete;

    bool Initialize();
    void AfterInit();
    void BeforeUninit();
    void EnableLogging(bool enable);
    void EnableDebugLogging(bool enable);
    bool SettingsChanged(bool* reload);

    BOOL IsLogEnabled();
    void Log(PCWSTR format, va_list args);

    int GetIntValue(PCWSTR valueName, int defaultValue);
    BOOL SetIntValue(PCWSTR valueName, int value);
    size_t GetStringValue(PCWSTR valueName,
                          PWSTR stringBuffer,
                          size_t bufferChars);
    BOOL SetStringValue(PCWSTR valueName, PCWSTR value);
    size_t GetBinaryValue(PCWSTR valueName, BYTE* buffer, size_t bufferSize);
    BOOL SetBinaryValue(PCWSTR valueName,
                        const BYTE* buffer,
                        size_t bufferSize);

    int GetIntSetting(PCWSTR valueName, va_list args);
    PCWSTR GetStringSetting(PCWSTR valueName, va_list args);
    void FreeStringSetting(PCWSTR string);

    BOOL SetFunctionHook(void* targetFunction,
                         void* hookFunction,
                         void** originalFunction);
    BOOL RemoveFunctionHook(void* targetFunction);
    BOOL ApplyHookOperations();

    // For backwards compatibility, replaced with FindFirstSymbol2:
    HANDLE FindFirstSymbol(HMODULE hModule,
                           PCWSTR symbolServer,
                           void* findData);
    HANDLE FindFirstSymbol2(HMODULE hModule,
                            PCWSTR symbolServer,
                            WH_FIND_SYMBOL* findData);
    // For backwards compatibility, replaced with FindNextSymbol2:
    BOOL FindNextSymbol(HANDLE symSearch, void* findData);
    BOOL FindNextSymbol2(HANDLE symSearch, WH_FIND_SYMBOL* findData);
    void FindCloseSymbol(HANDLE symSearch);

    BOOL Disasm(void* address, WH_DISASM_RESULT* result);

   private:
    void SetTask(PCWSTR task);
    void LogFunctionError(const std::exception& e);

    std::wstring m_modName;
    std::wstring m_modInstanceId;
    wil::unique_hfile m_modTaskFile;
    std::atomic<bool> m_loggingEnabled = false;
    std::atomic<bool> m_debugLoggingEnabled = false;
    wil::unique_hmodule m_modModule;
    std::atomic<bool> m_initialized = false;
    std::atomic<bool> m_uninitializing = false;

    // Temporary compatibility flag.
    const bool m_compatDemangling = false;
};

class Mod {
   public:
    Mod(PCWSTR modName);

    void Load();
    void AfterInit();
    void BeforeUninit();
    bool ApplyChangedSettings(bool* reload);
    void Unload();

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
