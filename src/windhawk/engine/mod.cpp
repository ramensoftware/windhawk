#include "stdafx.h"

#include "critical_processes.h"
#include "customization_session.h"
#include "functions.h"
#include "logger.h"
#include "mod.h"
#include "session_private_namespace.h"
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
        fullProcessImageName.filename().native() + L'|' + value;

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
    for (const auto& patternPart :
         Functions::SplitStringToViews(pattern, L'|')) {
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
        {L"start-menu-all-apps", {L"1.0", L"1.0.1"}},
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

wil::unique_hmodule SetModShimsLibraryIfNeeded(HMODULE mod,
                                               std::wstring_view modName) {
    std::unordered_set<std::wstring_view> targetModsForHookSymbolsShim = {
        // WindhawkUtils::HookSymbols.
        L"acrylic-effect-radius-changer",
        L"aero-flyout-fix",
        L"aero-tray",
        L"basic-themer",
        L"change-explorer-default-location",
        L"classic-desktop-icons",
        L"classic-explorer-treeview",
        L"classic-file-picker-dialog",
        L"classic-list-group-fix",
        L"classic-menus",
        L"classic-taskbar-buttons-lite-vs-without-spacing",
        L"classic-taskbar-buttons-lite",
        L"classic-taskdlg-fix",
        L"classic-uwp-fix",
        L"custom-shutdown-dialog",
        L"desktop-watermark-tweaks",
        L"disable-rounded-corners",
        L"dwm-ghost-mods",
        L"dwm-unextend-frames",
        L"eradicate-immersive-menus",
        L"explorer-32px-icons",
        L"explorer-frame-classic",
        L"fix-basic-caption-text",
        L"isretailready-false",
        L"legacy-search-bar",
        L"msg-box-font-fix",
        L"no-run-icon",
        L"no-taskbar-item-glow",
        L"notepad-remove-launch-new-app-banner",
        L"old-this-pc-commands",
        L"regedit-auto-trim-whitespace-on-navigation-bar",
        L"regedit-disable-beep",
        L"regedit-fix-copy-key-name",
        L"remove-command-bar",
        L"start-menu-all-apps",
        L"suppress-run-box-error-message",
        L"syslistview32-enabler",
        L"taskbar-autohide-better",
        L"taskbar-notification-icon-spacing",
        L"taskbar-vertical",
        L"uifile-override",
        L"unlock-taskmgr-server",
        L"uxtheme-hook",
        L"w11-dwm-fix",
        L"win32-tray-clock-experience",
        L"win7-style-uac-dim",
        L"windows-7-clock-spacing",
        // CmwfHookSymbols.
        L"aerexplorer",
        L"classic-maximized-windows-fix",
        // Local copy of HookSymbols.
        L"pinned-items-double-click",
        L"taskbar-button-click",
        L"taskbar-button-scroll",
        L"taskbar-clock-customization",
        L"taskbar-grouping",
        L"taskbar-icon-size",
        L"taskbar-labels",
        L"taskbar-thumbnail-reorder",
        L"taskbar-volume-control",
        L"taskbar-wheel-cycle",
        L"virtual-desktop-taskbar-order",
    };

    if (!targetModsForHookSymbolsShim.contains(modName)) {
        return nullptr;
    }

    constexpr WCHAR kShimLibraryFileName[] = L"windhawk-mod-shim.dll";
    wil::unique_hmodule shimModule;

    auto patchFunction = [](void* patchTarget, void* newCallTarget) {
#ifdef _WIN64
#pragma pack(push, 1)
        // 64-bit indirect absolute jump.
        typedef struct _JMP_ABS {
            UINT8 opcode0;  // FF25 00000000: JMP [+6]
            UINT8 opcode1;
            UINT32 dummy;
            UINT64 address;  // Absolute destination address
        } JMP_ABS, *PJMP_ABS;
#pragma pack(pop)

        JMP_ABS* pJmp = (JMP_ABS*)patchTarget;

        DWORD dwOldProtect;
        THROW_IF_WIN32_BOOL_FALSE(VirtualProtect(
            pJmp, sizeof(*pJmp), PAGE_EXECUTE_READWRITE, &dwOldProtect));

        pJmp->opcode0 = 0xFF;
        pJmp->opcode1 = 0x25;
        pJmp->dummy = 0x00000000;
        pJmp->address = (UINT64)newCallTarget;

        THROW_IF_WIN32_BOOL_FALSE(
            VirtualProtect(pJmp, sizeof(*pJmp), dwOldProtect, &dwOldProtect));
#else
#pragma pack(push, 1)
        // 32-bit direct relative jump/call.
        typedef struct _JMP_REL {
            UINT8 opcode;    // E9/E8 xxxxxxxx: JMP/CALL +5+xxxxxxxx
            UINT32 operand;  // Relative destination address
        } JMP_REL, *PJMP_REL, CALL_REL;
#pragma pack(pop)

        JMP_REL* pJmp = (JMP_REL*)patchTarget;

        DWORD dwOldProtect;
        THROW_IF_WIN32_BOOL_FALSE(VirtualProtect(
            pJmp, sizeof(*pJmp), PAGE_EXECUTE_READWRITE, &dwOldProtect));

        pJmp->opcode = 0xE9;
        pJmp->operand = (UINT32)((BYTE*)newCallTarget -
                                 ((BYTE*)patchTarget + sizeof(JMP_REL)));

        THROW_IF_WIN32_BOOL_FALSE(
            VirtualProtect(pJmp, sizeof(*pJmp), dwOldProtect, &dwOldProtect));
#endif
    };

#ifdef _WIN64
    // WindhawkUtils::HookSymbols(
    //     HINSTANCE__*, WindhawkUtils::SYMBOL_HOOK const*, unsigned long long)
    auto* hookSymbolsOld1 = GetProcAddress(
        mod,
        R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_11SYMBOL_HOOKEy)");

    // WindhawkUtils::HookSymbols(
    //     HINSTANCE__*, WindhawkUtils::SYMBOL_HOOK const*, unsigned long long,
    //     tagWH_FIND_SYMBOL_OPTIONS const*)
    auto* hookSymbolsOld2 = GetProcAddress(
        mod,
        R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_11SYMBOL_HOOKEyPK25tagWH_FIND_SYMBOL_OPTIONS)");

    // HookSymbols(HINSTANCE__*, SYMBOL_HOOK const*, unsigned long long)
    auto* localHookSymbols =
        GetProcAddress(mod, R"(_Z11HookSymbolsP11HINSTANCE__PK11SYMBOL_HOOKy)");

    // HookSymbolsWithOnlineCacheFallback(
    //     HINSTANCE__*, SYMBOL_HOOK const*, unsigned long long)
    auto* localHookSymbolsWithOnlineCacheFallback = GetProcAddress(
        mod,
        R"(_Z34HookSymbolsWithOnlineCacheFallbackP11HINSTANCE__PK11SYMBOL_HOOKy)");

    // CmwfHookSymbols(
    //     HINSTANCE__*, CMWF_SYMBOL_HOOK const*, unsigned long long)
    auto* localCmwfHookSymbols = GetProcAddress(
        mod, R"(_Z15CmwfHookSymbolsP11HINSTANCE__PK16CMWF_SYMBOL_HOOKy)");

    if (!hookSymbolsOld1 && !hookSymbolsOld2 && !localHookSymbols &&
        !localHookSymbolsWithOnlineCacheFallback && !localCmwfHookSymbols) {
        return nullptr;
    }

    auto& storageManager = StorageManager::GetInstance();

    auto libraryPath = storageManager.GetModsPath() / kShimLibraryFileName;

    shimModule.reset(LoadLibraryEx(libraryPath.c_str(), nullptr,
                                   LOAD_WITH_ALTERED_SEARCH_PATH));
    if (!shimModule) {
        return nullptr;
    }

    if (hookSymbolsOld1 || localHookSymbols ||
        localHookSymbolsWithOnlineCacheFallback) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_17SYMBOL_HOOK_OLD_1Ey)");
        if (newCallTarget) {
            if (hookSymbolsOld1) {
                patchFunction(hookSymbolsOld1, newCallTarget);
            }

            if (localHookSymbols) {
                patchFunction(localHookSymbols, newCallTarget);
            }

            if (localHookSymbolsWithOnlineCacheFallback) {
                patchFunction(localHookSymbolsWithOnlineCacheFallback,
                              newCallTarget);
            }
        }
    }

    if (hookSymbolsOld2) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_17SYMBOL_HOOK_OLD_2EyPKNS_29tagWH_FIND_SYMBOL_OPTIONS_OLDE)");
        if (newCallTarget) {
            patchFunction(hookSymbolsOld2, newCallTarget);
        }
    }

    if (localCmwfHookSymbols) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_Z15CmwfHookSymbolsP11HINSTANCE__PK16CMWF_SYMBOL_HOOKy)");
        if (newCallTarget) {
            patchFunction(localCmwfHookSymbols, newCallTarget);
        }
    }
