#include "stdafx.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"
#include "symbol_enum.h"

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

    VERBOSE(L"%.*S", len, p);
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
    wil::com_ptr<IDiaDataSource> diaSource = LoadMsdia();

    std::wstring symSearchPath = GetSymbolsSearchPath(symbolServer);

    g_symbolServerCallbacks = &callbacks;
    auto msdiaCallbacksCleanup =
        wil::scope_exit([] { g_symbolServerCallbacks = nullptr; });

    THROW_IF_FAILED(
        diaSource->loadDataForExe(modulePath, symSearchPath.c_str(), nullptr));

    wil::com_ptr<IDiaSession> diaSession;
    THROW_IF_FAILED(diaSource->openSession(&diaSession));

    wil::com_ptr<IDiaSymbol> diaGlobal;
    THROW_IF_FAILED(diaSession->get_globalScope(&diaGlobal));

    THROW_IF_FAILED(
        diaGlobal->findChildren(SymTagNull, nullptr, nsNone, &m_diaSymbols));
}

std::optional<SymbolEnum::Symbol> SymbolEnum::GetNextSymbol() {
    while (true) {
        wil::com_ptr<IDiaSymbol> diaSymbol;
        ULONG count = 0;
        HRESULT hr = m_diaSymbols->Next(1, &diaSymbol, &count);
        THROW_IF_FAILED(hr);

        if (hr == S_FALSE || count == 0) {
            return std::nullopt;
        }

        DWORD currentSymbolRva;
        hr = diaSymbol->get_relativeVirtualAddress(&currentSymbolRva);
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            continue;  // no RVA
        }

        // Temporary compatibility code.
        if (m_undecorateMode == UndecorateMode::OldVersionCompatible) {
            // get_undecoratedName uses 0x20800 as flags:
            // * UNDNAME_32_BIT_DECODE (0x800)
            // * UNDNAME_NO_PTR64 (0x20000)
            // For some reason, the old msdia version still included ptr64 in
            // the output. For compatibility, use get_undecoratedNameEx and
            // don't pass this flag.
            constexpr DWORD kUndname32BitDecode = 0x800;
            hr = diaSymbol->get_undecoratedNameEx(kUndname32BitDecode,
                                                  &m_currentSymbolName);
        } else if (m_undecorateMode == UndecorateMode::Default) {
            hr = diaSymbol->get_undecoratedName(&m_currentSymbolName);
        } else {
            m_currentSymbolName.reset();
            hr = S_OK;
        }
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            m_currentSymbolName.reset();  // no name
        }

        hr = diaSymbol->get_name(&m_currentDecoratedSymbolName);
        THROW_IF_FAILED(hr);
        if (hr == S_FALSE) {
            m_currentDecoratedSymbolName.reset();  // no name
        }

        return SymbolEnum::Symbol{
            reinterpret_cast<void*>(reinterpret_cast<BYTE*>(m_moduleBase) +
                                    currentSymbolRva),
            m_currentSymbolName.get(), m_currentDecoratedSymbolName.get()};
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
    THROW_IF_WIN32_BOOL_FALSE(
        VirtualProtect(msdiaLoadLibraryExWPtr, sizeof(*msdiaLoadLibraryExWPtr),
                       PAGE_EXECUTE_READWRITE, &dwOldProtect));
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
