#include "stdafx.h"

#include "logger.h"
#include "mods_manager.h"
#include "no_destructor.h"
#include "storage_manager.h"
#include "tool_mod_process.h"
#include "var_init_once.h"

namespace {

DWORD GetModuleSizeOfImage(HMODULE module) {
    IMAGE_DOS_HEADER* dosHeader = (IMAGE_DOS_HEADER*)module;
    IMAGE_NT_HEADERS* ntHeader =
        (IMAGE_NT_HEADERS*)((BYTE*)dosHeader + dosHeader->e_lfanew);
    return ntHeader->OptionalHeader.SizeOfImage;
}

// The sessions which have just gained a user and haven't been launched for,
// and the event which wakes whoever serves them. Process-wide, since a logon
// can arrive whether or not a customization session is running.
class SessionLogonQueue {
   public:
    HANDLE GetEvent() const { return m_event.get(); }

    bool Push(DWORD sessionId) {
        if (!m_event) {
            return false;
        }

        {
            auto lock = m_lock.lock_exclusive();
            m_sessionIds.push_back(sessionId);
        }

        return !!SetEvent(m_event.get());
    }

    std::vector<DWORD> Take() {
        std::vector<DWORD> sessionIds;

        auto lock = m_lock.lock_exclusive();
        sessionIds.swap(m_sessionIds);
        return sessionIds;
    }

   private:
    wil::unique_event m_event{CreateEvent(nullptr, FALSE, FALSE, nullptr)};
    wil::srwlock m_lock;
    std::vector<DWORD> m_sessionIds;
};

SessionLogonQueue& GetSessionLogonQueue() {
    STATIC_INIT_ONCE(NoDestructorIfTerminating<SessionLogonQueue>, queue);
    return **queue;
}

}  // namespace

ModsManager::ModsManager() {
    ToolMods toolMods;

    StorageManager::GetInstance().EnumMods([this, &toolMods](PCWSTR modName) {
        try {
            Mod::ToolModLaunchInfo launchInfo;
            switch (
                Mod::GetLoadDecisionForRunningProcess(modName, &launchInfo)) {
                case Mod::LoadDecision::kLoad: {
                    auto result = m_mods.emplace(modName, modName);
                    if (!result.second) {
                        throw std::logic_error(
                            "A mod with that name is already loaded");
                    }
                    break;
                }

                case Mod::LoadDecision::kRunInToolModProcess:
                    toolMods.emplace(modName, std::move(launchInfo));
                    break;

                case Mod::LoadDecision::kSkip:
                    break;
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

    // AfterInit is what launches the hosts: the hook which injects the engine
    // into a new process is only applied once this constructor returns, and a
    // host the engine doesn't reach in time gives up.
    m_toolMods = std::move(toolMods);
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

    LaunchToolModHosts(m_toolMods);
}

// static
std::unordered_map<std::wstring, ToolModProcess::ToolModInfo>
ModsManager::ToolModHostInfos(const ToolMods& toolMods) {
    std::unordered_map<std::wstring, ToolModProcess::ToolModInfo> hosts;
    for (const auto& [modName, launchInfo] : toolMods) {
        hosts.emplace(modName, launchInfo.info);
    }

    return hosts;
}

// static
void ModsManager::LaunchToolModHosts(const ToolMods& toolMods) {
    ToolModProcess::LaunchHosts(ToolModHostInfos(toolMods));
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
    ToolMods toolMods;

    StorageManager::GetInstance().EnumMods([this, &modsToKeepLoaded,
                                            &modsToKeepUnloaded, &modsToLoad,
                                            &toolMods](PCWSTR modName) {
        try {
            Mod::ToolModLaunchInfo launchInfo;
            auto loadDecision =
                Mod::GetLoadDecisionForRunningProcess(modName, &launchInfo);
            if (loadDecision == Mod::LoadDecision::kRunInToolModProcess) {
                toolMods.emplace(modName, std::move(launchInfo));
                return;
            }

            if (loadDecision != Mod::LoadDecision::kLoad) {
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

            // Nothing was learned about the mod, so keep its record: dropping
            // it would have the next sweep launch a second host next to the one
            // the mod already has.
            auto it = m_toolMods.find(modName);
            if (it != m_toolMods.end()) {
                toolMods.insert(*it);
            }
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

    ToolMods toolModsToLaunchFor;
    for (const auto& [modName, launchInfo] : toolMods) {
        auto it = m_toolMods.find(modName);
        if (it == m_toolMods.end() || it->second != launchInfo) {
            toolModsToLaunchFor.emplace(modName, launchInfo);
        }
    }

    // The record becomes what this sweep found ahead of the launch, and stays
    // that way whatever the launch runs into.
    m_toolMods = std::move(toolMods);

    LaunchToolModHosts(toolModsToLaunchFor);
}

// static
HANDLE ModsManager::GetSessionLogonEvent() {
    return GetSessionLogonQueue().GetEvent();
}

// static
bool ModsManager::QueueSessionLogon(DWORD sessionId) {
    if (!GetSessionLogonQueue().Push(sessionId)) {
        LOG(L"Failed to note the logon of session %u", sessionId);
        return false;
    }

    return true;
}

void ModsManager::HandleQueuedSessionLogons() {
    std::vector<DWORD> sessionIds = GetSessionLogonQueue().Take();

    if (m_toolMods.empty()) {
        return;
    }

    auto hosts = ToolModHostInfos(m_toolMods);

    for (DWORD sessionId : sessionIds) {
        ToolModProcess::LaunchHostsOnSession(sessionId, hosts);
    }
}
