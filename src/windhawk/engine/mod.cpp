#include "stdafx.h"

#include "critical_processes.h"
#include "customization_session.h"
#include "functions.h"
#include "logger.h"
#include "mod.h"
#include "storage_manager.h"
#include "symbol_enum.h"
#include "version.h"

extern HINSTANCE g_hDllInst;

namespace {

const PCWSTR emptySettingStringValue = L"";

class ModDebugLoggingScope {
   public:
    ModDebugLoggingScope(PCSTR funcName)
        : m_funcName(funcName),
          m_scopedThreadVerbosity(Logger::Verbosity::kVerbose) {
        if (m_funcName) {
            VERBOSE(L">>> Entering %S", m_funcName);
        }
    }

    ~ModDebugLoggingScope() {
        if (m_funcName) {
            VERBOSE(L"<<< Exiting %S", m_funcName);
        }
    }

    ModDebugLoggingScope(const ModDebugLoggingScope&) = delete;
    ModDebugLoggingScope(ModDebugLoggingScope&&) = delete;
    ModDebugLoggingScope& operator=(const ModDebugLoggingScope&) = delete;
    ModDebugLoggingScope& operator=(ModDebugLoggingScope&&) = delete;

   private:
    PCSTR m_funcName;
    Logger::ScopedThreadVerbosity m_scopedThreadVerbosity;
};

class ModDebugLoggingScopeHelper {
   public:
    ModDebugLoggingScopeHelper(bool debugLoggingEnabled, PCSTR funcName) {
        if (debugLoggingEnabled) {
            m_modDebugLoggingScope.emplace(funcName);
        }
    }

