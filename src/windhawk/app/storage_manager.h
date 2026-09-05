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
    std::filesystem::path GetEngineAppDataPath();

    // Signals when an app config section changes, whichever process wrote it.
    // Portable installs can only watch the directory holding settings.ini, so
    // they also signal for unrelated files in it. One-shot; re-arm with
    // ContinueMonitoring().
    class AppConfigChangeNotification {
       public:
        AppConfigChangeNotification(PCWSTR section);

        HANDLE GetHandle();
        void ContinueMonitoring();

       private:
        struct RegistryState {
            wil::unique_hkey key;
            wil::unique_event_nothrow eventHandle;
        };

        struct IniFileState {
            wil::unique_hfind_change handle;
        };

        std::variant<std::monostate, RegistryState, IniFileState>
            monitoringState;
    };

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
