#pragma once

// Volatile-registry channel for transient, per-injected-process mod metadata:
// mod status (which mods are loaded in which processes) and in-progress tasks
// (initializing, loading symbols, etc.). The engine writes it from inside every
// injected process; the app reads it to populate its task-manager dialogs.
//
// It lives in the registry, not in files, so that frequent updates from many
// processes don't trigger antivirus file scanning, and in volatile keys, so the
// data is memory-backed and vanishes with the hive holding it. Volatile keys
// are never written to the file behind an application hive either.
//
// Layout (the HKEY_LOCAL_MACHINE container is opened with KEY_WOW64_64KEY, so
// 32-bit engines share the 64-bit view, and keys opened through a container
// handle inherit its view):
//
//   <container>                                          volatile container
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
// The session id doesn't depend on the configurable settings location, so the
// path is the same in registry and portable (INI) storage modes.

namespace SessionMetadata {

// The container under HKEY_LOCAL_MACHINE, which holds the store whenever the
// session manager can create it there.
inline constexpr WCHAR kRootSubKey[] = L"SOFTWARE\\WindhawkSessions";

// A portable session manager without administrative rights is denied that key
// and puts the store in an application hive instead, whose root is then the
// container. A non-portable session manager runs as SYSTEM and never falls
// back: a store only it could reach would be worse than none.
//
// Addressing the hive by file path is what makes it work where a user hive
// doesn't: MSIX redirects a packaged process's HKEY_CURRENT_USER writes into a
// per-package hive nothing outside the package can read, and application hives
// are exempt from that redirection.
//
// It can't be a hive RegLoadAppKey creates for itself, which carries a Low
// integrity label that shuts out engines running below Low. Nothing can correct
// that afterwards - all keys in an application hive share the hive's own
// descriptor, and RegSetKeySecurity and per-key descriptors at creation are
// both refused - so the session manager writes the file from a template built
// into the engine.
//
// The file lives in the engine's data folder, which only engine.ini names, so
// the app reads it from there to arrive at the same file. RegLoadAppKey needs
// write access to it, so every party has it, which makes the file untrusted
// input to the kernel's hive parser for as long as it sits on disk unloaded.
//
// Naming the file after the session that writes it is what keeps that from
// mattering. The folder grants everyone read access only, so the one thing an
// outside party can do is rewrite the bytes of a file that's already there, and
// a session manager only ever creates its own: from the moment it loads the
// file the registry holds it exclusively, and it removes it at session end. A
// file left behind by a session manager that crashed carries the id of a
// session that no longer exists, so no party ever derives its name to load it
// again, and the next session manager deletes it.
//
// Every party resolves the container the same way, HKEY_LOCAL_MACHINE first.
// Lookups carry the session id, which is unique to the session manager process,
// so another session manager's container never matches.

// The file backing a session's hive, "sessions-<sessionId>.hiv" in the engine's
// data folder. Throws if that folder can't be determined.
std::filesystem::path MakeHiveFilePath(std::wstring_view sessionId);

// The session id embedded in a hive file name, without validating it as one.
// Nullopt for a name of any other shape.
std::optional<std::wstring> ParseHiveFileName(std::wstring_view fileName);

// Loads the application hive backing the given session's store and returns its
// root key.
//
// The hive stays loaded while a handle to any key inside it is open. The
// session manager keeps this handle for the whole session; everyone else can
// let it go once they hold the key they came for, which keeps it loaded just as
// well.
//
// The file has to be there already: only the session manager places one, for
// its own session, and only on a portable install.
LSTATUS LoadStoreHive(std::wstring_view sessionId, wil::unique_hkey& hiveOut);

inline constexpr WCHAR kCategoryModStatus[] = L"mod-status";
inline constexpr WCHAR kCategoryModTask[] = L"mod-task";

std::wstring MakeSessionId(DWORD sessionManagerProcessId,
                           ULONGLONG sessionManagerProcessCreationTime);

// "<sessionId>\<category>", relative to the container.
std::wstring MakeCategorySubKey(std::wstring_view sessionId,
                                std::wstring_view category);

// Opens an existing category key of the given session, under whichever
// container holds that session's store. On failure, reports the
// HKEY_LOCAL_MACHINE error, the more informative one.
LSTATUS OpenStoreCategoryKey(std::wstring_view sessionId,
                             PCWSTR category,
                             REGSAM sam,
                             wil::unique_hkey& keyOut);

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
            // A concurrent writer grew a value past the reported size.
            // RegEnumValue doesn't say which buffer is too small, so re-query
            // the maximums and retry the same index. A pass that grows neither
            // buffer stops the enumeration instead of retrying forever.
            DWORD newMaxNameLen = 0;
            DWORD newMaxDataLen = 0;
            if (RegQueryInfoKey(key, nullptr, nullptr, nullptr, nullptr,
                                nullptr, nullptr, nullptr, &newMaxNameLen,
                                &newMaxDataLen, nullptr,
                                nullptr) != ERROR_SUCCESS) {
                break;
            }

            size_t newNameBufferSize = static_cast<size_t>(newMaxNameLen) + 1;
            size_t newDataBufferSize =
                static_cast<size_t>(newMaxDataLen) / sizeof(WCHAR) + 1;
            if (newNameBufferSize <= nameBuffer.size() &&
                newDataBufferSize <= dataBuffer.size()) {
                break;
            }

            if (newNameBufferSize > nameBuffer.size()) {
                nameBuffer.resize(newNameBufferSize);
            }
            if (newDataBufferSize > dataBuffer.size()) {
                dataBuffer.resize(newDataBufferSize);
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
