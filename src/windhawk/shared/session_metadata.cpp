#include "stdafx.h"

#include "session_metadata.h"
#include "storage_manager.h"
#include "var_init_once.h"

namespace {

// Snapshots every running process and its creation time via
// NtQuerySystemInformation(SystemProcessInformation), filling *result. Returns
// false if the enumeration couldn't be taken. Unlike OpenProcess it sees
// processes the caller can't open, which is the whole reason to fall back to
// it.
bool BuildProcessSnapshot(std::unordered_map<DWORD, ULONGLONG>* result) {
    // Layout matches the native SYSTEM_PROCESS_INFORMATION up to the fields
    // read below. This is shared with the app, which has no phnt on its include
    // path, so the struct is hand-declared from <windows.h> types;
    // UNICODE_STRING ImageName is inlined as its members to avoid a
    // native-header dependency.
    typedef struct _WH_SYSTEM_PROCESS_INFORMATION {
        ULONG NextEntryOffset;
        ULONG NumberOfThreads;
        LARGE_INTEGER WorkingSetPrivateSize;
        ULONG HardFaultCount;
        ULONG NumberOfThreadsHighWatermark;
        ULONGLONG CycleTime;
        LARGE_INTEGER CreateTime;
        LARGE_INTEGER UserTime;
        LARGE_INTEGER KernelTime;
        USHORT ImageNameLength;
        USHORT ImageNameMaximumLength;
        PVOID ImageNameBuffer;
        LONG BasePriority;
        HANDLE UniqueProcessId;
    } WH_SYSTEM_PROCESS_INFORMATION;

    // NtQuerySystemInformation returns NTSTATUS (a LONG); LONG is used directly
    // so this needs no native-status header.
    using NtQuerySystemInformation_t =
        LONG(WINAPI*)(ULONG SystemInformationClass, PVOID SystemInformation,
                      ULONG SystemInformationLength, PULONG ReturnLength);
    GET_PROC_ADDRESS_ONCE(NtQuerySystemInformation_t, pNtQuerySystemInformation,
                          L"ntdll.dll", "NtQuerySystemInformation");

    if (!pNtQuerySystemInformation) {
        return false;
    }

    constexpr ULONG kSystemProcessInformation = 5;
    constexpr LONG kStatusInfoLengthMismatch = static_cast<LONG>(0xC0000004);

    std::vector<BYTE> buffer(0x10000);
    size_t usedLength = 0;
    for (;;) {
        ULONG returnLength = 0;
        LONG status = pNtQuerySystemInformation(
            kSystemProcessInformation, buffer.data(),
            static_cast<ULONG>(buffer.size()), &returnLength);
        if (status == kStatusInfoLengthMismatch) {
            // The buffer was too small; grow to the reported size plus a
            // margin, since more processes can appear before the retry.
            size_t newSize =
                returnLength ? returnLength + 0x10000 : buffer.size() * 2;
            if (newSize <= buffer.size()) {
                return false;  // no progress; give up rather than spin
            }
            buffer.resize(newSize);
            continue;
        }
        if (status < 0) {
            return false;  // couldn't enumerate
        }
        // ReturnLength bounds the walk below. Treat it as advisory: a zero or
        // oversized value falls back to the whole buffer, which is still a
        // bound and keeps the snapshot usable.
        usedLength = (returnLength && returnLength <= buffer.size())
                         ? returnLength
                         : buffer.size();
        break;
    }

    size_t offset = 0;
    while (offset + sizeof(WH_SYSTEM_PROCESS_INFORMATION) <= usedLength) {
        auto* info = reinterpret_cast<const WH_SYSTEM_PROCESS_INFORMATION*>(
            buffer.data() + offset);
        DWORD processId = static_cast<DWORD>(
            reinterpret_cast<ULONG_PTR>(info->UniqueProcessId));
        (*result)[processId] =
            static_cast<ULONGLONG>(info->CreateTime.QuadPart);
        // Zero terminates the list, and a step smaller than one entry can't
        // point at a valid one, so either way there's nothing more to read.
        if (info->NextEntryOffset < sizeof(WH_SYSTEM_PROCESS_INFORMATION)) {
            break;
        }
        offset += info->NextEntryOffset;
    }

    return true;
}

// Strict decimal parse of a whole string into an unsigned T: the string must be
// non-empty, all ASCII digits, and in range. std::stoul and friends are unfit
// here because they skip leading whitespace, accept a '+'/'-' sign, and wrap a
// negative value into the unsigned result, and they report those characters as
// consumed, so a length check doesn't catch them.
template <typename T>
std::optional<T> ParseDecimal(std::wstring_view str) {
    static_assert(std::is_unsigned_v<T>);
    constexpr T kMaxValue = static_cast<T>(-1);

    if (str.empty()) {
        return std::nullopt;
    }

    T result = 0;
    for (WCHAR c : str) {
        if (c < L'0' || c > L'9') {
            return std::nullopt;
        }

        T digit = static_cast<T>(c - L'0');
        if (result > (kMaxValue - digit) / 10) {
            return std::nullopt;
        }

        result = result * 10 + digit;
    }

    return result;
}

constexpr std::wstring_view kHiveFileNamePrefix = L"sessions-";
constexpr std::wstring_view kHiveFileNameSuffix = L".hiv";

std::wstring MakeHiveFileName(std::wstring_view sessionId) {
    std::wstring fileName(kHiveFileNamePrefix);
    fileName += sessionId;
    fileName += kHiveFileNameSuffix;
    return fileName;
}

}  // namespace

