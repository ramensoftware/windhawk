#pragma once

#include "no_destructor.h"
#include "portable_settings.h"

class StorageManager {
   public:
    StorageManager(const StorageManager&) = delete;
    StorageManager(StorageManager&&) = delete;
    StorageManager& operator=(const StorageManager&) = delete;
    StorageManager& operator=(StorageManager&&) = delete;

    static StorageManager& GetInstance();

    std::unique_ptr<PortableSettings> GetAppConfig(PCWSTR section);
    std::unique_ptr<PortableSettings> GetModConfig(PCWSTR modName,
                                                   PCWSTR section);
    std::unique_ptr<PortableSettings> GetModWritableConfig(PCWSTR modName,
                                                           PCWSTR section,
                                                           bool write);
    void EnumMods(std::function<void(PCWSTR)> enumCallback);

    std::filesystem::path GetModMetadataPath(PCWSTR metadataCategory);
    wil::unique_hfile CreateModMetadataFile(PCWSTR metadataCategory,
                                            PCWSTR modInstanceId);
    void SetModMetadataValue(wil::unique_hfile& file, PCWSTR value);

    std::filesystem::path GetEnginePath(
        USHORT machine = IMAGE_FILE_MACHINE_UNKNOWN);
    std::filesystem::path GetModsPath(
        USHORT machine = IMAGE_FILE_MACHINE_UNKNOWN);
    std::filesystem::path GetSymbolsPath();

    class ModConfigChangeNotification {
       public:
        ModConfigChangeNotification();

        HANDLE GetHandle();
        void ContinueMonitoring();
        bool CanMonitorAcrossThreads();

       private:
        struct RegistryState {
            wil::unique_hkey key;
            DWORD regNotifyChangeKeyValueFlags;
            wil::unique_event_nothrow eventHandle;
        };

        struct IniFileState {
            wil::unique_hfind_change handle;
        };

        std::variant<std::monostate, RegistryState, IniFileState>
            monitoringState;
    };

   private:
    friend class NoDestructorIfTerminating<StorageManager>;

    StorageManager();
    ~StorageManager();

    void RegistryEnumMods(std::function<void(PCWSTR)> enumCallback);
    void IniFilesEnumMods(std::function<void(PCWSTR)> enumCallback);

    struct RegistryPath {
        HKEY hKey = 0;
        std::wstring subKey;
    };

    struct IniFilePath {
        std::wstring path;
    };

    bool portableStorage;
    std::filesystem::path appDataPath;
    std::variant<std::monostate, RegistryPath, IniFilePath> settingsPath;
};