   private:
    std::optional<ModDebugLoggingScope> m_modDebugLoggingScope;
};

// Causes all events to be logged in that scope. Prints "entering" and "exiting"
// messages on start/end.
#define MOD_DEBUG_LOGGING_SCOPE() \
    ModDebugLoggingScopeHelper(m_debugLoggingEnabled, __FUNCTION__)

// Same as above, but without the "entering" and "exiting" messages.
#define MOD_DEBUG_LOGGING_SCOPE_QUIET() \
    ModDebugLoggingScopeHelper(m_debugLoggingEnabled, nullptr)

std::wstring GenerateModInstanceId(PCWSTR modName) {
    DWORD sessionManagerProcessId =
        CustomizationSession::GetSessionManagerProcessId();

    FILETIME sessionManagerProcessCreationTime =
        CustomizationSession::GetSessionManagerProcessCreationTime();

    return std::to_wstring(sessionManagerProcessId) + L"_" +
           std::to_wstring(
               wil::filetime::to_int64(sessionManagerProcessCreationTime)) +
           L"_" + std::to_wstring(GetCurrentProcessId()) + L"_" + modName;
}

void SetModMetadataValue(wil::unique_hfile& metadataFile,
                         PCWSTR value,
                         PCWSTR metadataCategory,
                         PCWSTR modInstanceId) {
    if (!value) {
        metadataFile.reset();
        return;
    }

    auto& storageManager = StorageManager::GetInstance();

    if (!metadataFile) {
        metadataFile = storageManager.CreateModMetadataFile(metadataCategory,
                                                            modInstanceId);
    }

    std::filesystem::path fullProcessImageName =
        wil::QueryFullProcessImageName<std::wstring>(GetCurrentProcess());
    std::wstring fullValue =
        fullProcessImageName.filename().wstring() + L'|' + std::wstring(value);

    storageManager.SetModMetadataValue(metadataFile, fullValue.c_str());
}

bool DoesArchitectureMatchPatternPart(std::wstring_view patternPart) {
#ifdef _WIN64
    if (patternPart == L"x86-64") {
        return true;
    }
#else   // !_WIN64
    if (patternPart == L"x86") {
        return true;
    }
#endif  // _WIN64

    return false;
}

bool DoesArchitectureMatchPattern(std::wstring_view pattern) {
    for (const auto& patternPart : Functions::SplitString(pattern, L'|')) {
        if (DoesArchitectureMatchPatternPart(patternPart)) {
            return true;
        }
    }

    return false;
}

std::wstring GetModVersion(PCWSTR modName) {
    auto settings =
        StorageManager::GetInstance().GetModConfig(modName, nullptr);

    return settings->GetString(L"Version").value_or(L"-");
}

std::string GetModuleVersion(HMODULE hModule) {
    HRSRC hResource =
        FindResource(hModule, MAKEINTRESOURCE(VS_VERSION_INFO), VS_FILE_INFO);
    if (!hResource) {
        return {};
    }

    HGLOBAL hGlobal = LoadResource(hModule, hResource);
    if (!hGlobal) {
        return {};
    }

    void* pData = LockResource(hGlobal);
    if (!pData) {
        return {};
    }

    VS_FIXEDFILEINFO* pFixedFileInfo = nullptr;
    UINT uPtrLen = 0;
    if (!VerQueryValue(pData, L"\\", reinterpret_cast<void**>(&pFixedFileInfo),
                       &uPtrLen) ||
        uPtrLen == 0) {
        return {};
    }

    WORD nMajor = HIWORD(pFixedFileInfo->dwFileVersionMS);
    WORD nMinor = LOWORD(pFixedFileInfo->dwFileVersionMS);
    WORD nBuild = HIWORD(pFixedFileInfo->dwFileVersionLS);
    WORD nQFE = LOWORD(pFixedFileInfo->dwFileVersionLS);

    return std::to_string(nMajor) + "." + std::to_string(nMinor) + "." +
           std::to_string(nBuild) + "." + std::to_string(nQFE);
}

// Temporary compatibility code.
bool ShouldUseCompatDemangling(wil::zwstring_view modName) {
    struct {
        std::wstring_view modNamePrefix;
        std::vector<std::wstring_view> versions;
    } compatMods[] = {
        {L"start-menu-all-apps", {L"1.0.1"}},
        {L"taskbar-button-click", {L"1.0", L"1.0.1"}},
        {L"taskbar-clock-customization",
         {L"1.0", L"1.0.1", L"1.0.2", L"1.0.3", L"1.0.4", L"1.0.5"}},
        {L"taskbar-grouping", {L"1.0"}},
        {L"taskbar-icon-size", {L"1.0", L"1.0.1", L"1.0.2"}},
        {L"taskbar-labels", {L"1.0", L"1.0.1", L"1.0.2", L"1.0.3"}},
        {L"taskbar-thumbnail-reorder", {L"1.0", L"1.0.1", L"1.0.2"}},
    };

    for (const auto& compatMod : compatMods) {
        if (modName == compatMod.modNamePrefix ||
            (modName.starts_with(L"local@") &&
             modName.substr(6).starts_with(compatMod.modNamePrefix))) {
            auto modVersion = GetModVersion(modName.c_str());
            if (modVersion == L"-") {
                return true;
            }

            for (const auto& compatModVersion : compatMod.versions) {
                if (modVersion == compatModVersion) {
                    return true;
                }
            }

            return false;
        }
    }

    return false;
}

}  // namespace

LoadedMod::LoadedMod(PCWSTR modName,
                     PCWSTR modInstanceId,
                     PCWSTR libraryPath,
                     bool loggingEnabled,
                     bool debugLoggingEnabled)
    : m_modName(modName),
      m_modInstanceId(modInstanceId),
      m_loggingEnabled(loggingEnabled),
      m_debugLoggingEnabled(debugLoggingEnabled),
      m_compatDemangling(ShouldUseCompatDemangling(m_modName)) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    ULONG majorVersion = 0;
    ULONG minorVersion = 0;
    ULONG buildNumber = 0;
    Functions::GetNtVersionNumbers(&majorVersion, &minorVersion, &buildNumber);

    VERBOSE(L"Windows %u.%u.%u", majorVersion, minorVersion, buildNumber);
    VERBOSE(L"Windhawk v" VER_FILE_VERSION_WSTR);
    VERBOSE(L"Mod id: %s", m_modName.c_str());
    VERBOSE(L"Mod version: %s", GetModVersion(m_modName.c_str()).c_str());

    m_modModule.reset(
        LoadLibraryEx(libraryPath, nullptr, LOAD_WITH_ALTERED_SEARCH_PATH));
    THROW_LAST_ERROR_IF_NULL(m_modModule);

    VERBOSE(L"Mod base address: %p", m_modModule.get());

    LoadedMod** pModPtr = reinterpret_cast<LoadedMod**>(
        GetProcAddress(m_modModule.get(), "InternalWhModPtr"));
    if (pModPtr) {
        *pModPtr = this;
    } else {
        LOG(L"Mod %s: InternalWhModPtr not found", m_modName.c_str());
    }
}

