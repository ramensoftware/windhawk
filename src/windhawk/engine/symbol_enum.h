#pragma once

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
        PCWSTR nameDecorated;
    };

    std::optional<Symbol> GetNextSymbol();

   private:
    wil::com_ptr<IDiaDataSource> LoadMsdia();

    HMODULE m_moduleBase;
    UndecorateMode m_undecorateMode;
    wil::unique_hmodule m_msdiaModule;
    wil::com_ptr<IDiaEnumSymbols> m_diaSymbols;
    wil::unique_bstr m_currentSymbolName;
    wil::unique_bstr m_currentDecoratedSymbolName;
};
