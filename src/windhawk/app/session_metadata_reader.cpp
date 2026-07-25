#include "stdafx.h"

#include "session_metadata_reader.h"

#include "session_metadata.h"

namespace {

// REG_NOTIFY_THREAD_AGNOSTIC keeps the notification alive after the thread that
// registered it exits, and lets it be re-armed from another thread.
constexpr DWORD kRegNotifyChangeKeyValueFlags =
    REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC;

// Opens this session's registry key for one metadata category in the 64-bit
// view, with set-value access for pruning unusable entries. Returns an empty
// handle if the key doesn't exist (e.g. the engine isn't running or safe mode
// is on).
wil::unique_hkey OpenSessionCategoryKey(const std::wstring& sessionId,
                                        PCWSTR category) {
    std::wstring subKey =
        SessionMetadata::MakeCategorySubKey(sessionId, category);

    wil::unique_hkey key;
    RegOpenKeyEx(HKEY_LOCAL_MACHINE, subKey.c_str(), 0,
                 KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64_64KEY, &key);
    return key;
}

// Enumerates one category's entries, deleting the ones whose owning process has
// exited and the malformed ones as it encounters them, and invoking
// onLiveEntry(name, parsedName, parsedData) for each entry whose process is
// still alive. onLiveEntry returns false to stop early; entries seen before the
// stop are still deleted, while ones after it are left for a later read or the
// session manager.
template <typename Fn>
void ForEachLiveEntry(const std::wstring& sessionId,
                      PCWSTR category,
                      Fn&& onLiveEntry) {
    wil::unique_hkey key = OpenSessionCategoryKey(sessionId, category);
    if (!key) {
        return;
    }

    SessionMetadata::PruneDeadEntriesAndVisitLive(
        key.get(), std::forward<Fn>(onLiveEntry));
}

}  // namespace

std::vector<SessionMetadataEntry> ReadSessionMetadata(
    const std::wstring& sessionId,
    PCWSTR category) {
    std::vector<SessionMetadataEntry> entries;

    ForEachLiveEntry(
        sessionId, category,
        [&](const std::wstring& name,
            SessionMetadata::ParsedValueName& parsedName,
            SessionMetadata::ParsedValueData& parsedData) {
            entries.push_back({
                .valueName = name,
                .targetProcessId = parsedName.targetProcessId,
                .targetProcessCreationTime =
                    parsedData.targetProcessCreationTime,
                .modName = std::move(parsedName.modName),
                .entryCreationTime = parsedData.entryCreationTime,
                .processImageName = std::move(parsedData.processImageName),
                .value = std::move(parsedData.value),
            });
            return true;  // continue
        });

    return entries;
}

bool IsSessionMetadataEmpty(const std::wstring& sessionId, PCWSTR category) {
    // Stop at the first live entry; a category with only stale or malformed
    // entries (which ForEachLiveEntry deletes) reports empty.
    bool hasLiveEntry = false;
    ForEachLiveEntry(sessionId, category,
                     [&](const std::wstring&, SessionMetadata::ParsedValueName&,
                         SessionMetadata::ParsedValueData&) {
                         hasLiveEntry = true;
                         return false;  // stop at the first live entry
                     });

    return !hasLiveEntry;
}

ModMetadataChangeNotification::ModMetadataChangeNotification(
    const std::wstring& sessionId,
    PCWSTR category) {
    std::wstring subKey =
        SessionMetadata::MakeCategorySubKey(sessionId, category);

    THROW_IF_WIN32_ERROR(RegOpenKeyEx(HKEY_LOCAL_MACHINE, subKey.c_str(), 0,
                                      KEY_NOTIFY | KEY_WOW64_64KEY, &m_key));

    m_eventHandle.reset(CreateEvent(nullptr, FALSE, FALSE, nullptr));
    THROW_LAST_ERROR_IF(!m_eventHandle);

    THROW_IF_WIN32_ERROR(RegNotifyChangeKeyValue(m_key.get(), FALSE,
                                                 kRegNotifyChangeKeyValueFlags,
                                                 m_eventHandle.get(), TRUE));
}

HANDLE ModMetadataChangeNotification::GetHandle() {
    return m_eventHandle.get();
}

void ModMetadataChangeNotification::ContinueMonitoring() {
    THROW_IF_WIN32_ERROR(RegNotifyChangeKeyValue(m_key.get(), FALSE,
                                                 kRegNotifyChangeKeyValueFlags,
                                                 m_eventHandle.get(), TRUE));
}
