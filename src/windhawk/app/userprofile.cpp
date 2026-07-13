#include "stdafx.h"

#include "userprofile.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"
#include "version.h"

using json = nlohmann::json;

namespace {

namespace keys {
constexpr const char* kId = "id";
constexpr const char* kOs = "os";
constexpr const char* kApp = "app";
constexpr const char* kMods = "mods";
constexpr const char* kVersion = "version";
constexpr const char* kVersionPreRelease = "versionPreRelease";
constexpr const char* kLatestVersion = "latestVersion";
constexpr const char* kLatestVersionPreRelease = "latestVersionPreRelease";
constexpr const char* kMetadata = "metadata";
}  // namespace keys

// Returns the string at key, or fallback if the key is missing or not a string.
// Unlike json::value(), never throws on a type mismatch.
std::string GetString(const json& obj,
                      const char* key,
                      std::string_view fallback = {}) {
    auto it = obj.find(key);
    if (it != obj.end() && it->is_string()) {
        return it->get<std::string>();
    }
    return std::string(fallback);
}

// Assigns value into node, setting *dirty only when the value actually changes.
template <typename T>
void AssignIfChanged(json& node, const T& value, bool* dirty) {
    if (node != value) {
        node = value;
        *dirty = true;
    }
}

// Returns node[key] as an object, replacing a missing or wrong-typed value with
// an empty object and setting *dirty when it does so.
json& EnsureObject(json& node, const char* key, bool* dirty) {
    json& child = node[key];
    if (!child.is_object()) {
        child = json::object();
        *dirty = true;
    }
    return child;
}

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

void SaveUserProfileJsonToFile(const std::filesystem::path& userProfileJsonPath,
                               const json& userProfileJson) {
    if (!Functions::WriteFileContentAtomically(userProfileJsonPath,
                                               userProfileJson.dump(2))) {
        LOG(L"Updating userprofile.json failed (%s)",
            userProfileJsonPath.c_str());
    }
}

json GetLocalUpdatedContent() {
    auto userProfileJsonPath =
        StorageManager::GetInstance().GetUserProfileJsonPath();

    json userProfileJson = ReadUserProfileJsonFromFile(userProfileJsonPath);

    bool updatedData = false;

    // Update user id if necessary.
    auto& id = userProfileJson[keys::kId];
    if (!id.is_string() || !ValidateUserId(id.get<std::string>())) {
        id = GenerateUserId();
        updatedData = true;
    }

    // Update OS version if necessary.
    AssignIfChanged(userProfileJson[keys::kOs], GetCurrentOSVersion(),
                    &updatedData);

    // Update app version if necessary.
    json& app = EnsureObject(userProfileJson, keys::kApp, &updatedData);
    AssignIfChanged(app[keys::kVersion], VER_FILE_VERSION_STR, &updatedData);

    // Save data.
    if (updatedData) {
        SaveUserProfileJsonToFile(userProfileJsonPath, userProfileJson);
    }

    return userProfileJson;
}

bool StringIsAllDigits(std::string_view s) {
    return !s.empty() &&
           s.find_first_not_of("0123456789") == std::string_view::npos;
}

std::vector<std::string_view> SplitOn(std::string_view s, char delim) {
    std::vector<std::string_view> parts;
    size_t start = 0;
    for (;;) {
        size_t pos = s.find(delim, start);
        if (pos == std::string_view::npos) {
            parts.push_back(s.substr(start));
            break;
        }
        parts.push_back(s.substr(start, pos - start));
        start = pos + 1;
    }
    return parts;
}

// Compare two decimal strings by numeric value without overflow: drop leading
// zeros, then order by length and finally lexically. Returns -1, 0 or 1.
int CompareNumericStrings(std::string_view a, std::string_view b) {
    size_t ia = a.find_first_not_of('0');
    size_t ib = b.find_first_not_of('0');
    std::string_view na =
        ia == std::string_view::npos ? std::string_view() : a.substr(ia);
    std::string_view nb =
        ib == std::string_view::npos ? std::string_view() : b.substr(ib);
    if (na.length() != nb.length()) {
        return na.length() < nb.length() ? -1 : 1;
    }
    int c = na.compare(nb);
    return c < 0 ? -1 : (c > 0 ? 1 : 0);
}

// Compare the numeric release parts (major.minor.patch...), treating missing or
// non-numeric parts as 0. Returns -1, 0 or 1.
int CompareVersionBase(std::string_view base1, std::string_view base2) {
    std::vector<std::string_view> p1 = SplitOn(base1, '.');
    std::vector<std::string_view> p2 = SplitOn(base2, '.');
    size_t count = p1.size() > p2.size() ? p1.size() : p2.size();
    for (size_t i = 0; i < count; i++) {
        std::string_view a = i < p1.size() && StringIsAllDigits(p1[i])
                                 ? p1[i]
                                 : std::string_view("0");
        std::string_view b = i < p2.size() && StringIsAllDigits(p2[i])
                                 ? p2[i]
                                 : std::string_view("0");
        int c = CompareNumericStrings(a, b);
        if (c != 0) {
            return c;
        }
    }
    return 0;
}

// Compare two SemVer pre-release tags (the dot-separated identifiers after the
// '-'). An empty tag means "no pre-release", which outranks any pre-release.
// Numeric identifiers compare numerically and rank below alphanumeric ones; a
// larger set of identifiers outranks a smaller one with an equal prefix.
// Returns -1, 0 or 1.
int CompareVersionPrerelease(std::string_view pre1, std::string_view pre2) {
    if (pre1.empty() || pre2.empty()) {
        if (pre1.empty() && pre2.empty()) {
            return 0;
        }
        return pre1.empty() ? 1 : -1;
    }

    std::vector<std::string_view> id1 = SplitOn(pre1, '.');
    std::vector<std::string_view> id2 = SplitOn(pre2, '.');
    size_t count = id1.size() < id2.size() ? id1.size() : id2.size();
    for (size_t i = 0; i < count; i++) {
        bool numeric1 = StringIsAllDigits(id1[i]);
        bool numeric2 = StringIsAllDigits(id2[i]);
        if (numeric1 && numeric2) {
            int c = CompareNumericStrings(id1[i], id2[i]);
            if (c != 0) {
                return c;
            }
        } else if (numeric1 != numeric2) {
            return numeric1 ? -1 : 1;
        } else {
            int c = id1[i].compare(id2[i]);
            if (c != 0) {
                return c < 0 ? -1 : 1;
            }
        }
    }
    if (id1.size() != id2.size()) {
        return id1.size() < id2.size() ? -1 : 1;
    }
    return 0;
}

// Order two version strings by SemVer precedence: the numeric release parts
// first, then the pre-release tag (a pre-release is older than its release, so
// e.g. "2.0-alpha.1" < "2.0-alpha.2" < "2.0-beta.1" < "2.0"). Build metadata is
// not used by Windhawk versions.
bool version_less_than(std::string_view v1, std::string_view v2) {
    auto split = [](std::string_view v, std::string_view* base,
                    std::string_view* pre) {
        size_t dash = v.find('-');
        if (dash == std::string_view::npos) {
            *base = v;
            *pre = {};
        } else {
            *base = v.substr(0, dash);
            *pre = v.substr(dash + 1);
        }
    };

    std::string_view base1, pre1, base2, pre2;
    split(v1, &base1, &pre1);
    split(v2, &base2, &pre2);

    int baseCompare = CompareVersionBase(base1, base2);
    if (baseCompare != 0) {
        return baseCompare < 0;
    }

    return CompareVersionPrerelease(pre1, pre2) < 0;
}

// True when a version string carries a SemVer pre-release tag (a non-empty part
// after the first '-', e.g. "2.0-alpha.1"); a final release is not. Used to
// decide whether the running build is on the pre-release channel and should
// fold the pre-release latest version into its update check.
bool version_is_prerelease(std::string_view v) {
    size_t dash = v.find('-');
    return dash != std::string_view::npos && dash + 1 < v.length();
}

// The higher of a cached "latest" and an extra pre-release candidate by SemVer
// precedence: returns extra only when it strictly outranks base, so folding it
// in never lowers the offered version. An empty side yields the other.
std::string_view version_higher(std::string_view base, std::string_view extra) {
    if (extra.empty()) {
        return base;
    }
    if (base.empty()) {
        return extra;
    }
    return version_less_than(base, extra) ? extra : base;
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
        std::string onlineLatestVersionPreRelease;
        auto& onlineApp = onlineDataJson.at(keys::kApp);
        if (onlineApp.is_string()) {
            onlineLatestVersion = onlineApp.get<std::string>();
        } else {
            onlineLatestVersion =
                onlineApp.at(keys::kVersion).get<std::string>();
            onlineLatestVersionPreRelease =
                GetString(onlineApp, keys::kVersionPreRelease);
        }

        json& app = EnsureObject(userProfileJson, keys::kApp, &updatedData);

        std::string prevLatestVersion = GetString(app, keys::kLatestVersion);
        AssignIfChanged(app[keys::kLatestVersion], onlineLatestVersion,
                        &updatedData);

        // Record the pre-release channel's latest version, but only when the
        // server reports one: an absent/empty value leaves any cached value
        // untouched rather than clearing it.
        std::string prevLatestVersionPreRelease =
            GetString(app, keys::kLatestVersionPreRelease);
        if (!onlineLatestVersionPreRelease.empty()) {
            AssignIfChanged(app[keys::kLatestVersionPreRelease],
                            onlineLatestVersionPreRelease, &updatedData);
        }

        if (!onlineLatestVersion.empty()) {
            auto version = app.find(keys::kVersion);
            if (version != app.end() && version->is_string()) {
                std::string currentVersion = version->get<std::string>();

                // On a pre-release build, fold the pre-release channel into the
                // latest version so an alpha/beta tester is told about the next
                // pre-release, not only the next stable release.
                std::string_view effectiveLatest = onlineLatestVersion;
                std::string_view effectivePrevLatest = prevLatestVersion;
                if (version_is_prerelease(currentVersion)) {
                    effectiveLatest = version_higher(
                        onlineLatestVersion, onlineLatestVersionPreRelease);
                    effectivePrevLatest = version_higher(
                        prevLatestVersion, prevLatestVersionPreRelease);
                }

                if (version_less_than(currentVersion, effectiveLatest)) {
                    updateStatus.appUpdateAvailable = true;
                    if (effectivePrevLatest.empty() ||
                        currentVersion == effectivePrevLatest) {
                        updateStatus.newUpdatesFound = true;
                    }
                }
            }
        }
    }