LoadedMod::~LoadedMod() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (m_initialized) {
        using WH_MOD_UNINIT_T = void(__cdecl*)();
        auto pWH_ModUninit = reinterpret_cast<WH_MOD_UNINIT_T>(
            GetProcAddress(m_modModule.get(), "_Z12Wh_ModUninitv"));
        if (pWH_ModUninit) {
            pWH_ModUninit();
        }
    }

    MH_STATUS status =
        MH_RemoveHookEx(reinterpret_cast<ULONG_PTR>(this), MH_ALL_HOOKS);
    if (status == MH_ERROR_NOT_INITIALIZED) {
        // MH_ERROR_NOT_INITIALIZED can be returned when unloading, that's OK.
    } else if (status != MH_OK) {
        LOG(L"Mod %s error: MH_RemoveHookEx returned %d", m_modName.c_str(),
            status);
    }
}

bool LoadedMod::Initialize() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (m_initialized) {
        throw std::logic_error("Already initialized");
    }

    SetTask(L"Initializing...");

    using WH_MOD_INIT_T = BOOL(__cdecl*)();
    auto pWH_ModInit = reinterpret_cast<WH_MOD_INIT_T>(
        GetProcAddress(m_modModule.get(), "_Z10Wh_ModInitv"));
    if (pWH_ModInit) {
        m_initialized = pWH_ModInit();
    } else {
        m_initialized = true;
    }

    SetTask(nullptr);

    return m_initialized;
}

void LoadedMod::AfterInit() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    using WH_MOD_AFTER_INIT_T = void(__cdecl*)();
    auto pWH_ModAfterInit = reinterpret_cast<WH_MOD_AFTER_INIT_T>(
        GetProcAddress(m_modModule.get(), "_Z15Wh_ModAfterInitv"));
    if (pWH_ModAfterInit) {
        pWH_ModAfterInit();
    }
}

void LoadedMod::BeforeUninit() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    using WH_MOD_BEFORE_UNINIT_T = void(__cdecl*)();
    auto pWH_ModBeforeUninit = reinterpret_cast<WH_MOD_BEFORE_UNINIT_T>(
        GetProcAddress(m_modModule.get(), "_Z18Wh_ModBeforeUninitv"));
    if (pWH_ModBeforeUninit) {
        pWH_ModBeforeUninit();
    }

    m_uninitializing = true;

    MH_STATUS status =
        MH_QueueDisableHookEx(reinterpret_cast<ULONG_PTR>(this), MH_ALL_HOOKS);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_QueueDisableHookEx returned %d",
            m_modName.c_str(), status);
    }
}

void LoadedMod::EnableLogging(bool enable) {
    m_loggingEnabled = enable;
}

void LoadedMod::EnableDebugLogging(bool enable) {
    m_debugLoggingEnabled = enable;
}

