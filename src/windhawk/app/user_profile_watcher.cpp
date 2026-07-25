#include "stdafx.h"

#include "user_profile_watcher.h"

UserProfileChangeNotification::UserProfileChangeNotification(
    const std::filesystem::path& directory) {
    m_findHandle.reset(FindFirstChangeNotification(
        directory.c_str(), FALSE,
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE));
    THROW_LAST_ERROR_IF(!m_findHandle);
}

HANDLE UserProfileChangeNotification::GetHandle() {
    return m_findHandle.get();
}

void UserProfileChangeNotification::ContinueMonitoring() {
    THROW_IF_WIN32_BOOL_FALSE(FindNextChangeNotification(m_findHandle.get()));
}
