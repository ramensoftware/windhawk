#ifndef __WINHTTPSIMPLE_H__
#define __WINHTTPSIMPLE_H__

struct CWinHTTPSimpleOptions
{
	std::wstring   sURL;
	std::wstring   sVerb;                          // GET/POST etc.
	std::wstring   sUserAgent;
	std::wstring   sReferrer;
	std::vector<std::wstring> AcceptTypes;

	ULONGLONG      nDownloadStartPos = 0;          // Offset to resume the download at
	bool           bNoURLRedirect = FALSE;         // Set to true if you want to disable URL redirection following
	std::wstring   sFileToUpload;                  // The path of the file to upload
	std::wstring   sFileToDownloadInto;            // The path of the file to download into
	LPCVOID        lpOptional = NULL;              // Optional data to send immediately after the request headers
	DWORD          dwOptionalSize = 0;             // The size in bytes of lpOptional
	LPCVOID        lpRequest = NULL;               // The in memory data to send in the HTTP request
	DWORD          dwRequestSize = 0;              // The size in bytes of lpRequest
	double         dbLimit = 0;                    // For bandwidth throttling, The value in KB/Second to limit the connection to

	DWORD          dwAccessType = WINHTTP_ACCESS_TYPE_DEFAULT_PROXY;
	                                               // WINHTTP_ACCESS_TYPE_XXX, proxy/direct connection type
	std::wstring   sProxyServer;                   // The server for Proxy authentication
	std::wstring   sProxyUserName;                 // The username to use for Proxy authentication
	std::wstring   sProxyPassword;                 // The password to use for Proxy authentication
	DWORD          dwProxyPreauthenticationScheme = WINHTTP_AUTH_SCHEME_NEGOTIATE;
	                                               // The authentication scheme to use for proxy preauthentication
	bool           bProxyPreauthentication = TRUE; // Should we supply credentials on the first request for the Proxy rather than starting out with anonymous credentials
												   // and only authenticating when challenged

	std::wstring   sHTTPUserName;                  // The username to use for HTTP authentication
	std::wstring   sHTTPPassword;                  // the password to use for HTTP authentication
	DWORD          dwHTTPPreauthenticationScheme = WINHTTP_AUTH_SCHEME_NEGOTIATE;
	                                               // The authentication scheme to use for HTTP server preauthentication
	bool           bHTTPPreauthentication = TRUE;  // Should we supply credentials on the first request for the HTTP Server rather than starting out with anonymous credentials
												   // and only authenticating when challenged
};

class CWinHTTPSimple
{
	class CSimpleWinHTTPDownloader : public WinHTTPWrappers::CSyncDownloader
	{
	public:
		CSimpleWinHTTPDownloader()
			: m_hHandleClosedEvent(CreateEvent(NULL, TRUE, FALSE, NULL)) {
			if (!m_hHandleClosedEvent)
				AtlThrowLastWin32();
		}

		~CSimpleWinHTTPDownloader()
		{
			Close();
			WaitForSingleObject(m_hHandleClosedEvent, INFINITE);
		}

		HRESULT SendRequest(_In_ std::function<void()> doneCallback, _In_reads_bytes_opt_(dwOptionalLength) LPVOID lpOptional = WINHTTP_NO_REQUEST_DATA, _In_ DWORD dwOptionalLength = 0)
		{
			// Call the base class
			HRESULT hr = __super::SendRequest(lpOptional, dwOptionalLength);

			if (FAILED(hr)) {
				m_hr = hr;
			}
			else {
				m_hr = E_PENDING;
				if (doneCallback) {
					m_doneCallback = std::move(doneCallback);
				}
			}

			return hr;
		}

		HRESULT SendRequestSync(_In_reads_bytes_opt_(dwOptionalLength) LPVOID lpOptional = WINHTTP_NO_REQUEST_DATA, _In_ DWORD dwOptionalLength = 0) override
		{
			// Call the base class
			HRESULT hr = __super::SendRequestSync(lpOptional, dwOptionalLength);

			m_hr = hr;

			return hr;
		}

		HRESULT GetHresult()
		{
			return m_hr;
		}

	protected:
		virtual HRESULT OnCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
		{
			// Let the base class do the real work
			HRESULT ret = __super::OnCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);

			if (dwInternetStatus == WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING)
			{
				SetEvent(m_hHandleClosedEvent);
			}

			return ret;
		}

		virtual HRESULT OnCallbackComplete(_In_ HRESULT hr, _In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
		{
			m_hr = hr;

			// Let the base class do the cleanup
			HRESULT ret = __super::OnCallbackComplete(hr, hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);

			if (m_doneCallback)
			{
				m_doneCallback();
				m_doneCallback = nullptr;
			}

			return ret;
		}

		ATL::CHandle m_hHandleClosedEvent;
		std::function<void()> m_doneCallback;
		HRESULT m_hr = E_FAIL;
	};