bool LoadedMod::SettingsChanged(bool* reload) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    *reload = false;

    using WH_MOD_SETTINGS_CHANGED_EX_T = BOOL(__cdecl*)(BOOL*);
    auto pWH_ModSettingsChangedEx =
        reinterpret_cast<WH_MOD_SETTINGS_CHANGED_EX_T>(
            GetProcAddress(m_modModule.get(), "_Z21Wh_ModSettingsChangedPi"));
    if (pWH_ModSettingsChangedEx) {
        BOOL bReload = FALSE;
        bool result = pWH_ModSettingsChangedEx(&bReload);
        *reload = bReload;
        return result;
    }

    using WH_MOD_SETTINGS_CHANGED_T = void(__cdecl*)();
    auto pWH_ModSettingsChanged = reinterpret_cast<WH_MOD_SETTINGS_CHANGED_T>(
        GetProcAddress(m_modModule.get(), "_Z21Wh_ModSettingsChangedv"));
    if (pWH_ModSettingsChanged) {
        pWH_ModSettingsChanged();
        return true;
    }

    return true;
}

BOOL LoadedMod::IsLogEnabled() {
    return m_loggingEnabled || m_debugLoggingEnabled;
}

void LoadedMod::Log(PCWSTR format, va_list args) {
    try {
        va_list argsCopy;
        va_copy(argsCopy, args);  // https://stackoverflow.com/q/55274350
        std::wstring logFormatted(_vscwprintf(format, argsCopy), L'\0');
        va_end(argsCopy);
        vswprintf_s(logFormatted.data(), logFormatted.length() + 1, format,
                    args);

        Logger::GetInstance().LogLine(L"[WH] [%s] %s\n", m_modName.c_str(),
                                      logFormatted.c_str());
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }
}

int LoadedMod::GetIntValue(PCWSTR valueName, int defaultValue) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", false);
        int value = settings->GetInt(valueName).value_or(defaultValue);
        VERBOSE(L"value: %d", value);
        return value;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return defaultValue;
}

BOOL LoadedMod::SetIntValue(PCWSTR valueName, int value) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);
    VERBOSE(L"value: %d", value);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", true);
        settings->SetInt(valueName, value);
        return TRUE;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return FALSE;
}

size_t LoadedMod::GetStringValue(PCWSTR valueName,
                                 PWSTR stringBuffer,
                                 size_t bufferChars) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    if (bufferChars == 0) {
        return 0;
    }

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", false);
        auto value = settings->GetString(valueName).value_or(L"");
        if (value.length() <= bufferChars - 1) {
            wcscpy_s(stringBuffer, bufferChars, value.c_str());
            VERBOSE(L"value: %s", value.c_str());
            return value.length();
        } else {
            LOG(L"Buffer size too small: %zu < %zu",
                bufferChars - 1 < value.length());
        }
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    stringBuffer[0] = L'\0';
    return 0;
}

BOOL LoadedMod::SetStringValue(PCWSTR valueName, PCWSTR value) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);
    VERBOSE(L"value: %s", value);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", true);
        settings->SetString(valueName, value);
        return TRUE;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return FALSE;
}

size_t LoadedMod::GetBinaryValue(PCWSTR valueName,
                                 void* buffer,
                                 size_t bufferSize) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", false);
        auto value =
            settings->GetBinary(valueName).value_or(std::vector<BYTE>{});
        if (value.size() <= bufferSize) {
            memcpy(buffer, value.data(), value.size());
            return value.size();
        }
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return 0;
}

BOOL LoadedMod::SetBinaryValue(PCWSTR valueName,
                               const void* buffer,
                               size_t bufferSize) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", true);
        settings->SetBinary(valueName, reinterpret_cast<const BYTE*>(buffer),
                            bufferSize);
        return TRUE;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return FALSE;
}

int LoadedMod::GetIntSetting(PCWSTR valueName, va_list args) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        va_list argsCopy;
        va_copy(argsCopy, args);  // https://stackoverflow.com/q/55274350
        std::wstring valueNameFormatted(_vscwprintf(valueName, argsCopy),
                                        L'\0');
        va_end(argsCopy);
        vswprintf_s(valueNameFormatted.data(), valueNameFormatted.length() + 1,
                    valueName, args);

        VERBOSE(L"valueNameFormatted: %s", valueNameFormatted.c_str());

        auto settings = StorageManager::GetInstance().GetModConfig(
            m_modName.c_str(), L"Settings");
        int value = settings->GetInt(valueNameFormatted.c_str()).value_or(0);
        VERBOSE(L"value: %d", value);
        return value;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return 0;
}

