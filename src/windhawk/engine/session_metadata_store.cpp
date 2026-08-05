#include "stdafx.h"

#include "functions.h"
#include "logger.h"
#include "session_metadata.h"
#include "session_metadata_hive_template.h"
#include "session_metadata_store.h"
#include "storage_manager.h"

namespace {

using unique_sid_local =
    wil::unique_any<PSID, decltype(&::LocalFree), ::LocalFree>;

// Creates (or opens) a registry key, applying the SDDL security descriptor when
// the key is newly created. A null sddl leaves it with the security it
// inherits, which is all the application hive allows. Returns a Win32 error
// code.
DWORD CreateKeyWithSddl(HKEY parent,
                        PCWSTR subKey,
                        DWORD options,
                        REGSAM sam,
                        PCWSTR sddl,
                        wil::unique_hkey& keyOut) {
    wil::unique_hlocal secDesc;
    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};

    if (sddl) {
        if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
                sddl, SDDL_REVISION_1, &secDesc, nullptr)) {
            return GetLastError();
        }

        secAttr.lpSecurityDescriptor = secDesc.get();
        secAttr.bInheritHandle = FALSE;
    }

    return RegCreateKeyEx(parent, subKey, 0, nullptr, options, sam,
                          sddl ? &secAttr : nullptr, &keyOut, nullptr);
}

// Lets injected engines of any integrity level open the hive file, which
// RegLoadAppKey needs read and write access to; the data folder holding it
// grants sandboxed processes read access only.
//
// Read and write, deliberately not full control: the grantees include
// low-integrity and sandboxed processes, which must not be able to rewrite the
// file's DACL (WRITE_DAC) or take ownership (WRITE_OWNER). DELETE is withheld
// too, nothing but the session manager having reason to remove the file.
void EnsureHiveFileAccess(const std::wstring& filePath) {
    unique_sid_local sids[3];
    PCWSTR sidStrings[] = {L"S-1-1-0", L"S-1-15-2-1", L"S-1-15-2-2"};

    Functions::DaclAce aces[ARRAYSIZE(sidStrings)];
    for (size_t i = 0; i < ARRAYSIZE(sidStrings); i++) {
        PSID sid = nullptr;
        if (!ConvertStringSidToSid(sidStrings[i], &sid)) {
            LOG(L"ConvertStringSidToSid(%s) failed: %u", sidStrings[i],
                GetLastError());
            return;
        }

        sids[i].reset(sid);
        aces[i] = {sid, FILE_GENERIC_READ | FILE_GENERIC_WRITE, 0};
    }

    DWORD error = Functions::EnsureFileDaclContainsAces(filePath.c_str(), aces,
                                                        ARRAYSIZE(aces));
    if (error != ERROR_SUCCESS) {
        LOG(L"Failed to set permissions for %s: %u", filePath.c_str(), error);
        return;
    }

    // An unlabeled file counts as medium integrity, which No-Write-Up turns
    // into a write denial for every low-integrity engine. Label it Untrusted
    // like the category keys, so the hive's own label is what decides.
    wil::unique_hlocal secDesc;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
            L"S:(ML;;NW;;;S-1-16-0)", SDDL_REVISION_1, &secDesc, nullptr)) {
        LOG(L"Failed to build the hive file label: %u", GetLastError());
        return;
    }

    BOOL saclPresent = FALSE;
    PACL sacl = nullptr;
    BOOL saclDefaulted = FALSE;
    if (!GetSecurityDescriptorSacl(secDesc.get(), &saclPresent, &sacl,
                                   &saclDefaulted)) {
        LOG(L"Failed to read the hive file label: %u", GetLastError());
        return;
    }

    error = SetNamedSecurityInfo(const_cast<PWSTR>(filePath.c_str()),
                                 SE_FILE_OBJECT, LABEL_SECURITY_INFORMATION,
                                 nullptr, nullptr, nullptr, sacl);
    if (error != ERROR_SUCCESS) {
        LOG(L"Failed to label %s: %u", filePath.c_str(), error);
    }
}

