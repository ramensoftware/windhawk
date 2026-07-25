#pragma once

#include "portable_settings.h"

class StorageManager {
   public:
    StorageManager(const StorageManager&) = delete;
    StorageManager(StorageManager&&) = delete;
    StorageManager& operator=(const StorageManager&) = delete;
    StorageManager& operator=(StorageManager&&) = delete;

    static StorageManager& GetInstance();

    std::unique_ptr<PortableSettings> GetAppConfig(PCWSTR section, bool write);
    bool FlushAppConfig(PCWSTR section);

    bool IsPortable();
    std::filesystem::path GetEnginePath(
        USHORT machine = IMAGE_FILE_MACHINE_UNKNOWN);
    std::filesystem::path GetUIPath();
    std::filesystem::path GetCompilerPath();
    std::filesystem::path GetUIDataPath();
    std::filesystem::path GetEditorWorkspacePath();
    std::filesystem::path GetUserProfileJsonPath();

   private:
    StorageManager();
    ~StorageManager();

    struct RegistryPath {
        HKEY hKey = 0;
        std::wstring subKey;
    };

    struct IniFilePath {
        std::wstring path;
    };

    bool portableStorage;
    std::filesystem::path appDataPath;
    std::filesystem::path enginePath;
    std::filesystem::path uiPath;
    std::filesystem::path compilerPath;
    std::variant<std::monostate, RegistryPath, IniFilePath> settingsPath;
};
