#pragma once

// A tool mod asks for a process of its own instead of for a place inside
// another program. Its include patterns say so, by naming, by file name with no
// path and no wildcards, a windhawk-mod*.exe host outright, or windhawk.exe
// together with the tool mod marker in its export table. The session manager -
// the service on a regular install, the daemon on a portable one - launches a
// host process for such a mod, beside windhawk.exe and with the mod id on its
// command line, and the engine injected into the host loads the mod there.
// Being a tool mod costs a mod only its place in windhawk.exe; it keeps its
// place in whatever else it targets.
//
// A mod can host itself instead, launching whichever program suits it - a
// second explorer.exe, say - with the mod id on that process's command line.
// The flag is all the engine goes by, so the mod is taken there on its own
// patterns, which name the program it chose.
//
// A host process belongs to the mod it hosts. What loads there beside it is
// every mod which takes every process, which is a lone "*" among its include
// patterns.
//
// Which host runs is an elevation level, one per host file name. A mod names
// the levels it can run at and gets the highest of them the launch can provide,
// falling to the next one it names when a launch fails; a mod which names only
// levels the launch can't provide gets no host. A host drops its mod once the
// mod stops naming that level, which ends the host, and the settings change
// which did so asks for a host at a level the mod does name.
//
// Keeping the host alive is the mod's own job: the host is a stub whose entry
// point does nothing but report that the engine never reached it, so the mod
// hooks that entry point before it runs, and ends the process once the mod is
// uninitialized.
//
// A launch which fails leaves the mod without a host until its own settings
// change. Retrying on the next sweep would bring the mod up on an unrelated
// event, and, since a host is recorded per mod rather than per session, would
// add a second host on every session the launch did reach.
namespace ToolModProcess {

// The app's file name, which a legacy tool mod names in its patterns.
inline constexpr WCHAR kAppFileName[] = L"windhawk.exe";

// The elevation level a tool mod's host runs at, one per host file name,
// ordered least to most. The values are what HostLevelBit shifts by, so they
// run from zero without a gap.
enum class HostLevel {
    kNormal = 0,    // as the desktop user
    kUiAccess = 1,  // the desktop user, with UIAccess
    kElevated = 2,  // elevated
};

inline constexpr unsigned int HostLevelBit(HostLevel level) {
    return 1u << static_cast<unsigned int>(level);
}

// A host per level: the file name a mod names in its patterns to ask for that
// level, and which the host process is in turn recognized by. In level order,
// least privileged first, which is the order the levels are walked in.
struct Host {
    HostLevel level;
    PCWSTR fileName;
};

inline constexpr Host kHosts[] = {
    {HostLevel::kNormal, L"windhawk-mod.exe"},
    {HostLevel::kUiAccess, L"windhawk-mod-uiaccess.exe"},
    {HostLevel::kElevated, L"windhawk-mod-elevated.exe"},
};

// What a tool mod's include patterns ask for, read from its settings and
// carried to the launch, which reads the install to pick the host it runs.
struct ToolModInfo {
    // The levels the mod's includes name, as a bitmask of HostLevelBit values.
    // At least one bit is set.
    unsigned int requestedLevels = 0;

    // Whether the mod names windhawk.exe, which a mod carrying the older tool
    // mod implementation, the one which reads the -tool-mod flag, does. Such a
    // mod's host is passed both flags, since it may be one of those.
    bool legacy = false;

    bool operator==(const ToolModInfo&) const = default;
};

// The id of the mod this process was launched to host, from the flag on its
// command line, or nullptr when it carries none.
PCWSTR GetHostedModId();

// The level this process is a host for, by its own file name, or nothing in a
// process which isn't a host image.
std::optional<HostLevel> GetCurrentHostLevel();

// Whether the current process is windhawk.exe, the one process tool mods are
// kept out of.
bool IsAppProcess();

// Whether the mod library is marked as a tool mod in its export table. Reads
// the file as data, so nothing in it runs.
bool DoesLibraryExportToolModMarker(const std::filesystem::path& libraryPath);

// Launches a host process for each of the mods given: the service, which has no
// session of its own, one per logged-on session, a daemon one in the session it
// runs in. A mod left without a host is logged and doesn't stop the others;
// which mods have been asked for is the caller's to keep.
void LaunchHosts(
    const std::unordered_map<std::wstring, ToolModInfo>& toolMods);

// Launches a host process for each of the mods given on one session. A session
// which has just logged on is reached this way, which a host recorded per mod
// rather than per session doesn't otherwise get; a sweep under way at that
// moment sees the session's user too, and the second host it launches ends on
// the single-instance mutex a tool mod holds for its own host.
void LaunchHostsOnSession(
    DWORD sessionId,
    const std::unordered_map<std::wstring, ToolModInfo>& toolMods);

}  // namespace ToolModProcess
