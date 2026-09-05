#pragma once

#include "mods_api.h"
#include "tool_mod_process.h"

// Writes one mod's transient status or task into this session's volatile
// registry keys (see shared/session_metadata.h). Each instance owns a single
// registry value and deletes it on destruction. Best-effort: the methods throw
// on failure and callers log and continue. A writer that fails to reach the
// store gives up for good, so a process that can't reach it pays for one failed
// lookup rather than one per update.
//
// Set can be called from any thread, and never blocks: a call that arrives
// while another thread is still opening the store is dropped rather than
// queued, which suits a value that only carries the latest state. Concurrent
// updates are resolved by the registry, so the value ends up holding one of
// them whole. Destruction is not part of that, and must not race with a call.
class ModMetadataWriter {
   public:
    ModMetadataWriter(PCWSTR category, PCWSTR modName);
    ~ModMetadataWriter();

    ModMetadataWriter(const ModMetadataWriter&) = delete;
    ModMetadataWriter& operator=(const ModMetadataWriter&) = delete;

    // Sets or updates the value; nullptr clears it.
    void Set(PCWSTR value);

   private:
    enum class SetupState {
        kNotStarted,
        kInProgress,
        kDone,
        kFailed,
    };

    // Runs the store lookup at most once and returns whether the fields it
    // fills are usable. Throws out of the attempt that fails, so its caller
    // reports the reason.
    bool EnsureSetup();

    PCWSTR m_category;
    std::wstring m_modName;
    // Publishing kDone is what hands the fields below to the other threads, so
    // reading them without observing it first is what this guards against.
    std::atomic<SetupState> m_setupState = SetupState::kNotStarted;
    wil::unique_hkey m_key;
    std::wstring m_valueName;
    std::wstring m_processImageName;
    ULONGLONG m_processCreationTime = 0;
    // Stamped by whichever thread writes the value first and cleared along with
    // the value, so an entry keeps one creation time for as long as it exists.
    std::atomic<ULONGLONG> m_entryCreationTime = 0;
};

class LoadedMod {
   public:
    LoadedMod(PCWSTR modName,
              PCWSTR modVersion,
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
    PCWSTR GetModVersion();
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

    HANDLE FindFirstSymbol(HMODULE module, PCWSTR symbolServer, BYTE* findData);
    HANDLE FindFirstSymbol2(HMODULE module,
                            PCWSTR symbolServer,
                            WH_FIND_SYMBOL* findData);
    HANDLE FindFirstSymbol3(HMODULE module,
                            const BYTE* options,
                            WH_FIND_SYMBOL* findData);
    HANDLE FindFirstSymbol4(HMODULE module,
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
    // Whether long running work in flight, such as a symbol load or a
    // download, should stop, because the mod is on its way out of this process
    // or the session is ending. Latches, so the answer stays true for the rest
    // of the mod's life even if the condition flips back.
    bool ShouldAbortLongOperation();

    // Asks ShouldAbortLongOperation at most once a second, for loops which can
    // ask far more often than its settings lookup allows.
    class AbortPoller {
       public:
        explicit AbortPoller(LoadedMod* mod) : m_mod(mod) {}

        bool ShouldAbort();

       private:
        LoadedMod* m_mod;
        DWORD m_lastCheckTick = GetTickCount();
    };

    std::optional<std::wstring> HookSymbolsGetOnlineCache(
        PCWSTR onlineCacheBaseUrl,
        std::wstring_view cacheStrKey);

    void SetTask(PCWSTR task);
    void LogFunctionError(const std::exception& e);

    std::wstring m_modName;
    std::wstring m_modVersion;
    ModMetadataWriter m_modTaskWriter;
    bool m_loadedOnStartup;
    std::atomic<bool> m_loggingEnabled = false;
    std::atomic<bool> m_debugLoggingEnabled = false;
    std::atomic<bool> m_initialized = false;
    std::atomic<bool> m_uninitializing = false;
    std::atomic<bool> m_longOperationAborted = false;

    // Held shared while a hook is created and queued, and exclusively while
    // queued operations are applied and disabled hooks are reclaimed, or while
    // teardown queues all hooks for disabling. Keeps a hook from slipping in
    // after the disable covers it, or from being reclaimed before it's applied.
    wil::srwlock m_hookOperationsLock;

    // Temporary compatibility flag.
    const bool m_compatDemangling = false;

    // Temporary compatibility shim library.
    wil::unique_hmodule m_modShimLibrary;

    wil::unique_hmodule m_modModule;
};

class Mod {
   public:
    // What becomes of a mod in the process asking.
    enum class LoadDecision {
        kSkip,
        kLoad,
        // A tool mod, which gets a host process of its own instead of a place
        // in this one. Only returned in the session manager, the process which
        // launches hosts.
        kRunInToolModProcess,
    };

    // The library the mod loads and the time its settings were last written:
    // a marker which has moved is a mod which has changed.
    struct ChangeMarker {
        std::wstring libraryFileName;
        int settingsChangeTime = 0;

        bool operator==(const ChangeMarker&) const = default;
    };

    // What the session manager keeps for a tool mod it launched a host for. A
    // record which differs from the one kept is a mod whose host is due again.
    struct ToolModLaunchInfo {
        ChangeMarker changeMarker;
        ToolModProcess::ToolModInfo info;

        bool operator==(const ToolModLaunchInfo&) const = default;
    };

    Mod(PCWSTR modName);

    bool Load(bool loadedOnStartup);
    void AfterInit();
    void BeforeUninit();
    void Uninitialize();
    bool ApplyChangedSettings(bool* reload);
    void Unload();

    HMODULE GetLoadedModModuleHandle();

    // What becomes of the mod in this process, as its settings stand. Also
    // what a mod already loaded here asks to learn whether it should stop what
    // it's doing: anything but kLoad means the next sweep won't keep it.
    //
    // toolModLaunchInfo, when given, is filled in on the kRunInToolModProcess
    // decision, from the settings the decision was made on.
    static LoadDecision GetLoadDecisionForRunningProcess(
        PCWSTR modName,
        ToolModLaunchInfo* toolModLaunchInfo = nullptr);

   private:
    void SetStatus(PCWSTR status);

    std::wstring m_modName;
    ModMetadataWriter m_modStatusWriter;
    ChangeMarker m_changeMarker;
    std::unique_ptr<LoadedMod> m_loadedMod;
};