#else
    // WindhawkUtils::HookSymbols(
    //     HINSTANCE__*, WindhawkUtils::SYMBOL_HOOK const*, unsigned int)
    auto* hookSymbolsOld1 = GetProcAddress(
        mod,
        R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_11SYMBOL_HOOKEj)");

    // WindhawkUtils::HookSymbols(
    //     HINSTANCE__*, WindhawkUtils::SYMBOL_HOOK const*, unsigned int,
    //     tagWH_FIND_SYMBOL_OPTIONS const*)
    auto* hookSymbolsOld2 = GetProcAddress(
        mod,
        R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_11SYMBOL_HOOKEjPK25tagWH_FIND_SYMBOL_OPTIONS)");

    // HookSymbols(HINSTANCE__*, SYMBOL_HOOK const*, unsigned int)
    auto* localHookSymbols =
        GetProcAddress(mod, R"(_Z11HookSymbolsP11HINSTANCE__PK11SYMBOL_HOOKj)");

    // CmwfHookSymbols(
    //     HINSTANCE__*, CMWF_SYMBOL_HOOK const*, unsigned int)
    auto* localCmwfHookSymbols = GetProcAddress(
        mod, R"(_Z15CmwfHookSymbolsP11HINSTANCE__PK16CMWF_SYMBOL_HOOKj)");

    if (!hookSymbolsOld1 && !hookSymbolsOld2 && !localHookSymbols &&
        !localCmwfHookSymbols) {
        return nullptr;
    }

    auto& storageManager = StorageManager::GetInstance();

    auto libraryPath = storageManager.GetModsPath() / kShimLibraryFileName;

    shimModule.reset(LoadLibraryEx(libraryPath.c_str(), nullptr,
                                   LOAD_WITH_ALTERED_SEARCH_PATH));
    if (!shimModule) {
        return nullptr;
    }

    if (hookSymbolsOld1 || localHookSymbols) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_17SYMBOL_HOOK_OLD_1Ej)");
        if (newCallTarget) {
            if (hookSymbolsOld1) {
                patchFunction(hookSymbolsOld1, newCallTarget);
            }

            if (localHookSymbols) {
                patchFunction(localHookSymbols, newCallTarget);
            }
        }
    }

    if (hookSymbolsOld2) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_ZN13WindhawkUtils11HookSymbolsEP11HINSTANCE__PKNS_17SYMBOL_HOOK_OLD_2EjPKNS_29tagWH_FIND_SYMBOL_OPTIONS_OLDE)");
        if (newCallTarget) {
            patchFunction(hookSymbolsOld2, newCallTarget);
        }
    }

    if (localCmwfHookSymbols) {
        void* newCallTarget = GetProcAddress(
            shimModule.get(),
            R"(_Z15CmwfHookSymbolsP11HINSTANCE__PK16CMWF_SYMBOL_HOOKj)");
        if (newCallTarget) {
            patchFunction(localCmwfHookSymbols, newCallTarget);
        }
    }
