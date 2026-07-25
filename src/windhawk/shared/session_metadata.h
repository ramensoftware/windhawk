#pragma once

// Volatile-registry channel for transient, per-injected-process mod metadata:
// mod status (which mods are loaded in which processes) and in-progress tasks
// (initializing, loading symbols, etc.). The engine writes it from inside every
// injected process; the app reads it to populate its task-manager dialogs.
//
// It lives in the registry, not in files, so that frequent updates from many
// processes don't trigger antivirus file scanning, and it uses volatile keys so
// the data is memory-backed and disappears on reboot.
//
// Layout (KEY_WOW64_64KEY throughout, so 32-bit engines share the 64-bit view):
//
//   HKLM\SOFTWARE\WindhawkSessions                       volatile container
//     <sessionId>                                        volatile
//       mod-status                                       volatile
//         <targetPid>_<modName> = REG_SZ  <value data>
//       mod-task                                         volatile
//         <targetPid>_<modName> = REG_SZ  <value data>
//
// sessionId  = "<sessionManagerProcessId>_<sessionManagerProcessCreationTime>".
// value data =
// "<entryCreationTime>|<targetProcessCreationTime>|<image>|<value>".
//
// The session id is fixed and independent of the configurable settings
// location, so the path is identical for the registry and portable (INI)
// storage modes. The root key is HKEY_LOCAL_MACHINE.

namespace SessionMetadata {

inline constexpr WCHAR kRootSubKey[] = L"SOFTWARE\\WindhawkSessions";

inline constexpr WCHAR kCategoryModStatus[] = L"mod-status";
inline constexpr WCHAR kCategoryModTask[] = L"mod-task";

std::wstring MakeSessionId(DWORD sessionManagerProcessId,
                           ULONGLONG sessionManagerProcessCreationTime);

// "SOFTWARE\WindhawkSessions\<sessionId>"
std::wstring MakeSessionSubKey(std::wstring_view sessionId);

// "SOFTWARE\WindhawkSessions\<sessionId>\<category>"
std::wstring MakeCategorySubKey(std::wstring_view sessionId,
                                std::wstring_view category);

// Value name: "<targetProcessId>_<modName>".
std::wstring MakeValueName(DWORD targetProcessId, std::wstring_view modName);

// Value data:
// "<entryCreationTime>|<targetProcessCreationTime>|<image>|<value>".
std::wstring FormatValueData(ULONGLONG entryCreationTime,
                             ULONGLONG targetProcessCreationTime,
                             std::wstring_view processImageName,
                             std::wstring_view value);

struct ParsedSessionId {
    DWORD processId;
    ULONGLONG processCreationTime;
};

// Parses a session subkey name back into its "<pid>_<creationTime>" parts.
std::optional<ParsedSessionId> ParseSessionId(std::wstring_view sessionId);

struct ParsedValueName {
    DWORD targetProcessId;
    std::wstring modName;
};

// Parses a value name back into "<targetPid>_<modName>". The mod name may
// itself contain underscores, so only the first underscore is treated as the
// separator.
std::optional<ParsedValueName> ParseValueName(std::wstring_view valueName);

struct ParsedValueData {
    ULONGLONG entryCreationTime;
    ULONGLONG targetProcessCreationTime;
    std::wstring processImageName;
    std::wstring value;
};

// Parses value data. The value field is last and may contain '|', so only the
// first three delimiters are used; the rest is the value verbatim.
std::optional<ParsedValueData> ParseValueData(std::wstring_view data);

// Answers "is this process still alive?" while guarding against pid reuse via
// the creation time. It first tries OpenProcess, which is cheap and definitive
// for any process the caller can open. Only when OpenProcess fails for a reason
// other than the process being gone - typically access-denied for a
// higher-integrity or another-user process - does it fall back to a one-shot
// NtQuerySystemInformation(SystemProcessInformation) snapshot, which sees those
// processes too. The snapshot is built lazily and reused, so the costly
// enumeration happens at most once per checker and only when actually needed.
// Callers build one per sweep and query it per entry, so it lives here next to
// the value format it validates.
class ProcessLivenessChecker {
   public:
    bool IsProcessAlive(DWORD processId, ULONGLONG processCreationTime);

   private:
    // Liveness via the lazily-built system-wide snapshot, the fallback for when
    // OpenProcess can't answer. Returns true if the snapshot couldn't be taken,
    // so a live entry is never pruned merely because enumeration failed.
    bool IsProcessAliveViaSnapshot(DWORD processId,
                                   ULONGLONG processCreationTime);

