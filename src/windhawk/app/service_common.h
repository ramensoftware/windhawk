#pragma once

namespace ServiceCommon {

static inline constexpr WCHAR kName[] = L"Windhawk";

static inline constexpr WCHAR kInfoFileMappingName[] =
    L"Global\\WindhawkServiceInfoFileMapping";

static inline constexpr WCHAR kMutexName[] = L"Global\\WindhawkServiceMutex";

static inline constexpr WCHAR kScanForProcessesEventName[] =
    L"Global\\WindhawkScanForProcesses";

static inline constexpr WCHAR kEmergencyStopEventName[] =
    L"Global\\WindhawkServiceEmergencyStopEvent";

static inline constexpr WCHAR kSafeModeStopEventName[] =
    L"Global\\WindhawkServiceSafeModeStopEvent";

static inline constexpr WCHAR kLaunchAdminCmdEventName[] =
    L"Global\\WindhawkLaunchAdminCmdEvent";

static inline constexpr DWORD kControlLaunchAdminCmd = 128;

static inline constexpr WCHAR kLaunchAdminUIEventName[] =
    L"Global\\WindhawkLaunchAdminUIEvent";

static inline constexpr DWORD kControlLaunchAdminUI = 129;

struct ServiceInfo {
    DWORD version;
    DWORD processId;
    ULONGLONG processCreationTime;
};

}  // namespace ServiceCommon
