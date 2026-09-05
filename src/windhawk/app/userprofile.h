#pragma once

namespace UserProfile {

struct UpdateStatus {
    bool appUpdateAvailable;
    int modUpdatesAvailable;
    bool newUpdatesFound;
};

// Empty when the profile can't be read.
std::string GetLocalUpdatedContentAsString();

// std::nullopt when the profile can't be read, which isn't the same answer as
// nothing being available.
std::optional<UpdateStatus> UpdateContentWithOnlineData(
    PCSTR onlineData,
    size_t onlineDataLength);
std::optional<UpdateStatus> GetUpdateStatus();

}  // namespace UserProfile
