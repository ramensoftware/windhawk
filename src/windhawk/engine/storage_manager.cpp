#include "stdafx.h"

#include "functions.h"
#include "storage_manager.h"
#include "var_init_once.h"

extern HINSTANCE g_hDllInst;

namespace {

std::filesystem::path PathFromStorage(
    const PortableSettings& storage,
    PCWSTR valueName,
    const std::filesystem::path& baseFolderPath) {
    auto storedPath = storage.GetString(valueName).value_or(L"");
    if (storedPath.empty()) {
        throw std::runtime_error("Missing path value");
    }

#ifndef _WIN64
    BOOL isWow64;
    if (IsWow64Process(GetCurrentProcess(), &isWow64) && isWow64) {
        // Get the native Program Files path regardless of the current process
        // architecture.
        storedPath =
            Functions::ReplaceAll(storedPath, L"%ProgramFiles%",
                                  L"%ProgramW6432%", /*ignoreCase=*/true);
    }
#endif  // _WIN64

    auto expandedPath =
        wil::ExpandEnvironmentStrings<std::wstring>(storedPath.c_str());

    // Some processes, e.g. csrss.exe, have a limited amount of environment
    // variables set. Specifically, we need %ProgramData%, so if it's missing,
    // replace it manually.
    if (GetEnvironmentVariable(L"ProgramData", nullptr, 0) == 0 &&
        GetLastError() == ERROR_ENVVAR_NOT_FOUND) {
        bool replaced = false;

        // Avoid having shell32.dll in the import table, since it might not be
        // available in all cases, e.g. sandboxed processes.
        using SHGetKnownFolderPath_t = decltype(&SHGetKnownFolderPath);

        LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
            SHGetKnownFolderPath_t, pSHGetKnownFolderPath, L"shell32.dll",
            LOAD_LIBRARY_SEARCH_SYSTEM32, "SHGetKnownFolderPath");

        if (pSHGetKnownFolderPath) {
            PWSTR programData;
            HRESULT hr = pSHGetKnownFolderPath(FOLDERID_ProgramData, 0, nullptr,
                                               &programData);
            if (SUCCEEDED(hr)) {
                expandedPath = Functions::ReplaceAll(
                    expandedPath, L"%ProgramData%", programData,
                    /*ignoreCase=*/true);
                replaced = true;
            }

            // Avoid having ole32.dll in the import table, since it might not
            // be available in all cases, e.g. sandboxed processes.
            using CoTaskMemFree_t = decltype(&CoTaskMemFree);

            LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
                CoTaskMemFree_t, pCoTaskMemFree, L"ole32.dll",
                LOAD_LIBRARY_SEARCH_SYSTEM32, "CoTaskMemFree");

            if (pCoTaskMemFree) {
                pCoTaskMemFree(programData);
            }
        }

        // If SHGetKnownFolderPath failed, try to get the system drive from
        // environment variables.
        if (!replaced) {
            std::wstring systemDrive;
            if (SUCCEEDED(
                    wil::GetEnvironmentVariable(L"SystemDrive", systemDrive)) &&
                !systemDrive.empty()) {
                expandedPath = Functions::ReplaceAll(
                    expandedPath, L"%ProgramData%",
                    systemDrive + L"\\ProgramData", /*ignoreCase=*/true);
                replaced = true;
            }
        }

        // If all else fails, replace %ProgramData% with a hardcoded path.
        if (!replaced) {
            expandedPath = Functions::ReplaceAll(expandedPath, L"%ProgramData%",
                                                 L"C:\\ProgramData",
                                                 /*ignoreCase=*/true);
        }
    }

    return (baseFolderPath / expandedPath).lexically_normal();
}

}  // namespace

// static
StorageManager& StorageManager::GetInstance() {
    STATIC_INIT_ONCE(NoDestructorIfTerminating<StorageManager>, s);
    return **s;
}

std::unique_ptr<PortableSettings> StorageManager::GetAppConfig(PCWSTR section) {
    if (portableStorage) {
        const auto& iniFileSettingsPath = std::get<IniFilePath>(settingsPath);
        return std::make_unique<IniFileSettings>(
            iniFileSettingsPath.path.c_str(), section, false);
    } else {
        const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
        std::wstring subKey = registrySettingsPath.subKey + L'\\' + section;
        return std::make_unique<RegistrySettings>(registrySettingsPath.hKey,
                                                  subKey.c_str(), false);
    }
}

