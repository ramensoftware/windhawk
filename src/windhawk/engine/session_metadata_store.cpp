#include "stdafx.h"

#include "logger.h"
#include "session_metadata.h"
#include "session_metadata_store.h"

namespace {

// Creates (or opens) a registry key with the given options and access in the
// 64-bit view, applying the SDDL security descriptor when the key is newly
// created. Returns a Win32 error code.
DWORD CreateKeyWithSddl(HKEY parent,
                        PCWSTR subKey,
                        DWORD options,
                        REGSAM sam,
                        PCWSTR sddl,
                        wil::unique_hkey& keyOut) {
    wil::unique_hlocal secDesc;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
            sddl, SDDL_REVISION_1, &secDesc, nullptr)) {
        return GetLastError();
    }

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    return RegCreateKeyEx(parent, subKey, 0, nullptr, options,
                          sam | KEY_WOW64_64KEY, &secAttr, &keyOut, nullptr);
}

// Removes session subkeys left behind by session managers that are no longer
// running (e.g. crashed without cleanup). Volatile keys survive a process exit
// and only vanish on reboot, so same-boot stale keys are pruned here.
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
// and the app derive the same id from the session-manager process's pid and
// creation time, so all three agree on the registry path.
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
// malformed ones written by any local process (the category key is
// world-writable). Volatile registry values, unlike the delete-on-close temp
// files they replace, aren't removed when a process exits (only on a graceful
// mod unload), so the session manager sweeps them here.
void SweepDeadEntriesInCategory(const std::wstring& sessionId,
                                PCWSTR category) {
    std::wstring subKey =
        SessionMetadata::MakeCategorySubKey(sessionId, category);

    wil::unique_hkey key;
    if (RegOpenKeyEx(HKEY_LOCAL_MACHINE, subKey.c_str(), 0,
                     KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY,
                     &key) != ERROR_SUCCESS) {
        return;
    }

    // Pruning is the whole job here; live entries need no visiting, so the
    // callback just keeps the walk going.
    SessionMetadata::PruneDeadEntriesAndVisitLive(
        key.get(),
        [](const std::wstring&, const SessionMetadata::ParsedValueName&,
           const SessionMetadata::ParsedValueData&) { return true; });
}

}  // namespace

void EnsureSessionKeys() noexcept try {
    std::wstring sessionId = GetCurrentSessionId();

    // Read/traverse for the shared sandbox SIDs, full control for SYSTEM and
    // Administrators. The container and per-session key are only read by
    // injected engines (to reach the category keys), so they need no integrity
    // label: the default No-Write-Up policy blocks writes, not reads.
    constexpr WCHAR kReadSddl[] =
        L"D:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;WD)(A;;KR;;;S-1-15-2-1)"
        L"(A;;KR;;;S-1-15-2-2)";

    // The category keys are written by injected engines of any integrity level.
    // Grant the shared SIDs
    // KEY_QUERY_VALUE|KEY_SET_VALUE|KEY_NOTIFY|READ_CONTROL (0x00020013) -
    // deliberately not KEY_WRITE, which would add KEY_CREATE_SUB_KEY and link
    // creation across the trust boundary - and stamp an Untrusted integrity
    // label so the mandatory No-Write-Up policy doesn't block a low-integrity
    // writer.
    //
    // Because any local process can therefore add, overwrite, or delete values
    // here, readers must treat every value as untrusted input: the worst a
    // hostile writer can do is spoof or drop entries in the task-manager
    // dialogs (the pid-reuse guard in IsProcessAlive limits spoofs to genuinely
    // live pids), which is a cosmetic annoyance, not a privilege boundary.
    constexpr WCHAR kCategorySddl[] =
        L"D:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;0x00020013;;;WD)"
        L"(A;;0x00020013;;;S-1-15-2-1)(A;;0x00020013;;;S-1-15-2-2)"
        L"S:(ML;;NW;;;S-1-16-0)";

    wil::unique_hkey containerKey;
    DWORD error = CreateKeyWithSddl(
        HKEY_LOCAL_MACHINE, SessionMetadata::kRootSubKey, REG_OPTION_VOLATILE,
        DELETE | KEY_READ | KEY_SET_VALUE | KEY_CREATE_SUB_KEY, kReadSddl,
        containerKey);
    if (error != ERROR_SUCCESS) {
        LOG(L"Failed to ensure session container key: %u", error);
        return;
    }

    // Explain the key to anyone browsing the registry.
    constexpr WCHAR kContainerNote[] =
        L"Windhawk session data. This key is volatile (temporary) and is "
        L"removed after reboot.";
    RegSetValueEx(containerKey.get(), nullptr, 0, REG_SZ,
                  reinterpret_cast<const BYTE*>(kContainerNote),
                  sizeof(kContainerNote));

    CleanupStaleSessionKeys(containerKey.get(), sessionId);

    wil::unique_hkey sessionKey;
    error = CreateKeyWithSddl(
        containerKey.get(), sessionId.c_str(), REG_OPTION_VOLATILE,
        KEY_READ | KEY_CREATE_SUB_KEY, kReadSddl, sessionKey);
    if (error != ERROR_SUCCESS) {
        LOG(L"Failed to create session key: %u", error);
        return;
    }

    for (PCWSTR category : {SessionMetadata::kCategoryModStatus,
                            SessionMetadata::kCategoryModTask}) {
        wil::unique_hkey categoryKey;
        error =
            CreateKeyWithSddl(sessionKey.get(), category, REG_OPTION_VOLATILE,
                              KEY_READ, kCategorySddl, categoryKey);
        if (error != ERROR_SUCCESS) {
            LOG(L"Failed to create %s key: %u", category, error);
        }
    }
} catch (const std::exception& e) {
    LOG(L"EnsureSessionKeys failed: %S", e.what());
} catch (...) {
    LOG(L"EnsureSessionKeys failed");
}

void SweepDeadSessionMetadata() noexcept try {
    std::wstring sessionId = GetCurrentSessionId();
    SweepDeadEntriesInCategory(sessionId, SessionMetadata::kCategoryModStatus);
    SweepDeadEntriesInCategory(sessionId, SessionMetadata::kCategoryModTask);
} catch (const std::exception& e) {
    LOG(L"SweepDeadSessionMetadata failed: %S", e.what());
} catch (...) {
    LOG(L"SweepDeadSessionMetadata failed");
}

void DeleteSessionKeys() noexcept try {
    std::wstring sessionId = GetCurrentSessionId();

    wil::unique_hkey containerKey;
    if (RegOpenKeyEx(HKEY_LOCAL_MACHINE, SessionMetadata::kRootSubKey, 0,
                     DELETE | KEY_READ | KEY_WOW64_64KEY,
                     &containerKey) == ERROR_SUCCESS) {
        RegDeleteTree(containerKey.get(), sessionId.c_str());
    }
} catch (const std::exception& e) {
    LOG(L"DeleteSessionKeys failed: %S", e.what());
} catch (...) {
    LOG(L"DeleteSessionKeys failed");
}
