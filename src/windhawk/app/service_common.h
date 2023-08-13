#pragma once

namespace ServiceCommon {

static inline constexpr WCHAR kName[] = L"Windhawk";

static inline constexpr WCHAR kInfoFileMappingName[] =
    L"Global\\WindhawkServiceInfoFileMapping";

static inline constexpr WCHAR kMutexName[] = L"Global\\WindhawkServiceMutex";

static inline constexpr WCHAR kEmergencyStopEventName[] =
    L"Global\\WindhawkServiceEmergencyStopEvent";

static inline constexpr WCHAR kSafeModeStopEventName[] =
    L"Global\\WindhawkServiceSafeModeStopEvent";

struct ServiceInfo {
    DWORD version;
    DWORD processId;
    ULONGLONG processCreationTime;
};

}  // namespace ServiceCommon
