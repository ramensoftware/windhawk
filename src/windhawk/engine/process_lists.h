#pragma once

namespace ProcessLists {

// Based on:
// https://www.elastic.co/guide/en/security/current/unusual-parent-child-relationship.html
// https://github.com/elastic/security-docs/blob/9e98d789cb7b8d8fe98a3c3dec5012c4e1f22e99/docs/detections/prebuilt-rules/rule-details/unusual-parent-child-relationship.asciidoc
inline constexpr WCHAR kCriticalProcesses[] =
    LR"(%systemroot%\system32\autochk.exe|)"
    LR"(%systemroot%\syswow64\autochk.exe|)"
    LR"(%systemroot%\system32\chkdsk.exe|)"
    LR"(%systemroot%\syswow64\chkdsk.exe|)"
    // LR"(%systemroot%\system32\conhost.exe|)"
    LR"(%systemroot%\system32\consent.exe|)"
    LR"(%systemroot%\system32\csrss.exe|)"
    // LR"(%systemroot%\system32\dllhost.exe|)"
    // LR"(%systemroot%\syswow64\dllhost.exe|)"
    LR"(%systemroot%\system32\doskey.exe|)"
    LR"(%systemroot%\syswow64\doskey.exe|)"
    LR"(%systemroot%\system32\dwm.exe|)"
    LR"(%systemroot%\system32\fontdrvhost.exe|)"
    LR"(%systemroot%\system32\logonui.exe|)"
    LR"(%systemroot%\system32\lsaiso.exe|)"
    LR"(%systemroot%\system32\lsass.exe|)"
    // LR"(%systemroot%\system32\runtimebroker.exe|)"
    LR"(%systemroot%\system32\searchindexer.exe|)"
    LR"(%systemroot%\syswow64\searchindexer.exe|)"
    LR"(%systemroot%\system32\searchprotocolhost.exe|)"
    LR"(%systemroot%\syswow64\searchprotocolhost.exe|)"
    LR"(%systemroot%\system32\services.exe|)"
    LR"(%systemroot%\system32\setupcl.exe|)"
    LR"(%systemroot%\system32\smss.exe|)"
    LR"(%systemroot%\system32\spoolsv.exe|)"
    // LR"(%systemroot%\system32\svchost.exe|)"
    // LR"(%systemroot%\syswow64\svchost.exe|)"
    LR"(%systemroot%\system32\taskhostw.exe|)"
    // LR"(%systemroot%\system32\userinit.exe|)"
    // LR"(%systemroot%\syswow64\userinit.exe|)"
    // LR"(%systemroot%\system32\werfault.exe|)"
    // LR"(%systemroot%\syswow64\werfault.exe|)"
    LR"(%systemroot%\system32\werfaultsecure.exe|)"
    LR"(%systemroot%\syswow64\werfaultsecure.exe|)"
    LR"(%systemroot%\system32\wermgr.exe|)"
    LR"(%systemroot%\syswow64\wermgr.exe|)"
    LR"(%systemroot%\system32\wininit.exe|)"
    // LR"(%systemroot%\system32\winlogon.exe|)"
    LR"(%systemroot%\system32\winrshost.exe|)"
    LR"(%systemroot%\syswow64\winrshost.exe|)"
    LR"(%systemroot%\system32\wbem\wmiprvse.exe|)"
    LR"(%systemroot%\syswow64\wbem\wmiprvse.exe|)"
    LR"(%systemroot%\system32\wsmprovhost.exe|)"
    LR"(%systemroot%\syswow64\wsmprovhost.exe)";

inline constexpr WCHAR kCriticalProcessesForMods[] =
    LR"(%systemroot%\system32\svchost.exe|)"
    LR"(%systemroot%\syswow64\svchost.exe|)"
    LR"(%systemroot%\system32\werfault.exe|)"
    LR"(%systemroot%\syswow64\werfault.exe|)"
    LR"(%systemroot%\system32\winlogon.exe)";

inline constexpr WCHAR kIncompatiblePrograms[] =
    LR"(%ProgramFiles%\Oracle\VirtualBox\*|)"
    LR"(%ProgramFiles(X86)%\Oracle\VirtualBox\*)";

#define ALL_PROGRAM_FILES(folder) \
    LR"(?:\Program Files\)" folder LR"(\*|?:\Program Files (x86)\)" folder LR"(\*|)"

inline constexpr WCHAR kGames[] =
    ALL_PROGRAM_FILES(L"2K Games")
    ALL_PROGRAM_FILES(L"Activision")
    ALL_PROGRAM_FILES(L"Battle.net")
    ALL_PROGRAM_FILES(L"Bethesda Softworks")
    ALL_PROGRAM_FILES(L"Bethesda.net Launcher")
    ALL_PROGRAM_FILES(L"Blizzard Entertainment")
    ALL_PROGRAM_FILES(L"EA Games")
    ALL_PROGRAM_FILES(L"EA")
    ALL_PROGRAM_FILES(L"EasyAntiCheat_EOS")
    ALL_PROGRAM_FILES(L"Electronic Arts")
    ALL_PROGRAM_FILES(L"Epic Games")
    ALL_PROGRAM_FILES(L"GOG Galaxy")
    ALL_PROGRAM_FILES(L"Google\\Play Games")
    ALL_PROGRAM_FILES(L"Google\\Play Games Services")
    ALL_PROGRAM_FILES(L"Grinding Gear Games")
    ALL_PROGRAM_FILES(L"Microsoft Games")
    ALL_PROGRAM_FILES(L"Origin Games")
    ALL_PROGRAM_FILES(L"Paradox Interactive")
    ALL_PROGRAM_FILES(L"Riot Games")
    ALL_PROGRAM_FILES(L"Rockstar Games")
    ALL_PROGRAM_FILES(L"Square Enix")
    ALL_PROGRAM_FILES(L"Steam")
    ALL_PROGRAM_FILES(L"Ubisoft")
    ALL_PROGRAM_FILES(L"Valve")
    ALL_PROGRAM_FILES(L"Wargaming.net")
    LR"(?:\Epic Games\*|)"
    LR"(?:\Games\*|)"
    LR"(?:\Riot Games\*|)"
    LR"(?:\WindowsApps\Microsoft.MinecraftUWP_*\*|)"
    LR"(?:\WindowsApps\Microsoft.SunriseBaseGame_*\*|)"
    LR"(*\steamapps\common\*)";

#undef ALL_PROGRAM_FILES

}  // namespace ProcessLists
