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

    // Launches a host process for each of the tool mods the session's user
    // has, on every session queued since the last call. Only the session
    // manager launches hosts.
    void HandleQueuedSessionLogons();

   private:
    using ToolMods = std::unordered_map<std::wstring, Mod::ToolModLaunchInfo>;

    // A user's SID, or nothing for a user who couldn't be read, who gets the
    // global configurations.
    using UserSid = std::optional<std::wstring>;

    using ToolModsByUser = std::unordered_map<UserSid, ToolMods>;

    // The tool mods each user of the sessions given gets a host for, each read
    // in the user's configuration scope. A mod which can't be read for a user
    // keeps what previous holds for them.
    static ToolModsByUser ReadToolMods(
        const std::vector<ToolModProcess::HostSession>& sessions,
        const ToolModsByUser& previous);

    // The host each of the mods given asks for, by mod id.
    static std::unordered_map<std::wstring, ToolModProcess::ToolModInfo>
    ToolModHostInfos(const ToolMods& toolMods);

    std::unordered_map<std::wstring, Mod> m_mods;

    // The tool mods a host launch has been attempted for, per user of a
    // session, each as the mod was in the user's scope when it was attempted,
    // whether or not the attempt reached a running host. A mod which changes,
    // or which stops being a tool mod, leaves the record, so the mod's own
    // configuration is the only thing which asks for another attempt. A user
    // with no session left leaves it too, and their next logon is served anew.
    //
    // Only the thread the mods manager runs on reaches it, which is why a logon
    // learned of elsewhere is queued instead of served where it arrives.
    ToolModsByUser m_toolMods;
};
