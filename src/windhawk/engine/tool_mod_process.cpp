#include "stdafx.h"

#include "tool_mod_process.h"

#include "logger.h"
#include "pe_image.h"
#include "session_processes.h"
#include "shared_functions.h"
#include "var_init_once.h"

namespace {

// The command line flags a host takes the mod id in: windhawk_tool_mod.h reads
// the first, a legacy mod which brought a tool mod implementation of its own
// the second. Matched as written, the way those implementations read them.
constexpr WCHAR kModIdParam[] = L"-windhawk-tool-mod";
constexpr WCHAR kLegacyModIdParam[] = L"-tool-mod";

// The exports which mark a mod as a tool mod: the marker windhawk_tool_mod.h
// defines, and the mangled name of BOOL WhTool_ModInit(), which only the older
// implementation requires a mod to have.
constexpr std::string_view kToolModMarkerNames[] = {
    "InternalWhToolModMarker",
    "_Z14WhTool_ModInitv",
};

std::filesystem::path GetCurrentProcessFileName() {
    return std::filesystem::path(wil::GetModuleFileName<std::wstring>())
        .filename();
}

PCWSTR HostFileNameForLevel(ToolModProcess::HostLevel level) {
    for (const auto& host : ToolModProcess::kHosts) {
        if (host.level == level) {
            return host.fileName;
        }
    }

    throw std::logic_error("No host file name for that level");
}

// The hosts ship beside the app, which is the process launching them.
std::filesystem::path GetHostPath(ToolModProcess::HostLevel level) {
    return std::filesystem::path(wil::GetModuleFileName<std::wstring>())
               .parent_path() /
           HostFileNameForLevel(level);
}

// By file name: a process which takes the name and passes the flag runs the
// user's own mod on their own account, which they can do anyway.
std::optional<ToolModProcess::HostLevel> GetCurrentHostLevelImpl() {
    auto fileName = GetCurrentProcessFileName();
    for (const auto& host : ToolModProcess::kHosts) {
        if (_wcsicmp(fileName.c_str(), host.fileName) == 0) {
            return host.level;
        }
    }

    return std::nullopt;
}

std::optional<std::wstring> GetHostedModIdImpl() {
    PCWSTR commandLine = GetCommandLine();

    // Parsing costs a shell32.dll load, which most injected processes have no
    // reason to pay for, and both flags end in the legacy one, so a single
    // search rules them out.
    if (!wcsstr(commandLine, kLegacyModIdParam)) {
        return std::nullopt;
    }

    using CommandLineToArgvW_t = decltype(&CommandLineToArgvW);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        CommandLineToArgvW_t, pCommandLineToArgvW, L"shell32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "CommandLineToArgvW");
    if (!pCommandLineToArgvW) {
        LOG(L"Failed to get CommandLineToArgvW address");
        return std::nullopt;
    }

    int argc = 0;
    PWSTR* argv = pCommandLineToArgvW(commandLine, &argc);
    if (!argv) {
        LOG(L"CommandLineToArgvW failed with error %u", GetLastError());
        return std::nullopt;
    }

    auto argvCleanup = wil::scope_exit([argv] { LocalFree(argv); });

    for (int i = 1; i < argc - 1; i++) {
        if ((wcscmp(argv[i], kModIdParam) == 0 ||
             wcscmp(argv[i], kLegacyModIdParam) == 0) &&
            *argv[i + 1]) {
            return argv[i + 1];
        }
    }

    return std::nullopt;
}

DWORD GetCurrentSessionId() {
    DWORD sessionId;
    THROW_IF_WIN32_BOOL_FALSE(
        ProcessIdToSessionId(GetCurrentProcessId(), &sessionId));
    return sessionId;
}

std::wstring MakeHostCommandLine(const std::filesystem::path& hostPath,
                                 PCWSTR modId,
                                 bool legacy) {
    std::wstring commandLine =
        L'"' + hostPath.native() + L"\" " + kModIdParam + L" \"" + modId + L'"';
    if (legacy) {
        commandLine += L' ';
        commandLine += kLegacyModIdParam;
        commandLine += L" \"";
        commandLine += modId;
        commandLine += L'"';
    }
    return commandLine;
}

// The levels the mod names among those the launch can provide, highest first,
// so that a level whose launch fails is followed by the next one down. Empty
// when it names none of them.
std::vector<ToolModProcess::HostLevel> EffectiveHostLevels(
    const ToolModProcess::ToolModInfo& info,
    unsigned int supportedLevels) {
    using ToolModProcess::HostLevel;
    using ToolModProcess::HostLevelBit;

    unsigned int effective = info.requestedLevels & supportedLevels;

    std::vector<HostLevel> levels;
    for (const auto& host : std::views::reverse(ToolModProcess::kHosts)) {
        if (effective & HostLevelBit(host.level)) {
            levels.push_back(host.level);
        }
    }

    return levels;
}

// The levels the service can provide on a session. As the system account it
// can set UIAccess on any session user's token; only elevation needs something
// of the user, a full token, which a standard user hasn't got.
unsigned int SupportedLevelsOnSession(DWORD sessionId) {
    using ToolModProcess::HostLevel;
    using ToolModProcess::HostLevelBit;

    unsigned int levels =
        HostLevelBit(HostLevel::kNormal) | HostLevelBit(HostLevel::kUiAccess);

    try {
        if (Functions::CanCreateProcessOnSessionIdElevated(sessionId)) {
            levels |= HostLevelBit(HostLevel::kElevated);
        }
    } catch (const std::exception& e) {
        // A session which can't be asked keeps the levels which ask nothing of
        // it.
        LOG(L"Reading the elevation of session %u failed: %S", sessionId,
            e.what());
    }

    return levels;
}

// The levels a daemon can provide in the session it serves. Elevation takes
// its own token and UIAccess borrows privileges from the system account, both
// of which need the daemon to run elevated itself.
unsigned int SupportedLevelsInOwnSession(bool callerElevated) {
    using ToolModProcess::HostLevel;
    using ToolModProcess::HostLevelBit;

    unsigned int levels = HostLevelBit(HostLevel::kNormal);
    if (callerElevated) {
        levels |= HostLevelBit(HostLevel::kUiAccess) |
                  HostLevelBit(HostLevel::kElevated);
    }

    return levels;
}

// The launch a level needs, one per level: onto a session for the service,
// which has no session of its own, into its own for a daemon.
struct HostLauncher {
    ToolModProcess::HostLevel level;
    void (*onSession)(DWORD sessionId,
                      PCWSTR applicationName,
                      PWSTR commandLine);
    void (*inOwnSession)(PCWSTR applicationName, PWSTR commandLine);
};

constexpr HostLauncher kHostLaunchers[] = {
    {ToolModProcess::HostLevel::kNormal, Functions::CreateProcessOnSessionId,
     Functions::CreateProcessAsDesktopUser},
    {ToolModProcess::HostLevel::kUiAccess,
     Functions::CreateProcessOnSessionIdWithUiAccess,
     Functions::CreateProcessAsDesktopUserWithUiAccess},
    {ToolModProcess::HostLevel::kElevated,
     Functions::CreateProcessOnSessionIdElevated,
     Functions::CreateProcessInOwnSessionElevated},
};

static_assert(std::size(kHostLaunchers) == std::size(ToolModProcess::kHosts),
              "A launch for every host level");

const HostLauncher& HostLauncherForLevel(ToolModProcess::HostLevel level) {
    for (const auto& launcher : kHostLaunchers) {
        if (launcher.level == level) {
            return launcher;
        }
    }

    throw std::logic_error("No host launcher for that level");
}

// Only the service gets here: as the system account, it can build the token a
// level needs out of the session's own user.
void LaunchHostOnSessionAtLevel(DWORD sessionId,
                                PCWSTR modId,
                                bool legacy,
                                ToolModProcess::HostLevel level) {
    auto hostPath = GetHostPath(level);

    VERBOSE(L"Launching tool mod host %s for %s on session %u",
            hostPath.filename().c_str(), modId, sessionId);

    auto commandLine = MakeHostCommandLine(hostPath, modId, legacy);

    HostLauncherForLevel(level).onSession(sessionId, hostPath.c_str(),
                                          commandLine.data());
}

void LaunchHostOnSession(DWORD sessionId,
                         PCWSTR modId,
                         const ToolModProcess::ToolModInfo& info,
                         unsigned int supportedLevels) {
    auto levels = EffectiveHostLevels(info, supportedLevels);
    if (levels.empty()) {
        LOG(L"Tool mod %s asks for a host level session %u can't provide",
            modId, sessionId);
        return;
    }

    // A level whose launch fails costs the mod that level, not its host: what
    // it named below is still there to fall to.
    for (ToolModProcess::HostLevel level : levels) {
        try {
            LaunchHostOnSessionAtLevel(sessionId, modId, info.legacy, level);
            return;
        } catch (const std::exception& e) {
            LOG(L"Launching a %s host for %s on session %u failed: %S",
                HostFileNameForLevel(level), modId, sessionId, e.what());
        }
    }
}

// Launches the host in the session the caller runs in, which is what a daemon
// does. At the normal level the mod runs for the user at the desktop, not for
// whoever started the daemon with administrative rights. This process creates
// the host either way, which keeps it on the path the hook which injects the
// engine into new processes watches: a host the engine doesn't reach before its
// entry point runs gives up.
void LaunchHostInOwnSessionAtLevel(PCWSTR modId,
                                   bool legacy,
                                   ToolModProcess::HostLevel level) {
    auto hostPath = GetHostPath(level);

    VERBOSE(L"Launching tool mod host %s for %s in this session",
            hostPath.filename().c_str(), modId);

    auto commandLine = MakeHostCommandLine(hostPath, modId, legacy);

    HostLauncherForLevel(level).inOwnSession(hostPath.c_str(),
                                             commandLine.data());
}

void LaunchHostInOwnSession(PCWSTR modId,
                            const ToolModProcess::ToolModInfo& info,
                            unsigned int supportedLevels) {
    auto levels = EffectiveHostLevels(info, supportedLevels);
    if (levels.empty()) {
        LOG(L"Tool mod %s asks for a host level this install can't provide",
            modId);
        return;
    }

    // A level whose launch fails costs the mod that level, not its host: what
    // it named below is still there to fall to.
    for (ToolModProcess::HostLevel level : levels) {
        try {
            LaunchHostInOwnSessionAtLevel(modId, info.legacy, level);
            return;
        } catch (const std::exception& e) {
            LOG(L"Launching a %s host for %s failed: %S",
                HostFileNameForLevel(level), modId, e.what());
        }
    }
}

}  // namespace

