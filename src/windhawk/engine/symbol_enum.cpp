#include "stdafx.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"
#include "symbol_enum.h"
#include "var_init_once.h"

void MySysFreeString(BSTR bstrString) {
    // Avoid having oleaut32.dll in the import table, since it might not be
    // available in all cases, e.g. sandboxed processes.
    using SysFreeString_t = decltype(&SysFreeString);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        SysFreeString_t, pSysFreeString, L"oleaut32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "SysFreeString");

    if (!pSysFreeString) {
        LOG(L"Failed to get SysFreeString, skipping");
        return;
    }

    pSysFreeString(bstrString);
}

namespace {

ThreadLocal<SymbolEnum::Callbacks*> g_symbolServerCallbacks;

std::wstring GetSymbolsSearchPath(PCWSTR symbolServer) {
    PCWSTR defaultSymbolServer = L"https://msdl.microsoft.com/download/symbols";

    std::wstring symSearchPath = L"srv*";
    symSearchPath += StorageManager::GetInstance().GetSymbolsPath();
    symSearchPath += L'*';
    symSearchPath += symbolServer ? symbolServer : defaultSymbolServer;

    return symSearchPath;
}

void LogSymbolServerEvent(PCSTR msg) {
    // Trim leading and trailing whitespace and control characters (mainly \b
    // which is used for console output).

    PCSTR p = msg;
    while (*p != '\0' && (isspace(*p) || iscntrl(*p))) {
        p++;
    }

    if (*p == '\0') {
        return;
    }

    size_t len = strlen(p);
    while (len > 0 && (isspace(p[len - 1]) || iscntrl(p[len - 1]))) {
        len--;
    }

    VERBOSE(L"%.*S", wil::safe_cast<int>(len), p);
}

int PercentFromSymbolServerEvent(PCSTR msg) {
    size_t msgLen = strlen(msg);
    while (msgLen > 0 && isspace(msg[msgLen - 1])) {
        msgLen--;
    }

    constexpr char suffix[] = " percent";
    constexpr size_t suffixLen = ARRAYSIZE(suffix) - 1;

    if (msgLen <= suffixLen ||
        strncmp(suffix, msg + msgLen - suffixLen, suffixLen) != 0) {
        return -1;
    }

    char percentStr[] = "000";
    int digitsCount = 0;

    for (size_t i = 1; i <= 3; i++) {
        if (i > msgLen - suffixLen) {
            break;
        }

        char p = msg[msgLen - suffixLen - i];
        if (p < '0' || p > '9') {
            break;
        }

        percentStr[3 - i] = p;
        digitsCount++;
    }

    if (digitsCount == 0) {
        return -1;
    }

    int percent = (percentStr[0] - '0') * 100 + (percentStr[1] - '0') * 10 +
                  (percentStr[2] - '0');
    if (percent > 100) {
        return -1;
    }

    return percent;
}

BOOL CALLBACK SymbolServerCallback(UINT_PTR action,
                                   ULONG64 data,
                                   ULONG64 context) {
    SymbolEnum::Callbacks* callbacks = g_symbolServerCallbacks;
    if (!callbacks) {
        return FALSE;
    }

    switch (action) {
        case SSRVACTION_QUERYCANCEL: {
            if (callbacks->queryCancel) {
                ULONG64* doCancel = (ULONG64*)data;
                *doCancel = callbacks->queryCancel();
                return TRUE;
            }
            return FALSE;
        }

        case SSRVACTION_EVENT: {
            IMAGEHLP_CBA_EVENT* evt = (IMAGEHLP_CBA_EVENT*)data;
            LogSymbolServerEvent(evt->desc);
            int percent = PercentFromSymbolServerEvent(evt->desc);
            if (percent >= 0 && callbacks->notifyProgress) {
                callbacks->notifyProgress(percent);
            }
            return TRUE;
        }
    }

    return FALSE;
}

struct DiaLoadCallback : public IDiaLoadCallback2 {
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid,
                                             void** ppvObject) override {
        if (riid == __uuidof(IUnknown) || riid == __uuidof(IDiaLoadCallback)) {
            *ppvObject = static_cast<IDiaLoadCallback*>(this);
            return S_OK;
        } else if (riid == __uuidof(IDiaLoadCallback2)) {
            *ppvObject = static_cast<IDiaLoadCallback2*>(this);
            return S_OK;
        }
        return E_NOINTERFACE;
    }