std::unique_ptr<PortableSettings> StorageManager::GetModConfig(PCWSTR modName,
                                                               PCWSTR section) {
    if (portableStorage) {
        std::wstring iniFileName = modName;
        iniFileName += L".ini";
        auto modConfigPath = appDataPath / L"Mods" / iniFileName;
        return std::make_unique<IniFileSettings>(
            modConfigPath.c_str(), section ? section : L"Mod", false);
    } else {
        const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
        std::wstring subKey =
            registrySettingsPath.subKey + L"\\Mods\\" + modName;
        if (section) {
            subKey += L"\\";
            subKey += section;
        }

        return std::make_unique<RegistrySettings>(registrySettingsPath.hKey,
                                                  subKey.c_str(), false);
    }
}

std::unique_ptr<PortableSettings> StorageManager::GetModWritableConfig(
    PCWSTR modName,
    PCWSTR section,
    bool write) {
    if (portableStorage) {
        auto modsWritablePath = appDataPath / L"ModsWritable";

        if (write && !std::filesystem::is_directory(modsWritablePath)) {
            std::error_code ec;
            std::filesystem::create_directories(modsWritablePath, ec);
        }

        std::wstring iniFileName = modName;
        iniFileName += L".ini";
        auto modConfigPath = modsWritablePath / iniFileName;
        return std::make_unique<IniFileSettings>(
            modConfigPath.c_str(), section ? section : L"Mod", write);
    } else {
        const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
        std::wstring subKey =
            registrySettingsPath.subKey + L"\\ModsWritable\\" + modName;
        if (section) {
            subKey += L"\\";
            subKey += section;
        }

        return std::make_unique<RegistrySettings>(registrySettingsPath.hKey,
                                                  subKey.c_str(), write);
    }
}

void StorageManager::EnumMods(std::function<void(PCWSTR)> enumCallback) {
    if (portableStorage) {
        IniFilesEnumMods(std::move(enumCallback));
    } else {
        RegistryEnumMods(std::move(enumCallback));
    }
}

std::filesystem::path StorageManager::GetModStoragePath(PCWSTR modName) {
    auto modStoragePath =
        appDataPath / L"ModsWritable" / L"mod-storage" / modName;

    if (!std::filesystem::is_directory(modStoragePath)) {
        std::error_code ec;
        std::filesystem::create_directories(modStoragePath, ec);
    }

    return modStoragePath;
}

std::filesystem::path StorageManager::GetModMetadataPath(
    PCWSTR metadataCategory) {
    return appDataPath / L"ModsWritable" / metadataCategory;
}

wil::unique_hfile StorageManager::CreateModMetadataFile(PCWSTR metadataCategory,
                                                        PCWSTR modInstanceId) {
    auto metadataCategoryPath = GetModMetadataPath(metadataCategory);

    if (!std::filesystem::is_directory(metadataCategoryPath)) {
        std::error_code ec;
        std::filesystem::create_directories(metadataCategoryPath, ec);
    }

    auto metadataFilePath = metadataCategoryPath / modInstanceId;

    wil::unique_hfile file(CreateFile(
        metadataFilePath.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr,
        CREATE_ALWAYS, FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE,
        nullptr));
    THROW_LAST_ERROR_IF(!file);

    return file;
}

void StorageManager::SetModMetadataValue(wil::unique_hfile& file,
                                         PCWSTR value) {
    THROW_LAST_ERROR_IF(SetFilePointer(file.get(), 0, nullptr, FILE_BEGIN) ==
                        INVALID_SET_FILE_POINTER);
    THROW_IF_WIN32_BOOL_FALSE(SetEndOfFile(file.get()));

    DWORD dwNumberOfBytesWritten;
    THROW_IF_WIN32_BOOL_FALSE(WriteFile(
        file.get(), value, wil::safe_cast<DWORD>(wcslen(value) * sizeof(WCHAR)),
        &dwNumberOfBytesWritten, nullptr));
}

std::filesystem::path StorageManager::GetEnginePath(USHORT machine) {
    std::filesystem::path libraryPath =
        wil::GetModuleFileName<std::wstring>(g_hDllInst);

    auto folderPath = libraryPath.parent_path();

    if (machine == IMAGE_FILE_MACHINE_UNKNOWN) {
        // Use current architecture.
#if defined(_M_IX86)
        machine = IMAGE_FILE_MACHINE_I386;
#elif defined(_M_X64)
        machine = IMAGE_FILE_MACHINE_AMD64;
#elif defined(_M_ARM64)
        machine = IMAGE_FILE_MACHINE_ARM64;
#else
#error "Unsupported architecture"
#endif
    }

    PCWSTR newFolderName;
    switch (machine) {
        case IMAGE_FILE_MACHINE_I386:
            newFolderName = L"32";
            break;

        case IMAGE_FILE_MACHINE_AMD64:
            newFolderName = L"64";
            break;

        case IMAGE_FILE_MACHINE_ARM64:
            newFolderName = L"arm64";
            break;

        default:
            throw std::logic_error("Unknown architecture");
    }

    return folderPath.parent_path() / newFolderName;
}

