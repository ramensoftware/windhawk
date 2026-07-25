#pragma once

// App-side access to this session's volatile mod status/task metadata store
// (see shared/session_metadata.h): reading the live entries a category holds
// and watching a category for changes. The session manager (engine) owns
// writing and cleanup; the app only observes.

struct SessionMetadataEntry {
    std::wstring valueName;
    DWORD targetProcessId;
    ULONGLONG targetProcessCreationTime;
    std::wstring modName;
    ULONGLONG entryCreationTime;
    std::wstring processImageName;
    std::wstring value;
};

// Reads the entries for one category whose owning process is still alive, and
// deletes the entries it finds whose process has exited.
std::vector<SessionMetadataEntry> ReadSessionMetadata(
    const std::wstring& sessionId,
    PCWSTR category);

// True if the category holds no live entries. Faster than ReadSessionMetadata
// when only presence matters: it stops at the first live entry. Deletes the
// stale entries it passes over.
bool IsSessionMetadataEmpty(const std::wstring& sessionId, PCWSTR category);

// Watches one category's registry key and signals when an entry is added,
// changed, or removed.
class ModMetadataChangeNotification {
   public:
    ModMetadataChangeNotification(const std::wstring& sessionId,
                                  PCWSTR category);

    HANDLE GetHandle();
    void ContinueMonitoring();

   private:
    wil::unique_event_nothrow m_eventHandle;
    wil::unique_hkey m_key;
};
