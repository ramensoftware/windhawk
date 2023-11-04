#include "stdafx.h"

#include "userprofile.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"
#include "version.h"

using json = nlohmann::json;

namespace {

bool ValidateUserId(const std::string& id) {
    GUID guid;
    return SUCCEEDED(IIDFromString(
        CStringW(id.c_str(), static_cast<int>(id.length())), &guid));
}

std::string GenerateUserId() {
    GUID guid;
    if (FAILED(CoCreateGuid(&guid))) {
        return {};
    }

    // GUID to string: https://stackoverflow.com/a/12934635
    const CComBSTR guidBstr(guid);  // Converts from binary GUID to BSTR
    const CStringA guidStr(
        guidBstr);  // Converts from BSTR to appropriate string

    return guidStr.GetString();
}

std::string GetCurrentOSVersion() {
    ULONG majorVersion = 0;
    ULONG minorVersion = 0;
    ULONG buildNumber = 0;
    Functions::GetNtVersionNumbers(&majorVersion, &minorVersion, &buildNumber);

    return std::to_string(majorVersion) + "." + std::to_string(minorVersion) +
           "." + std::to_string(buildNumber);
}

json ReadUserProfileJsonFromFile(
    const std::filesystem::path& userProfileJsonPath) {
    json userProfileJson;

    std::ifstream userProfileFile(userProfileJsonPath);
    if (userProfileFile) {
        try {
            userProfileFile >> userProfileJson;
        } catch (const std::exception& e) {
            LOG(L"Parsing userprofile.json failed: %S", e.what());
        }
    }

    if (!userProfileJson.is_object()) {
        userProfileJson = json::object();
    }

    return userProfileJson;
}

json GetLocalUpdatedContent() {
    auto userProfileJsonPath =
        StorageManager::GetInstance().GetUserProfileJsonPath();

    json userProfileJson = ReadUserProfileJsonFromFile(userProfileJsonPath);

    bool updatedData = false;

    // Update user id if necessary.
    auto& id = userProfileJson["id"];
    if (!id.is_string() || !ValidateUserId(id.get<std::string>())) {
        id = GenerateUserId();
        updatedData = true;
    }

    // Update OS version if necessary.
    auto& os = userProfileJson["os"];
    auto currentOsVersion = GetCurrentOSVersion();
    if (os != currentOsVersion) {
        os = currentOsVersion;
        updatedData = true;
    }

    // Update app version if necessary.
    auto& app = userProfileJson["app"];
    if (!app.is_object()) {
        app = json::object();
        updatedData = true;
    }

    auto& version = app["version"];
    if (version != VER_FILE_VERSION_STR) {
        version = VER_FILE_VERSION_STR;
        updatedData = true;
    }

    // Save data.
    if (updatedData) {
        std::ofstream userProfileFile(userProfileJsonPath);
        if (userProfileFile) {
            userProfileFile << std::setw(2) << userProfileJson;
        } else {
            LOG(L"Updating userprofile.json failed (%s)",
                userProfileJsonPath.c_str());
        }
    }

    return userProfileJson;
}

// https://stackoverflow.com/a/54067471
// Method to compare two version strings.
bool version_less_than(std::string v1, std::string v2) {
    size_t i = 0, j = 0;
    while (i < v1.length() || j < v2.length()) {
        int acc1 = 0, acc2 = 0;

        while (i < v1.length() && v1[i] != '.') {
            acc1 = acc1 * 10 + (v1[i] - '0');
            i++;
        }
        while (j < v2.length() && v2[j] != '.') {
            acc2 = acc2 * 10 + (v2[j] - '0');
            j++;
        }

        if (acc1 < acc2) {
            return true;
        }
        if (acc1 > acc2) {
            return false;
        }

        ++i;
        ++j;
    }
    return false;
}

}  // namespace

