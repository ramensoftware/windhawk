#include "stdafx.h"

#include "logger.h"
#include "mods_manager.h"
#include "storage_manager.h"

ModsManager::ModsManager() {
    StorageManager::GetInstance().EnumMods([this](PCWSTR modName) {
        try {
            if (Mod::ShouldLoadInRunningProcess(modName)) {
                auto result = loadedMods.emplace(modName, modName);
                if (!result.second) {
                    throw std::logic_error(
                        "A mod with that name is already loaded");
                }
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) initializing failed: %S", modName, e.what());
        }
    });

    for (auto& [name, mod] : loadedMods) {
        try {
            mod.Load();
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) loading failed: %S", name.c_str(), e.what());
        }
    }
}

void ModsManager::AfterInit() {
    for (auto& [name, mod] : loadedMods) {
        try {
            mod.AfterInit();
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) AfterInit failed: %S", name.c_str(), e.what());
        }
    }
}

void ModsManager::BeforeUninit() {
    for (auto& [name, mod] : loadedMods) {
        try {
            mod.BeforeUninit();
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) BeforeUninit failed: %S", name.c_str(), e.what());
        }
    }
}

void ModsManager::ReloadModsAndSettings() {
    std::unordered_set<std::wstring> modsToKeepLoaded;
    std::unordered_set<std::wstring> modsToKeepUnloaded;
    std::vector<std::wstring> modsToLoad;

    StorageManager::GetInstance().EnumMods([this, &modsToKeepLoaded,
                                            &modsToKeepUnloaded,
                                            &modsToLoad](PCWSTR modName) {
        try {
            bool shouldBeLoaded = Mod::ShouldLoadInRunningProcess(modName);
            if (!shouldBeLoaded) {
                return;
            }

            auto it = loadedMods.find(modName);
            if (it != loadedMods.end()) {
                auto& loadedMod = it->second;

                bool reload = false;
                if (!loadedMod.ApplyChangedSettings(&reload)) {
                    modsToKeepUnloaded.emplace(modName);
                } else if (reload) {
                    modsToLoad.emplace_back(modName);
                } else {
                    modsToKeepLoaded.emplace(modName);
                }
            } else {
                modsToLoad.emplace_back(modName);
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) reloading failed: %S", modName, e.what());
        }
    });

    for (auto& [name, mod] : loadedMods) {
        if (!modsToKeepLoaded.contains(name)) {
            try {
                mod.BeforeUninit();
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) BeforeUninit failed: %S", name.c_str(),
                    e.what());
            }
        }
    }

    MH_STATUS status = MH_ApplyQueuedEx(MH_ALL_IDENTS);
    if (status != MH_OK) {
        LOG(L"MH_ApplyQueuedEx failed with %d", status);
    }

    for (auto it = loadedMods.begin(); it != loadedMods.end();) {
        auto& [name, mod] = *it;
        if (modsToKeepLoaded.contains(name)) {
            ++it;
        } else if (modsToKeepUnloaded.contains(name)) {
            mod.Unload();
            ++it;
        } else {
            it = loadedMods.erase(it);
        }
    }

    for (const auto& modName : modsToLoad) {
        try {
            auto result = loadedMods.emplace(modName, modName.c_str());
            if (!result.second) {
                throw std::logic_error(
                    "A mod with that name is already loaded");
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) initializing failed: %S", modName.c_str(), e.what());
        }
    }

    for (const auto& modName : modsToLoad) {
        auto i = loadedMods.find(modName);
        if (i != loadedMods.end()) {
            auto& loadedMod = i->second;
            try {
                loadedMod.Load();
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) loading failed: %S", modName.c_str(), e.what());
            }
        }
    }

    status = MH_ApplyQueuedEx(MH_ALL_IDENTS);
    if (status != MH_OK) {
        LOG(L"MH_ApplyQueuedEx failed with %d", status);
    }

    for (const auto& modName : modsToLoad) {
        auto i = loadedMods.find(modName);
        if (i != loadedMods.end()) {
            auto& loadedMod = i->second;
            try {
                loadedMod.AfterInit();
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) AfterInit failed: %S", modName.c_str(),
                    e.what());
            }
        }
    }
}