// Recovery logs the registry creates next to a hive file it loads; they outlive
// the unload.
constexpr PCWSTR kHiveFileLogSuffixes[] = {L".LOG1", L".LOG2"};

// Removes the hive file and its logs. Silent about failure: a file another
// party still has loaded can't be removed, which the callers handle themselves.
void RemoveHiveFiles(const std::filesystem::path& hivePath) {
    std::error_code ec;
    std::filesystem::remove(hivePath, ec);

    for (PCWSTR logSuffix : kHiveFileLogSuffixes) {
        std::filesystem::path logPath = hivePath;
        logPath += logSuffix;
        std::filesystem::remove(logPath, ec);
    }
}

// Writes the template built into the engine out as the hive file. Returns false
// when no usable file ends up there, leaving the session without a store: only
// the template carries a descriptor the engines can write through.
//
// CREATE_NEW, and no adopting a file that's already there: the name belongs to
// this process alone, so the only way one exists is that a store of an earlier
// session in this same process couldn't remove it, having been outlived by
// another party's handle keeping the hive loaded. Such a file has had no window
// in which anyone could rewrite it, but nothing here can tell it apart from one
// that has, and the file is writable by every local process.
bool PlaceHiveFile(const std::filesystem::path& hivePath) {
    wil::unique_hfile file(CreateFile(hivePath.c_str(), GENERIC_WRITE, 0,
                                      nullptr, CREATE_NEW,
                                      FILE_ATTRIBUTE_NORMAL, nullptr));
    if (!file) {
        LOG(L"Failed to create the session metadata store hive: %u",
            GetLastError());
        return false;
    }

    constexpr DWORD kTemplateSize = sizeof(kSessionMetadataHiveTemplate);

    DWORD written = 0;
    if (!WriteFile(file.get(), kSessionMetadataHiveTemplate, kTemplateSize,
                   &written, nullptr) ||
        written != kTemplateSize) {
        LOG(L"Failed to write the session metadata store hive: %u",
            GetLastError());

        // A truncated hive would only have every reader try to load it in turn.
        file.reset();
        RemoveHiveFiles(hivePath);
        return false;
    }

    return true;
}

// Removes the hive files of session managers that are no longer running, left
// behind by ones that exited without removing their own. Nothing ever loads
// such a file, its name naming a session that's gone, but it stays writable by
// every local process, so there's no reason to leave it lying around.
void RemoveStaleHiveFiles(const std::wstring& currentSessionId) {
    // Collect the paths first: removing entries mid-iteration isn't guaranteed
    // to leave the walk on its feet.
    std::vector<std::filesystem::path> stalePaths;
    SessionMetadata::ProcessLivenessChecker livenessChecker;

    // The error-code overloads throughout: a folder that can't be walked leaves
    // nothing to do here, and the session has a store to set up either way.
    std::error_code ec;
    auto entry = std::filesystem::directory_iterator(
        StorageManager::GetInstance().GetEngineAppDataPath(), ec);
    const std::filesystem::directory_iterator end;

    for (; !ec && entry != end; entry.increment(ec)) {
        auto sessionId = SessionMetadata::ParseHiveFileName(
            entry->path().filename().native());
        if (!sessionId || *sessionId == currentSessionId) {
            continue;
        }

        auto parsed = SessionMetadata::ParseSessionId(*sessionId);
        if (!parsed) {
            // Unrecognized format, possibly written by another Windhawk
            // version; leave it alone.
            continue;
        }

        if (livenessChecker.IsProcessAlive(parsed->processId,
                                           parsed->processCreationTime)) {
            continue;  // owned by another live session manager
        }

        stalePaths.push_back(entry->path());
    }

    for (const auto& stalePath : stalePaths) {
        RemoveHiveFiles(stalePath);
    }
}

