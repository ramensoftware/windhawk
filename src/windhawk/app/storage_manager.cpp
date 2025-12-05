#include "stdafx.h"

#include "functions.h"
#include "storage_manager.h"

namespace {

std::filesystem::path PathFromStorage(
    const PortableSettings& storage,
    PCWSTR valueName,
    const std::filesystem::path& baseFolderPath) {
    auto storedPath = storage.GetString(valueName).value_or(L"");
    if (storedPath.empty()) {
        throw std::runtime_error("Missing path value: " + CStringA(valueName));
    }

#ifndef _WIN64
    BOOL isWow64;
    if (IsWow64Process(GetCurrentProcess(), &isWow64) && isWow64) {
        // Get the native Program Files path regardless of the current
        // process architecture.
        storedPath =
            Functions::ReplaceAll(storedPath, L"%ProgramFiles%",
                                  L"%ProgramW6432%", /*ignoreCase=*/true);
    }
#endif  // _WIN64

    auto expandedPath =
        wil::ExpandEnvironmentStrings<std::wstring>(storedPath.c_str());
    return (baseFolderPath / expandedPath).lexically_normal();
}

}  // namespace

// static
StorageManager& StorageManager::GetInstance() {
    static StorageManager s;
    return s;
}

std::unique_ptr<PortableSettings> StorageManager::GetAppConfig(PCWSTR section,
                                                               bool write) {
    if (portableStorage) {
        const auto& iniFileSettingsPath = std::get<IniFilePath>(settingsPath);
        return std::make_unique<IniFileSettings>(
            iniFileSettingsPath.path.c_str(), section, write);
    } else {
        const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
        std::wstring subKey = registrySettingsPath.subKey + L'\\' + section;
        return std::make_unique<RegistrySettings>(registrySettingsPath.hKey,
                                                  subKey.c_str(), write);
    }
}

bool StorageManager::FlushAppConfig(PCWSTR section) {
    if (portableStorage) {
        return false;
    }

    const auto& registrySettingsPath = std::get<RegistryPath>(settingsPath);
    std::wstring subKey = registrySettingsPath.subKey + L'\\' + section;

    wil::unique_hkey hKey;
    LSTATUS error = RegOpenKeyEx(registrySettingsPath.hKey, subKey.c_str(), 0,
                                 KEY_WOW64_64KEY | KEY_QUERY_VALUE, &hKey);
    if (error != ERROR_SUCCESS) {
        return false;
    }

    return RegFlushKey(hKey.get()) == ERROR_SUCCESS;
}

std::filesystem::path StorageManager::GetModMetadataPath(
    PCWSTR metadataCategory) {
    return GetEngineAppDataPath() / L"ModsWritable" / metadataCategory;
}

bool StorageManager::IsPortable() {
    return portableStorage;
}

std::filesystem::path StorageManager::GetEnginePath(USHORT machine) {
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

    return enginePath / folderName;
}

std::filesystem::path StorageManager::GetUIPath() {
    return uiPath;
}

std::filesystem::path StorageManager::GetCompilerPath() {
    return compilerPath;
}

std::filesystem::path StorageManager::GetUIDataPath() {
    return appDataPath / L"UIData";
}

std::filesystem::path StorageManager::GetEditorWorkspacePath() {
    return appDataPath / L"EditorWorkspace";
}

std::filesystem::path StorageManager::GetUserProfileJsonPath() {
    return appDataPath / L"userprofile.json";
}

StorageManager::StorageManager() {
    std::filesystem::path modulePath = wil::GetModuleFileName<std::wstring>();
    auto folderPath = modulePath.parent_path();

    std::filesystem::path iniFilePath = modulePath;
    iniFilePath.replace_extension("ini");

    auto storage = IniFileSettings(iniFilePath.c_str(), L"Storage", false);

    enginePath = PathFromStorage(storage, L"EnginePath", folderPath);
    uiPath = PathFromStorage(storage, L"UIPath", folderPath);
    compilerPath = PathFromStorage(storage, L"CompilerPath", folderPath);
    appDataPath = PathFromStorage(storage, L"AppDataPath", folderPath);

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

std::filesystem::path StorageManager::GetEngineAppDataPath() {
    return appDataPath / L"Engine";
}

StorageManager::ModMetadataChangeNotification::ModMetadataChangeNotification(
    PCWSTR metadataCategory) {
    auto& storageManager = GetInstance();

    auto metadataPath = storageManager.GetModMetadataPath(metadataCategory);

    if (!std::filesystem::is_directory(metadataPath)) {
        std::error_code ec;
        std::filesystem::create_directories(metadataPath, ec);
    }

    m_findChange = wil::unique_hfind_change(FindFirstChangeNotification(
        metadataPath.c_str(), FALSE,
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE));
    THROW_LAST_ERROR_IF(!m_findChange);
}

HANDLE StorageManager::ModMetadataChangeNotification::GetHandle() {
    return m_findChange.get();
}

void StorageManager::ModMetadataChangeNotification::ContinueMonitoring() {
    THROW_IF_WIN32_BOOL_FALSE(FindNextChangeNotification(m_findChange.get()));
}
