#include "stdafx.h"

#include "logger.h"
#include "mods_manager.h"
#include "storage_manager.h"

namespace {

DWORD GetModuleSizeOfImage(HMODULE module) {
    IMAGE_DOS_HEADER* dosHeader = (IMAGE_DOS_HEADER*)module;
    IMAGE_NT_HEADERS* ntHeader =
        (IMAGE_NT_HEADERS*)((BYTE*)dosHeader + dosHeader->e_lfanew);
    return ntHeader->OptionalHeader.SizeOfImage;
}

}  // namespace

ModsManager::ModsManager() {
    StorageManager::GetInstance().EnumMods([this](PCWSTR modName) {
        try {
            if (Mod::ShouldLoadInRunningProcess(modName)) {
                auto result = m_mods.emplace(modName, modName);
                if (!result.second) {
                    throw std::logic_error(
                        "A mod with that name is already loaded");
                }
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) initializing failed: %S", modName, e.what());
        }
    });

    for (auto& [name, mod] : m_mods) {
        try {
            mod.Load(/*loadedOnStartup=*/true);
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) loading failed: %S", name.c_str(), e.what());
        }
    }
}

ModsManager::~ModsManager() {
    std::vector<ThreadCallStackRegionInfo> regions;

    for (auto& [name, mod] : m_mods) {
        try {
            mod.Uninitialize();

            if (HMODULE module = mod.GetLoadedModModuleHandle()) {
                regions.push_back({
                    .address = reinterpret_cast<DWORD_PTR>(module),
                    .size = GetModuleSizeOfImage(module),
                });
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) Uninitialize failed: %S", name.c_str(), e.what());
        }
    }

    if (!regions.empty()) {
        ThreadsCallStackWaitForRegions(
            regions.data(), static_cast<DWORD>(regions.size()), 200, 400);
    }
}

void ModsManager::AfterInit() {
    for (auto& [name, mod] : m_mods) {
        try {
            mod.AfterInit();
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) AfterInit failed: %S", name.c_str(), e.what());
        }
    }
}

void ModsManager::BeforeUninit() {
    for (auto& [name, mod] : m_mods) {
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

            auto it = m_mods.find(modName);
            if (it != m_mods.end()) {
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

    for (auto& [name, mod] : m_mods) {
        if (!modsToKeepLoaded.contains(name)) {
            try {
                mod.BeforeUninit();
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) BeforeUninit failed: %S", name.c_str(),
                    e.what());
            }
        }
    }

#ifdef WH_HOOKING_ENGINE_MINHOOK
    MH_STATUS status = MH_ApplyQueuedEx(MH_ALL_IDENTS);
    if (status != MH_OK) {
        LOG(L"MH_ApplyQueuedEx failed with %d", status);
    }
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
// For testing without a hooking engine.
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE

    std::vector<ThreadCallStackRegionInfo> regions;

    for (auto& [name, mod] : m_mods) {
        if (!modsToKeepLoaded.contains(name)) {
            try {
                mod.Uninitialize();

                if (HMODULE module = mod.GetLoadedModModuleHandle()) {
                    regions.push_back({
                        .address = reinterpret_cast<DWORD_PTR>(module),
                        .size = GetModuleSizeOfImage(module),
                    });
                }
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) Uninitialize failed: %S", name.c_str(),
                    e.what());
            }
        }
    }

    if (!regions.empty()) {
        ThreadsCallStackWaitForRegions(
            regions.data(), static_cast<DWORD>(regions.size()), 200, 400);
    }

    for (auto it = m_mods.begin(); it != m_mods.end();) {
        auto& [name, mod] = *it;
        if (modsToKeepLoaded.contains(name)) {
            ++it;
        } else if (modsToKeepUnloaded.contains(name)) {
            mod.Unload();
            ++it;
        } else {
            it = m_mods.erase(it);
        }
    }

    for (const auto& modName : modsToLoad) {
        try {
            auto result = m_mods.emplace(modName, modName.c_str());
            if (!result.second) {
                throw std::logic_error(
                    "A mod with that name is already loaded");
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) initializing failed: %S", modName.c_str(), e.what());
        }
    }

    for (const auto& modName : modsToLoad) {
        auto i = m_mods.find(modName);
        if (i != m_mods.end()) {
            auto& loadedMod = i->second;
            try {
                loadedMod.Load(/*loadedOnStartup=*/false);
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) loading failed: %S", modName.c_str(), e.what());
            }
        }
    }

#ifdef WH_HOOKING_ENGINE_MINHOOK
    status = MH_ApplyQueuedEx(MH_ALL_IDENTS);
    if (status != MH_OK) {
        LOG(L"MH_ApplyQueuedEx failed with %d", status);
    }
#elif WH_HOOKING_ENGINE == WH_HOOKING_ENGINE_NONE
// For testing without a hooking engine.
#else
#error "Unsupported hooking engine"
#endif  // WH_HOOKING_ENGINE

    for (const auto& modName : modsToLoad) {
        auto i = m_mods.find(modName);
        if (i != m_mods.end()) {
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
