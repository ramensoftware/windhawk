#include "stdafx.h"

#include "customization_session.h"
#include "functions.h"
#include "logger.h"
#include "mod.h"
#include "process_lists.h"
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

class CrossModMutex {
   public:
    CrossModMutex(PCWSTR mutexIdentifier) {
        try {
            m_mutex.reset(CreateSymbolLoadLockMutex(mutexIdentifier, FALSE));
        } catch (const std::exception& e) {
            LOG(L"%S", e.what());
        }
    }

    operator bool() const { return !!m_mutex; }

    bool Acquire(DWORD milliseconds = INFINITE) {
        m_mutexLock = m_mutex.acquire(nullptr, milliseconds);
        return !!m_mutexLock;
    }

   private:
    HANDLE CreateSymbolLoadLockMutex(PCWSTR mutexIdentifier,
                                     BOOL initialOwner) {
        DWORD dwSessionManagerProcessId =
            CustomizationSession::GetSessionManagerProcessId();

        WCHAR sessionPrivateNamespaceName
            [SessionPrivateNamespace::kPrivateNamespaceMaxLen + 1];
        SessionPrivateNamespace::MakeName(sessionPrivateNamespaceName,
                                          dwSessionManagerProcessId);

        std::wstring mutexName = sessionPrivateNamespaceName;
        mutexName += L'\\';
        mutexName += mutexIdentifier;

        wil::unique_hlocal secDesc;
        THROW_IF_WIN32_BOOL_FALSE(
            Functions::GetFullAccessSecurityDescriptor(&secDesc, nullptr));

        SECURITY_ATTRIBUTES secAttr{
            .nLength = sizeof(secAttr),
            .lpSecurityDescriptor = secDesc.get(),
            .bInheritHandle = FALSE,
        };

        wil::unique_mutex_nothrow mutex(
            CreateMutex(&secAttr, initialOwner, mutexName.c_str()));
        THROW_LAST_ERROR_IF_NULL(mutex);

        return mutex.release();
    }

    wil::unique_mutex_nothrow m_mutex;
    wil::mutex_release_scope_exit m_mutexLock;
};

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
#if defined(_M_IX86)
    if (patternPart == L"x86") {
        return true;
    }
#elif defined(_M_X64)
    // For now, x86-64 matches both x64 and ARM64.
    if (patternPart == L"x86-64" || patternPart == L"amd64") {
        return true;
    }
#elif defined(_M_ARM64)
    // For now, x86-64 matches both x64 and ARM64.
    if (patternPart == L"x86-64" || patternPart == L"arm64") {
        return true;
    }
#else
#error "Unsupported architecture"
#endif

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

bool IsModBanned(wil::zwstring_view modName) {
    bool banned = false;

#if defined(_M_IX86)
    constexpr std::wstring_view kX86 =
        L"Not loading an incompatible mod: "
        L"https://github.com/ramensoftware/windhawk-mods/issues/1878";

    struct {
        std::wstring_view modName;
        std::vector<std::wstring_view> versions;
        std::wstring_view reason;
    } incompatibleMods[] = {
        // Incompatible with 32-bit programs, caused by a missing calling
        // convention.
        // https://github.com/ramensoftware/windhawk-mods/issues/1878
        {L"no-hidden-cursor", {L"1.0.0"}, kX86},
        {L"no-focus-rectangle", {L"1.0.0", L"1.0.1", L"1.0.2"}, kX86},
        {L"disable-office-hotkeys", {L"1.0.0"}, kX86},
        {L"disable-feedback-hub-hotkey", {L"1.0.0"}, kX86},
    };

    std::wstring_view reason;

    for (const auto& incompatibleMod : incompatibleMods) {
        if (modName == incompatibleMod.modName) {
            auto modVersion = GetModVersion(modName.c_str());
            if (modVersion == L"-") {
                banned = true;
                reason = incompatibleMod.reason;
                break;
            }

            for (const auto& compatModVersion : incompatibleMod.versions) {
                if (modVersion == compatModVersion) {
                    banned = true;
                    reason = incompatibleMod.reason;
                    break;
                }
            }

            break;
        }
    }

    if (banned && !reason.empty()) {
        LOG(L"%.*s", wil::safe_cast<int>(reason.length()), reason.data());
    }
#endif  // defined(_M_IX86)

    return banned;
}

wil::unique_hmodule SetModShimsLibraryIfNeeded(HMODULE mod) {
    constexpr WCHAR kShimLibraryFileName[] = L"windhawk-mod-shim.dll";
    wil::unique_hmodule shimModule;

    auto patchFunction = [](void* patchTarget, void* newCallTarget) {
#if defined(_M_X64)
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
#elif defined(_M_IX86)
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
#elif defined(_M_ARM64)
#pragma pack(push, 1)
        // 64-bit indirect absolute jump.
        typedef struct _JMP_ABS {
            UINT32 cmd1;     // ldr x9, +8
            UINT32 cmd2;     // br x9
            UINT64 address;  // Absolute destination address
        } JMP_ABS, *PJMP_ABS;
#pragma pack(pop)

        JMP_ABS* pJmp = (JMP_ABS*)patchTarget;

        DWORD dwOldProtect;
        THROW_IF_WIN32_BOOL_FALSE(VirtualProtect(
            pJmp, sizeof(*pJmp), PAGE_EXECUTE_READWRITE, &dwOldProtect));

        pJmp->cmd1 = 0x58000049;  // ldr x9, +8
        pJmp->cmd2 = 0xd61f0120;  // br x9
        pJmp->address = (UINT64)newCallTarget;

        THROW_IF_WIN32_BOOL_FALSE(
            VirtualProtect(pJmp, sizeof(*pJmp), dwOldProtect, &dwOldProtect));

        FlushInstructionCache(GetCurrentProcess(), pJmp, sizeof(*pJmp));
#else
#error "Unsupported architecture"
#endif
    };

#if defined(_M_X64) || defined(_M_ARM64)
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
#elif defined(_M_IX86)
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
#else
#error "Unsupported architecture"
#endif

    return shimModule;
}

// Checks whether the module is CHPE, ARM64EC or ARM64X.
bool IsHybridModule(const IMAGE_DOS_HEADER* dosHeader,
                    const IMAGE_NT_HEADERS* ntHeader) {
    auto* opt = &ntHeader->OptionalHeader;

    if (opt->NumberOfRvaAndSizes <= IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG ||
        !opt->DataDirectory[IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG].VirtualAddress) {
        return false;
    }

    DWORD directorySize =
        opt->DataDirectory[IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG].Size;

    auto* cfg =
        (const IMAGE_LOAD_CONFIG_DIRECTORY*)((const char*)dosHeader +
                                             opt->DataDirectory
                                                 [IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG]
                                                     .VirtualAddress);

    constexpr DWORD kMinSize =
        offsetof(IMAGE_LOAD_CONFIG_DIRECTORY, CHPEMetadataPointer) +
        sizeof(IMAGE_LOAD_CONFIG_DIRECTORY::CHPEMetadataPointer);

    if (directorySize < kMinSize || cfg->Size < kMinSize) {
        return false;
    }

    return cfg->CHPEMetadataPointer != 0;
}

