#pragma once

// Watches the directory that holds userprofile.json and signals when a file in
// it is added, removed, or written. userprofile.json is written atomically as a
// sibling temp file renamed over the target, so the rename must be observed via
// FILE_NOTIFY_CHANGE_FILE_NAME. A single file can't be watched on its own, only
// its containing directory, so the observer filters directory events down to
// the profile itself.
class UserProfileChangeNotification {
   public:
    UserProfileChangeNotification(const std::filesystem::path& directory);

    HANDLE GetHandle();
    void ContinueMonitoring();

   private:
    wil::unique_hfind_change m_findHandle;
};
