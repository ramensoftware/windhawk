#include "stdafx.h"

#include "logger.h"
#include "object_security.h"
#include "storage_manager.h"
#include "storage_permissions.h"

namespace {

using unique_sid_local =
    wil::unique_any<PSID, decltype(&::LocalFree), ::LocalFree>;

unique_sid_local MakeSid(PCWSTR sidString) {
    PSID sid = nullptr;
    if (!ConvertStringSidToSid(sidString, &sid)) {
        LOG(L"ConvertStringSidToSid(%s) failed: %u", sidString, GetLastError());
        return {};
    }

    return unique_sid_local{sid};
}

}  // namespace

void EnsureStoragePermissions() noexcept try {
    // The three principals the installer grants access to: Everyone, all
    // application packages, and all restricted application packages, plus the
    // integrity level the writable objects are labeled with.
    auto everyone = MakeSid(L"S-1-1-0");
    auto allAppPackages = MakeSid(L"S-1-15-2-1");
    auto allRestrictedAppPackages = MakeSid(L"S-1-15-2-2");
    auto untrusted = MakeSid(L"S-1-16-0");
    if (!everyone || !allAppPackages || !allRestrictedAppPackages ||
        !untrusted) {
        return;
    }

    PSID sids[] = {everyone.get(), allAppPackages.get(),
                   allRestrictedAppPackages.get()};
    constexpr size_t sidCount = ARRAYSIZE(sids);

    auto& storageManager = StorageManager::GetInstance();

    auto ensureFile = [&](const std::filesystem::path& path, ACCESS_MASK access,
                          const Functions::MandatoryLabel* label = nullptr) {
        std::error_code ec;
        std::filesystem::create_directories(path, ec);

        Functions::DaclAce aces[sidCount];
        for (size_t i = 0; i < sidCount; i++) {
            aces[i] = {sids[i], access,
                       CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE};
        }

        DWORD error =
            Functions::EnsureFileDaclContainsAces(path.c_str(), aces, sidCount);
        if (error != ERROR_SUCCESS) {
            LOG(L"Failed to set permissions for %s: %u", path.c_str(), error);
        }

        if (label) {
            error = Functions::EnsureFileMandatoryLabel(path.c_str(), *label);
            if (error != ERROR_SUCCESS) {
                LOG(L"Failed to label %s: %u", path.c_str(), error);
            }
        }
    };

    // The writable folders get "Modify" (read, write, delete), not full
    // control: the grantees include low-integrity and sandboxed processes,
    // which must not be able to rewrite the DACL (WRITE_DAC) or take ownership
    // (WRITE_OWNER) of these shared locations. With the inheritable ACE,
    // children inherit modify+delete, so creating and deleting files and
    // subdirectories still works.
    //
    // The Untrusted label is what lets a low integrity caller use the write
    // half of that grant, here and for the registry key below: an object
    // carrying no explicit label counts as medium for such a caller, and
    // No-Write-Up denies the write whatever the DACL says. An app container is
    // decided by the package SIDs in the DACL instead, so the label neither
    // grants nor denies it anything. The label is inheritable the way the grant
    // is, and setting it carries to the objects already underneath, not only to
    // ones created afterwards.
    constexpr ACCESS_MASK kFileModifyAccess =
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE;
    const Functions::MandatoryLabel fileLabel = {
        untrusted.get(), SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
        CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE};

    // The engine binaries and the app data root stay readable, and the writable
    // folders stay writable, from every target process. Only the writable ones
    // are labeled: nothing is meant to write the rest.
    ensureFile(storageManager.GetEngineBinariesPath(),
               GENERIC_READ | GENERIC_EXECUTE);

    ensureFile(storageManager.GetEngineAppDataPath(),
               GENERIC_READ | GENERIC_EXECUTE);
    ensureFile(storageManager.GetModsWritablePath(), kFileModifyAccess,
               &fileLabel);
    ensureFile(storageManager.GetSymbolsPath(), kFileModifyAccess, &fileLabel);

    // Portable installs keep settings in INI files, so there are no registry
    // permissions to ensure.
    auto registryKey = storageManager.GetSettingsRegistryKey();
    if (!registryKey) {
        return;
    }

    auto ensureRegistry = [&](const std::wstring& subKey, ACCESS_MASK access,
                              const Functions::MandatoryLabel* label =
                                  nullptr) {
        Functions::DaclAce aces[sidCount];
        for (size_t i = 0; i < sidCount; i++) {
            aces[i] = {sids[i], access, CONTAINER_INHERIT_ACE};
        }

        DWORD error = Functions::EnsureRegistryKeyDaclContainsAces(
            registryKey->first, subKey.c_str(), aces, sidCount);
        if (error != ERROR_SUCCESS) {
            LOG(L"Failed to set permissions for %s: %u", subKey.c_str(), error);
        }

        if (label) {
            error = Functions::EnsureRegistryKeyMandatoryLabel(
                registryKey->first, subKey.c_str(), *label);
            if (error != ERROR_SUCCESS) {
                LOG(L"Failed to label %s: %u", subKey.c_str(), error);
            }
        }
    };

    ensureRegistry(registryKey->second, GENERIC_READ);
    // Read, write, and delete, but not full control: this key is writable by
    // low-integrity and sandboxed processes, which must not be able to create
    // registry symbolic-link keys (KEY_CREATE_LINK), rewrite the DACL
    // (WRITE_DAC), or take ownership (WRITE_OWNER) - the bits that cross a
    // trust boundary. DELETE only lets a grantee remove subkeys in this
    // by-design world-writable area, whose contents they can already overwrite,
    // so it adds no meaningful privilege while matching the file grant.
    const Functions::MandatoryLabel registryLabel = {
        untrusted.get(), SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
        CONTAINER_INHERIT_ACE};
    ensureRegistry(registryKey->second + L"\\ModsWritable",
                   KEY_READ | KEY_WRITE | DELETE, &registryLabel);
} catch (const std::exception& e) {
    LOG(L"EnsureStoragePermissions failed: %S", e.what());
} catch (...) {
    LOG(L"EnsureStoragePermissions failed");
}
