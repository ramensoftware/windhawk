#include "stdafx.h"

#include "customization_session.h"
#include "logger.h"
#include "mod_config_user.h"
#include "no_destructor.h"
#include "session_metadata.h"
#include "session_processes.h"
#include "storage_manager.h"
#include "var_init_once.h"

namespace {

// The local system, local service and network service accounts, and the
// per-session accounts of the window manager (DWM-<n>) and of the user-mode
// font driver host (UMFD-<n>).
bool IsSystemAccountSid(std::wstring_view sid) {
    return sid == L"S-1-5-18" || sid == L"S-1-5-19" || sid == L"S-1-5-20" ||
           sid.starts_with(L"S-1-5-90-0-") || sid.starts_with(L"S-1-5-96-0-");
}

std::optional<DWORD> ReadFollowedSessionId() {
    try {
        // Portable storage has no user configurations.
        if (StorageManager::GetInstance().IsPortable()) {
            return std::nullopt;
        }

        const auto& processUserSid =
            Functions::GetCurrentProcessUserSidString();
        if (!processUserSid || !IsSystemAccountSid(*processUserSid)) {
            return std::nullopt;
        }

        DWORD sessionId = 0;
        DWORD returnLength = 0;
        THROW_IF_WIN32_BOOL_FALSE(
            GetTokenInformation(GetCurrentProcessToken(), TokenSessionId,
                                &sessionId, sizeof(sessionId), &returnLength));

        // Session 0 is the services' own, which nobody signs in to.
        if (sessionId == 0) {
            return std::nullopt;
        }

        VERBOSE(L"Taking the mod configurations of the user of session %u",
                sessionId);
        return sessionId;
    } catch (const std::exception& e) {
        LOG(L"Reading the process session failed: %S", e.what());
        return std::nullopt;
    }
}

// The session whose user the process follows, read once. Nothing for a process
// which takes its own user's configurations.
std::optional<DWORD> GetFollowedSessionId() {
    STATIC_INIT_ONCE_TRIVIAL(std::optional<DWORD>, followedSessionId,
                             ReadFollowedSessionId());
    return followedSessionId;
}

LSTATUS OpenSessionUsersKey(REGSAM sam, wil::unique_hkey& keyOut) {
    std::wstring sessionId = SessionMetadata::MakeSessionId(
        CustomizationSession::GetSessionManagerProcessId(),
        wil::filetime::to_int64(
            CustomizationSession::GetSessionManagerProcessCreationTime()));

    return SessionMetadata::OpenStoreCategoryKey(
        sessionId, SessionMetadata::kSessionUsersSubKey, sam, keyOut);
}

bool IsCanonicalSidString(PCWSTR sidString) {
    PSID rawSid = nullptr;
    if (!ConvertStringSidToSid(sidString, &rawSid)) {
        return false;
    }

    wil::unique_hlocal sid(rawSid);

    wil::unique_hlocal_string canonicalSidString;
    if (!ConvertSidToStringSid(sid.get(), &canonicalSidString)) {
        return false;
    }

    return wcscmp(canonicalSidString.get(), sidString) == 0;
}

std::optional<std::wstring> ReadSessionUserSid(DWORD sessionId) {
    // No key is a session manager which records no session users, and no
    // value a session nobody is signed in to.
    wil::unique_hkey key;
    LSTATUS error = OpenSessionUsersKey(KEY_QUERY_VALUE, key);
    if (error == ERROR_FILE_NOT_FOUND) {
        return std::nullopt;
    }

    THROW_IF_WIN32_ERROR(error);

    std::wstring valueName = std::to_wstring(sessionId);
    WCHAR userSid[SECURITY_MAX_SID_STRING_CHARACTERS];
    DWORD userSidSize = sizeof(userSid);
    error = RegGetValue(key.get(), nullptr, valueName.c_str(), RRF_RT_REG_SZ,
                        nullptr, userSid, &userSidSize);
    if (error == ERROR_FILE_NOT_FOUND) {
        return std::nullopt;
    }

    THROW_IF_WIN32_ERROR(error);

    // It names a registry key of the user's configuration, so only a SID in
    // the form the session manager writes it is taken.
    if (!IsCanonicalSidString(userSid)) {
        throw std::runtime_error("The session user isn't a SID");
    }

    return userSid;
}

struct LastSessionUser {
    wil::srwlock lock;
    std::optional<std::wstring> sid;
};

LastSessionUser& GetLastSessionUser() {
    STATIC_INIT_ONCE(NoDestructorIfTerminating<LastSessionUser>,
                     lastSessionUser);
    return **lastSessionUser;
}

}  // namespace

namespace ModConfigUser {

bool FollowsSessionUser() {
    return GetFollowedSessionId().has_value();
}

std::optional<std::wstring> GetSid() {
    auto followedSessionId = GetFollowedSessionId();
    if (!followedSessionId) {
        return Functions::GetCurrentProcessUserSidString();
    }

    std::optional<std::wstring> sid;
    try {
        sid = ReadSessionUserSid(*followedSessionId);
    } catch (const std::exception& e) {
        LOG(L"Reading the user of session %u failed: %S", *followedSessionId,
            e.what());
    }

    auto& lastSessionUser = GetLastSessionUser();
    auto lock = lastSessionUser.lock.lock_exclusive();
    if (sid) {
        lastSessionUser.sid = std::move(sid);
    }

    return lastSessionUser.sid;
}

ChangeNotification::ChangeNotification()
    : m_event(CreateEvent(nullptr, FALSE, FALSE, nullptr)) {
    THROW_LAST_ERROR_IF_NULL(m_event);
    THROW_IF_WIN32_ERROR(OpenSessionUsersKey(KEY_NOTIFY, m_key));

    ContinueMonitoring();
}

HANDLE ChangeNotification::GetHandle() const {
    return m_event.get();
}

void ChangeNotification::ContinueMonitoring() {
    // Thread agnostic: the main loop re-arms it from whichever thread it's on.
    THROW_IF_WIN32_ERROR(RegNotifyChangeKeyValue(
        m_key.get(), FALSE,
        REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_THREAD_AGNOSTIC, m_event.get(),
        TRUE));
}

}  // namespace ModConfigUser