// Removes session subkeys left behind by session managers that are no longer
// running (e.g. crashed without cleanup): volatile keys outlive the process and
// only vanish once the hive holding them is unloaded.
void CleanupStaleSessionKeys(HKEY containerKey,
                             const std::wstring& currentSessionId) {
    DWORD maxNameLen = 0;  // in characters, without the terminating null
    if (RegQueryInfoKey(containerKey, nullptr, nullptr, nullptr, nullptr,
                        &maxNameLen, nullptr, nullptr, nullptr, nullptr,
                        nullptr, nullptr) != ERROR_SUCCESS) {
        return;
    }

    // Snapshot the subkey names first: deleting a subkey shifts the enumeration
    // indices, which would otherwise skip entries.
    std::vector<std::wstring> sessionIds;
    std::wstring nameBuffer(maxNameLen + 1, L'\0');
    for (DWORD index = 0;;) {
        DWORD nameLen = static_cast<DWORD>(nameBuffer.size());
        LSTATUS error =
            RegEnumKeyEx(containerKey, index, nameBuffer.data(), &nameLen,
                         nullptr, nullptr, nullptr, nullptr);
        if (error == ERROR_MORE_DATA) {
            // A concurrent writer created a subkey longer than the size
            // RegQueryInfoKey reported. RegEnumKeyEx doesn't report the
            // required size, so grow the buffer and retry the same index.
            nameBuffer.resize(nameBuffer.size() * 2);
            continue;
        }
        if (error != ERROR_SUCCESS) {
            break;  // ERROR_NO_MORE_ITEMS or an unexpected error
        }
        index++;

        sessionIds.emplace_back(nameBuffer.data(), nameLen);
    }

    SessionMetadata::ProcessLivenessChecker livenessChecker;

    for (const auto& sessionId : sessionIds) {
        if (sessionId == currentSessionId) {
            continue;
        }

        auto parsed = SessionMetadata::ParseSessionId(sessionId);
        if (!parsed) {
            // Unrecognized format, possibly written by another Windhawk
            // version; leave it alone.
            continue;
        }

        if (livenessChecker.IsProcessAlive(parsed->processId,
                                           parsed->processCreationTime)) {
            continue;  // owned by another live session manager
        }

        RegDeleteTree(containerKey, sessionId.c_str());
    }
}

// The session id for the current (session-manager) process. Injected engines
// and the app derive the same id from its pid and creation time.
std::wstring GetCurrentSessionId() {
    FILETIME creationTime;
    FILETIME exitTime;
    FILETIME kernelTime;
    FILETIME userTime;
    THROW_IF_WIN32_BOOL_FALSE(GetProcessTimes(
        GetCurrentProcess(), &creationTime, &exitTime, &kernelTime, &userTime));

    return SessionMetadata::MakeSessionId(
        GetCurrentProcessId(), wil::filetime::to_int64(creationTime));
}

// Deletes entries in one category whose owning process is gone, along with
// malformed ones any local process may have written to this world-writable key.
// Volatile values are removed on a graceful mod unload but not on process exit,
// so the session manager sweeps them.
void SweepDeadEntriesInCategory(HKEY containerKey,
                                const std::wstring& sessionId,
                                PCWSTR category) {
    std::wstring subKey =
        SessionMetadata::MakeCategorySubKey(sessionId, category);

    wil::unique_hkey key;
    if (RegOpenKeyEx(containerKey, subKey.c_str(), 0,
                     KEY_QUERY_VALUE | KEY_SET_VALUE, &key) != ERROR_SUCCESS) {
        return;
    }

    // Pruning is the whole job, so the callback just keeps the walk going.
    SessionMetadata::PruneDeadEntriesAndVisitLive(
        key.get(),
        [](const std::wstring&, const SessionMetadata::ParsedValueName&,
           const SessionMetadata::ParsedValueData&) { return true; });
}

}  // namespace