namespace SessionMetadata {

std::wstring MakeSessionId(DWORD sessionManagerProcessId,
                           ULONGLONG sessionManagerProcessCreationTime) {
    return std::to_wstring(sessionManagerProcessId) + L'_' +
           std::to_wstring(sessionManagerProcessCreationTime);
}

std::wstring MakeCategorySubKey(std::wstring_view sessionId,
                                std::wstring_view category) {
    std::wstring subKey(sessionId);
    subKey += L'\\';
    subKey += category;
    return subKey;
}

std::filesystem::path MakeHiveFilePath(std::wstring_view sessionId) {
    return StorageManager::GetInstance().GetEngineAppDataPath() /
           MakeHiveFileName(sessionId);
}

std::optional<std::wstring> ParseHiveFileName(std::wstring_view fileName) {
    if (!fileName.starts_with(kHiveFileNamePrefix) ||
        !fileName.ends_with(kHiveFileNameSuffix) ||
        fileName.size() <=
            kHiveFileNamePrefix.size() + kHiveFileNameSuffix.size()) {
        return std::nullopt;
    }

    fileName.remove_prefix(kHiveFileNamePrefix.size());
    fileName.remove_suffix(kHiveFileNameSuffix.size());
    return std::wstring(fileName);
}

LSTATUS LoadStoreHive(std::wstring_view sessionId, wil::unique_hkey& hiveOut) {
    std::wstring filePath;
    try {
        // Only a portable session manager places a hive, so anywhere else
        // there's nothing to look for and never will be.
        if (!StorageManager::GetInstance().IsPortable()) {
            return ERROR_FILE_NOT_FOUND;
        }

        filePath = MakeHiveFilePath(sessionId).native();
    } catch (...) {
        // Windhawk's data folder is unknown, so neither is the hive.
        return ERROR_PATH_NOT_FOUND;
    }

    // RegLoadAppKey would create the file, with a security descriptor fixed at
    // creation that shuts out the engines the store is there for. The file
    // comes from the session manager, which writes it from a template, or not
    // at all.
    if (GetFileAttributes(filePath.c_str()) == INVALID_FILE_ATTRIBUTES) {
        return ERROR_FILE_NOT_FOUND;
    }

    // Not REG_PROCESS_APPKEY, which would make the hive exclusive to this
    // process; the session manager, the injected engines and the app share one.
    // Loading an already loaded file hands back the same hive, not a copy.
    return RegLoadAppKey(filePath.c_str(), &hiveOut, KEY_READ | KEY_WRITE,
                         /*dwOptions=*/0, /*Reserved=*/0);
}

LSTATUS OpenStoreCategoryKey(std::wstring_view sessionId,
                             PCWSTR category,
                             REGSAM sam,
                             wil::unique_hkey& keyOut) {
    std::wstring subKey = MakeCategorySubKey(sessionId, category);

    std::wstring machineSubKey(kRootSubKey);
    machineSubKey += L'\\';
    machineSubKey += subKey;

    wil::unique_hkey key;
    LSTATUS machineError =
        RegOpenKeyEx(HKEY_LOCAL_MACHINE, machineSubKey.c_str(), 0,
                     sam | KEY_WOW64_64KEY, &key);
    if (machineError == ERROR_SUCCESS) {
        keyOut = std::move(key);
        return ERROR_SUCCESS;
    }

    // The key handle is itself a handle into the hive, so dropping the root
    // here keeps the hive loaded and leaves nothing for a caller to forget.
    wil::unique_hkey hive;
    if (LoadStoreHive(sessionId, hive) == ERROR_SUCCESS &&
        RegOpenKeyEx(hive.get(), subKey.c_str(), 0, sam, &key) ==
            ERROR_SUCCESS) {
        keyOut = std::move(key);
        return ERROR_SUCCESS;
    }

    return machineError;
}

std::wstring MakeValueName(DWORD targetProcessId, std::wstring_view modName) {
    std::wstring valueName = std::to_wstring(targetProcessId);
    valueName += L'_';
    valueName += modName;
    return valueName;
}

std::wstring FormatValueData(ULONGLONG entryCreationTime,
                             ULONGLONG targetProcessCreationTime,
                             std::wstring_view processImageName,
                             std::wstring_view value) {
    std::wstring data = std::to_wstring(entryCreationTime);
    data += L'|';
    data += std::to_wstring(targetProcessCreationTime);
    data += L'|';
    data += processImageName;
    data += L'|';
    data += value;
    return data;
}

std::optional<ParsedSessionId> ParseSessionId(std::wstring_view sessionId) {
    auto underscore = sessionId.find(L'_');
    if (underscore == std::wstring_view::npos) {
        return std::nullopt;
    }

    auto processId = ParseDecimal<DWORD>(sessionId.substr(0, underscore));
    if (!processId) {
        return std::nullopt;
    }

    auto processCreationTime =
        ParseDecimal<ULONGLONG>(sessionId.substr(underscore + 1));
    if (!processCreationTime) {
        return std::nullopt;
    }

    return ParsedSessionId{*processId, *processCreationTime};
}

std::optional<ParsedValueName> ParseValueName(std::wstring_view valueName) {
    auto underscore = valueName.find(L'_');
    if (underscore == std::wstring_view::npos) {
        return std::nullopt;
    }

    auto targetProcessId = ParseDecimal<DWORD>(valueName.substr(0, underscore));
    if (!targetProcessId) {
        return std::nullopt;
    }

    return ParsedValueName{*targetProcessId,
                           std::wstring(valueName.substr(underscore + 1))};
}

std::optional<ParsedValueData> ParseValueData(std::wstring_view data) {
    auto p1 = data.find(L'|');
    if (p1 == std::wstring_view::npos) {
        return std::nullopt;
    }
    auto p2 = data.find(L'|', p1 + 1);
    if (p2 == std::wstring_view::npos) {
        return std::nullopt;
    }
    auto p3 = data.find(L'|', p2 + 1);
    if (p3 == std::wstring_view::npos) {
        return std::nullopt;
    }

    auto entryCreationTime = ParseDecimal<ULONGLONG>(data.substr(0, p1));
    if (!entryCreationTime) {
        return std::nullopt;
    }

    auto targetProcessCreationTime =
        ParseDecimal<ULONGLONG>(data.substr(p1 + 1, p2 - (p1 + 1)));
    if (!targetProcessCreationTime) {
        return std::nullopt;
    }

    return ParsedValueData{*entryCreationTime, *targetProcessCreationTime,
                           std::wstring(data.substr(p2 + 1, p3 - (p2 + 1))),
                           std::wstring(data.substr(p3 + 1))};
}

bool ProcessLivenessChecker::IsProcessAlive(DWORD processId,
                                            ULONGLONG processCreationTime) {
    wil::unique_process_handle process(OpenProcess(
        SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId));
    if (!process) {
        // A missing pid (ERROR_INVALID_PARAMETER) means the process is gone.
        // Any other failure - typically access-denied for a higher-integrity or
        // another-user process - leaves liveness undetermined via OpenProcess,
        // so fall back to the system-wide snapshot.
        if (GetLastError() == ERROR_INVALID_PARAMETER) {
            return false;
        }
        return IsProcessAliveViaSnapshot(processId, processCreationTime);
    }

    FILETIME creationTime;
    FILETIME exitTime;
    FILETIME kernelTime;
    FILETIME userTime;
    if (!GetProcessTimes(process.get(), &creationTime, &exitTime, &kernelTime,
                         &userTime)) {
        return IsProcessAliveViaSnapshot(processId, processCreationTime);
    }

    // Guard against pid reuse: a different process now owns this pid.
    if (wil::filetime::to_int64(creationTime) != processCreationTime) {
        return false;
    }

    // The pid stays openable after the process exits while an open handle keeps
    // its process object alive, so confirm it hasn't exited.
    return WaitForSingleObject(process.get(), 0) == WAIT_TIMEOUT;
}

bool ProcessLivenessChecker::IsProcessAliveViaSnapshot(
    DWORD processId,
    ULONGLONG processCreationTime) {
    if (!snapshotBuilt_) {
        snapshotBuilt_ = true;
        snapshotValid_ = BuildProcessSnapshot(&snapshot_);
    }

    if (!snapshotValid_) {
        return true;  // couldn't enumerate; don't prune a live entry
    }

    auto it = snapshot_.find(processId);
    return it != snapshot_.end() && it->second == processCreationTime;
}

}  // namespace SessionMetadata