class HookSymbolsSession {
   public:
    HookSymbolsSession(LoadedMod* loadedMod,
                       HMODULE module,
                       const WH_SYMBOL_HOOK* symbolHooks,
                       size_t symbolHooksCount)
        : m_loadedMod(loadedMod), m_module(module) {
        CalculateHookSymbolsInitialParams();

        std::transform(symbolHooks, symbolHooks + symbolHooksCount,
                       std::back_inserter(m_symbolHooksUnresolved),
                       [](auto& elem) { return &elem; });
    }

    bool OnSymbolResolved(std::wstring_view symbol, void* address) {
        auto it = std::find_if(
            m_symbolHooksUnresolved.begin(), m_symbolHooksUnresolved.end(),
            [&symbol](const auto* symbolHook) {
                for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                    auto hookSymbol =
                        std::wstring_view(symbolHook->symbols[s].string,
                                          symbolHook->symbols[s].length);
                    if (hookSymbol == symbol) {
                        return true;
                    }
                }
                return false;
            });
        if (it == m_symbolHooksUnresolved.end()) {
            return false;
        }

        const auto* symbolHook = *it;

        if (symbolHook->hookFunction) {
            m_pendingHooks.emplace_back(address, symbolHook->hookFunction,
                                        symbolHook->pOriginalFunction);
            VERBOSE(L"To be hooked %p: %.*s", address,
                    wil::safe_cast<int>(symbol.length()), symbol.data());
        } else {
            if (symbolHook->pOriginalFunction) {
                *symbolHook->pOriginalFunction = address;
            }
            VERBOSE(L"Found %p: %.*s", address,
                    wil::safe_cast<int>(symbol.length()), symbol.data());
        }

        m_newSystemCacheStr += m_cacheSep;
        m_newSystemCacheStr += symbol;
        m_newSystemCacheStr += m_cacheSep;
        m_newSystemCacheStr +=
            std::to_wstring((ULONG_PTR)address - (ULONG_PTR)m_module);