#endif

    return shimModule;
}

HRESULT STDAPICALLTYPE
TaskbarEmptySpaceClicksCoInitializeExHook(LPVOID pvReserved, DWORD dwCoInit) {
    return CoInitializeEx(pvReserved, dwCoInit == COINIT_MULTITHREADED
                                          ? COINIT_APARTMENTTHREADED
                                          : dwCoInit);
}

void SetModShims(HMODULE mod, wil::zwstring_view modName) {
    // https://github.com/m1lhaus/windhawk-mods/issues/12
    if (modName == L"taskbar-empty-space-clicks") {
        auto modVersion = GetModVersion(modName.c_str());
        if (modVersion == L"-" || modVersion == L"1.0" ||
            modVersion == L"1.1" || modVersion == L"1.2" ||
            modVersion == L"1.3") {
            void** pCoInitializeEx =
                Functions::FindImportPtr(mod, "ole32.dll", "CoInitializeEx");
            if (pCoInitializeEx) {
                DWORD dwOldProtect;
                THROW_IF_WIN32_BOOL_FALSE(
                    VirtualProtect(pCoInitializeEx, sizeof(*pCoInitializeEx),
                                   PAGE_EXECUTE_READWRITE, &dwOldProtect));
                *pCoInitializeEx = TaskbarEmptySpaceClicksCoInitializeExHook;
                THROW_IF_WIN32_BOOL_FALSE(
                    VirtualProtect(pCoInitializeEx, sizeof(*pCoInitializeEx),
                                   dwOldProtect, &dwOldProtect));
            }
        }
    }
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

    try {
        m_modShimLibrary =
            SetModShimsLibraryIfNeeded(m_modModule.get(), m_modName);
    } catch (const std::exception& e) {
        LOG(L"Mod %s: %S", m_modName.c_str(), e.what());
    }

    try {
        SetModShims(m_modModule.get(), m_modName);
    } catch (const std::exception& e) {
        LOG(L"Mod %s: %S", m_modName.c_str(), e.what());
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
        } else {
            LOG(L"Buffer size too small: %zu < %zu", bufferSize, value.size());
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

BOOL LoadedMod::DeleteValue(PCWSTR valueName) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"valueName: %s", valueName);

    try {
        auto settings = StorageManager::GetInstance().GetModWritableConfig(
            m_modName.c_str(), L"LocalStorage", true);
        settings->Remove(valueName);
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
                                  BYTE* findData) {
    WH_FIND_SYMBOL_OPTIONS options = {
        .optionsSize = sizeof(options),
        .symbolServer = symbolServer,
    };
    WH_FIND_SYMBOL newFindData;
    HANDLE findHandle = FindFirstSymbol4(hModule, &options, &newFindData);
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
    WH_FIND_SYMBOL_OPTIONS options = {
        .optionsSize = sizeof(options),
        .symbolServer = symbolServer,
    };
    return FindFirstSymbol4(hModule, &options, findData);
}

HANDLE LoadedMod::FindFirstSymbol3(HMODULE hModule,
                                   const BYTE* options,
                                   WH_FIND_SYMBOL* findData) {
    struct {
        PCWSTR symbolServer;
        BOOL noUndecoratedSymbols;
    }* optionsOldStruct = (decltype(optionsOldStruct))options;

    WH_FIND_SYMBOL_OPTIONS optionsNewStruct = {
        .optionsSize = sizeof(optionsNewStruct),
        .symbolServer =
            optionsOldStruct ? optionsOldStruct->symbolServer : nullptr,
        .noUndecoratedSymbols =
            optionsOldStruct && optionsOldStruct->noUndecoratedSymbols,
    };
    return FindFirstSymbol4(hModule, &optionsNewStruct, findData);
}

HANDLE LoadedMod::FindFirstSymbol4(HMODULE hModule,
                                   const WH_FIND_SYMBOL_OPTIONS* options,
                                   WH_FIND_SYMBOL* findData) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    constexpr size_t kMaxPdbIdentifierLength =
        sizeof("AAAAAAAABBBBCCCCDDDDEEEEEEEEEEEE12345678") - 1;

    class SymbolLoadLock {
       public:
        SymbolLoadLock(HANDLE moduleBase) {
            GUID pdbGuid;
            DWORD pdbAge;
            if (!Functions::ModuleGetPDBInfo(moduleBase, &pdbGuid, &pdbAge)) {
                return;
            }

            WCHAR pdbIdentifier[kMaxPdbIdentifierLength + 1];
            swprintf_s(pdbIdentifier,
                       L"%08X%04X%04X%02X%02X%02X%02X%02X%02X%02X%02X%x",
                       pdbGuid.Data1, pdbGuid.Data2, pdbGuid.Data3,
                       pdbGuid.Data4[0], pdbGuid.Data4[1], pdbGuid.Data4[2],
                       pdbGuid.Data4[3], pdbGuid.Data4[4], pdbGuid.Data4[5],
                       pdbGuid.Data4[6], pdbGuid.Data4[7], pdbAge);

            try {
                m_mutex.reset(CreateSymbolLoadLockMutex(pdbIdentifier, FALSE));
            } catch (const std::exception& e) {
                LOG(L"%S", e.what());
                return;
            }
        }

        operator bool() const { return !!m_mutex; }

        bool Acquire(DWORD milliseconds = INFINITE) {
            m_mutexLock = m_mutex.acquire(nullptr, milliseconds);
            return !!m_mutexLock;
        }

       private:
        HANDLE CreateSymbolLoadLockMutex(PCWSTR pdbIdentifier,
                                         BOOL initialOwner) {
            DWORD dwSessionManagerProcessId =
                CustomizationSession::GetSessionManagerProcessId();

            wil::unique_private_namespace_close privateNamespace;
            if (dwSessionManagerProcessId != GetCurrentProcessId()) {
                privateNamespace =
                    SessionPrivateNamespace::Open(dwSessionManagerProcessId);
            }

            WCHAR szMutexName[SessionPrivateNamespace::kPrivateNamespaceMaxLen +
                              (sizeof("\\SymbolLoadLockMutex-") - 1) +
                              kMaxPdbIdentifierLength + 1];
            int mutexNamePos = SessionPrivateNamespace::MakeName(
                szMutexName, dwSessionManagerProcessId);
            swprintf_s(szMutexName + mutexNamePos,
                       ARRAYSIZE(szMutexName) - mutexNamePos,
                       L"\\SymbolLoadLockMutex-%s", pdbIdentifier);

            wil::unique_hlocal secDesc;
            THROW_IF_WIN32_BOOL_FALSE(
                Functions::GetFullAccessSecurityDescriptor(&secDesc, nullptr));

            SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
            secAttr.lpSecurityDescriptor = secDesc.get();
            secAttr.bInheritHandle = FALSE;

            wil::unique_mutex_nothrow mutex(
                CreateMutex(&secAttr, initialOwner, szMutexName));
            THROW_LAST_ERROR_IF_NULL(mutex);

            return mutex.release();
        }

        wil::unique_mutex_nothrow m_mutex;
        wil::mutex_release_scope_exit m_mutexLock;
    };

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
        LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_LOWERCASE, &moduleName[0],
                      wil::safe_cast<int>(moduleName.length()), &moduleName[0],
                      wil::safe_cast<int>(moduleName.length()), nullptr,
                      nullptr, 0);

        SetTask((L"Loading symbols... (" + moduleName + L")").c_str());

        auto activityStatusCleanup = wil::scope_exit(
            [this] { SetTask(m_initialized ? nullptr : L"Initializing..."); });

        SymbolEnum::Callbacks callbacks;

        bool canceled = false;
        DWORD lastQueryCancelTick = GetTickCount();

        callbacks.queryCancel = [this, &canceled, &lastQueryCancelTick]() {
            if (canceled) {
                return true;
            }

            DWORD tick = GetTickCount();
            if (tick - lastQueryCancelTick < 1000) {
                return false;
            }

            lastQueryCancelTick = tick;

            try {
                if (!Mod::ShouldLoadInRunningProcess(m_modName.c_str()) ||
                    CustomizationSession::IsEndingSoon()) {
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

        std::unique_ptr<SymbolEnum> symbolEnum;
        if (options && options->symbolServer && !*options->symbolServer) {
            // No symbol server, no lock needed.
            symbolEnum = std::make_unique<SymbolEnum>(
                modulePath.c_str(), hModule, L"", undecorateMode);
        } else {
            SymbolLoadLock symbolLoadLock(moduleBase);

            // If lock is not acquired, try loading a local symbol file first.
            // If it fails, wait for the lock before proceeding with the symbol
            // server to avoid multiple processes downloading the same file.
            if (symbolLoadLock && !symbolLoadLock.Acquire(/*milliseconds=*/0)) {
                try {
                    // Try loading a local symbol file first.
                    symbolEnum = std::make_unique<SymbolEnum>(
                        modulePath.c_str(), hModule, L"", undecorateMode);
                } catch (const std::exception& e) {
                    VERBOSE(L"Failed to load local symbol file: %S", e.what());

                    SetTask((L"Waiting for symbols... (" + moduleName + L")")
                                .c_str());

                    symbolLoadLock.Acquire();

                    // In case the mod was disabled, abort without starting the
                    // symbol server flow.
                    if (!Mod::ShouldLoadInRunningProcess(m_modName.c_str()) ||
                        CustomizationSession::IsEndingSoon()) {
                        VERBOSE(L"Aborting symbol loading");
                        return nullptr;
                    }

                    SetTask(
                        (L"Loading symbols... (" + moduleName + L")").c_str());
                }
            }

            if (!symbolEnum) {
                symbolEnum = std::make_unique<SymbolEnum>(
                    modulePath.c_str(), hModule,
                    options ? options->symbolServer : nullptr, undecorateMode,
                    std::move(callbacks));
            }
        }

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

BOOL LoadedMod::FindNextSymbol(HANDLE symSearch, BYTE* findData) {
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

BOOL LoadedMod::HookSymbols(HMODULE module,
                            const WH_SYMBOL_HOOK* symbolHooks,
                            size_t symbolHooksCount,
                            const WH_HOOK_SYMBOLS_OPTIONS* options) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (symbolHooksCount == 0) {
        return TRUE;
    }

    try {
        constexpr WCHAR kCacheVer = L'1';
        constexpr WCHAR kCacheSep = L'#';

        std::filesystem::path modulePath =
            wil::GetModuleFileName<std::wstring>(module);
        auto moduleFileName = modulePath.filename().wstring();
        LCMapStringEx(
            LOCALE_NAME_USER_DEFAULT, LCMAP_LOWERCASE, &moduleFileName[0],
            wil::safe_cast<int>(moduleFileName.length()), &moduleFileName[0],
            wil::safe_cast<int>(moduleFileName.length()), nullptr, nullptr, 0);

        VERBOSE(L"Module: %p", module);
        VERBOSE(L"Path: %s", modulePath.c_str());
        VERBOSE(L"Version: %S", GetModuleVersion(module).c_str());

        IMAGE_DOS_HEADER* dosHeader = (IMAGE_DOS_HEADER*)module;
        IMAGE_NT_HEADERS* header =
            (IMAGE_NT_HEADERS*)((BYTE*)dosHeader + dosHeader->e_lfanew);
        auto timeStamp = std::to_wstring(header->FileHeader.TimeDateStamp);
        auto imageSize = std::to_wstring(header->OptionalHeader.SizeOfImage);

        std::wstring cacheStrKey;

        GUID pdbGuid;
        DWORD pdbAge;
        if (Functions::ModuleGetPDBInfo(module, &pdbGuid, &pdbAge)) {
            constexpr size_t kMaxPdbIdentifierLength =
                sizeof("AAAAAAAABBBBCCCCDDDDEEEEEEEEEEEE12345678") - 1;
            WCHAR pdbIdentifier[kMaxPdbIdentifierLength + 1];
            swprintf_s(pdbIdentifier,
                       L"%08X%04X%04X%02X%02X%02X%02X%02X%02X%02X%02X%x",
                       pdbGuid.Data1, pdbGuid.Data2, pdbGuid.Data3,
                       pdbGuid.Data4[0], pdbGuid.Data4[1], pdbGuid.Data4[2],
                       pdbGuid.Data4[3], pdbGuid.Data4[4], pdbGuid.Data4[5],
                       pdbGuid.Data4[6], pdbGuid.Data4[7], pdbAge);

            cacheStrKey = L"pdb_";
            cacheStrKey += pdbIdentifier;
        } else {
            cacheStrKey = L"pe_";
            cacheStrKey +=
#if defined(_M_IX86)
                L"x86";
#elif defined(_M_X64)
                L"x86-64";
#else
#error "Unsupported architecture"
#endif
            cacheStrKey += L'_';
            cacheStrKey += timeStamp;
            cacheStrKey += L'_';
            cacheStrKey += imageSize;
            cacheStrKey += L'_';
            cacheStrKey += moduleFileName;
        }

        std::wstring cacheBuffer;
        std::vector<std::wstring_view> cacheParts;

        {
            auto symbolCache =
                StorageManager::GetInstance().GetModWritableConfig(
                    m_modName.c_str(), L"SymbolCache", false);
            cacheBuffer =
                symbolCache->GetString(cacheStrKey.c_str()).value_or(L"");
            cacheParts = Functions::SplitStringToViews(cacheBuffer, kCacheSep);
        }

        // If the cache is empty, try the old location.
        if (cacheBuffer.empty()) {
            std::wstring legacyCacheStrKey =
#if defined(_M_IX86)
                L"symbol-x86-cache-";
#elif defined(_M_X64)
                L"symbol-cache-";
#else
#error "Unsupported architecture"
#endif
            legacyCacheStrKey += moduleFileName;

            auto settings = StorageManager::GetInstance().GetModWritableConfig(
                m_modName.c_str(), L"LocalStorage", false);
            cacheBuffer =
                settings->GetString(legacyCacheStrKey.c_str()).value_or(L"");
            cacheParts = Functions::SplitStringToViews(cacheBuffer, kCacheSep);

            if (cacheParts.size() < 3 ||
                cacheParts[0] != std::wstring_view(&kCacheVer, 1) ||
                cacheParts[1] != timeStamp || cacheParts[2] != imageSize) {
                cacheBuffer = L"";
                cacheParts = {};
            } else {
                VERBOSE(L"Using symbol cache %.*s (%.*s): %.*s",
                        wil::safe_cast<int>(legacyCacheStrKey.length()),
                        legacyCacheStrKey.data(),
                        wil::safe_cast<int>(cacheStrKey.length()),
                        cacheStrKey.data(),
                        wil::safe_cast<int>(cacheBuffer.length()),
                        cacheBuffer.data());
            }
        } else {
            VERBOSE(
                L"Using symbol cache %.*s: %.*s",
                wil::safe_cast<int>(cacheStrKey.length()), cacheStrKey.data(),
                wil::safe_cast<int>(cacheBuffer.length()), cacheBuffer.data());
        }

        std::vector<bool> symbolResolved(symbolHooksCount, false);
        std::wstring newSystemCacheStr;

        auto onSymbolResolved = [this, symbolHooks, symbolHooksCount,
                                 &symbolResolved, &newSystemCacheStr, module](
                                    std::wstring_view symbol, void* address) {
            for (size_t i = 0; i < symbolHooksCount; i++) {
                if (symbolResolved[i]) {
                    continue;
                }

                bool match = false;
                for (size_t s = 0; s < symbolHooks[i].symbolsCount; s++) {
                    auto hookSymbol =
                        std::wstring_view(symbolHooks[i].symbols[s].string,
                                          symbolHooks[i].symbols[s].length);
                    if (hookSymbol == symbol) {
                        match = true;
                        break;
                    }
                }

                if (!match) {
                    continue;
                }

                if (symbolHooks[i].hookFunction) {
                    SetFunctionHook(address, symbolHooks[i].hookFunction,
                                    symbolHooks[i].pOriginalFunction);
                    VERBOSE(L"Hooked %p: %.*s", address,
                            wil::safe_cast<int>(symbol.length()),
                            symbol.data());
                } else {
                    *symbolHooks[i].pOriginalFunction = address;
                    VERBOSE(L"Found %p: %.*s", address,
                            wil::safe_cast<int>(symbol.length()),
                            symbol.data());
                }

                symbolResolved[i] = true;

                newSystemCacheStr += kCacheSep;
                newSystemCacheStr += symbol;
                newSystemCacheStr += kCacheSep;
                newSystemCacheStr +=
                    std::to_wstring((ULONG_PTR)address - (ULONG_PTR)module);

                break;
            }
        };

        newSystemCacheStr += kCacheVer;
        newSystemCacheStr += kCacheSep;
        newSystemCacheStr += moduleFileName;
        newSystemCacheStr += kCacheSep;
        newSystemCacheStr += timeStamp;
        newSystemCacheStr += L'-';
        newSystemCacheStr += imageSize;

        auto resolveSymbolsFromCache = [&kCacheVer, symbolHooks,
                                        symbolHooksCount, &symbolResolved,
                                        &onSymbolResolved, &newSystemCacheStr,
                                        module](std::vector<std::wstring_view>&
                                                    cacheParts) {
            // In the new format, cacheParts[1] and cacheParts[2] are
            // ignored and act like comments.
            if (cacheParts.size() < 3 ||
                cacheParts[0] != std::wstring_view(&kCacheVer, 1)) {
                return false;
            }

            for (size_t i = 3; i + 1 < cacheParts.size(); i += 2) {
                const auto& symbol = cacheParts[i];
                const auto& address = cacheParts[i + 1];
                if (address.length() == 0) {
                    continue;
                }

                void* addressPtr =
                    (void*)(std::stoull(std::wstring(address), nullptr, 10) +
                            (ULONG_PTR)module);

                onSymbolResolved(symbol, addressPtr);
            }

            for (size_t i = 0; i < symbolHooksCount; i++) {
                if (symbolResolved[i] || !symbolHooks[i].optional) {
                    continue;
                }

                size_t noAddressMatchCount = 0;
                for (size_t j = 3; j + 1 < cacheParts.size(); j += 2) {
                    const auto& symbol = cacheParts[j];
                    const auto& address = cacheParts[j + 1];
                    if (address.length() != 0) {
                        continue;
                    }

                    for (size_t s = 0; s < symbolHooks[i].symbolsCount; s++) {
                        auto hookSymbol =
                            std::wstring_view(symbolHooks[i].symbols[s].string,
                                              symbolHooks[i].symbols[s].length);
                        if (hookSymbol == symbol) {
                            noAddressMatchCount++;
                            break;
                        }
                    }
                }

                if (noAddressMatchCount == symbolHooks[i].symbolsCount) {
                    VERBOSE(L"Optional symbol %d doesn't exist (from cache)",
                            i);

                    symbolResolved[i] = true;

                    for (size_t s = 0; s < symbolHooks[i].symbolsCount; s++) {
                        auto hookSymbol =
                            std::wstring_view(symbolHooks[i].symbols[s].string,
                                              symbolHooks[i].symbols[s].length);
                        newSystemCacheStr += kCacheSep;
                        newSystemCacheStr += hookSymbol;
                        newSystemCacheStr += kCacheSep;
                    }
                }
            }

            return std::all_of(symbolResolved.begin(), symbolResolved.end(),
                               [](bool b) { return b; });
        };

        if (resolveSymbolsFromCache(cacheParts)) {
            return TRUE;
        }

        VERBOSE(L"Couldn't resolve all symbols from local cache");

        std::wstring onlineCacheUrl;
        if (options && options->onlineCacheUrl) {
            onlineCacheUrl = options->onlineCacheUrl;
            if (!onlineCacheUrl.empty() && onlineCacheUrl.back() != L'/') {
                onlineCacheUrl += L'/';
            }
        } else if (!m_modName.starts_with(L"local@")) {
            onlineCacheUrl =
                L"https://ramensoftware.github.io/windhawk-mod-symbol-cache/";
            onlineCacheUrl += m_modName;
            onlineCacheUrl += L'/';
        }

        if (!onlineCacheUrl.empty()) {
            onlineCacheUrl += cacheStrKey;
            onlineCacheUrl += L".txt";

            const WH_URL_CONTENT* onlineCacheUrlContent =
                GetUrlContent(onlineCacheUrl.c_str(), nullptr);
            if (onlineCacheUrlContent) {
                std::wstring onlineCache;
                if (onlineCacheUrlContent->statusCode == 200) {
                    onlineCache =
                        std::wstring(onlineCacheUrlContent->data,
                                     onlineCacheUrlContent->data +
                                         onlineCacheUrlContent->length);
                }

                FreeUrlContent(onlineCacheUrlContent);

                if (!onlineCache.empty()) {
                    VERBOSE(L"Using online symbol cache %.*s: %.*s",
                            wil::safe_cast<int>(cacheStrKey.length()),
                            cacheStrKey.data(),
                            wil::safe_cast<int>(onlineCache.length()),
                            onlineCache.data());

                    auto onlineCacheParts =
                        Functions::SplitStringToViews(onlineCache, kCacheSep);

                    if (resolveSymbolsFromCache(onlineCacheParts)) {
                        auto symbolCache =
                            StorageManager::GetInstance().GetModWritableConfig(
                                m_modName.c_str(), L"SymbolCache", true);
                        symbolCache->SetString(cacheStrKey.c_str(),
                                               newSystemCacheStr.c_str());
                        return TRUE;
                    }
                }
            } else {
                VERBOSE(L"Couldn't contact the online cache server");
                VERBOSE(
                    L"In case of a firewall, you can open cmd manually and run "
                    L"the following command, then disable and re-enable the "
                    L"mod:");
                VERBOSE(
                    LR"(for /f "delims=" %%I in ('curl -f %s') do @reg add "HKLM\SOFTWARE\Windhawk\Engine\ModsWritable\%s\SymbolCache" /t REG_SZ /v %s /d "%%I" /f)",
                    onlineCacheUrl.c_str(), m_modName.c_str(),
                    cacheStrKey.c_str());
            }

            VERBOSE(L"Couldn't resolve all symbols from online cache");
        }

        WH_FIND_SYMBOL findSymbol;
        WH_FIND_SYMBOL_OPTIONS findFirstSymbolOptions = {
            .optionsSize = sizeof(findFirstSymbolOptions),
            .symbolServer = options ? options->symbolServer : nullptr,
            .noUndecoratedSymbols = options && options->noUndecoratedSymbols,
        };
        HANDLE findSymbolHandle =
            FindFirstSymbol4(module, &findFirstSymbolOptions, &findSymbol);
        if (!findSymbolHandle) {
            return FALSE;
        }

        do {
            onSymbolResolved((options && options->noUndecoratedSymbols)
                                 ? findSymbol.symbolDecorated
                                 : findSymbol.symbol,
                             findSymbol.address);
        } while (FindNextSymbol2(findSymbolHandle, &findSymbol));

        FindCloseSymbol(findSymbolHandle);

        for (size_t i = 0; i < symbolHooksCount; i++) {
            if (symbolResolved[i]) {
                continue;
            }

            if (!symbolHooks[i].optional) {
                VERBOSE(L"Unresolved symbol: %d", i);
                return FALSE;
            }

            VERBOSE(L"Optional symbol %d doesn't exist", i);

            for (size_t s = 0; s < symbolHooks[i].symbolsCount; s++) {
                auto hookSymbol =
                    std::wstring_view(symbolHooks[i].symbols[s].string,
                                      symbolHooks[i].symbols[s].length);
                newSystemCacheStr += kCacheSep;
                newSystemCacheStr += hookSymbol;
                newSystemCacheStr += kCacheSep;
            }
        }

        {
            auto symbolCache =
                StorageManager::GetInstance().GetModWritableConfig(
                    m_modName.c_str(), L"SymbolCache", true);
            symbolCache->SetString(cacheStrKey.c_str(),
                                   newSystemCacheStr.c_str());
        }
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return TRUE;
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

const WH_URL_CONTENT* LoadedMod::GetUrlContent(PCWSTR url, void* reserved) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"URL: %s", url);

    try {
        URL_COMPONENTS urlComp = {sizeof(urlComp)};
        urlComp.dwHostNameLength = (DWORD)-1;
        urlComp.dwUrlPathLength = (DWORD)-1;
        THROW_IF_WIN32_BOOL_FALSE(WinHttpCrackUrl(url, 0, 0, &urlComp));

        wil::unique_winhttp_hinternet session{
            WinHttpOpen(L"Windhawk/" VER_FILE_VERSION_WSTR,
                        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                        WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0)};
        THROW_LAST_ERROR_IF_NULL(session);

        wil::unique_winhttp_hinternet connect{WinHttpConnect(
            session.get(),
            std::wstring(urlComp.lpszHostName, urlComp.dwHostNameLength)
                .c_str(),
            urlComp.nPort, 0)};
        THROW_LAST_ERROR_IF_NULL(connect);

        wil::unique_winhttp_hinternet request{WinHttpOpenRequest(
            connect.get(), L"GET",
            std::wstring(urlComp.lpszUrlPath, urlComp.dwUrlPathLength).c_str(),
            nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES,
            urlComp.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE
                                                     : 0)};
        THROW_LAST_ERROR_IF_NULL(request);

        THROW_IF_WIN32_BOOL_FALSE(
            WinHttpSendRequest(request.get(), WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                               WINHTTP_NO_REQUEST_DATA, 0, 0, 0));

        THROW_IF_WIN32_BOOL_FALSE(
            WinHttpReceiveResponse(request.get(), nullptr));

        DWORD statusCode = 0;
        DWORD statusCodeSize = sizeof(statusCode);
        THROW_IF_WIN32_BOOL_FALSE(WinHttpQueryHeaders(
            request.get(),
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            WINHTTP_HEADER_NAME_BY_INDEX, &statusCode, &statusCodeSize,
            WINHTTP_NO_HEADER_INDEX));

        auto content = std::make_unique<WH_URL_CONTENT>();
        content->statusCode = statusCode;

        std::vector<std::string> chunks;
        DWORD downloaded = 0;
        size_t downloadedTotal = 0;
        do {
            DWORD size = 0;
            THROW_IF_WIN32_BOOL_FALSE(
                WinHttpQueryDataAvailable(request.get(), &size));

            if (size == 0) {
                break;
            }

            chunks.push_back(std::string(size, '\0'));

            THROW_IF_WIN32_BOOL_FALSE(WinHttpReadData(
                request.get(), (PVOID)chunks.back().data(), size, &downloaded));

            chunks.back().resize(downloaded);
            downloadedTotal += downloaded;
        } while (downloaded > 0);

        auto data = std::make_unique<char[]>(downloadedTotal + 1);
        size_t dataIter = 0;
        for (const auto& chunk : chunks) {
            std::copy(chunk.begin(), chunk.end(), data.get() + dataIter);
            dataIter += chunk.size();
        }
        data[dataIter] = '\0';

        content->data = data.release();
        content->length = downloadedTotal;

        return content.release();
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return nullptr;
}

void LoadedMod::FreeUrlContent(const WH_URL_CONTENT* content) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (content) {
        delete[] content->data;
        delete content;
    }
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
