#pragma once

#include "mod.h"

class ModsManager {
   public:
    ModsManager();

    void AfterInit();
    void BeforeUninit();
    void ReloadModsAndSettings();

   private:
    std::unordered_map<std::wstring, Mod> loadedMods;
};
