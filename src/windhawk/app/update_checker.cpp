#include "stdafx.h"

#include "update_checker.h"

#include "logger.h"
#include "version.h"

namespace {

constexpr auto* kUpdateCheckerUrl =
    L"https://update.windhawk.net/versions.json";

CWinHTTPSimpleOptions GetUpdateCheckerOptions(DWORD flags,
                                              const void* postData,
                                              size_t postDataSize) {
    CWinHTTPSimpleOptions options;

    options.sURL = kUpdateCheckerUrl;
    options.sUserAgent = L"Windhawk/" VER_FILE_VERSION_WSTR;
    if (flags & UpdateChecker::kFlagPortable) {
        options.sUserAgent += L" (portable)";
    }

    if (postData && postDataSize > 0) {
        options.sVerb = L"POST";
        options.lpOptional = postData;
        options.dwOptionalSize = static_cast<DWORD>(postDataSize);
    }

    return options;
}

}  // namespace

UpdateChecker::UpdateChecker(DWORD flags,
                             std::function<void()> onUpdateCheckDone)
    : m_flags(flags),
      m_postedData(UserProfile::GetLocalUpdatedContentAsString()),
      m_httpSimple(GetUpdateCheckerOptions(m_flags,
                                           m_postedData.data(),
                                           m_postedData.length()),
                   onUpdateCheckDone != nullptr),
      m_onUpdateCheckDone(std::move(onUpdateCheckDone)) {
    if (!m_postedData.empty()) {
        THROW_IF_FAILED(m_httpSimple.AddHeaders(
            L"Content-Type: application/json", -1L, WINHTTP_ADDREQ_FLAG_ADD));
    }

    if (m_onUpdateCheckDone) {
        THROW_IF_FAILED(m_httpSimple.SendRequest([this] { OnRequestDone(); }));
    } else {
        m_httpSimple.SendRequest(nullptr);
        if (ShouldRetryWithAGetRequest()) {
            m_httpSimpleGetRequest = std::make_unique<CWinHTTPSimple>(
                GetUpdateCheckerOptions(m_flags, nullptr, 0), false);
            m_httpSimpleGetRequest->SendRequest(nullptr);
        }
    }
}

void UpdateChecker::Abort() {
    m_aborted = true;

    m_httpSimple.Abort();

    {
        std::lock_guard<std::mutex> guard(m_httpSimpleGetRequestMutex);
        if (m_httpSimpleGetRequest) {
            m_httpSimpleGetRequest->Abort();
        }
    }
}

UpdateChecker::Result UpdateChecker::HandleResponse() {
    CWinHTTPSimple& httpSimple =
        m_httpSimpleGetRequest ? *m_httpSimpleGetRequest : m_httpSimple;

    Result result = {};
    result.hrError = httpSimple.GetRequestResult();
    result.httpStatusCode = httpSimple.GetLastStatusCode();

    if (SUCCEEDED(result.hrError)) {
        try {
            const auto& response = httpSimple.GetResponse();
            result.updateStatus = UserProfile::UpdateContentWithOnlineData(
                reinterpret_cast<PCSTR>(response.data()), response.size());
        } catch (const std::exception& e) {
            LOG(L"Handling server response failed: %S", e.what());
            result.hrError = E_FAIL;
        }
    }

    return result;
}

bool UpdateChecker::ShouldRetryWithAGetRequest() {
    // If the server doesn't support POST requests,
    // it can return 405 NOT ALLOWED.
    // Try with a GET request.
    return m_httpSimple.GetRequestResult() ==
               HRESULT_FROM_WIN32(ERROR_WINHTTP_INVALID_HEADER) &&
           m_httpSimple.GetLastStatusCode() == 405;
}

void UpdateChecker::OnRequestDone() {
    if (ShouldRetryWithAGetRequest() && !m_aborted) {
        std::lock_guard<std::mutex> guard(m_httpSimpleGetRequestMutex);

        if (!m_aborted) {
            try {
                m_httpSimpleGetRequest = std::make_unique<CWinHTTPSimple>(
                    GetUpdateCheckerOptions(m_flags, nullptr, 0), true);
                THROW_IF_FAILED(m_httpSimpleGetRequest->SendRequest(
                    [this] { m_onUpdateCheckDone(); }));
            } catch (const std::exception& e) {
                m_httpSimpleGetRequest.reset();
                LOG(L"Get request failed: %S", e.what());
                m_onUpdateCheckDone();
            }

            return;
        }
    }

    m_onUpdateCheckDone();
}
