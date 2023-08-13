#pragma once

class SymbolEnum {
   public:
    struct Callbacks {
        std::function<bool()> queryCancel;
        std::function<void(int)> notifyProgress;
    };

    SymbolEnum(HMODULE moduleBase,
               PCWSTR symbolServer,
               Callbacks callbacks = {});
    SymbolEnum(PCWSTR modulePath,
               HMODULE moduleBase,
               PCWSTR symbolServer,
               Callbacks callbacks = {});

    struct Symbol {
        void* address;
        PCWSTR name;
        PCWSTR nameDecorated;
    };

    std::optional<Symbol> GetNextSymbol(bool compatDemangling);

   private:
    wil::com_ptr<IDiaDataSource> LoadMsdia();

    HMODULE m_moduleBase;
    wil::unique_hmodule m_msdiaModule;
    wil::com_ptr<IDiaEnumSymbols> m_diaSymbols;
    wil::unique_bstr m_currentSymbolName;
    wil::unique_bstr m_currentDecoratedSymbolName;
};
