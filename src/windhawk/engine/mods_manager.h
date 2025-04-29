#pragma once

#include "mod.h"

class ModsManager {
   public:
    ModsManager();
    ~ModsManager();

    ModsManager(const ModsManager&) = delete;
    ModsManager& operator=(const ModsManager&) = delete;

    void AfterInit();
    void BeforeUninit();
    void ReloadModsAndSettings();

   private:
    std::unordered_map<std::wstring, Mod> m_mods;
};