    ULONG STDMETHODCALLTYPE AddRef() override {
        return 2;  // On stack
    }

    ULONG STDMETHODCALLTYPE Release() override {
        return 1;  // On stack
    }

    HRESULT STDMETHODCALLTYPE NotifyDebugDir(BOOL fExecutable,
                                             DWORD cbData,
                                             BYTE* pbData) override {
        // VERBOSE(L"Debug directory found in %s file",
        //         fExecutable ? L"exe" : L"dbg");
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE NotifyOpenDBG(LPCOLESTR dbgPath,
                                            HRESULT resultCode) override {
        VERBOSE(L"Opened dbg file %s: %s (%08X)", dbgPath,
                resultCode == S_OK ? L"success" : L"error", resultCode);
        return S_OK;
    }

    HRESULT STDMETHODCALLTYPE NotifyOpenPDB(LPCOLESTR pdbPath,
                                            HRESULT resultCode) override {
        VERBOSE(L"Opened pdb file %s: %s (%08X)", pdbPath,
                resultCode == S_OK ? L"success" : L"error", resultCode);
        return S_OK;
    }

    // Only use explicitly specified search paths, restrict all but symbol
    // server access:
    HRESULT STDMETHODCALLTYPE RestrictRegistryAccess() override {
        return E_FAIL;
    }
    HRESULT STDMETHODCALLTYPE RestrictSymbolServerAccess() override {
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE RestrictOriginalPathAccess() override {
        return E_FAIL;
    }
    HRESULT STDMETHODCALLTYPE RestrictReferencePathAccess() override {
        return E_FAIL;
    }
    HRESULT STDMETHODCALLTYPE RestrictDBGAccess() override { return E_FAIL; }
    HRESULT STDMETHODCALLTYPE RestrictSystemRootAccess() override {
        return E_FAIL;
    }
};

HMODULE WINAPI MsdiaLoadLibraryExWHook(LPCWSTR lpLibFileName,
                                       HANDLE hFile,
                                       DWORD dwFlags) {
    if (wcscmp(lpLibFileName, L"SYMSRV.DLL") != 0) {
        return LoadLibraryExW(lpLibFileName, hFile, dwFlags);
    }

    try {
        auto enginePath = StorageManager::GetInstance().GetEnginePath();

        DWORD dwNewFlags = dwFlags;
        dwNewFlags |= LOAD_WITH_ALTERED_SEARCH_PATH;

        // Strip flags incompatible with LOAD_WITH_ALTERED_SEARCH_PATH.
        dwNewFlags &= ~LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR;
        dwNewFlags &= ~LOAD_LIBRARY_SEARCH_APPLICATION_DIR;
        dwNewFlags &= ~LOAD_LIBRARY_SEARCH_USER_DIRS;
        dwNewFlags &= ~LOAD_LIBRARY_SEARCH_SYSTEM32;
        dwNewFlags &= ~LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;

        auto symsrvPath = enginePath / L"symsrv_windhawk.dll";
        HMODULE symsrvModule =
            LoadLibraryExW(symsrvPath.c_str(), hFile, dwNewFlags);
        if (!symsrvModule) {
            DWORD error = GetLastError();
            LOG(L"Couldn't load symsrv: %u", error);
            SetLastError(error);
            return symsrvModule;
        }

        PSYMBOLSERVERSETOPTIONSPROC pSymbolServerSetOptions =
            reinterpret_cast<PSYMBOLSERVERSETOPTIONSPROC>(
                GetProcAddress(symsrvModule, "SymbolServerSetOptions"));
        if (pSymbolServerSetOptions) {
            pSymbolServerSetOptions(SSRVOPT_UNATTENDED, TRUE);
            pSymbolServerSetOptions(SSRVOPT_CALLBACK,
                                    (ULONG_PTR)SymbolServerCallback);
            pSymbolServerSetOptions(SSRVOPT_TRACE, TRUE);
        } else {
            LOG(L"Couldn't find SymbolServerSetOptions");
        }

        return symsrvModule;
    } catch (const std::exception& e) {
        LOG(L"Couldn't load symsrv: %S", e.what());
        SetLastError(ERROR_MOD_NOT_FOUND);
        return nullptr;
    }
}

template <typename IMAGE_NT_HEADERS_T, typename IMAGE_LOAD_CONFIG_DIRECTORY_T>
std::optional<std::span<const SymbolEnum::IMAGE_CHPE_RANGE_ENTRY>>
GetChpeRanges(const IMAGE_DOS_HEADER* dosHeader,
              const IMAGE_NT_HEADERS_T* ntHeader) {
    auto* opt = &ntHeader->OptionalHeader;

    if (opt->NumberOfRvaAndSizes <= IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG ||
        !opt->DataDirectory[IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG].VirtualAddress) {
        return std::nullopt;
    }

    DWORD directorySize =
        opt->DataDirectory[IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG].Size;

    auto* cfg =
        (const IMAGE_LOAD_CONFIG_DIRECTORY_T*)((const char*)dosHeader +
                                               opt->DataDirectory
                                                   [IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG]
                                                       .VirtualAddress);

    constexpr DWORD kMinSize =
        offsetof(IMAGE_LOAD_CONFIG_DIRECTORY_T, CHPEMetadataPointer) +
        sizeof(IMAGE_LOAD_CONFIG_DIRECTORY_T::CHPEMetadataPointer);

    if (directorySize < kMinSize || cfg->Size < kMinSize) {
        return std::nullopt;
    }

    if (!cfg->CHPEMetadataPointer) {
        return std::nullopt;
    }

    // Either IMAGE_CHPE_METADATA_X86 or IMAGE_ARM64EC_METADATA.
    const void* metadata =
        (const char*)dosHeader + cfg->CHPEMetadataPointer - opt->ImageBase;

    ULONG codeMapRva = ((const ULONG*)metadata)[1];
    ULONG codeMapCount = ((const ULONG*)metadata)[2];

    auto* codeMap =
        (const SymbolEnum::IMAGE_CHPE_RANGE_ENTRY*)((const char*)dosHeader +
                                                    codeMapRva);

    return std::span(codeMap, codeMapCount);
}

}  // namespace

SymbolEnum::SymbolEnum(HMODULE moduleBase,
                       PCWSTR symbolServer,
                       UndecorateMode undecorateMode,
                       Callbacks callbacks) {
    if (!moduleBase) {
        moduleBase = GetModuleHandle(nullptr);
    }

    std::wstring modulePath = wil::GetModuleFileName<std::wstring>(moduleBase);

    SymbolEnum(modulePath.c_str(), moduleBase, symbolServer, undecorateMode,
               std::move(callbacks));
}

SymbolEnum::SymbolEnum(PCWSTR modulePath,
                       HMODULE moduleBase,
                       PCWSTR symbolServer,
                       UndecorateMode undecorateMode,
                       Callbacks callbacks)
    : m_moduleBase(moduleBase), m_undecorateMode(undecorateMode) {
    InitModuleInfo(moduleBase);

    wil::com_ptr<IDiaDataSource> diaSource = LoadMsdia();

    std::wstring symSearchPath = GetSymbolsSearchPath(symbolServer);

    g_symbolServerCallbacks = &callbacks;
    auto msdiaCallbacksCleanup =
        wil::scope_exit([] { g_symbolServerCallbacks = nullptr; });

    DiaLoadCallback diaLoadCallback;
    THROW_IF_FAILED(diaSource->loadDataForExe(modulePath, symSearchPath.c_str(),
                                              &diaLoadCallback));

    wil::com_ptr<IDiaSession> diaSession;
    THROW_IF_FAILED(diaSource->openSession(&diaSession));

    THROW_IF_FAILED(diaSession->get_globalScope(&m_diaGlobal));

    THROW_IF_FAILED(
        m_diaGlobal->findChildren(kSymTags[0], nullptr, nsNone, &m_diaSymbols));
}

std::optional<SymbolEnum::Symbol> SymbolEnum::GetNextSymbol() {
    while (true) {
        wil::com_ptr<IDiaSymbol> diaSymbol;
        ULONG count = 0;
        HRESULT hr = m_diaSymbols->Next(1, &diaSymbol, &count);
        THROW_IF_FAILED(hr);

        if (hr == S_FALSE || count == 0) {
            m_symTagIndex++;
            if (m_symTagIndex < ARRAYSIZE(kSymTags)) {
                THROW_IF_FAILED(m_diaGlobal->findChildren(
                    kSymTags[m_symTagIndex], nullptr, nsNone, &m_diaSymbols));
                continue;
            }

            return std::nullopt;
        }

        DWORD currentSymbolRva;
        hr = diaSymbol->get_relativeVirtualAddress(&currentSymbolRva);
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            continue;  // no RVA
        }

        hr = diaSymbol->get_name(&m_currentSymbolName);
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            m_currentSymbolName.reset();  // no name
        }