namespace UserProfile {

std::string GetLocalUpdatedContentAsString() {
    return GetLocalUpdatedContent().dump(2);
}

UpdateStatus UpdateContentWithOnlineData(PCSTR onlineData,
                                         size_t onlineDataLength) {
    UpdateStatus updateStatus{};

    const json onlineDataJson =
        json::parse(onlineData, onlineData + onlineDataLength);

    auto userProfileJsonPath =
        StorageManager::GetInstance().GetUserProfileJsonPath();

    json userProfileJson = ReadUserProfileJsonFromFile(userProfileJsonPath);

    bool updatedData = false;

    // Update app latest version if necessary.
    {
        std::string onlineLatestVersion;
        auto& onlineApp = onlineDataJson.at("app");
        if (onlineApp.is_string()) {
            onlineLatestVersion = onlineApp.get<std::string>();
        } else {
            onlineLatestVersion = onlineApp.at("version").get<std::string>();
        }

        auto& app = userProfileJson["app"];
        if (!app.is_object()) {
            app = json::object();
            updatedData = true;
        }

        auto& latestVersion = app["latestVersion"];
        std::string prevLatestVersion =
            latestVersion.is_string() ? latestVersion.get<std::string>() : "";
        if (latestVersion != onlineLatestVersion) {
            latestVersion = onlineLatestVersion;
            updatedData = true;
        }

        if (!onlineLatestVersion.empty()) {
            auto version = app.find("version");
            if (version != app.end() && version->is_string() &&
                version_less_than(version->get<std::string>(),
                                  onlineLatestVersion)) {
                updateStatus.appUpdateAvailable = true;
                if (prevLatestVersion.empty() ||
                    *version == prevLatestVersion) {
                    updateStatus.newUpdatesFound = true;
                }
            }
        }
    }

    // Update mods latest version if necessary.
    auto& mods = userProfileJson["mods"];
    if (!mods.is_object()) {
        mods = json::object();
        updatedData = true;
    }

    for (auto& [key, value] : onlineDataJson.at("mods").items()) {
        auto it = mods.find(key);
        if (it == mods.end()) {
            continue;
        }

        auto& mod = *it;
        if (!mod.is_object()) {
            mod = json::object();
            updatedData = true;
        }

        std::string onlineLatestModVersion;
        if (value.is_string()) {
            onlineLatestModVersion = value.get<std::string>();
        } else {
            onlineLatestModVersion =
                value.at("metadata").at("version").get<std::string>();
        }

        auto& latestModVersion = mod["latestVersion"];
        std::string prevLatestModVersion =
            latestModVersion.is_string() ? latestModVersion.get<std::string>()
                                         : "";
        if (latestModVersion != onlineLatestModVersion) {
            latestModVersion = onlineLatestModVersion;
            updatedData = true;
        }

        if (!onlineLatestModVersion.empty()) {
            auto modVersion = mod.find("version");
            if (modVersion != mod.end() &&
                *modVersion != onlineLatestModVersion) {
                updateStatus.modUpdatesAvailable++;
                if (prevLatestModVersion.empty() ||
                    *modVersion == prevLatestModVersion) {
                    updateStatus.newUpdatesFound = true;
                }
            }
        }
    }

    // Save data.
    if (updatedData) {
        std::ofstream userProfileFile(userProfileJsonPath);
        if (userProfileFile) {
            userProfileFile << std::setw(2) << userProfileJson;
        } else {
            LOG(L"Updating userprofile.json failed (%s)",
                userProfileJsonPath.c_str());
        }
    }

    return updateStatus;
}

UpdateStatus GetUpdateStatus() {
    UpdateStatus updateStatus{};

    const json userProfileJson = GetLocalUpdatedContent();

    // Check app update.
    {
        auto app = userProfileJson.find("app");
        if (app != userProfileJson.end() && app->is_object()) {
            auto version = app->find("version");
            auto latestVersion = app->find("latestVersion");
            if (version != app->end() && latestVersion != app->end() &&
                version->is_string() && latestVersion->is_string() &&
                *latestVersion != "" &&
                version_less_than(version->get<std::string>(),
                                  latestVersion->get<std::string>())) {
                updateStatus.appUpdateAvailable = true;
            }
        }
    }

    // Check mod updates.
    auto mods = userProfileJson.find("mods");
    if (mods != userProfileJson.end() && mods->is_object()) {
        for (auto& [key, mod] : mods->items()) {
            if (mod.is_object()) {
                auto modVersion = mod.find("version");
                auto latestModVersion = mod.find("latestVersion");
                if (modVersion != mod.end() && latestModVersion != mod.end() &&
                    modVersion->is_string() && latestModVersion->is_string() &&
                    *latestModVersion != "" &&
                    *modVersion != *latestModVersion) {
                    updateStatus.modUpdatesAvailable++;
                }
            }
        }
    }

    return updateStatus;
}

}  // namespace UserProfile
