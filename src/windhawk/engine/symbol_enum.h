#pragma once

void MySysFreeString(BSTR bstrString);

using my_unique_bstr =
    wil::unique_any<BSTR, decltype(&MySysFreeString), MySysFreeString>;

class SymbolEnum {
   public:
    enum class UndecorateMode {
        Default = 0,
        OldVersionCompatible,
        None,
    };

    struct Callbacks {
        std::function<bool()> queryCancel;
        std::function<void(int)> notifyProgress;
    };

    SymbolEnum(HMODULE moduleBase,
               PCWSTR symbolServer,
               UndecorateMode undecorateMode,
               Callbacks callbacks = {});
    SymbolEnum(PCWSTR modulePath,
               HMODULE moduleBase,
               PCWSTR symbolServer,
               UndecorateMode undecorateMode,
               Callbacks callbacks = {});

    struct Symbol {
        void* address;
        PCWSTR name;
        PCWSTR nameUndecorated;
    };

    std::optional<Symbol> GetNextSymbol();

    // https://ntdoc.m417z.com/image_chpe_range_entry
    typedef struct _IMAGE_CHPE_RANGE_ENTRY {
        union {
            ULONG StartOffset;
            struct {
                ULONG NativeCode : 1;
                ULONG AddressBits : 31;
            } DUMMYSTRUCTNAME;
        } DUMMYUNIONNAME;

        ULONG Length;
    } IMAGE_CHPE_RANGE_ENTRY, *PIMAGE_CHPE_RANGE_ENTRY;

   private:
    void InitModuleInfo(HMODULE module);
    wil::com_ptr<IDiaDataSource> LoadMsdia();

    static constexpr enum SymTagEnum kSymTags[] = {
        SymTagPublicSymbol,
        SymTagFunction,
        SymTagData,
    };

    struct ModuleInfo {
        WORD magic;
        bool isHybrid;
        std::vector<IMAGE_CHPE_RANGE_ENTRY> chpeRanges;
    };

    HMODULE m_moduleBase;
    UndecorateMode m_undecorateMode;
    ModuleInfo m_moduleInfo;
    wil::unique_hmodule m_msdiaModule;
    wil::com_ptr<IDiaSymbol> m_diaGlobal;
    wil::com_ptr<IDiaEnumSymbols> m_diaSymbols;
    size_t m_symTagIndex = 0;
    my_unique_bstr m_currentSymbolName;
    my_unique_bstr m_currentSymbolNameUndecorated;
    std::wstring m_currentSymbolNameUndecoratedWithPrefixes;
};
