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

    // Notes a session which has just gained a user, from whichever thread
    // learns of it, and wakes the loop which serves it. False when the queue
    // can't be reached, which leaves the session without hosts.
    static bool QueueSessionLogon(DWORD sessionId);

    // Signaled while sessions wait to be served. Null when the event couldn't
    // be created.
    static HANDLE GetSessionLogonEvent();

    // Launches a host process for each of the tool mods in the record, on
    // every session queued since the last call. Only the session manager has a
    // record to launch from.
    void HandleQueuedSessionLogons();

   private:
    using ToolMods = std::unordered_map<std::wstring, Mod::ToolModLaunchInfo>;

    // The host each of the mods given asks for, by mod id.
    static std::unordered_map<std::wstring, ToolModProcess::ToolModInfo>
    ToolModHostInfos(const ToolMods& toolMods);

    static void LaunchToolModHosts(const ToolMods& toolMods);

    std::unordered_map<std::wstring, Mod> m_mods;

    // The tool mods a host launch has been attempted for, each as the mod was
    // when it was attempted, whether or not the attempt reached a running host.
    // A mod which changes, or which stops being a tool mod, leaves the record,
    // so the mod's own settings are the only thing which asks for another
    // attempt.
    //
    // Only the thread the mods manager runs on reaches it, which is why a logon
    // learned of elsewhere is queued instead of served where it arrives.
    ToolMods m_toolMods;
};