namespace ToolModProcess {

PCWSTR GetHostedModId() {
    // An unqualified name, which is what the destructor call the macro expands
    // to accepts.
    using OptionalModId = std::optional<std::wstring>;

    STATIC_INIT_ONCE(OptionalModId, hostedModId, GetHostedModIdImpl());
    return *hostedModId ? (*hostedModId)->c_str() : nullptr;
}

std::optional<HostLevel> GetCurrentHostLevel() {
    STATIC_INIT_ONCE_TRIVIAL(std::optional<HostLevel>, hostLevel,
                             GetCurrentHostLevelImpl());
    return hostLevel;
}

bool IsAppProcess() {
    // By file name: a tool mod has no place in any windhawk.exe, not only in
    // the one it was installed beside.
    STATIC_INIT_ONCE_TRIVIAL(
        bool, isAppProcess,
        _wcsicmp(GetCurrentProcessFileName().c_str(), kAppFileName) == 0);
    return isAppProcess;
}

bool DoesLibraryExportToolModMarker(const std::filesystem::path& libraryPath) {
    return Functions::DoesFileExportAnyName(libraryPath, kToolModMarkerNames);
}

void LaunchHosts(
    const std::unordered_map<std::wstring, ToolModInfo>& toolMods) {
    if (toolMods.empty()) {
        return;
    }

    // The service, in session 0, launches a host per logged-on session. A
    // daemon serves the one session it runs in.
    if (GetCurrentSessionId() == 0) {
        std::vector<DWORD> sessionIds;
        try {
            sessionIds = Functions::GetLoggedOnSessionIds();
        } catch (const std::exception& e) {
            // Without the sessions there is nothing to launch into.
            LOG(L"Reading the logged-on sessions failed: %S", e.what());
            return;
        }

        for (DWORD sessionId : sessionIds) {
            LaunchHostsOnSession(sessionId, toolMods);
        }

        return;
    }

    unsigned int supportedLevels =
        SupportedLevelsInOwnSession(Functions::IsCurrentProcessElevated());

    for (const auto& [modId, info] : toolMods) {
        // A mod which can't be launched for doesn't stop the others.
        try {
            LaunchHostInOwnSession(modId.c_str(), info, supportedLevels);
        } catch (const std::exception& e) {
            LOG(L"Mod (%s) tool mod host launching failed: %S", modId.c_str(),
                e.what());
        }
    }
}

void LaunchHostsOnSession(
    DWORD sessionId,
    const std::unordered_map<std::wstring, ToolModInfo>& toolMods) {
    if (toolMods.empty()) {
        return;
    }

    unsigned int supportedLevels = SupportedLevelsOnSession(sessionId);

    for (const auto& [modId, info] : toolMods) {
        // A mod which can't be launched for doesn't stop the others.
        try {
            LaunchHostOnSession(sessionId, modId.c_str(), info,
                                supportedLevels);
        } catch (const std::exception& e) {
            LOG(L"Launching a host for %s on session %u failed: %S",
                modId.c_str(), sessionId, e.what());
        }
    }
}

}  // namespace ToolModProcess