        PCWSTR currentSymbolNameUndecoratedPrefix1 = L"";
        PCWSTR currentSymbolNameUndecoratedPrefix2 = L"";

        // Temporary compatibility code.
        if (m_undecorateMode == UndecorateMode::OldVersionCompatible) {
            // get_undecoratedName uses 0x20800 as flags:
            // * UNDNAME_32_BIT_DECODE (0x800)
            // * UNDNAME_NO_PTR64 (0x20000)
            // For some reason, the old msdia version still included ptr64 in
            // the output. For compatibility, use get_undecoratedNameEx and
            // don't pass this flag.
            constexpr DWORD kUndname32BitDecode = 0x800;
            hr = diaSymbol->get_undecoratedNameEx(
                kUndname32BitDecode, &m_currentSymbolNameUndecorated);
        } else if (m_undecorateMode == UndecorateMode::Default) {
            hr =
                diaSymbol->get_undecoratedName(&m_currentSymbolNameUndecorated);
        } else {
            m_currentSymbolNameUndecorated.reset();
            hr = S_OK;
        }
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            m_currentSymbolNameUndecorated.reset();  // no name
        } else if (m_currentSymbolNameUndecorated) {
            // For hybrid binaries, add an arch=x\ prefix.
            if (m_moduleInfo.isHybrid) {
                bool is32Bit =
                    m_moduleInfo.magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC;
                for (const auto& range : m_moduleInfo.chpeRanges) {
                    ULONG start = is32Bit ? (range.StartOffset & ~1)
                                          : (range.StartOffset & ~3);
                    if (currentSymbolRva < start ||
                        currentSymbolRva >= start + range.Length) {
                        continue;
                    }

                    if (is32Bit) {
                        constexpr PCWSTR prefixes[] = {
#if defined(_M_IX86)
                            L"",
#else
                            L"arch=x86\\",
#endif
#if defined(_M_ARM64)
                            L"",
#else
                            L"arch=ARM64\\",
#endif
                        };
                        currentSymbolNameUndecoratedPrefix1 =
                            prefixes[range.StartOffset & 1];
                    } else {
                        constexpr PCWSTR prefixes[] = {
#if defined(_M_ARM64)
                            L"",
#else
                            L"arch=ARM64\\",
#endif
                            L"arch=ARM64EC\\",
#if defined(_M_X64)
                            L"",
#else
                            L"arch=x64\\",
#endif
                            L"arch=3\\",
                        };
                        currentSymbolNameUndecoratedPrefix1 =
                            prefixes[range.StartOffset & 3];
                    }

                    break;
                }
            }

            // For ARM64EC binaries, functions with native and ARM64EC versions
            // have the same undecorated names. The only difference between them
            // is the "$$h" tag. This tag is mentioned here:
            // https://learn.microsoft.com/en-us/cpp/build/reference/decorated-names?view=msvc-170
            // An example from comctl32.dll version 6.10.22621.4825:
            // Decorated, native:
            // ??1CLink@@UEAA@XZ
            // Decorated, ARM64EC:
            // ??1CLink@@$$hUEAA@XZ
            // Undecorated (in both cases):
            // public: virtual __cdecl CLink::~CLink(void)
            //
            // To be able to disambiguate between these two undecorated names,
            // we add a prefix to the ARM64EC undecorated name. In the above
            // example, it becomes:
            // tag=ARM64EC\public: virtual __cdecl CLink::~CLink(void)
            //
            // The "\" symbol was chosen after looking for an ASCII character
            // that's not being used in symbol names. It looks like the only
            // three such characters in the ASCII range of 0x21-0x7E are: " ; \.
            // Note: The # character doesn't seem to be used outside of ARM64
            // symbols, but it's being used extensively as an ARM64-related
            // marker in hybrid binaries.
            //
            // Below is a simplistic check that only checks that the "$$h"
            // string is present in the symbol name. Hopefully it's good enough
            // so that full parsing of the decorated name is not needed.
            bool isArm64Ec =
                m_currentSymbolName &&
                wcsstr(m_currentSymbolName.get(), L"$$h") != nullptr;
            if (isArm64Ec) {
                currentSymbolNameUndecoratedPrefix2 = L"tag=ARM64EC\\";
            }
        }

        PCWSTR currentSymbolNameUndecorated;
        if (!*currentSymbolNameUndecoratedPrefix1 &&
            !*currentSymbolNameUndecoratedPrefix2) {
            currentSymbolNameUndecorated = m_currentSymbolNameUndecorated.get();
        } else {
            m_currentSymbolNameUndecoratedWithPrefixes =
                currentSymbolNameUndecoratedPrefix1;
            m_currentSymbolNameUndecoratedWithPrefixes +=
                currentSymbolNameUndecoratedPrefix2;
            m_currentSymbolNameUndecoratedWithPrefixes +=
                m_currentSymbolNameUndecorated.get();
            currentSymbolNameUndecorated =
                m_currentSymbolNameUndecoratedWithPrefixes.c_str();
        }

        return SymbolEnum::Symbol{
            reinterpret_cast<void*>(reinterpret_cast<BYTE*>(m_moduleBase) +
                                    currentSymbolRva),
            m_currentSymbolName.get(), currentSymbolNameUndecorated};
    }
}