	bool                          m_bAsync;
	WinHTTPWrappers::CSession     m_session;
	WinHTTPWrappers::CConnection  m_connection;
	std::vector<BYTE>             m_optionalData;
	std::vector<BYTE>             m_requestData;
	CSimpleWinHTTPDownloader      m_downloadRequest;

public:
	CWinHTTPSimple(CWinHTTPSimpleOptions options, bool bAsync = false)
		: m_bAsync(bAsync), m_downloadRequest() {
		// Crack the URL provided into its constituent parts
		URL_COMPONENTS urlComponents = { sizeof(urlComponents) };
		urlComponents.dwSchemeLength = static_cast<DWORD>(-1);
		urlComponents.dwHostNameLength = static_cast<DWORD>(-1);
		urlComponents.dwUrlPathLength = static_cast<DWORD>(-1);
		urlComponents.dwExtraInfoLength = static_cast<DWORD>(-1);
		BOOL bSuccess = WinHttpCrackUrl(options.sURL.c_str(), 0, 0, &urlComponents);
		ATLENSURE_THROW(bSuccess, AtlHresultFromLastError());

		// Create the session object
		HRESULT hr = m_session.Initialize(options.sUserAgent.c_str(), options.dwAccessType,
			options.sProxyServer.c_str() ? options.sProxyServer.c_str() : WINHTTP_NO_PROXY_NAME,
			WINHTTP_NO_PROXY_BYPASS, bAsync ? WINHTTP_FLAG_ASYNC : 0);
		ATLENSURE_SUCCEEDED(hr);

		// Create the connection object
		hr = m_connection.Initialize(m_session,
			std::wstring(urlComponents.lpszHostName, urlComponents.dwHostNameLength).c_str(),
			urlComponents.nPort);
		ATLENSURE_SUCCEEDED(hr);

		// Fill in all the member variables
		if (options.lpOptional)
		{
			m_optionalData.assign(reinterpret_cast<const BYTE*>(options.lpOptional),
				reinterpret_cast<const BYTE*>(options.lpOptional) + options.dwOptionalSize);
		}

		if (options.lpRequest)
		{
			m_requestData.assign(reinterpret_cast<const BYTE*>(options.lpRequest),
				reinterpret_cast<const BYTE*>(options.lpRequest) + options.dwRequestSize);
		}

		m_downloadRequest.m_sHTTPUserName = std::move(options.sHTTPUserName);
		m_downloadRequest.m_sHTTPPassword = std::move(options.sHTTPPassword);
		m_downloadRequest.m_sProxyUserName = std::move(options.sProxyUserName);
		m_downloadRequest.m_sProxyPassword = std::move(options.sProxyPassword);
		m_downloadRequest.m_nDownloadStartPos = options.nDownloadStartPos;
		m_downloadRequest.m_bHTTPPreauthentication = options.bHTTPPreauthentication;
		m_downloadRequest.m_dwHTTPPreauthenticationScheme = options.dwHTTPPreauthenticationScheme;
		m_downloadRequest.m_bProxyPreauthentication = options.bProxyPreauthentication;
		m_downloadRequest.m_dwProxyPreauthenticationScheme = options.dwProxyPreauthenticationScheme;
		m_downloadRequest.m_bNoURLRedirect = options.bNoURLRedirect;
		m_downloadRequest.m_sFileToDownloadInto = std::move(options.sFileToDownloadInto);
		m_downloadRequest.m_sFileToUpload = std::move(options.sFileToUpload);
		m_downloadRequest.m_lpRequest = m_requestData.data();
		m_downloadRequest.m_dwRequestSize = options.dwRequestSize;
		m_downloadRequest.m_dbLimit = options.dbLimit;

		size_t nAcceptTypes = options.AcceptTypes.size();
		std::vector<LPCWSTR> ppszAcceptTypes;
		if (nAcceptTypes)
		{
			ppszAcceptTypes.reserve(nAcceptTypes + 1);
			for (const auto& str : options.AcceptTypes)
				ppszAcceptTypes.push_back(str.c_str());
			ppszAcceptTypes.push_back(nullptr);
		}

		std::wstring sUrl =
			std::wstring(urlComponents.lpszUrlPath, urlComponents.dwUrlPathLength) +
			std::wstring(urlComponents.lpszExtraInfo, urlComponents.dwExtraInfoLength);

		// Create the request
		hr = m_downloadRequest.Initialize(m_connection, sUrl.c_str(),
			options.sVerb.length() ? options.sVerb.c_str() : NULL, NULL,
			options.sReferrer.length() ? options.sReferrer.c_str() : WINHTTP_NO_REFERER,
			nAcceptTypes ? ppszAcceptTypes.data() : WINHTTP_DEFAULT_ACCEPT_TYPES,
			urlComponents.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0);
		ATLENSURE_SUCCEEDED(hr);
	}

	HRESULT AddHeaders(LPCWSTR pwszHeaders, DWORD dwHeadersLength, DWORD dwModifiers)
	{
		return m_downloadRequest.AddHeaders(pwszHeaders, dwHeadersLength, dwModifiers);
	}

	HRESULT SendRequest(std::function<void()> doneCallback)
	{
		void* lpOptional = NULL;
		DWORD dwOptionalLength = 0;

		if (!m_optionalData.empty())
		{
			lpOptional = m_optionalData.data();
			dwOptionalLength = static_cast<DWORD>(m_optionalData.size());
		}

		if (m_bAsync)
		{
			return m_downloadRequest.SendRequest(std::move(doneCallback), lpOptional, dwOptionalLength);
		}
		else
		{
			return m_downloadRequest.SendRequestSync(lpOptional, dwOptionalLength);
		}
	}

	HRESULT QueryHeaders(DWORD dwInfoLevel, LPCWSTR pwszName, LPVOID lpBuffer, DWORD& dwBufferLength, DWORD* lpdwIndex)
	{
		return m_downloadRequest.QueryHeaders(dwInfoLevel, pwszName, lpBuffer, dwBufferLength, lpdwIndex);
	}

	HRESULT GetRequestResult()
	{
		return m_downloadRequest.GetHresult();
	}

	DWORD GetLastStatusCode()
	{
		bool valid;
		DWORD code = m_downloadRequest.GetLastStatusCode(valid);
		return valid ? code : 0;
	}

	const std::vector<BYTE>& GetResponse()
	{
		return m_downloadRequest.m_Response;
	}

	void Abort()
	{
		m_downloadRequest.Close();
	}
};

#endif // __WINHTTPSIMPLE_H__
