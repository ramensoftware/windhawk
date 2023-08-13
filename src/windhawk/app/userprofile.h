#pragma once

namespace UserProfile {

struct UpdateStatus {
    bool appUpdateAvailable;
    int modUpdatesAvailable;
    bool newUpdatesFound;
};

std::string GetLocalUpdatedContentAsString();
UpdateStatus UpdateContentWithOnlineData(PCSTR onlineData,
                                         size_t onlineDataLength);
UpdateStatus GetUpdateStatus();

}  // namespace UserProfile