void SymbolEnum::InitModuleInfo(HMODULE module) {
    auto* dosHeader = (const IMAGE_DOS_HEADER*)module;
    auto* ntHeader =
        (const IMAGE_NT_HEADERS*)((const char*)dosHeader + dosHeader->e_lfanew);
    WORD magic = ntHeader->OptionalHeader.Magic;

    std::optional<std::span<const IMAGE_CHPE_RANGE_ENTRY>> chpeRanges;
    switch (magic) {
        case IMAGE_NT_OPTIONAL_HDR32_MAGIC:
            chpeRanges = GetChpeRanges<IMAGE_NT_HEADERS32,
                                       IMAGE_LOAD_CONFIG_DIRECTORY32>(
                dosHeader, (const IMAGE_NT_HEADERS32*)ntHeader);
            break;

        case IMAGE_NT_OPTIONAL_HDR64_MAGIC:
            chpeRanges = GetChpeRanges<IMAGE_NT_HEADERS64,
                                       IMAGE_LOAD_CONFIG_DIRECTORY64>(
                dosHeader, (const IMAGE_NT_HEADERS64*)ntHeader);
            break;
    }

    m_moduleInfo.magic = magic;

    if (chpeRanges) {
        m_moduleInfo.isHybrid = true;
        m_moduleInfo.chpeRanges.assign(chpeRanges->begin(), chpeRanges->end());
    } else {
        m_moduleInfo.isHybrid = false;
    }
}

