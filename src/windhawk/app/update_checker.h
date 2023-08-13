#pragma once

#include "userprofile.h"
#include "winhttpsimple.h"

class UpdateChecker {
   public:
    struct Result {
        HRESULT hrError;
        DWORD httpStatusCode;
        UserProfile::UpdateStatus updateStatus;
    };

    enum {
        kFlagPortable = 1,
    };

    UpdateChecker(DWORD flags, std::function<void()> onUpdateCheckDone);
    void Abort();
    Result HandleResponse();

   private:
    bool ShouldRetryWithAGetRequest();
    void OnRequestDone();

    std::atomic<bool> m_aborted = false;
    DWORD m_flags = 0;
    std::string m_postedData;
    CWinHTTPSimple m_httpSimple;
    std::unique_ptr<CWinHTTPSimple> m_httpSimpleGetRequest;
    std::mutex m_httpSimpleGetRequestMutex;
    std::function<void()> m_onUpdateCheckDone;
};