SessionMetadataStore::SessionMetadataStore() noexcept {
    // A constructor's function-try-block rethrows at the end of its handler, so
    // the guard that makes this best-effort lives in EnsureKeys.
    EnsureKeys();
}

SessionMetadataStore::~SessionMetadataStore() {
    DeleteKeys();
}

void SessionMetadataStore::EnsureKeys() noexcept try {
    std::wstring sessionId = GetCurrentSessionId();

    // Only a portable instance ever places hive files, and it can reach the
    // machine container in a session that follows one that couldn't, so the
    // files of past sessions are swept before the container is settled on
    // rather than only where new ones are placed.
    if (StorageManager::GetInstance().IsPortable()) {
        RemoveStaleHiveFiles(sessionId);
    }

    // Read/traverse for the shared sandbox SIDs, full control for SYSTEM and
    // Administrators, which is what the session manager runs as wherever these
    // apply: the application hive gives its keys the hive's own security
    // instead. Injected engines only read the container and session keys, on
    // the way to the category keys, so no integrity label is needed here -
    // No-Write-Up blocks writes, not reads.
    constexpr WCHAR kReadSddl[] =
        L"D:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;WD)(A;;KR;;;S-1-15-2-1)"
        L"(A;;KR;;;S-1-15-2-2)";

    // Written by injected engines of any integrity level, so the shared SIDs
    // get KEY_QUERY_VALUE|KEY_SET_VALUE|KEY_NOTIFY|READ_CONTROL (0x00020013) -
    // deliberately not KEY_WRITE, which would add KEY_CREATE_SUB_KEY and link
    // creation across the trust boundary - under an Untrusted integrity label,
    // so No-Write-Up doesn't block a low-integrity writer.
    //
    // Any local process can therefore add, overwrite or delete values, so
    // readers treat them as untrusted input. The worst a hostile writer gets is
    // spoofed or missing entries in the task-manager dialogs (the pid-reuse
    // guard in IsProcessAlive limits spoofs to live pids), a cosmetic annoyance
    // rather than a privilege boundary.
    constexpr WCHAR kCategorySddl[] =
        L"D:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;0x00020013;;;WD)"
        L"(A;;0x00020013;;;S-1-15-2-1)(A;;0x00020013;;;S-1-15-2-2)"
        L"S:(ML;;NW;;;S-1-16-0)";

    // Settle on a container: the key under HKEY_LOCAL_MACHINE, or the
    // application hive when a portable instance is denied it. See
    // shared/session_metadata.h. Keys reached through the container handle
    // inherit its registry view, so KEY_WOW64_64KEY is asked for here only.
    DWORD containerError = CreateKeyWithSddl(
        HKEY_LOCAL_MACHINE, SessionMetadata::kRootSubKey, REG_OPTION_VOLATILE,
        DELETE | KEY_READ | KEY_SET_VALUE | KEY_CREATE_SUB_KEY |
            KEY_WOW64_64KEY,
        kReadSddl, m_container);

    // Keys inside the application hive take the hive's own security descriptor
    // and can't be given one of their own.
    PCWSTR sessionKeySddl = kReadSddl;
    PCWSTR categoryKeySddl = kCategorySddl;

    if (containerError == ERROR_SUCCESS) {
        LOG(L"Session metadata store: HKEY_LOCAL_MACHINE");

        // Explain the key to anyone browsing the registry.
        constexpr WCHAR kContainerNote[] =
            L"Windhawk session data. This key is volatile (temporary) and is "
            L"removed once the hive holding it is unloaded, at reboot or at "
            L"logoff.";
        RegSetValueEx(m_container.get(), nullptr, 0, REG_SZ,
                      reinterpret_cast<const BYTE*>(kContainerNote),
                      sizeof(kContainerNote));

        // The machine container is shared by every session that reaches it, so
        // it accumulates the keys of the ones that are gone.
        CleanupStaleSessionKeys(m_container.get(), sessionId);
    } else {
        VERBOSE(
            L"Failed to ensure the session container key under "
            L"HKEY_LOCAL_MACHINE: %u",
            containerError);

        // Expected when a portable session manager has no administrative
        // rights, which is what the hive is there for. Any other failure stops
        // here rather than putting the store somewhere it doesn't belong.
        if (containerError != ERROR_ACCESS_DENIED ||
            !StorageManager::GetInstance().IsPortable()) {
            LOG(L"Failed to ensure the session container key: %u",
                containerError);
            return;
        }

        std::filesystem::path hivePath =
            SessionMetadata::MakeHiveFilePath(sessionId);

        // A store of an earlier session in this process may have been unable to
        // remove the file; taking it out of the way here is what lets this one
        // start from a file it wrote itself.
        RemoveHiveFiles(hivePath);

        if (!PlaceHiveFile(hivePath)) {
            return;
        }

        DWORD hiveError =
            SessionMetadata::LoadStoreHive(sessionId, m_container);
        if (hiveError != ERROR_SUCCESS) {
            LOG(L"Failed to load the session metadata store hive: %u",
                hiveError);

            // Leaving a file that failed to load would only have every reader
            // try it in turn.
            RemoveHiveFiles(hivePath);
            return;
        }

        m_containerIsHive = true;

        EnsureHiveFileAccess(hivePath.native());

        sessionKeySddl = nullptr;
        categoryKeySddl = nullptr;

        LOG(L"Session metadata store: application hive");
    }

    wil::unique_hkey sessionKey;
    DWORD error = CreateKeyWithSddl(
        m_container.get(), sessionId.c_str(), REG_OPTION_VOLATILE,
        KEY_READ | KEY_CREATE_SUB_KEY, sessionKeySddl, sessionKey);
    if (error != ERROR_SUCCESS) {
        LOG(L"Failed to create session key: %u", error);
        return;
    }

    for (PCWSTR category : {SessionMetadata::kCategoryModStatus,
                            SessionMetadata::kCategoryModTask}) {
        wil::unique_hkey categoryKey;
        error =
            CreateKeyWithSddl(sessionKey.get(), category, REG_OPTION_VOLATILE,
                              KEY_READ, categoryKeySddl, categoryKey);
        if (error != ERROR_SUCCESS) {
            LOG(L"Failed to create %s key: %u", category, error);
        }
    }
} catch (const std::exception& e) {
    LOG(L"Ensuring the session keys failed: %S", e.what());
} catch (...) {
    LOG(L"Ensuring the session keys failed");
}