wil::com_ptr<IDiaDataSource> SymbolEnum::LoadMsdia() {
    auto enginePath = StorageManager::GetInstance().GetEnginePath();
    auto msdiaPath = enginePath / L"msdia140_windhawk.dll";

    m_msdiaModule.reset(LoadLibraryEx(msdiaPath.c_str(), nullptr,
                                      LOAD_WITH_ALTERED_SEARCH_PATH));
    THROW_LAST_ERROR_IF_NULL(m_msdiaModule);

    // msdia loads symsrv.dll by using the following call:
    // LoadLibraryExW(L"SYMSRV.DLL");
    // This is problematic for the following reasons:
    // * If another file named symsrv.dll is already loaded,
    //   it will be used instead.
    // * If not, the library loading search path doesn't include our folder
    //   by default.
    // Especially due to the first point, we patch msdia in memory to use
    // the full path to our copy of symsrv.dll.
    // Also, to prevent from other msdia instances to load our version of
    // symsrv, we name it differently.

    void** msdiaLoadLibraryExWPtr = Functions::FindImportPtr(
        m_msdiaModule.get(), "kernel32.dll", "LoadLibraryExW");

    DWORD dwOldProtect;
    THROW_IF_WIN32_BOOL_FALSE(VirtualProtect(msdiaLoadLibraryExWPtr,
                                             sizeof(*msdiaLoadLibraryExWPtr),
                                             PAGE_READWRITE, &dwOldProtect));
    *msdiaLoadLibraryExWPtr = MsdiaLoadLibraryExWHook;
    THROW_IF_WIN32_BOOL_FALSE(VirtualProtect(msdiaLoadLibraryExWPtr,
                                             sizeof(*msdiaLoadLibraryExWPtr),
                                             dwOldProtect, &dwOldProtect));

    wil::com_ptr<IDiaDataSource> diaSource;
    THROW_IF_FAILED(NoRegCoCreate(msdiaPath.c_str(), CLSID_DiaSource,
                                  IID_PPV_ARGS(&diaSource)));

    // Decrements the reference count incremented by NoRegCoCreate.
    FreeLibrary(m_msdiaModule.get());

    return diaSource;
}