    // Update mods latest version if necessary.
    json& mods = EnsureObject(userProfileJson, keys::kMods, &updatedData);

    for (auto& [key, value] : onlineDataJson.at(keys::kMods).items()) {
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
                value.at(keys::kMetadata).at(keys::kVersion).get<std::string>();
        }

        std::string prevLatestModVersion = GetString(mod, keys::kLatestVersion);
        AssignIfChanged(mod[keys::kLatestVersion], onlineLatestModVersion,
                        &updatedData);

        if (!onlineLatestModVersion.empty()) {
            auto modVersion = mod.find(keys::kVersion);
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
        SaveUserProfileJsonToFile(userProfileJsonPath, userProfileJson);
    }

    return updateStatus;
}

UpdateStatus GetUpdateStatus() {
    UpdateStatus updateStatus{};

    const json userProfileJson = GetLocalUpdatedContent();

    // Check app update.
    {
        auto app = userProfileJson.find(keys::kApp);
        if (app != userProfileJson.end() && app->is_object()) {
            auto version = app->find(keys::kVersion);
            std::string latest = GetString(*app, keys::kLatestVersion);
            if (version != app->end() && version->is_string() &&
                !latest.empty()) {
                std::string currentVersion = version->get<std::string>();

                // On a pre-release build, fold the cached pre-release channel
                // version into the latest so an alpha/beta tester is told about
                // the next pre-release, not only the next stable release.
                std::string_view effectiveLatest = latest;
                std::string latestPreRelease;
                if (version_is_prerelease(currentVersion)) {
                    latestPreRelease =
                        GetString(*app, keys::kLatestVersionPreRelease);
                    if (!latestPreRelease.empty()) {
                        effectiveLatest =
                            version_higher(latest, latestPreRelease);
                    }
                }

                if (version_less_than(currentVersion, effectiveLatest)) {
                    updateStatus.appUpdateAvailable = true;
                }
            }
        }
    }

    // Check mod updates.
    auto mods = userProfileJson.find(keys::kMods);
    if (mods != userProfileJson.end() && mods->is_object()) {
        for (auto& [key, mod] : mods->items()) {
            if (mod.is_object()) {
                auto modVersion = mod.find(keys::kVersion);
                auto latestModVersion = mod.find(keys::kLatestVersion);
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