PCWSTR LoadedMod::GetStringSetting(PCWSTR valueName, va_list args) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        va_list argsCopy;
        va_copy(argsCopy, args);  // https://stackoverflow.com/q/55274350
        std::wstring valueNameFormatted(_vscwprintf(valueName, argsCopy),
                                        L'\0');
        va_end(argsCopy);
        vswprintf_s(valueNameFormatted.data(), valueNameFormatted.length() + 1,
                    valueName, args);

        VERBOSE(L"valueNameFormatted: %s", valueNameFormatted.c_str());

        auto settings = StorageManager::GetInstance().GetModConfig(
            m_modName.c_str(), L"Settings");
        auto value =
            settings->GetString(valueNameFormatted.c_str()).value_or(L"");

        auto valueAllocated = std::make_unique<WCHAR[]>(value.length() + 1);
        wcscpy_s(valueAllocated.get(), value.length() + 1, value.c_str());
        VERBOSE(L"value: %s", valueAllocated.get());
        return valueAllocated.release();
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return emptySettingStringValue;
}

void LoadedMod::FreeStringSetting(PCWSTR string) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (string != emptySettingStringValue) {
        delete[] string;
    }
}

BOOL LoadedMod::SetFunctionHook(void* targetFunction,
                                void* hookFunction,
                                void** originalFunction) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"Target: %p", targetFunction);
    VERBOSE(L"Hook: %p", hookFunction);

    if (m_uninitializing) {
        VERBOSE(L"Uninitializing, not allowed to set hooks");
        return FALSE;
    }

    MH_STATUS status =
        MH_CreateHookEx(reinterpret_cast<ULONG_PTR>(this), targetFunction,
                        hookFunction, originalFunction);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_CreateHookEx returned %d", m_modName.c_str(),
            status);
        return FALSE;
    }

    status =
        MH_QueueEnableHookEx(reinterpret_cast<ULONG_PTR>(this), targetFunction);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_QueueEnableHookEx returned %d",
            m_modName.c_str(), status);
        return FALSE;
    }

    return TRUE;
}

BOOL LoadedMod::RemoveFunctionHook(void* targetFunction) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"Target: %p", targetFunction);

    if (!m_initialized) {
        VERBOSE(L"Not initialized, not allowed to remove hooks");
        return FALSE;
    }

    if (m_uninitializing) {
        VERBOSE(L"Uninitializing, not allowed to remove hooks");
        return FALSE;
    }

    MH_STATUS status = MH_QueueDisableHookEx(reinterpret_cast<ULONG_PTR>(this),
                                             targetFunction);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_QueueDisableHookEx returned %d",
            m_modName.c_str(), status);
        return FALSE;
    }

    return TRUE;
}

BOOL LoadedMod::ApplyHookOperations() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (!m_initialized) {
        VERBOSE(L"Not initialized, not allowed to apply hooks");
        return FALSE;
    }

    if (m_uninitializing) {
        VERBOSE(L"Uninitializing, not allowed to apply hooks");
        return FALSE;
    }

    MH_STATUS status = MH_ApplyQueuedEx(reinterpret_cast<ULONG_PTR>(this));
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_ApplyQueuedEx returned %d", m_modName.c_str(),
            status);
    }

    MH_STATUS removeDisabledHooksStatus =
        MH_RemoveDisabledHooksEx(reinterpret_cast<ULONG_PTR>(this));
    if (removeDisabledHooksStatus != MH_OK) {
        LOG(L"Mod %s error: MH_RemoveDisabledHooksEx returned %d",
            m_modName.c_str(), removeDisabledHooksStatus);
    }

    return status == MH_OK;
}