std::filesystem::path StorageManager::GetModsPath(USHORT machine) {
    if (machine == IMAGE_FILE_MACHINE_UNKNOWN) {
        // Use current architecture.
#if defined(_M_IX86)
        machine = IMAGE_FILE_MACHINE_I386;
#elif defined(_M_X64)
        machine = IMAGE_FILE_MACHINE_AMD64;
#elif defined(_M_ARM64)
        machine = IMAGE_FILE_MACHINE_ARM64;
#else
#error "Unsupported architecture"
#endif
    }

    PCWSTR folderName;
    switch (machine) {
        case IMAGE_FILE_MACHINE_I386:
            folderName = L"32";
            break;

        case IMAGE_FILE_MACHINE_AMD64:
            folderName = L"64";
            break;

        case IMAGE_FILE_MACHINE_ARM64:
            folderName = L"arm64";
            break;

        default:
            throw std::logic_error("Unknown architecture");
    }

    return appDataPath / L"Mods" / folderName;
}

std::filesystem::path StorageManager::GetSymbolsPath() {
    return appDataPath / L"Symbols";
}

StorageManager::StorageManager() {
    std::filesystem::path dllPath =
        wil::GetModuleFileName<std::wstring>(g_hDllInst);

    std::filesystem::path iniFileFolder = dllPath.parent_path().parent_path();
    std::filesystem::path iniFilePath = iniFileFolder / L"engine.ini";

    if (!std::filesystem::is_regular_file(iniFilePath)) {
        throw std::runtime_error("engine.ini not found");
    }

    auto storage = IniFileSettings(iniFilePath.c_str(), L"Storage", false);

    appDataPath = PathFromStorage(storage, L"AppDataPath", iniFileFolder);

    if (!std::filesystem::is_directory(appDataPath)) {
        std::error_code ec;
        std::filesystem::create_directories(appDataPath, ec);
    }

    portableStorage = storage.GetInt(L"Portable").value_or(0);
    if (portableStorage) {
        settingsPath = IniFilePath{appDataPath / L"settings.ini"};
    } else {
        std::wstring registryKey =
            storage.GetString(L"RegistryKey").value_or(L"");
        if (registryKey.empty()) {
            throw std::runtime_error("Missing RegistryKey value");
        }

        auto firstBackslash = registryKey.find(L'\\');
        if (firstBackslash == registryKey.npos) {
            throw std::runtime_error("Invalid RegistryKey value");
        }

        HKEY hkey;

        std::wstring baseKey = registryKey.substr(0, firstBackslash);
        if (baseKey == L"HKEY_CURRENT_USER" || baseKey == L"HKCU") {
            hkey = HKEY_CURRENT_USER;
        } else if (baseKey == L"HKEY_USERS" || baseKey == L"HKU") {
            hkey = HKEY_USERS;
        } else if (baseKey == L"HKEY_LOCAL_MACHINE" || baseKey == L"HKLM") {
            hkey = HKEY_LOCAL_MACHINE;
        } else {
            throw std::runtime_error("Unsupported RegistryKey value");
        }

        std::wstring subKey = registryKey.substr(firstBackslash + 1);

        settingsPath = RegistryPath{hkey, std::move(subKey)};
    }
}

StorageManager::~StorageManager() = default;

void StorageManager::RegistryEnumMods(
    std::function<void(PCWSTR)> enumCallback) {
    const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
    std::wstring subKey = registrySettingsPath.subKey + L"\\Mods";

    wil::unique_hkey hModsKey;
    LSTATUS error =
        RegCreateKeyEx(registrySettingsPath.hKey, subKey.c_str(), 0, nullptr, 0,
                       KEY_READ | KEY_WOW64_64KEY, nullptr, &hModsKey, nullptr);
    THROW_IF_WIN32_ERROR(error);

    std::wstring subKeyName;
    DWORD dwMaxSubKeyLen;
    bool shouldUpdateMaxSubKeyLen = true;

    DWORD dwIndex = 0;
    while (true) {
        if (shouldUpdateMaxSubKeyLen) {
            error = RegQueryInfoKey(hModsKey.get(), nullptr, nullptr, nullptr,
                                    nullptr, &dwMaxSubKeyLen, nullptr, nullptr,
                                    nullptr, nullptr, nullptr, nullptr);
            THROW_IF_WIN32_ERROR(error);

            subKeyName.resize(wil::safe_cast<size_t>(dwMaxSubKeyLen) + 1);

            shouldUpdateMaxSubKeyLen = false;
        }

        DWORD dwSubKeyLen = dwMaxSubKeyLen + 1;

        error = RegEnumKeyEx(hModsKey.get(), dwIndex, &subKeyName[0],
                             &dwSubKeyLen, nullptr, nullptr, nullptr, nullptr);
        if (error == ERROR_NO_MORE_ITEMS) {
            break;
        }

        if (error == ERROR_MORE_DATA) {
            shouldUpdateMaxSubKeyLen = true;
            continue;  // perhaps value was updated, try again
        }

        THROW_IF_WIN32_ERROR(error);

        enumCallback(subKeyName.c_str());

        dwIndex++;
    }
}