        m_symbolHooksUnresolved.erase(it);
        return true;
    }

    enum class ResolveSymbolsFromCacheResult {
        kSuccess,
        kError,
        kNoCache,
        kCachedErrorForThrottle,
    };

    ResolveSymbolsFromCacheResult ResolveSymbolsFromCache(
        std::optional<ULONGLONG> cachedErrorForThrottleMaxTime) {
        std::wstring cacheBuffer;
        try {
            auto symbolCache =
                StorageManager::GetInstance().GetModWritableConfig(
                    m_loadedMod->GetModName(), L"SymbolCache", false);
            cacheBuffer =
                symbolCache->GetString(m_cacheStrKey.c_str()).value_or(L"");
            if (cacheBuffer.empty()) {
                return ResolveSymbolsFromCacheResult::kNoCache;
            }
        } catch (const std::exception& e) {
            LOG(L"%S", e.what());
            return ResolveSymbolsFromCacheResult::kError;
        }

        VERBOSE(L"Using symbol cache %.*s: %.*s",
                wil::safe_cast<int>(m_cacheStrKey.length()),
                m_cacheStrKey.data(), wil::safe_cast<int>(cacheBuffer.length()),
                cacheBuffer.data());

        if (cacheBuffer.starts_with(kErrorCachePrefix)) {
            auto cacheBufferWithoutPrefix =
                std::wstring_view(cacheBuffer).substr(kErrorCachePrefix.size());
            auto cacheBufferParts = Functions::SplitStringToViews(
                cacheBufferWithoutPrefix, kErrorCacheSep);
            if (cacheBufferParts.size() < 4 ||
                cacheBufferParts[0] != std::wstring_view(&kErrorCacheVer, 1)) {
                return ResolveSymbolsFromCacheResult::kNoCache;
            }

            if (cacheBufferParts[1] !=
                    std::to_wstring(
                        CustomizationSession::GetSessionManagerProcessId()) ||
                cacheBufferParts[2] !=
                    std::to_wstring(wil::filetime::to_int64(
                        CustomizationSession::
                            GetSessionManagerProcessCreationTime()))) {
                return ResolveSymbolsFromCacheResult::kNoCache;
            }

            ULONGLONG errorTime = std::wcstoull(
                std::wstring(cacheBufferParts[3]).c_str(), nullptr, 10);

            ULONGLONG currentTime =
                wil::filetime::to_int64(wil::filetime::get_system_time());

            if (cachedErrorForThrottleMaxTime &&
                currentTime - errorTime <= *cachedErrorForThrottleMaxTime) {
                return ResolveSymbolsFromCacheResult::kCachedErrorForThrottle;
            }

            return ResolveSymbolsFromCacheResult::kNoCache;
        }

        if (!ResolveSymbolsFromCacheString(cacheBuffer)) {
            return ResolveSymbolsFromCacheResult::kNoCache;
        }

        return ResolveSymbolsFromCacheResult::kSuccess;
    }

    bool ResolveSymbolsFromCacheString(std::wstring_view cache) {
        auto cacheParts = Functions::SplitStringToViews(cache, m_cacheSep);
        return ResolveSymbolsFromCacheStringParts(cacheParts);
    }

    bool ResolveSymbolsFromCacheStringParts(
        std::vector<std::wstring_view>& cacheParts) {
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
                (void*)(std::wcstoull(std::wstring(address).c_str(), nullptr,
                                      10) +
                        (ULONG_PTR)m_module);

            OnSymbolResolved(symbol, addressPtr);
        }

        std::erase_if(m_symbolHooksUnresolved, [this, &cacheParts](
                                                   const auto* symbolHook) {
            if (!symbolHook->optional) {
                return false;
            }

            size_t noAddressMatchCount = 0;
            for (size_t j = 3; j + 1 < cacheParts.size(); j += 2) {
                const auto& symbol = cacheParts[j];
                const auto& address = cacheParts[j + 1];
                if (address.length() != 0) {
                    continue;
                }

                for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                    auto hookSymbol =
                        std::wstring_view(symbolHook->symbols[s].string,
                                          symbolHook->symbols[s].length);
                    if (hookSymbol == symbol) {
                        noAddressMatchCount++;
                        break;
                    }
                }
            }

            if (noAddressMatchCount != symbolHook->symbolsCount) {
                return false;
            }

            VERBOSE(L"Optional symbol doesn't exist (from cache)");
            for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                auto hookSymbol =
                    std::wstring_view(symbolHook->symbols[s].string,
                                      symbolHook->symbols[s].length);
                VERBOSE(L"    %.*s", wil::safe_cast<int>(hookSymbol.length()),
                        hookSymbol.data());
            }

            for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                auto hookSymbol =
                    std::wstring_view(symbolHook->symbols[s].string,
                                      symbolHook->symbols[s].length);
                m_newSystemCacheStr += m_cacheSep;
                m_newSystemCacheStr += hookSymbol;
                m_newSystemCacheStr += m_cacheSep;
            }

            return true;  // Mark for removal.
        });

        return true;
    }

    bool UpdateSymbolsCache() {
        try {
            auto symbolCache =
                StorageManager::GetInstance().GetModWritableConfig(
                    m_loadedMod->GetModName(), L"SymbolCache", true);
            symbolCache->SetString(m_cacheStrKey.c_str(),
                                   m_newSystemCacheStr.c_str());
            return true;
        } catch (const std::exception& e) {
            LOG(L"%S", e.what());
        }

        return false;
    }

    bool UpdateSymbolsCacheWithErrorForThrottle() {
        auto errorForThrottleCacheStr = std::wstring(kErrorCachePrefix);
        errorForThrottleCacheStr += kErrorCacheVer;

        errorForThrottleCacheStr += kErrorCacheSep;
        errorForThrottleCacheStr +=
            std::to_wstring(CustomizationSession::GetSessionManagerProcessId());

        errorForThrottleCacheStr += kErrorCacheSep;
        errorForThrottleCacheStr += std::to_wstring(wil::filetime::to_int64(
            CustomizationSession::GetSessionManagerProcessCreationTime()));

        errorForThrottleCacheStr += kErrorCacheSep;
        errorForThrottleCacheStr += std::to_wstring(
            wil::filetime::to_int64(wil::filetime::get_system_time()));

        errorForThrottleCacheStr += kErrorCacheSep;
        errorForThrottleCacheStr += m_moduleFileName;
        errorForThrottleCacheStr += L'-';
        errorForThrottleCacheStr += m_timeStamp;
        errorForThrottleCacheStr += L'-';
        errorForThrottleCacheStr += m_imageSize;

        try {
            auto symbolCache =
                StorageManager::GetInstance().GetModWritableConfig(
                    m_loadedMod->GetModName(), L"SymbolCache", true);
            symbolCache->SetString(m_cacheStrKey.c_str(),
                                   errorForThrottleCacheStr.c_str());
            return true;
        } catch (const std::exception& e) {
            LOG(L"%S", e.what());
        }

        return false;
    }

    void MarkUnresolvedSymbolsAsMissing() {
        std::erase_if(m_symbolHooksUnresolved, [this](const auto* symbolHook) {
            VERBOSE(L"Unresolved symbol%s",
                    symbolHook->optional ? L" (optional)" : L"");
            for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                auto hookSymbol =
                    std::wstring_view(symbolHook->symbols[s].string,
                                      symbolHook->symbols[s].length);
                VERBOSE(L"    %.*s", wil::safe_cast<int>(hookSymbol.length()),
                        hookSymbol.data());
            }

            if (!symbolHook->optional) {
                return false;
            }

            for (size_t s = 0; s < symbolHook->symbolsCount; s++) {
                auto hookSymbol =
                    std::wstring_view(symbolHook->symbols[s].string,
                                      symbolHook->symbols[s].length);
                m_newSystemCacheStr += m_cacheSep;
                m_newSystemCacheStr += hookSymbol;
                m_newSystemCacheStr += m_cacheSep;
            }

            return true;  // Mark for removal.
        });
    }

    bool IsTargetModuleHybrid() const { return m_isHybridModule; }

    const std::wstring& GetCacheStrKey() const { return m_cacheStrKey; }

    const std::wstring& GetTargetModuleFileName() const {
        return m_moduleFileName;
    }

    bool AreAllSymbolsResolved() const {
        return m_symbolHooksUnresolved.empty();
    }

    void ApplyPendingHooks() {
        VERBOSE(L"Applying hooks");

        for (const auto& hook : m_pendingHooks) {
            m_loadedMod->SetFunctionHook(hook.targetFunction, hook.hookFunction,
                                         hook.originalFunction);
        }

        m_pendingHooks.clear();
    }

   private:
    void CalculateHookSymbolsInitialParams() {
        HMODULE module = m_module;

        std::filesystem::path modulePath =
            wil::GetModuleFileName<std::wstring>(module);
        auto moduleFileName = modulePath.filename().wstring();
        LCMapStringEx(
            LOCALE_NAME_USER_DEFAULT, LCMAP_LOWERCASE, &moduleFileName[0],
            wil::safe_cast<int>(moduleFileName.length()), &moduleFileName[0],
            wil::safe_cast<int>(moduleFileName.length()), nullptr, nullptr, 0);

        VERBOSE(L"Module: %p", module);
        VERBOSE(L"Path: %s", modulePath.c_str());
        VERBOSE(L"Version: %S", Functions::GetModuleVersion(module).c_str());

        IMAGE_DOS_HEADER* dosHeader = (IMAGE_DOS_HEADER*)module;
        IMAGE_NT_HEADERS* ntHeader =
            (IMAGE_NT_HEADERS*)((BYTE*)dosHeader + dosHeader->e_lfanew);
        auto timeStamp = std::to_wstring(ntHeader->FileHeader.TimeDateStamp);
        auto imageSize = std::to_wstring(ntHeader->OptionalHeader.SizeOfImage);

        bool isHybridModule = IsHybridModule(dosHeader, ntHeader);

        std::wstring cacheStrKey;

        constexpr WCHAR currentArch[] =
#if defined(_M_IX86)
            L"x86";
#elif defined(_M_X64)
            L"x86-64";
#elif defined(_M_ARM64)
            L"arm64";
#else
#error "Unsupported architecture"
#endif

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
            if (isHybridModule) {
                cacheStrKey += L"_hybrid-";
                cacheStrKey += currentArch;
            }
        } else {
            cacheStrKey = L"pe_";
            cacheStrKey += currentArch;
            cacheStrKey += L'_';
            cacheStrKey += timeStamp;
            cacheStrKey += L'_';
            cacheStrKey += imageSize;
            cacheStrKey += L'_';
            cacheStrKey += moduleFileName;
            if (isHybridModule) {
                cacheStrKey += L"_hybrid";
            }
        }

        m_isHybridModule = isHybridModule;
        m_cacheSep = isHybridModule ? L';' : L'#';
        m_moduleFileName = std::move(moduleFileName);
        m_timeStamp = std::move(timeStamp);
        m_imageSize = std::move(imageSize);
        m_cacheStrKey = std::move(cacheStrKey);

        m_newSystemCacheStr = kCacheVer;
        m_newSystemCacheStr += m_cacheSep;
        m_newSystemCacheStr += Functions::ReplaceAll(
            m_moduleFileName, std::wstring_view(&m_cacheSep, 1), L"%sep%");
        m_newSystemCacheStr += m_cacheSep;
        m_newSystemCacheStr += m_timeStamp;
        m_newSystemCacheStr += L'-';
        m_newSystemCacheStr += m_imageSize;
    }

    struct PendingHook {
        void* targetFunction;
        void* hookFunction;
        void** originalFunction;
    };

    static constexpr WCHAR kCacheVer = L'1';
    static constexpr std::wstring_view kErrorCachePrefix = L"error:"sv;
    static constexpr WCHAR kErrorCacheVer = L'1';
    static constexpr WCHAR kErrorCacheSep = L'#';

    LoadedMod* m_loadedMod;
    HMODULE m_module;
    bool m_isHybridModule;
    WCHAR m_cacheSep;
    std::wstring m_moduleFileName;
    std::wstring m_timeStamp;
    std::wstring m_imageSize;
    std::wstring m_cacheStrKey;
    std::wstring m_newSystemCacheStr;
    std::vector<const WH_SYMBOL_HOOK*> m_symbolHooksUnresolved;
    std::vector<PendingHook> m_pendingHooks;
};