HANDLE LoadedMod::FindFirstSymbol(HMODULE hModule,
                                  PCWSTR symbolServer,
                                  void* findData) {
    WH_FIND_SYMBOL newFindData;
    HANDLE findHandle = FindFirstSymbol2(hModule, symbolServer, &newFindData);
    if (!findHandle) {
        return nullptr;
    }

    struct {
        PCWSTR symbol;
        void* address;
    }* findDataOldStruct = (decltype(findDataOldStruct))findData;

    findDataOldStruct->symbol = newFindData.symbol;
    findDataOldStruct->address = newFindData.address;

    return findHandle;
}

HANDLE LoadedMod::FindFirstSymbol2(HMODULE hModule,
                                   PCWSTR symbolServer,
                                   WH_FIND_SYMBOL* findData) {
    WH_FIND_SYMBOL_OPTIONS options = {symbolServer, FALSE};
    return FindFirstSymbol3(hModule, &options, findData);
}

HANDLE LoadedMod::FindFirstSymbol3(HMODULE hModule,
                                   const WH_FIND_SYMBOL_OPTIONS* options,
                                   WH_FIND_SYMBOL* findData) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    try {
        HMODULE moduleBase = hModule;
        if (!moduleBase) {
            moduleBase = GetModuleHandle(nullptr);
        }

        std::filesystem::path modulePath =
            wil::GetModuleFileName<std::wstring>(moduleBase);

        VERBOSE(L"Module: %p%s", moduleBase, !hModule ? L" (main)" : L"");
        VERBOSE(L"Path: %s", modulePath.c_str());
        VERBOSE(L"Version: %S", GetModuleVersion(moduleBase).c_str());

        std::wstring moduleName = modulePath.filename();

        SetTask((L"Loading symbols... (" + moduleName + L")").c_str());

        auto activityStatusCleanup = wil::scope_exit(
            [this] { SetTask(m_initialized ? nullptr : L"Initializing..."); });

        SymbolEnum::Callbacks callbacks;

        bool canceled = false;
        DWORD lastQueryCancelTick = GetTickCount();

        auto settings = StorageManager::GetInstance().GetModConfig(
            m_modName.c_str(), nullptr);

        callbacks.queryCancel = [this, &canceled, &lastQueryCancelTick,
                                 &settings]() {
            if (canceled) {
                return true;
            }

            DWORD tick = GetTickCount();
            if (tick - lastQueryCancelTick < 200) {
                return false;
            }

            lastQueryCancelTick = tick;

            try {
                if (settings->GetInt(L"Disabled").value_or(0)) {
                    canceled = true;
                    return true;
                }

                if (CustomizationSession::IsEndingSoon()) {
                    canceled = true;
                    return true;
                }
            } catch (const std::exception& e) {
                LOG(L"%S", e.what());
            }

            return false;
        };

        callbacks.notifyProgress = [this, &moduleName](int progress) {
            try {
                std::wstring status = L"Loading symbols... " +
                                      std::to_wstring(progress) + L"% (" +
                                      moduleName + L")";
                SetTask(status.c_str());
            } catch (const std::exception& e) {
                LOG(L"%S", e.what());
            }
        };

        SymbolEnum::UndecorateMode undecorateMode =
            SymbolEnum::UndecorateMode::Default;
        if (options && options->noUndecoratedSymbols) {
            undecorateMode = SymbolEnum::UndecorateMode::None;
        } else if (m_compatDemangling) {
            undecorateMode = SymbolEnum::UndecorateMode::OldVersionCompatible;
        }

        auto symbolEnum = std::make_unique<SymbolEnum>(
            modulePath.c_str(), hModule,
            options ? options->symbolServer : nullptr, undecorateMode,
            std::move(callbacks));

        if (!FindNextSymbol2(symbolEnum.get(), findData)) {
            VERBOSE(L"No symbols found");
            return nullptr;
        }

        return symbolEnum.release();
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return nullptr;
}