void SessionMetadataStore::SweepDeadEntries() noexcept try {
    if (!m_container) {
        return;
    }

    std::wstring sessionId = GetCurrentSessionId();
    SweepDeadEntriesInCategory(m_container.get(), sessionId,
                               SessionMetadata::kCategoryModStatus);
    SweepDeadEntriesInCategory(m_container.get(), sessionId,
                               SessionMetadata::kCategoryModTask);
} catch (const std::exception& e) {
    LOG(L"Sweeping dead session metadata failed: %S", e.what());
} catch (...) {
    LOG(L"Sweeping dead session metadata failed");
}

void SessionMetadataStore::DeleteKeys() noexcept try {
    if (!m_container) {
        return;  // the store was never set up
    }

    std::wstring sessionId = GetCurrentSessionId();

    RegDeleteTree(m_container.get(), sessionId.c_str());

    if (m_containerIsHive) {
        // Releasing the last handle unloads the hive. The file is of no use to
        // anyone, naming a session that's over, and leaving it would only leave
        // something for another process to write to. The removal fails while
        // another party still has the hive loaded, in which case the next
        // session manager takes care of it.
        m_container.reset();

        RemoveHiveFiles(SessionMetadata::MakeHiveFilePath(sessionId));
    }
} catch (const std::exception& e) {
    LOG(L"Deleting the session keys failed: %S", e.what());
} catch (...) {
    LOG(L"Deleting the session keys failed");
}