    bool snapshotBuilt_ = false;
    bool snapshotValid_ = false;
    std::unordered_map<DWORD, ULONGLONG> snapshot_;
};

// Enumerates the REG_SZ values of an open key, invoking fn(name, data) for
// each. fn returns false to stop early. fn must not delete the current value:
// deleting shifts the indices and would skip entries, so collect names and
// delete after enumeration.
template <typename Fn>
inline void EnumRegistryStringValues(HKEY key, Fn&& fn) {
    DWORD maxNameLen = 0;  // in characters, without the terminating null
    DWORD maxDataLen = 0;  // in bytes
    if (RegQueryInfoKey(key, nullptr, nullptr, nullptr, nullptr, nullptr,
                        nullptr, nullptr, &maxNameLen, &maxDataLen, nullptr,
                        nullptr) != ERROR_SUCCESS) {
        return;
    }

    std::wstring nameBuffer(maxNameLen + 1, L'\0');
    std::wstring dataBuffer(maxDataLen / sizeof(WCHAR) + 1, L'\0');

    for (DWORD index = 0;;) {
        DWORD nameLen = static_cast<DWORD>(nameBuffer.size());
        DWORD dataSize = static_cast<DWORD>(dataBuffer.size() * sizeof(WCHAR));
        DWORD type = 0;
        LSTATUS status = RegEnumValue(
            key, index, nameBuffer.data(), &nameLen, nullptr, &type,
            reinterpret_cast<BYTE*>(dataBuffer.data()), &dataSize);
        if (status == ERROR_MORE_DATA) {
            // A concurrent writer grew a value past the size RegQueryInfoKey
            // reported. RegEnumValue reports the required data size but not the
            // name size, so grow the data buffer when it's the one that no
            // longer fits and the name buffer otherwise, then retry the same
            // index. Each pass enlarges exactly one buffer, so this converges.
            if (dataSize > dataBuffer.size() * sizeof(WCHAR)) {
                dataBuffer.resize(dataSize / sizeof(WCHAR) + 1);
            } else {
                nameBuffer.resize(nameBuffer.size() * 2);
            }
            continue;
        }
        if (status != ERROR_SUCCESS) {
            break;  // ERROR_NO_MORE_ITEMS or an unexpected error
        }
        index++;

        if (type != REG_SZ) {
            continue;
        }

        std::wstring data(dataBuffer.data(), dataSize / sizeof(WCHAR));
        if (!data.empty() && data.back() == L'\0') {
            data.pop_back();  // stored strings include the terminating null
        }

        std::wstring name(nameBuffer.data(), nameLen);
        if (!fn(name, data)) {
            break;
        }
    }
}

// Deletes the entries of an open category key whose owning process has exited,
// as well as the ones whose name or data doesn't match the expected format -
// those name no owning process, so nothing else could ever reclaim them. Names
// are collected during enumeration and deleted afterward, since deleting a
// value mid-enumeration shifts the indices and would skip entries. Invokes
// onLiveEntry(name, parsedName, parsedData) for each entry whose process is
// still alive. onLiveEntry returns false to stop early; entries seen before the
// stop are still deleted, later ones are left for a subsequent pass. The key
// must be open with KEY_QUERY_VALUE and KEY_SET_VALUE.
template <typename Fn>
inline void PruneDeadEntriesAndVisitLive(HKEY key, Fn&& onLiveEntry) {
    ProcessLivenessChecker livenessChecker;
    std::vector<std::wstring> valueNamesToDelete;

    EnumRegistryStringValues(key, [&](const std::wstring& name,
                                      const std::wstring& data) {
        auto parsedName = ParseValueName(name);
        auto parsedData = ParseValueData(data);
        if (!parsedName || !parsedData) {
            valueNamesToDelete.push_back(name);
            return true;
        }

        if (!livenessChecker.IsProcessAlive(
                parsedName->targetProcessId,
                parsedData->targetProcessCreationTime)) {
            valueNamesToDelete.push_back(name);
            return true;
        }

        return static_cast<bool>(onLiveEntry(name, *parsedName, *parsedData));
    });

    for (const auto& name : valueNamesToDelete) {
        RegDeleteValue(key, name.c_str());
    }
}

}  // namespace SessionMetadata