BOOL LoadedMod::FindNextSymbol(HANDLE symSearch, void* findData) {
    WH_FIND_SYMBOL newFindData;
    if (!FindNextSymbol2(symSearch, &newFindData)) {
        return FALSE;
    }

    struct {
        PCWSTR symbol;
        void* address;
    }* findDataOldStruct = (decltype(findDataOldStruct))findData;

    findDataOldStruct->symbol = newFindData.symbol;
    findDataOldStruct->address = newFindData.address;

    return TRUE;
}

BOOL LoadedMod::FindNextSymbol2(HANDLE symSearch, WH_FIND_SYMBOL* findData) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE_QUIET();

    try {
        auto symbolEnum = static_cast<SymbolEnum*>(symSearch);

        auto symbol = symbolEnum->GetNextSymbol();
        if (!symbol) {
            return FALSE;
        }

        findData->address = symbol->address;
        findData->symbol = symbol->name ? symbol->name : L"";
        findData->symbolDecorated =
            symbol->nameDecorated ? symbol->nameDecorated : L"";

        return TRUE;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return FALSE;
}

void LoadedMod::FindCloseSymbol(HANDLE symSearch) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    auto symbolEnum = static_cast<SymbolEnum*>(symSearch);
    delete symbolEnum;
}

BOOL LoadedMod::Disasm(void* address, WH_DISASM_RESULT* result) {
#ifdef _WIN64
    auto machineMode = ZYDIS_MACHINE_MODE_LONG_64;
#else
    auto machineMode = ZYDIS_MACHINE_MODE_LEGACY_32;
#endif

    ZydisDisassembledInstruction instruction;
    if (!ZYAN_SUCCESS(ZydisDisassembleIntel(
            /* machine_mode:    */ machineMode,
            /* runtime_address: */ (ZyanU64)address,
            /* buffer:          */ address,
            /* length:          */ ZYDIS_MAX_INSTRUCTION_LENGTH,
            /* instruction:     */ &instruction))) {
        return FALSE;
    }

    result->length = instruction.info.length;
    strcpy_s(result->text, instruction.text);

    return TRUE;
}

void LoadedMod::SetTask(PCWSTR task) {
    try {
        SetModMetadataValue(m_modTaskFile, task, L"mod-task",
                            m_modInstanceId.c_str());
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }
}

void LoadedMod::LogFunctionError(const std::exception& e) {
    LOG(L"Mod %s error: %S", m_modName.c_str(), e.what());
}

Mod::Mod(PCWSTR modName)
    : m_modName(modName), m_modInstanceId(GenerateModInstanceId(modName)) {
    SetStatus(L"Pending...");
}

void Mod::Load() {
    if (m_loadedMod) {
        throw std::logic_error("Already loaded");
    }

    auto setStatusOnExit = wil::scope_exit(
        [this] { SetStatus(m_loadedMod ? L"Loaded" : L"Unloaded"); });

    auto& storageManager = StorageManager::GetInstance();
    auto settings = storageManager.GetModConfig(m_modName.c_str(), nullptr);

    m_libraryFileName = settings->GetString(L"LibraryFileName").value_or(L"");
    if (m_libraryFileName.empty()) {
        throw std::runtime_error("Missing LibraryFileName value");
    }

    auto libraryPath = storageManager.GetModsPath() / m_libraryFileName;

    m_settingsChangeTime = settings->GetInt(L"SettingsChangeTime").value_or(0);

    bool loggingEnabled = settings->GetInt(L"LoggingEnabled").value_or(0);
    bool debugLoggingEnabled =
        settings->GetInt(L"DebugLoggingEnabled").value_or(0);

    m_loadedMod = std::make_unique<LoadedMod>(
        m_modName.c_str(), m_modInstanceId.c_str(), libraryPath.c_str(),
        loggingEnabled, debugLoggingEnabled);

    SetStatus(L"Loading...");

    if (!m_loadedMod->Initialize()) {
        m_loadedMod.reset();
    }
}

void Mod::AfterInit() {
    if (m_loadedMod) {
        m_loadedMod->AfterInit();
    }
}