std::wstring GetWindowsVersionForLogging() {
    static const std::wstring result = []() {
        ULONG majorVersion = 0;
        ULONG minorVersion = 0;
        ULONG buildNumber = 0;
        Functions::GetNtVersionNumbers(&majorVersion, &minorVersion,
                                       &buildNumber);

        WCHAR buildNumberReg[32] = L"";
        DWORD buildNumberRegSize = sizeof(buildNumberReg);
        RegGetValue(HKEY_LOCAL_MACHINE,
                    L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
                    L"CurrentBuild", RRF_RT_REG_SZ, nullptr, buildNumberReg,
                    &buildNumberRegSize);

        DWORD buildRevisionReg = 0;
        DWORD buildRevisionRegSize = sizeof(buildRevisionReg);
        RegGetValue(HKEY_LOCAL_MACHINE,
                    L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", L"UBR",
                    RRF_RT_REG_DWORD, nullptr, &buildRevisionReg,
                    &buildRevisionRegSize);

        std::wstring result;
        result += std::to_wstring(majorVersion);
        result += L'.';
        result += std::to_wstring(minorVersion);
        result += L'.';
        result += std::to_wstring(buildNumber);
        result += L" (";
        result += buildNumberReg;
        result += L'.';
        result += std::to_wstring(buildRevisionReg);
        result += L')';

        return result;
    }();

    return result;
}

}  // namespace

LoadedMod::LoadedMod(PCWSTR modName,
                     PCWSTR modInstanceId,
                     PCWSTR libraryPath,
                     bool loadedOnStartup,
                     bool loggingEnabled,
                     bool debugLoggingEnabled)
    : m_modName(modName),
      m_modInstanceId(modInstanceId),
      m_loadedOnStartup(loadedOnStartup),
      m_loggingEnabled(loggingEnabled),
      m_debugLoggingEnabled(debugLoggingEnabled),
      m_compatDemangling(ShouldUseCompatDemangling(m_modName)) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    VERBOSE(L"Windows %s", GetWindowsVersionForLogging().c_str());
#if defined(_M_IX86)
#define WINDHAWK_ARCH L"x86"
#elif defined(_M_X64)
#define WINDHAWK_ARCH L"x86-64"
#elif defined(_M_ARM64)
#define WINDHAWK_ARCH L"ARM64"
#else
#error "Unsupported architecture"
#endif
    VERBOSE(L"Windhawk v" VER_FILE_VERSION_WSTR L" " WINDHAWK_ARCH);
#undef WINDHAWK_ARCH
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
        m_modShimLibrary = SetModShimsLibraryIfNeeded(m_modModule.get());
    } catch (const std::exception& e) {
        LOG(L"Mod %s: %S", m_modName.c_str(), e.what());
    }
}

LoadedMod::~LoadedMod() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

#ifdef WH_HOOKING_ENGINE_MINHOOK
    MH_STATUS status =
        MH_RemoveHookEx(reinterpret_cast<ULONG_PTR>(this), MH_ALL_HOOKS);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_RemoveHookEx returned %d", m_modName.c_str(),
            status);
    }
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
// For testing without a hooking engine.
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE
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

    SetTask(L"Uninitializing...");

    using WH_MOD_BEFORE_UNINIT_T = void(__cdecl*)();
    auto pWH_ModBeforeUninit = reinterpret_cast<WH_MOD_BEFORE_UNINIT_T>(
        GetProcAddress(m_modModule.get(), "_Z18Wh_ModBeforeUninitv"));
    if (pWH_ModBeforeUninit) {
        pWH_ModBeforeUninit();
    }

    m_uninitializing = true;

#ifdef WH_HOOKING_ENGINE_MINHOOK
    MH_STATUS status =
        MH_QueueDisableHookEx(reinterpret_cast<ULONG_PTR>(this), MH_ALL_HOOKS);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_QueueDisableHookEx returned %d",
            m_modName.c_str(), status);
    }
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
// For testing without a hooking engine.
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE
}

