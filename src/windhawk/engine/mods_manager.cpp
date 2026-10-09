#include "stdafx.h"

#include "customization_session.h"
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

// Only the session manager launches hosts: the service for every logged-on
// session, a portable daemon for the session it serves.
bool LaunchesToolModHosts() {
    return ToolModProcess::IsAppProcess() &&
           CustomizationSession::GetSessionManagerProcessId() ==
               GetCurrentProcessId();
}

}  // namespace

ModsManager::ModsManager() {
    StorageManager::GetInstance().EnumMods([this](PCWSTR modName) {
        try {
            ModConfigScope configScope;
            if (Mod::ShouldLoadInRunningProcess(modName, &configScope)) {
                auto result = m_mods.try_emplace(modName, modName,
                                                 std::move(configScope));
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

    // Not in the constructor: the hook which injects the engine into a new
    // process is only applied once it returns, and a host the engine doesn't
    // reach in time gives up.
    if (LaunchesToolModHosts()) {
        auto sessions = ToolModProcess::GetHostSessions();
        m_toolMods = ReadToolMods(sessions, {});

        for (const auto& session : sessions) {
            ToolModProcess::LaunchHosts(
                session, ToolModHostInfos(m_toolMods.at(session.userSid)));
        }
    }
}

// static
ModsManager::ToolModsByUser ModsManager::ReadToolMods(
    const std::vector<ToolModProcess::HostSession>& sessions,
    const ToolModsByUser& previous) {
    ToolModsByUser toolMods;
    for (const auto& session : sessions) {
        toolMods[session.userSid];
    }

    StorageManager::GetInstance().EnumMods([&previous,
                                            &toolMods](PCWSTR modName) {
        // Read once for all the users who have no configuration of their own.
        std::optional<std::optional<Mod::ToolModLaunchInfo>> globalLaunchInfo;

        for (auto& [userSid, userToolMods] : toolMods) {
            try {
                auto resolvedScope = Mod::ResolveConfigScope(modName, userSid);

                std::optional<Mod::ToolModLaunchInfo> launchInfo;
                if (resolvedScope.scope.userSid) {
                    launchInfo =
                        Mod::GetToolModLaunchInfo(modName, resolvedScope);
                } else {
                    if (!globalLaunchInfo) {
                        globalLaunchInfo =
                            Mod::GetToolModLaunchInfo(modName, resolvedScope);
                    }

                    launchInfo = *globalLaunchInfo;
                }

                if (launchInfo) {
                    userToolMods.emplace(modName, std::move(*launchInfo));
                }
            } catch (const std::exception& e) {
                LOG(L"Mod (%s) reading the tool mod host of user %s failed: %S",
                    modName, userSid ? userSid->c_str() : L"(unknown)",
                    e.what());

                // Nothing was learned about the mod, so keep its record:
                // dropping it would have the next sweep launch a second host
                // next to the one the mod already has.
                auto userIt = previous.find(userSid);
                if (userIt != previous.end()) {
                    auto it = userIt->second.find(modName);
                    if (it != userIt->second.end()) {
                        userToolMods.insert(*it);
                    }
                }
            }
        }
    });

    return toolMods;
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
    std::vector<std::pair<std::wstring, ModConfigScope>> modsToLoad;

    StorageManager::GetInstance().EnumMods([this, &modsToKeepLoaded,
                                            &modsToKeepUnloaded,
                                            &modsToLoad](PCWSTR modName) {
        try {
            ModConfigScope configScope;
            if (!Mod::ShouldLoadInRunningProcess(modName, &configScope)) {
                return;
            }

            auto it = m_mods.find(modName);
            if (it != m_mods.end()) {
                auto& loadedMod = it->second;

                // An instance keeps the scope it was created in, so a scope
                // which changed takes a new instance.
                bool reload = loadedMod.GetConfigScope() != configScope;
                if (!reload && !loadedMod.ApplyChangedSettings(&reload)) {
                    modsToKeepUnloaded.emplace(modName);
                } else if (reload) {
                    modsToLoad.emplace_back(modName, std::move(configScope));
                } else {
                    modsToKeepLoaded.emplace(modName);
                }
            } else {
                modsToLoad.emplace_back(modName, std::move(configScope));
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

    for (const auto& [modName, configScope] : modsToLoad) {
        try {
            auto result =
                m_mods.try_emplace(modName, modName.c_str(), configScope);
            if (!result.second) {
                throw std::logic_error(
                    "A mod with that name is already loaded");
            }
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) initializing failed: %S", modName.c_str(), e.what());
        }
    }

    for (const auto& [modName, configScope] : modsToLoad) {
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

    for (const auto& [modName, configScope] : modsToLoad) {
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

    if (!LaunchesToolModHosts()) {
        return;
    }

    auto sessions = ToolModProcess::GetHostSessions();
    ToolModsByUser toolMods = ReadToolMods(sessions, m_toolMods);

    std::vector<std::pair<ToolModProcess::HostSession, ToolMods>>
        toolModsToLaunchFor;
    for (const auto& session : sessions) {
        // A user who wasn't in the record has just logged on, and the logon
        // is what launches for them.
        auto previousIt = m_toolMods.find(session.userSid);
        if (previousIt == m_toolMods.end()) {
            continue;
        }

        const ToolMods& previous = previousIt->second;

        ToolMods changed;
        for (const auto& [modName, launchInfo] : toolMods.at(session.userSid)) {
            auto it = previous.find(modName);
            if (it == previous.end() || it->second != launchInfo) {
                changed.emplace(modName, launchInfo);
            }
        }

        toolModsToLaunchFor.emplace_back(session, std::move(changed));
    }

    // The record becomes what this sweep found ahead of the launch, and stays
    // that way whatever the launch runs into.
    m_toolMods = std::move(toolMods);

    for (const auto& [session, changed] : toolModsToLaunchFor) {
        ToolModProcess::LaunchHosts(session, ToolModHostInfos(changed));
    }
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

    if (!LaunchesToolModHosts()) {
        return;
    }

    for (DWORD sessionId : sessionIds) {
        auto session = ToolModProcess::GetHostSession(sessionId);
        if (!m_toolMods.contains(session.userSid)) {
            m_toolMods.merge(ReadToolMods({session}, {}));
        }

        // A sweep launches on a user's sessions only for what changed, so a
        // new session gets its hosts here. A sweep under way at the logon can
        // launch there too, and the second host ends on the single-instance
        // mutex a tool mod holds for its own host.
        ToolModProcess::LaunchHosts(
            session, ToolModHostInfos(m_toolMods.at(session.userSid)));
    }
}