void Mod::BeforeUninit() {
    if (m_loadedMod) {
        m_loadedMod->BeforeUninit();
    }
}

bool Mod::ApplyChangedSettings(bool* reload) {
    *reload = false;

    auto& storageManager = StorageManager::GetInstance();
    auto settings = storageManager.GetModConfig(m_modName.c_str(), nullptr);

    if (settings->GetString(L"LibraryFileName").value_or(L"") !=
        m_libraryFileName) {
        *reload = true;
        return true;
    }

    int oldSettingsChangeTime = m_settingsChangeTime;
    m_settingsChangeTime = settings->GetInt(L"SettingsChangeTime").value_or(0);

    if (m_settingsChangeTime != oldSettingsChangeTime) {
        if (!m_loadedMod) {
            *reload = true;
            return true;
        }

        if (!m_loadedMod->SettingsChanged(reload)) {
            return false;
        }
    }

    if (m_loadedMod) {
        m_loadedMod->EnableLogging(
            settings->GetInt(L"LoggingEnabled").value_or(0));

        m_loadedMod->EnableDebugLogging(
            settings->GetInt(L"DebugLoggingEnabled").value_or(0));
    }

    return true;
}

void Mod::Unload() {
    m_loadedMod.reset();
    SetStatus(L"Unloaded");
}

// static
bool Mod::ShouldLoadInRunningProcess(PCWSTR modName) {
    auto settings =
        StorageManager::GetInstance().GetModConfig(modName, nullptr);

    if (settings->GetInt(L"Disabled").value_or(0)) {
        return false;
    }

    auto architecturePattern =
        settings->GetString(L"Architecture").value_or(L"");
    if (!architecturePattern.empty() &&
        !DoesArchitectureMatchPattern(architecturePattern)) {
        return false;
    }

    struct LoadModsInCriticalSystemProcesses {
        enum {
            Never,
            OnlyExplicitMatch,
            Always,
        };
    };

    auto appSettings = StorageManager::GetInstance().GetAppConfig(L"Settings");
    int loadModsInCriticalSystemProcesses =
        appSettings->GetInt(L"LoadModsInCriticalSystemProcesses")
            .value_or(LoadModsInCriticalSystemProcesses::OnlyExplicitMatch);

    std::wstring processPath = wil::GetModuleFileName<std::wstring>();

    if (loadModsInCriticalSystemProcesses ==
            LoadModsInCriticalSystemProcesses::Never &&
        Functions::DoesPathMatchPattern(processPath, kCriticalProcesses)) {
        return false;
    }

    bool includeExcludeCustomOnly =
        settings->GetInt(L"IncludeExcludeCustomOnly").value_or(0);

    bool matchPatternExplicitOnly =
        loadModsInCriticalSystemProcesses !=
            LoadModsInCriticalSystemProcesses::Always &&
        Functions::DoesPathMatchPattern(processPath, kCriticalProcesses);

    bool include =
        (!includeExcludeCustomOnly &&
         Functions::DoesPathMatchPattern(
             processPath, settings->GetString(L"Include").value_or(L""),
             matchPatternExplicitOnly)) ||
        Functions::DoesPathMatchPattern(
            processPath, settings->GetString(L"IncludeCustom").value_or(L""),
            matchPatternExplicitOnly);

    if (!include) {
        return false;
    }

    bool exclude =
        (!includeExcludeCustomOnly &&
         Functions::DoesPathMatchPattern(
             processPath, settings->GetString(L"Exclude").value_or(L""))) ||
        Functions::DoesPathMatchPattern(
            processPath, settings->GetString(L"ExcludeCustom").value_or(L""));

    return !exclude;
}

void Mod::SetStatus(PCWSTR status) {
    try {
        SetModMetadataValue(m_modStatusFile, status, L"mod-status",
                            m_modInstanceId.c_str());
    } catch (const std::exception& e) {
        LOG(L"%S", e.what());
    }
}