void LoadedMod::Uninitialize() {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    using WH_MOD_UNINIT_T = void(__cdecl*)();
    auto pWH_ModUninit = reinterpret_cast<WH_MOD_UNINIT_T>(
        GetProcAddress(m_modModule.get(), "_Z12Wh_ModUninitv"));
    if (pWH_ModUninit) {
        pWH_ModUninit();
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

PCWSTR LoadedMod::GetModName() {
    return m_modName.c_str();
}

HMODULE LoadedMod::GetModModuleHandle() {
    return m_modModule.get();
}

BOOL LoadedMod::IsLogEnabled() {
    return m_loggingEnabled || m_debugLoggingEnabled;
}

void LoadedMod::Log(PCWSTR format, va_list args) {
    va_list argsCopy;
    va_copy(argsCopy, args);  // https://stackoverflow.com/q/55274350
    WCHAR logFormatted[1025];
    _vsnwprintf_s(logFormatted, _TRUNCATE, format, args);
    va_end(argsCopy);

    Logger::GetInstance().LogLine(L"[WH] [%s] %s\n", m_modName.c_str(),
                                  logFormatted);
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

size_t LoadedMod::GetModStoragePath(PWSTR pathBuffer, size_t bufferChars) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();

    if (bufferChars == 0) {
        return 0;
    }

    try {
        auto modStoragePath =
            StorageManager::GetInstance().GetModStoragePath(m_modName.c_str());
        const auto& value = modStoragePath.native();
        if (value.length() <= bufferChars - 1) {
            wcscpy_s(pathBuffer, bufferChars, value.c_str());
            VERBOSE(L"value: %s", value.c_str());
            return value.length();
        } else {
            LOG(L"Buffer size too small: %zu < %zu",
                bufferChars - 1 < value.length());
        }
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    pathBuffer[0] = L'\0';
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

#ifdef WH_HOOKING_ENGINE_MINHOOK
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
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
    // For testing without a hooking engine.
    LOG(L"Mod %s error: No hooking engine", m_modName.c_str());
    return FALSE;
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE
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

#ifdef WH_HOOKING_ENGINE_MINHOOK
    MH_STATUS status = MH_QueueDisableHookEx(reinterpret_cast<ULONG_PTR>(this),
                                             targetFunction);
    if (status != MH_OK) {
        LOG(L"Mod %s error: MH_QueueDisableHookEx returned %d",
            m_modName.c_str(), status);
        return FALSE;
    }

    return TRUE;
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
    // For testing without a hooking engine.
    LOG(L"Mod %s error: No hooking engine", m_modName.c_str());
    return FALSE;
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE
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

#ifdef WH_HOOKING_ENGINE_MINHOOK
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
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
    // For testing without a hooking engine.
    LOG(L"Mod %s error: No hooking engine", m_modName.c_str());
    return FALSE;
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE
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

    if (options && options->optionsSize != sizeof(WH_FIND_SYMBOL_OPTIONS)) {
        struct WH_FIND_SYMBOL_OPTIONS_V1 {
            size_t optionsSize;
            PCWSTR symbolServer;
            BOOL noUndecoratedSymbols;
        };
        static_assert(
            sizeof(WH_FIND_SYMBOL_OPTIONS) == sizeof(WH_FIND_SYMBOL_OPTIONS_V1),
            "Struct was updated, update this code too");

        LOG(L"Unsupported options->optionsSize value: %zu",
            options->optionsSize);
        return nullptr;
    }

    try {
        HMODULE moduleBase = hModule;
        if (!moduleBase) {
            moduleBase = GetModuleHandle(nullptr);
        }

        std::filesystem::path modulePath =
            wil::GetModuleFileName<std::wstring>(moduleBase);

        VERBOSE(L"Module: %p%s", moduleBase, !hModule ? L" (main)" : L"");
        VERBOSE(L"Path: %s", modulePath.c_str());
        VERBOSE(L"Version: %S",
                Functions::GetModuleVersion(moduleBase).c_str());

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
            std::optional<CrossModMutex> symbolLoadLock;

            GUID pdbGuid;
            DWORD pdbAge;
            if (Functions::ModuleGetPDBInfo(moduleBase, &pdbGuid, &pdbAge)) {
                constexpr size_t kMaxPdbIdentifierLength =
                    sizeof("AAAAAAAABBBBCCCCDDDDEEEEEEEEEEEE12345678") - 1;
                WCHAR mutexIdentifier[sizeof("SymbolLoadLockMutex-") - 1 +
                                      kMaxPdbIdentifierLength + 1];
                swprintf_s(
                    mutexIdentifier,
                    L"SymbolLoadLockMutex-%08X%04X%04X%02X%02X%02X%02X%02X%"
                    L"02X%02X%02X%x",
                    pdbGuid.Data1, pdbGuid.Data2, pdbGuid.Data3,
                    pdbGuid.Data4[0], pdbGuid.Data4[1], pdbGuid.Data4[2],
                    pdbGuid.Data4[3], pdbGuid.Data4[4], pdbGuid.Data4[5],
                    pdbGuid.Data4[6], pdbGuid.Data4[7], pdbAge);

                symbolLoadLock.emplace(mutexIdentifier);
                if (!*symbolLoadLock) {
                    symbolLoadLock.reset();
                }
            }

            // If lock is not acquired, try loading a local symbol file first.
            // If it fails, wait for the lock before proceeding with the symbol
            // server to avoid multiple processes downloading the same file.
            if (symbolLoadLock &&
                !symbolLoadLock->Acquire(/*milliseconds=*/0)) {
                try {
                    // Try loading a local symbol file first.
                    symbolEnum = std::make_unique<SymbolEnum>(
                        modulePath.c_str(), hModule, L"", undecorateMode);
                } catch (const std::exception& e) {
                    VERBOSE(L"Failed to load local symbol file: %S", e.what());

                    SetTask((L"Waiting for symbols... (" + moduleName + L")")
                                .c_str());

                    symbolLoadLock->Acquire();

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
        findData->symbol =
            symbol->nameUndecorated ? symbol->nameUndecorated : L"";
        findData->symbolDecorated = symbol->name ? symbol->name : L"";

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

    struct WH_HOOK_SYMBOLS_OPTIONS_CURRENT {
        size_t optionsSize;
        PCWSTR symbolServer;
        BOOL noUndecoratedSymbols;
        PCWSTR onlineCacheUrl;
    };
    static_assert(sizeof(WH_HOOK_SYMBOLS_OPTIONS) ==
                      sizeof(WH_HOOK_SYMBOLS_OPTIONS_CURRENT),
                  "Struct was updated, update this code too");

    struct WH_HOOK_SYMBOLS_OPTIONS_V1 {
        size_t optionsSize;
        PCWSTR symbolServer;
        BOOL noUndecoratedSymbols;
    };

    WH_HOOK_SYMBOLS_OPTIONS optionsResolved;

    switch (options ? options->optionsSize : 0) {
        case sizeof(WH_HOOK_SYMBOLS_OPTIONS):
            optionsResolved = *options;
            break;

        case sizeof(WH_HOOK_SYMBOLS_OPTIONS_V1): {
            const WH_HOOK_SYMBOLS_OPTIONS_V1* optionsV1 =
                reinterpret_cast<const WH_HOOK_SYMBOLS_OPTIONS_V1*>(options);
            optionsResolved = {
                .optionsSize = sizeof(optionsResolved),
                .symbolServer = optionsV1->symbolServer,
                .noUndecoratedSymbols = optionsV1->noUndecoratedSymbols,
            };
            break;
        }

        case 0:
            optionsResolved = {
                .optionsSize = sizeof(optionsResolved),
            };
            break;

        default:
            LOG(L"Unsupported options->optionsSize value: %zu",
                options->optionsSize);
            return FALSE;
    }

    if (!module) {
        LOG(L"Module handle is null");
        return FALSE;
    }

    if (symbolHooksCount == 0) {
        return TRUE;
    }

    if (!symbolHooks) {
        LOG(L"symbolHooks is null");
        return FALSE;
    }

    std::optional<CrossModMutex> symbolLoadLock;

    try {
        auto hookSymbolsSession =
            HookSymbolsSession(this, module, symbolHooks, symbolHooksCount);

#if !defined(_M_ARM64)
        if (hookSymbolsSession.IsTargetModuleHybrid()) {
            auto settings = StorageManager::GetInstance().GetModWritableConfig(
                m_modName.c_str(), L"LocalStorage", false);

            // By default, hybrid modules are not supported on non-ARM64
            // architectures. A large amount of the code in such modules is
            // ARM64EC, which is not supported by the hooking engine.
            //
            // Currently, as a temporary escape hatch, a mod can set the value
            // below to customize this behavior.
            switch (
                settings->GetInt(L"hook_symbols_non_arm64_hybrid_modules_mode")
                    .value_or(0)) {
                default:
                    LOG(L"Hybrid modules are currently only supported on "
                        L"ARM64");
                    return FALSE;

                case 1:
                    // Proceed as usual.
                    break;

                case 2:
                    // Do nothing but return TRUE, can be useful for mods which
                    // can provide partial functionality without the symbol
                    // hooks.
                    return TRUE;
            }
        }
#endif

        // Disable throttling when logging is enabled, to help with
        // troubleshooting.
        //
        // If the mod is loaded on startup, allow cached errors for throttling
        // to prevent the mod from trying to load symbols again and again for
        // each newly created process in case of an error.
        //
        // If the mod is loaded later, allow cached errors for throttling for a
        // shorter time, so that the mod can try to load symbols again after
        // disabling and re-enabling the mod.
        std::optional<ULONGLONG> cachedErrorForThrottleMaxTime =
            (m_loggingEnabled || m_debugLoggingEnabled)
                ? std::nullopt
                : std::make_optional(m_loadedOnStartup
                                         ? 4 * wil::filetime_duration::one_hour
                                         : wil::filetime_duration::one_minute);

        switch (hookSymbolsSession.ResolveSymbolsFromCache(
            cachedErrorForThrottleMaxTime)) {
            case HookSymbolsSession::ResolveSymbolsFromCacheResult::kSuccess:
                if (hookSymbolsSession.AreAllSymbolsResolved()) {
                    hookSymbolsSession.ApplyPendingHooks();
                    return TRUE;
                }
                break;

            case HookSymbolsSession::ResolveSymbolsFromCacheResult::
                kCachedErrorForThrottle:
                VERBOSE(L"Returning FALSE due to a previous failure");
                return FALSE;
        }

        VERBOSE(L"Couldn't resolve all symbols from local cache");

        SetTask((L"Waiting for symbols... (" +
                 hookSymbolsSession.GetTargetModuleFileName() + L")")
                    .c_str());

        auto activityStatusCleanup = wil::scope_exit(
            [this] { SetTask(m_initialized ? nullptr : L"Initializing..."); });

        std::wstring mutexIdentieir = L"SymbolGetOnlineCacheMutex_";
        mutexIdentieir += m_modName;
        mutexIdentieir += L'_';
        mutexIdentieir += hookSymbolsSession.GetCacheStrKey();

        // At this point, if the mod is loaded into multiple processes, all of
        // them will try to use the online cache. Use a cross-mod mutex, and
        // hopefully the first mod to acquire it will get and store the online
        // cache. Then, the other processes will be able to use it without
        // having to go online too.
        symbolLoadLock.emplace(mutexIdentieir.c_str());
        if (!*symbolLoadLock || !symbolLoadLock->Acquire()) {
            symbolLoadLock.reset();
        }

        SetTask((L"Loading symbols... (" +
                 hookSymbolsSession.GetTargetModuleFileName() + L")")
                    .c_str());

        if (symbolLoadLock) {
            // Retry resolving symbols from cache after acquiring the lock.
            switch (hookSymbolsSession.ResolveSymbolsFromCache(
                cachedErrorForThrottleMaxTime)) {
                case HookSymbolsSession::ResolveSymbolsFromCacheResult::
                    kSuccess:
                    if (hookSymbolsSession.AreAllSymbolsResolved()) {
                        hookSymbolsSession.ApplyPendingHooks();
                        return TRUE;
                    }
                    break;

                case HookSymbolsSession::ResolveSymbolsFromCacheResult::
                    kCachedErrorForThrottle:
                    VERBOSE(L"Returning FALSE due to a previous failure");
                    return FALSE;
            }
        } else {
            LOG(L"Couldn't acquire the symbol load lock");
        }

        auto scopeUpdateSymbolsCacheWithErrorForThrottle =
            wil::scope_exit([&hookSymbolsSession]() {
                hookSymbolsSession.UpdateSymbolsCacheWithErrorForThrottle();
            });

        auto applyHooksAndUpdateCache =
            [&hookSymbolsSession,
             &scopeUpdateSymbolsCacheWithErrorForThrottle]() {
                hookSymbolsSession.ApplyPendingHooks();
                hookSymbolsSession.UpdateSymbolsCache();
                scopeUpdateSymbolsCacheWithErrorForThrottle.release();
            };

        auto onlineCache =
            HookSymbolsGetOnlineCache(optionsResolved.onlineCacheUrl,
                                      hookSymbolsSession.GetCacheStrKey())
                .value_or(L"");
        if (!onlineCache.empty()) {
            const auto& cacheStrKey = hookSymbolsSession.GetCacheStrKey();
            VERBOSE(
                L"Using online symbol cache %.*s: %.*s",
                wil::safe_cast<int>(cacheStrKey.length()), cacheStrKey.data(),
                wil::safe_cast<int>(onlineCache.length()), onlineCache.data());

            hookSymbolsSession.ResolveSymbolsFromCacheString(onlineCache);
            if (hookSymbolsSession.AreAllSymbolsResolved()) {
                applyHooksAndUpdateCache();
                return TRUE;
            }
        }

        VERBOSE(L"Couldn't resolve all symbols from online cache");

        WH_FIND_SYMBOL findSymbol;
        WH_FIND_SYMBOL_OPTIONS findFirstSymbolOptions = {
            .optionsSize = sizeof(findFirstSymbolOptions),
            .symbolServer = optionsResolved.symbolServer,
            .noUndecoratedSymbols = optionsResolved.noUndecoratedSymbols,
        };
        HANDLE findSymbolHandle =
            FindFirstSymbol4(module, &findFirstSymbolOptions, &findSymbol);
        if (!findSymbolHandle) {
            return FALSE;
        }

        // Prefer closing the handle on function exit, not earlier. Closing the
        // handle unloads the MSDIA library, and that was observed to cause
        // hangs if Application Verifier is used. Example:
        // https://github.com/ramensoftware/windhawk-mods/issues/920
        // By closing the handle on function exit, at least the symbol offsets
        // will be written to cache, so symbols will just be loaded from cache
        // on the next try.
        auto findSymbolHandleScopeClose = wil::scope_exit(
            [this, findSymbolHandle]() { FindCloseSymbol(findSymbolHandle); });

        do {
            PCWSTR symbol = optionsResolved.noUndecoratedSymbols
                                ? findSymbol.symbolDecorated
                                : findSymbol.symbol;
            if (!symbol || !hookSymbolsSession.OnSymbolResolved(
                               symbol, findSymbol.address)) {
                continue;
            }

            if (hookSymbolsSession.AreAllSymbolsResolved()) {
                break;
            }
        } while (FindNextSymbol2(findSymbolHandle, &findSymbol));

        if (!hookSymbolsSession.AreAllSymbolsResolved()) {
            hookSymbolsSession.MarkUnresolvedSymbolsAsMissing();
            if (!hookSymbolsSession.AreAllSymbolsResolved()) {
                return FALSE;
            }
        }

        applyHooksAndUpdateCache();
        return TRUE;
    } catch (const std::exception& e) {
        LogFunctionError(e);
    }

    return FALSE;
}

BOOL LoadedMod::Disasm(void* address, WH_DISASM_RESULT* result) {
#if defined(_M_ARM64)
    int rc = aarch64_decompose_and_disassemble(
        reinterpret_cast<ULONG_PTR>(address),
        *reinterpret_cast<DWORD*>(address), result->text, sizeof(result->text));
    if (rc) {
        LOG(L"Mod %s error: aarch64_decompose_and_disassemble returned %d",
            m_modName.c_str(), rc);
        return FALSE;
    }

    result->length = sizeof(DWORD);

    return TRUE;
#else
#if defined(_M_IX86)
    auto machineMode = ZYDIS_MACHINE_MODE_LEGACY_32;
#elif defined(_M_X64)
    auto machineMode = ZYDIS_MACHINE_MODE_LONG_64;
#else
#error "Unsupported architecture"
#endif

    ZydisDisassembledInstruction instruction;
    ZyanStatus status = ZydisDisassembleIntel(
        /* machine_mode:    */ machineMode,
        /* runtime_address: */ (ZyanU64)address,
        /* buffer:          */ address,
        /* length:          */ ZYDIS_MAX_INSTRUCTION_LENGTH,
        /* instruction:     */ &instruction);
    if (!ZYAN_SUCCESS(status)) {
        LOG(L"Mod %s error: ZydisDisassembleIntel returned %u",
            m_modName.c_str(), status);
        return FALSE;
    }

    result->length = instruction.info.length;
    strcpy_s(result->text, instruction.text);

    return TRUE;
#endif  // defined(_M_ARM64)
}

const WH_URL_CONTENT* LoadedMod::GetUrlContent(
    PCWSTR url,
    const WH_GET_URL_CONTENT_OPTIONS* options) {
    auto modDebugLoggingScope = MOD_DEBUG_LOGGING_SCOPE();
    VERBOSE(L"URL: %s", url);
    VERBOSE(L"Target file path: %s", options && options->targetFilePath
                                         ? options->targetFilePath
                                         : L"(none)");

    if (options && options->optionsSize != sizeof(WH_GET_URL_CONTENT_OPTIONS)) {
        struct WH_GET_URL_CONTENT_OPTIONS_V1 {
            size_t optionsSize;
            PCWSTR targetFilePath;
        };
        static_assert(sizeof(WH_GET_URL_CONTENT_OPTIONS) ==
                          sizeof(WH_GET_URL_CONTENT_OPTIONS_V1),
                      "Struct was updated, update this code too");

        LOG(L"Unsupported options->optionsSize value: %zu",
            options->optionsSize);
        return nullptr;
    }

    // Avoid having winhttp.dll in the import table, since it might not be
    // available in all cases, e.g. sandboxed processes.
    using WinHttpCloseHandle_t = decltype(&WinHttpCloseHandle);
    using WinHttpOpen_t = decltype(&WinHttpOpen);
    using WinHttpConnect_t = decltype(&WinHttpConnect);
    using WinHttpQueryHeaders_t = decltype(&WinHttpQueryHeaders);
    using WinHttpReceiveResponse_t = decltype(&WinHttpReceiveResponse);
    using WinHttpSendRequest_t = decltype(&WinHttpSendRequest);
    using WinHttpOpenRequest_t = decltype(&WinHttpOpenRequest);
    using WinHttpQueryDataAvailable_t = decltype(&WinHttpQueryDataAvailable);
    using WinHttpReadData_t = decltype(&WinHttpReadData);
    using WinHttpCrackUrl_t = decltype(&WinHttpCrackUrl);

    class WinHttpFunctions {
       public:
        wil::unique_hmodule module;

        WinHttpCloseHandle_t CloseHandle;
        WinHttpOpen_t Open;
        WinHttpConnect_t Connect;
        WinHttpQueryHeaders_t QueryHeaders;
        WinHttpReceiveResponse_t ReceiveResponse;
        WinHttpSendRequest_t SendRequest;
        WinHttpOpenRequest_t OpenRequest;
        WinHttpQueryDataAvailable_t QueryDataAvailable;
        WinHttpReadData_t ReadData;
        WinHttpCrackUrl_t CrackUrl;

        WinHttpFunctions() {
            wil::unique_hmodule winhttpModule{LoadLibraryEx(
                L"winhttp.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32)};
            if (!winhttpModule) {
                LOG(L"Failed to load winhttp.dll");
                return;
            }

            HMODULE moduleRaw = winhttpModule.get();

            CloseHandle = reinterpret_cast<WinHttpCloseHandle_t>(
                GetProcAddress(moduleRaw, "WinHttpCloseHandle"));
            Open = reinterpret_cast<WinHttpOpen_t>(
                GetProcAddress(moduleRaw, "WinHttpOpen"));
            Connect = reinterpret_cast<WinHttpConnect_t>(
                GetProcAddress(moduleRaw, "WinHttpConnect"));
            QueryHeaders = reinterpret_cast<WinHttpQueryHeaders_t>(
                GetProcAddress(moduleRaw, "WinHttpQueryHeaders"));
            ReceiveResponse = reinterpret_cast<WinHttpReceiveResponse_t>(
                GetProcAddress(moduleRaw, "WinHttpReceiveResponse"));
            SendRequest = reinterpret_cast<WinHttpSendRequest_t>(
                GetProcAddress(moduleRaw, "WinHttpSendRequest"));
            OpenRequest = reinterpret_cast<WinHttpOpenRequest_t>(
                GetProcAddress(moduleRaw, "WinHttpOpenRequest"));
            QueryDataAvailable = reinterpret_cast<WinHttpQueryDataAvailable_t>(
                GetProcAddress(moduleRaw, "WinHttpQueryDataAvailable"));
            ReadData = reinterpret_cast<WinHttpReadData_t>(
                GetProcAddress(moduleRaw, "WinHttpReadData"));
            CrackUrl = reinterpret_cast<WinHttpCrackUrl_t>(
                GetProcAddress(moduleRaw, "WinHttpCrackUrl"));

            if (!CloseHandle || !Open || !Connect || !QueryHeaders ||
                !ReceiveResponse || !SendRequest || !OpenRequest ||
                !QueryDataAvailable || !ReadData || !CrackUrl) {
                LOG(L"Failed to get all winhttp.dll functions");
                return;
            }

            module = std::move(winhttpModule);
        }
    };

    STATIC_INIT_ONCE(WinHttpFunctions, winhttp, );

    if (!winhttp->module) {
        LOG(L"WinHttp functions are not available");
        return nullptr;
    }

    try {
        wil::unique_hfile targetFile;
        PCWSTR targetFilePath = options ? options->targetFilePath : nullptr;
        if (targetFilePath) {
            targetFile.reset(CreateFile(targetFilePath, GENERIC_WRITE,
                                        FILE_SHARE_READ, nullptr, CREATE_ALWAYS,
                                        FILE_ATTRIBUTE_NORMAL, nullptr));
            THROW_LAST_ERROR_IF(!targetFile);
        }

        URL_COMPONENTS urlComp = {sizeof(urlComp)};
        urlComp.dwHostNameLength = (DWORD)-1;
        urlComp.dwUrlPathLength = (DWORD)-1;
        THROW_IF_WIN32_BOOL_FALSE(winhttp->CrackUrl(url, 0, 0, &urlComp));

        HINTERNET session{winhttp->Open(L"Windhawk/" VER_FILE_VERSION_WSTR,
                                        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                                        WINHTTP_NO_PROXY_NAME,
                                        WINHTTP_NO_PROXY_BYPASS, 0)};
        THROW_LAST_ERROR_IF_NULL(session);

        auto sessionCleanup = wil::scope_exit(
            [winhttp, session] { winhttp->CloseHandle(session); });

        HINTERNET connect{winhttp->Connect(
            session,
            std::wstring(urlComp.lpszHostName, urlComp.dwHostNameLength)
                .c_str(),
            urlComp.nPort, 0)};
        THROW_LAST_ERROR_IF_NULL(connect);

        auto connectCleanup = wil::scope_exit(
            [winhttp, connect] { winhttp->CloseHandle(connect); });

        HINTERNET request{winhttp->OpenRequest(
            connect, L"GET",
            std::wstring(urlComp.lpszUrlPath, urlComp.dwUrlPathLength).c_str(),
            nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES,
            urlComp.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE
                                                     : 0)};
        THROW_LAST_ERROR_IF_NULL(request);

        auto requestCleanup = wil::scope_exit(
            [winhttp, request] { winhttp->CloseHandle(request); });

        THROW_IF_WIN32_BOOL_FALSE(
            winhttp->SendRequest(request, WINHTTP_NO_ADDITIONAL_HEADERS, 0,
                                 WINHTTP_NO_REQUEST_DATA, 0, 0, 0));

        THROW_IF_WIN32_BOOL_FALSE(winhttp->ReceiveResponse(request, nullptr));

        DWORD statusCode = 0;
        DWORD statusCodeSize = sizeof(statusCode);
        THROW_IF_WIN32_BOOL_FALSE(winhttp->QueryHeaders(
            request, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            WINHTTP_HEADER_NAME_BY_INDEX, &statusCode, &statusCodeSize,
            WINHTTP_NO_HEADER_INDEX));

        auto content = std::make_unique<WH_URL_CONTENT>();
        content->statusCode = statusCode;

        std::string chunk;
        std::vector<std::string> chunks;
        DWORD downloaded = 0;
        size_t downloadedTotal = 0;
        do {
            DWORD size = 0;
            THROW_IF_WIN32_BOOL_FALSE(
                winhttp->QueryDataAvailable(request, &size));

            if (size == 0) {
                break;
            }

            chunk.resize(size);
            THROW_IF_WIN32_BOOL_FALSE(winhttp->ReadData(
                request, (PVOID)chunk.data(), size, &downloaded));

            if (targetFile) {
                DWORD written = 0;
                THROW_IF_WIN32_BOOL_FALSE(WriteFile(targetFile.get(),
                                                    chunk.data(), downloaded,
                                                    &written, nullptr));
                THROW_WIN32_IF(ERROR_WRITE_FAULT, written != downloaded);
            } else {
                chunk.resize(downloaded);
                chunks.push_back(std::move(chunk));
                chunk.clear();
            }

            downloadedTotal += downloaded;
        } while (downloaded > 0);

        if (targetFile) {
            content->data = nullptr;
        } else {
            auto data = std::make_unique<char[]>(downloadedTotal + 1);
            size_t dataIter = 0;
            for (const auto& chunk : chunks) {
                std::copy(chunk.begin(), chunk.end(), data.get() + dataIter);
                dataIter += chunk.size();
            }
            data[dataIter] = '\0';
            content->data = data.release();
        }

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

std::optional<std::wstring> LoadedMod::HookSymbolsGetOnlineCache(
    PCWSTR onlineCacheBaseUrl,
    std::wstring_view cacheStrKey) {
    std::wstring onlineCacheUrl;
    if (onlineCacheBaseUrl) {
        onlineCacheUrl = onlineCacheBaseUrl;
        if (!onlineCacheUrl.empty() && onlineCacheUrl.back() != L'/') {
            onlineCacheUrl += L'/';
        }
    } else if (!m_modName.starts_with(L"local@")) {
        onlineCacheUrl =
            L"https://ramensoftware.github.io/windhawk-mod-symbol-cache/";
        onlineCacheUrl += m_modName;
        onlineCacheUrl += L'/';
    }

    if (onlineCacheUrl.empty()) {
        VERBOSE(L"Skipping online symbol cache");
        return std::wstring();
    }

    onlineCacheUrl += cacheStrKey;
    onlineCacheUrl += L".txt";

    // Keep trying shortly after launch in case it takes some time for internet
    // connectivity to be established on startup.
    ULONGLONG sessionCreationTime = wil::filetime::to_int64(
        CustomizationSession::GetSessionManagerProcessCreationTime());

    for (int i = 0;; i++) {
        if (i > 0) {
            ULONGLONG now =
                wil::filetime::to_int64(wil::filetime::get_system_time());
            if (now < sessionCreationTime ||
                now > sessionCreationTime +
                          wil::filetime_duration::one_second * 30) {
                break;
            }

            // In case the mod was disabled, abort.
            if (!Mod::ShouldLoadInRunningProcess(m_modName.c_str()) ||
                CustomizationSession::IsEndingSoon()) {
                VERBOSE(L"Aborting getting online symbol cache");
                return std::nullopt;
            }

            Sleep(5000);

            VERBOSE(L"Getting online symbol cache (attempt %d)", i + 1);
        } else {
            VERBOSE(L"Getting online symbol cache");
        }

        const WH_URL_CONTENT* onlineCacheUrlContent =
            GetUrlContent(onlineCacheUrl.c_str(), nullptr);
        if (!onlineCacheUrlContent) {
            LOG(L"Couldn't contact the online cache server");
            continue;
        }

        auto scopeFreeUrlContent =
            wil::scope_exit([this, onlineCacheUrlContent]() {
                FreeUrlContent(onlineCacheUrlContent);
            });

        if (onlineCacheUrlContent->statusCode == 200) {
            return std::wstring(
                onlineCacheUrlContent->data,
                onlineCacheUrlContent->data + onlineCacheUrlContent->length);
        }

        if (onlineCacheUrlContent->statusCode == 404) {
            VERBOSE(L"Online cache not found");
            return std::wstring();
        }

        LOG(L"Online cache server returned status %d",
            onlineCacheUrlContent->statusCode);
    }

    VERBOSE(
        L"In case of a firewall, you can open cmd manually and run the "
        L"following command, then disable and re-enable the mod:");
    VERBOSE(
        LR"(for /f "delims=" %%I in ('curl -f %s') do @reg add "HKLM\SOFTWARE\Windhawk\Engine\ModsWritable\%s\SymbolCache" /t REG_SZ /v %.*s /d "%%I" /f)",
        onlineCacheUrl.c_str(), m_modName.c_str(),
        wil::safe_cast<int>(cacheStrKey.length()), cacheStrKey.data());

    return std::nullopt;
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

bool Mod::Load(bool loadedOnStartup) {
    if (m_loadedMod) {
        throw std::logic_error("Already loaded");
    }

    if (IsModBanned(m_modName)) {
        return false;
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
        loadedOnStartup, loggingEnabled, debugLoggingEnabled);

    SetStatus(L"Loading...");

    if (!m_loadedMod->Initialize()) {
        m_loadedMod.reset();
        return false;
    }

    return true;
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

void Mod::Uninitialize() {
    if (m_loadedMod) {
        m_loadedMod->Uninitialize();
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

HMODULE Mod::GetLoadedModModuleHandle() {
    return m_loadedMod ? m_loadedMod->GetModModuleHandle() : nullptr;
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

    bool patternsMatchCriticalSystemProcesses =
        settings->GetInt(L"PatternsMatchCriticalSystemProcesses").value_or(0);

    std::wstring processPath = wil::GetModuleFileName<std::wstring>();

    bool includeExcludeCustomOnly =
        settings->GetInt(L"IncludeExcludeCustomOnly").value_or(0);

    bool matchPatternExplicitOnly =
        !patternsMatchCriticalSystemProcesses &&
        (Functions::DoesPathMatchPattern(processPath,
                                         ProcessLists::kCriticalProcesses) ||
         Functions::DoesPathMatchPattern(
             processPath, ProcessLists::kCriticalProcessesForMods));

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