void StorageManager::IniFilesEnumMods(
    std::function<void(PCWSTR)> enumCallback) {
    auto modsConfigPath = appDataPath / L"Mods";

    if (!std::filesystem::exists(modsConfigPath)) {
        return;
    }

    for (const auto& p : std::filesystem::directory_iterator(modsConfigPath)) {
        if (!p.is_regular_file()) {
            continue;
        }

        if (p.path().extension() == L".ini") {
            enumCallback(p.path().stem().c_str());
        }
    }
}

StorageManager::ModConfigChangeNotification::ModConfigChangeNotification() {
    auto& storageManager = GetInstance();

    if (storageManager.portableStorage) {
        auto modsPath = storageManager.appDataPath / L"Mods";

        if (!std::filesystem::is_directory(modsPath)) {
            std::error_code ec;
            std::filesystem::create_directories(modsPath, ec);
        }

        auto findHandle = wil::unique_hfind_change(FindFirstChangeNotification(
            modsPath.c_str(), FALSE,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE));
        THROW_LAST_ERROR_IF(!findHandle);

        monitoringState = IniFileState{std::move(findHandle)};
    } else {
        const auto& registrySettingsPath =
            std::get<RegistryPath>(storageManager.settingsPath);
        std::wstring subKey = registrySettingsPath.subKey + L"\\Mods";

        wil::unique_hkey key;
        THROW_IF_WIN32_ERROR(RegCreateKeyEx(
            registrySettingsPath.hKey, subKey.c_str(), 0, nullptr, 0,
            KEY_NOTIFY | KEY_WOW64_64KEY, nullptr, &key, nullptr));

        wil::unique_event_nothrow changeHandle(
            CreateEvent(nullptr, FALSE, FALSE, nullptr));
        THROW_LAST_ERROR_IF_NULL(changeHandle);

        DWORD regNotifyChangeKeyValueFlags =
            REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET;
        if (Functions::IsWindowsVersionOrGreaterWithBuildNumber(6, 2, 0)) {
            regNotifyChangeKeyValueFlags |= REG_NOTIFY_THREAD_AGNOSTIC;
        }

        THROW_IF_WIN32_ERROR(RegNotifyChangeKeyValue(
            key.get(), TRUE, regNotifyChangeKeyValueFlags, changeHandle.get(),
            TRUE));

        monitoringState =
            RegistryState{std::move(key), regNotifyChangeKeyValueFlags,
                          std::move(changeHandle)};
    }
}

HANDLE StorageManager::ModConfigChangeNotification::GetHandle() {
    auto& storageManager = GetInstance();

    if (storageManager.portableStorage) {
        return std::get<IniFileState>(monitoringState).handle.get();
    } else {
        return std::get<RegistryState>(monitoringState).eventHandle.get();
    }
}

void StorageManager::ModConfigChangeNotification::ContinueMonitoring() {
    auto& storageManager = GetInstance();

    if (storageManager.portableStorage) {
        THROW_IF_WIN32_BOOL_FALSE(FindNextChangeNotification(
            std::get<IniFileState>(monitoringState).handle.get()));
    } else {
        auto& regState = std::get<RegistryState>(monitoringState);
        THROW_IF_WIN32_ERROR(RegNotifyChangeKeyValue(
            regState.key.get(), TRUE, regState.regNotifyChangeKeyValueFlags,
            regState.eventHandle.get(), TRUE));
    }
}

bool StorageManager::ModConfigChangeNotification::CanMonitorAcrossThreads() {
    auto& storageManager = GetInstance();

    if (storageManager.portableStorage) {
        return true;
    } else {
        auto& regState = std::get<RegistryState>(monitoringState);
        return regState.regNotifyChangeKeyValueFlags &
               REG_NOTIFY_THREAD_AGNOSTIC;
    }
}
