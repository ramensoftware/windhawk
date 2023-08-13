/*
Module : WinHTTPWrappers.h
Purpose: Defines the interface for a set of C++ class which encapsulate WinHTTP. The classes are based on 
         the MSDN Magazine article by Kenny Kerr at 
         https://docs.microsoft.com/en-us/archive/msdn-magazine/2008/august/windows-with-c-asynchronous-winhttp
History: PJN / 30-05-2011 1. All tracing in CWinHTTPHandle::OnCallback has been moved into a new TraceCallback
                          method which client code is free to call. Also all tracing in CWinHTTPHandle::
                          OnCallbackComplete has been moved into a new TraceCallbackComplete method. Also all
                          tracing in CDownloadFileWinHttpRequest::OnCallbackComplete has been moved into a new
                          TraceCallbackComplete method.
                          2. Moved cleanup of resources from CDownloadFileWinHttpRequest::OnCallbackComplete
                          to a new public method called ReleaseResources()
         PJN / 30-07-2011 1. CDownloadFileWinHttpRequest class is now called CAsyncWinHttpDownloader
                          2. Major rework of the CAsyncWinHttpDownloader to now support HTTP and Proxy
                          authentication, pre-authentication, resumed downloads, file uploads, in-memory arrays
                          and bandwidth throttling.
                          3. Fixed an issue in TraceCallback where WINHTTP_CALLBACK_STATUS_SECURE_FAILURE would
                          be reported incorrectly by TRACE statements
                          4. Addition of a new CSyncWinHttpDownloader class which provides for synchronous 
                          WinHTTP downloads.
                          5. Updated the sample app to allow all of the new configuration settings of 
                          CAsyncWinHttpDownloader and CSyncWinHttpDownloader classes to be exercised.
                          6. Fixed a bug in CWinHTTPRequest::WriteData() where buffer parameter was incorrectly
                          set as a LPVOID instead of a LPCVOID.
         PJN / 30-03-2013 1. Updated copyright details.
                          2. Updated the sample app to correctly release the file handles when the file is 
                          downloaded. Thanks to David Lowndes for reporting this bug.
                          3. Updated the code to clean compile on VC 2012 
                          4. TimeSinceStartDownload() method has been extended to return a __int64 return value
                          instead of a DWORD.
                          5. Changed class names to use C*WinHTTP* prefix instead of C*WinHttp*.
         PJN / 01-12-2013 1. Updated the code to clean compile on VC 2013. 
         PJN / 08-06-2014 1. Updated copyright details.  
                          2. Updated CAsyncWinHTTPDownloader::Initialize to allow the dwShareMode parameter of 
                          the ATL::CAtlFile::Create call for the file instances to be download and uploaded to 
                          be customized. The default value for the share mode is now 0 instead of FILE_SHARE_READ. 
                          Thanks to Simon Orde for providing this nice addition.
                          3. All the class methods have had SAL annotations added
         PJN / 08-03-2015 1. Updated copyright details.
                          2. Reworked the classes to optionally compile without MFC. By default the classes now use 
                          STL classes and idioms but if you define WINHTTPWRAPPERS_MFC_EXTENSTIONS the classes will 
                          revert back to the MFC behaviour.
                          3. Moved all the classes to a WinHTTPWrappers namespace
                          4. Renamed CWinHTTPHandle class to CHandle
                          5. Renamed CWinHTTPSession class to CSession
                          6. Renamed CWinHTTPConnection class to CConnection
                          7. Renamed CWinHTTPRequest class to CRequest
                          8. Renamed CAsyncWinHTTPDownloader class to CAsyncDownloader
                          9. Renamed CSyncWinHTTPDownloader class to CSyncDownloader
                          10. DeleteDownloadedFile now checks to see if "m_sFileToDownloadInto" is valid before it
                          calls DeleteFile. Thanks to Paul Jackson for reporting this issue.
                          11. Reworked the CAsyncDownloader::SendRequest, On407Response, On401Response & 
                          OnRequestErrorCallback methods to pass a more correct value for the "dwTotalLength" 
                          parameter in the call to WinHttpSendRequest. Thanks to Paul Jackson for reporting this 
                          issue.
         PJN / 11-03-2015 1. Optimized allocation of temporary string stack variables in 
                          CAsyncDownloader::Initialize & CAsyncDownloader::DeleteDownloadedFile. Thanks to Paul 
                          Jackson for reporting this issue.
         PJN / 14-06-2015 1. Addition of a CAsyncDownloader::GetLastStatusCode method.
                          2. CAsyncDownloader::OnHeadersAvailableCallback and CSyncDownloader::SendRequestSync now 
                          preserves the HTTP status code when the value received is not 200, 206, 401 or 407 and 
                          the return value ATL::AtlHresultFromWin32(ERROR_WINHTTP_INVALID_HEADER) is about to be returned.
                          3. CSyncDownloader::SendRequestSync method has been made virtual.
                          4. Update the sample app to report the last status code if available when a download request
                          fails.
         PJN / 07-11-2015 1. Updated SAL annotations in CHandle::SetOption to be consistent with Windows 10 SDK.
                          2. Fixed an issue in the use of _When_ SAL annotation in CHandle::SetOption
                          3. Update the code to compile cleanly on VC 2015
         PJN / 06-03-2016 1. Updated copyright details.
                          2. The CAsyncDownloader destructor now resets the status callback function via 
                          SetStatusCallback(NULL, WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS). This prevents spurious 
                          callbacks occuring after the C++ object is destroyed which depending on how you allocated the 
                          C++ object could cause access violations in CHandle::_Callback.
                          3. Optimized the logic in the sample app when updating the edit box with status information
         PJN / 16-04-2017 1. Updated copyright details.
                          2. Added support for WinHttpCreateProxyResolver, WinHttpResetAutoProxy, WinHttpWriteProxySettings,
                          WinHttpReadProxySettings & WinHttpGetProxySettingsVersion from the latest Windows 10 SDK
         PJN / 18-09-2017 1. Replaced CString::operator LPC*STR() calls throughout the codebase with CString::GetString 
                          calls
         PJN / 23-05-2018 1. Replaced NULL with nullptr throughout the code.
                          3. Fixed a number of C++ core guidelines compiler warnings. These changes mean that the code
                          will now only compile on VC 2017 or later.
         PJN / 02-09-2018 1. Fixed a number of compiler warnings when using VS 2017 15.8.2
         PJN / 29-09-2018 1. Removed code which supported WINHTTPWRAPPERS_MFC_EXTENSIONS define
                          2. Added wrappers for WinHttpWebSocketCompleteUpgrade, WinHttpWebSocketSend, WinHttpWebSocketReceive,
                          WinHttpWebSocketShutdown, WinHttpWebSocketClose & WinHttpWebSocketQueryCloseStatus APIs.
                          3. Added wrappers for WinHttpGetProxyForUrlEx, WinHttpGetProxyForUrlEx2, WinHttpGetProxyResult & 
                          WinHttpGetProxyResultEx APIs.
                          4. Reworked TimeSinceStartDownload to use GetTickCount64 API.
         PJN / 24-11-2018 1. Fixed some further compiler warnings when using VS 2017 15.9.2
         PJN / 19-04-2019 1. Updated copyright details
                          2. Updated the code to clean compile on VC 2019
         PJN / 23-06-2019 1. Updated the code to clean compile when _ATL_NO_AUTOMATIC_NAMESPACE is defined.
         PJN / 14-08-2019 1. Fixed some further compiler warnings when using VC 2019 Preview v16.3.0 Preview 2.0
                          2. Added support for new WinHttpAddRequestHeadersEx API available in latest Windows 10 SDK
         PJN / 16-09-2019 1. Updated code to handle all 2XX response codes.
         PJN / 03-11-2019 1. Updated initialization of various structs to use C++ 11 list initialization
         PJN / 18-01-2020 1. Updated copyright details
                          2. Fixed more Clang-Tidy static code analysis warnings in the code.
                          3. Replaced BOOL with bool in various places
         PJN / 01-02-2020 1. Fixed a bug in the sample app when calling the WinHttpCrackUrl. Thanks to Onur Senturk for
                          reporting this issue.
                          2. Fixed a bug in CSyncDownloader::SendRequestSync where the resources would not be released if
                          the download was successful. Again thanks to to Onur Senturk for reporting this issue.
         PJN / 12-04-2020 1. Fixed more Clang-Tidy static code analysis warnings in the code.
         PJN / 13-11-2020 1. Added support for new WinHttpReadDataEx & WinHttpQueryHeadersEx APIs available in latest 
                          Windows 10 SDK.
         PJN / 09-04-2021 1. Updated copyright details
                          2. Fixed more /analyze static code analysis warnings in the code.

Copyright (c) 2011 - 2021 by PJ Naughter (Web: www.naughter.com, Email: pjna@naughter.com)

All rights reserved.

Copyright / Usage Details:

You are allowed to include the source code in any product (commercial, shareware, freeware or otherwise) 
when your product is released in binary form. You are allowed to modify the source code in any way you want 
except you cannot modify the copyright details at the top of each module. If you want to distribute source 
code with your application, then you are only allowed to distribute versions released by the author. This is 
to maintain a single distribution point for the source code.

*/


/////////////////////////// Macros / Defines //////////////////////////////////

#ifndef __WINHTTPWRAPPERS_H__
#define __WINHTTPWRAPPERS_H__

#ifndef CWINHTTPWRAPPERS_EXT_CLASS
#define CWINHTTPWRAPPERS_EXT_CLASS
#endif //#ifndef CWINHTTPWRAPPERS_EXT_CLASS

#pragma comment(lib, "Winhttp.lib")


/////////////////////////// Includes //////////////////////////////////////////

#ifndef _WINHTTPX_
#pragma message("To avoid this message, please put WinHttp.h in your pre compiled header (normally stdafx.h)")
#include <winhttp.h>
#endif //#ifndef _WINHTTPX_

#ifndef __ATLBASE_H__
#pragma message("To avoid this message, please put atlbase.h in your pre compiled header (normally stdafx.h)")
#include <atlbase.h>
#endif //#ifndef __ATLBASE_H__

#ifndef __ATLFILE_H__
#pragma message("To avoid this message, please put atlfile.h in your pre compiled header (normally stdafx.h)")
#include <atlfile.h>
#endif //#ifndef __ATLFILE_H__

#ifndef __ATLSTR_H__
#pragma message("To avoid this message, please put atlstr.h in your pre compiled header (normally stdafx.h)")
#include <atlfile.h>
#endif //#ifndef __ATLSTR_H__

#ifndef _STRING_
#pragma message("To avoid this message, please put string in your pre compiled header (normally stdafx.h)")
#include <string>
#endif //#ifndef _STRING_

#ifndef _VECTOR_
#pragma message("To avoid this message, please put vector in your pre compiled header (normally stdafx.h)")
#include <vector>
#endif //#ifndef _VECTOR_


/////////////////////////// Classes ///////////////////////////////////////////

namespace WinHTTPWrappers
{

//Typedefs
  using String = std::wstring;
  using ByteArray = std::vector<BYTE>;


//Wrapper for a WinHttp HINTERNET handle
class CWINHTTPWRAPPERS_EXT_CLASS CHandle
{
public:
//Constructors / Destructors
  CHandle() noexcept : m_h(nullptr)
  {
  }

  CHandle(_In_ const CHandle&) = delete;

  CHandle(_In_ CHandle&& handle) noexcept
  {
    m_h = handle.m_h;
    handle.m_h = nullptr;
  }

  explicit CHandle(_In_ HINTERNET h) noexcept : m_h(h)
  {
  }

  virtual ~CHandle()
  {
    if (m_h != nullptr)
      Close();
  }

//Methods
  CHandle& operator=(_In_ const CHandle&) = delete;

  CHandle& operator=(_In_ CHandle&& handle) noexcept
  {
    if (m_h != nullptr)
      Close();
    m_h = handle.m_h;
    handle.m_h = nullptr;

    return *this;
  }

  operator HINTERNET() const noexcept
  {
    return m_h;
  }

  void Attach(_In_ HINTERNET h) noexcept
  {
#pragma warning(suppress: 26477)
    ATLASSUME(m_h == nullptr);
    m_h = h;
  }

  HINTERNET Detach() noexcept
  {
    HINTERNET h = m_h;
    m_h = nullptr;
    return h;
  }

  void Close() noexcept
  {
    if (m_h != nullptr)
    {
      WinHttpCloseHandle(m_h);
      m_h = nullptr;
    }
  }

  HRESULT QueryOption(IN DWORD dwOption, _Out_writes_bytes_to_opt_(dwBufferLength, dwBufferLength) __out_data_source(NETWORK) void* pBuffer, IN OUT DWORD& dwBufferLength) noexcept
  {
    if (!WinHttpQueryOption(m_h, dwOption, pBuffer, &dwBufferLength))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT SetOption(_In_ DWORD dwOption,
                    _When_((dwOption == WINHTTP_OPTION_USERNAME ||
                     dwOption == WINHTTP_OPTION_PASSWORD ||
                     dwOption == WINHTTP_OPTION_PROXY_USERNAME ||
                     dwOption == WINHTTP_OPTION_PROXY_PASSWORD ||
                     dwOption == WINHTTP_OPTION_USER_AGENT),
                     _At_((LPCWSTR)lpBuffer, _In_reads_(dwBufferLength)))
                    _When_((dwOption == WINHTTP_OPTION_CLIENT_CERT_CONTEXT),
                      _In_reads_bytes_opt_(dwBufferLength))
                    _When_((dwOption != WINHTTP_OPTION_USERNAME &&
                     dwOption != WINHTTP_OPTION_PASSWORD &&
                     dwOption != WINHTTP_OPTION_PROXY_USERNAME &&
                     dwOption != WINHTTP_OPTION_PROXY_PASSWORD &&
                     dwOption != WINHTTP_OPTION_CLIENT_CERT_CONTEXT &&
                     dwOption != WINHTTP_OPTION_USER_AGENT),
                    _In_reads_bytes_(dwBufferLength))
                    LPVOID lpBuffer, _In_ DWORD dwBufferLength) noexcept
  {
  #pragma warning(suppress: 6387)
    if (!WinHttpSetOption(m_h, dwOption, lpBuffer, dwBufferLength))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  WINHTTP_STATUS_CALLBACK SetStatusCallback(_In_opt_ WINHTTP_STATUS_CALLBACK lpfnInternetCallback, _In_ DWORD dwNotificationFlags = WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS) noexcept
  {
    return WinHttpSetStatusCallback(m_h, lpfnInternetCallback, dwNotificationFlags, NULL);
  }

  WINHTTP_STATUS_CALLBACK SetStatusCallback(_In_ DWORD dwNotificationFlags = WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS) noexcept
  {
    return SetStatusCallback(_Callback, dwNotificationFlags);
  }

//Member variables
  HINTERNET m_h;

protected:
//Methods
  static void CALLBACK _Callback(_In_ HINTERNET hInternet, _In_ DWORD_PTR dwContext, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    //Check to see if we have a context value
    if (dwContext != 0)
    {
      //Convert from the SDK world to the C++ world.
#pragma warning(suppress: 26429 26490)
      auto pThis = reinterpret_cast<CHandle*>(dwContext);
#pragma warning(suppress: 26477)
      ATLASSERT(pThis != nullptr);

      //Call the virtual "OnCallback" method
#pragma warning(suppress: 26486)
      const HRESULT hr = pThis->OnCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);

      //If the "Callback" method failed called the "OnCallbackComplete" method
      if (FAILED(hr))
#pragma warning(suppress: 26486)
        pThis->OnCallbackComplete(hr, hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
    }
  }

#ifdef _DEBUG
  static void TraceCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    switch (dwInternetStatus)
    {
      case WINHTTP_CALLBACK_STATUS_CLOSING_CONNECTION:
      {
        ATLTRACE(_T("Closing the connection to the server, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_CONNECTED_TO_SERVER:
      {
        ATLTRACE(_T("Successfully connected to the server:%ls, Handle:%p\n"), static_cast<LPWSTR>(lpvStatusInformation), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_CONNECTING_TO_SERVER:
      {
        ATLTRACE(_T("Connecting to the server:%ls, Handle:%p\n"), static_cast<LPWSTR>(lpvStatusInformation), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_CONNECTION_CLOSED:
      {
        ATLTRACE(_T("Successfully closed the connection to the server, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_DATA_AVAILABLE:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Data is available to be retrieved, Handle:%p, Data Available:%u\n"), hInternet, *(static_cast<DWORD*>(lpvStatusInformation)));
        break;
      }
      case WINHTTP_CALLBACK_STATUS_HANDLE_CREATED:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Handle created, Handle:%p\n"), *(static_cast<HINTERNET*>(lpvStatusInformation)));
        break;
      }
      case WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Handle closing, Handle:%p\n"), *(static_cast<HINTERNET*>(lpvStatusInformation)));
        break;
      }
      case WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE:
      {
        ATLTRACE(_T("The response header has been received, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_INTERMEDIATE_RESPONSE:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Received an intermediate (100 level) status code message from the server, Handle:%p, Status:%u\n"), hInternet, *(static_cast<DWORD*>(lpvStatusInformation)));
        break;
      }
      case WINHTTP_CALLBACK_STATUS_NAME_RESOLVED:
      {
        ATLTRACE(_T("Successfully found the IP address of the server:%ls, Handle:%p\n"), static_cast<LPWSTR>(lpvStatusInformation), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_READ_COMPLETE:
      {
        ATLTRACE(_T("Data was successfully read from the server, Data Read:%u, Handle:%p\n"), dwStatusInformationLength, hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_RECEIVING_RESPONSE:
      {
        ATLTRACE(_T("Waiting for the server to respond to a request, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_REDIRECT:
      {
        ATLTRACE(_T("An HTTP request is about to automatically redirect the request to %ls, Handle:%p\n"), static_cast<LPWSTR>(lpvStatusInformation), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_REQUEST_ERROR:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        auto pResult = static_cast<const WINHTTP_ASYNC_RESULT*>(lpvStatusInformation);
        ATLTRACE(_T("An error occurred while sending an HTTP request, Error:%u, Handle:%p\n"), pResult->dwError, hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_REQUEST_SENT:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Successfully sent the information request to the server, Data Sent:%u, Handle:%p\n"), *(static_cast<DWORD*>(lpvStatusInformation)), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_RESOLVING_NAME:
      {
        ATLTRACE(_T("Looking up the IP address of a server name:%ls, Handle:%p\n"), static_cast<LPWSTR>(lpvStatusInformation), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_RESPONSE_RECEIVED:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Successfully received a response from the server, Data Received:%u, Handle:%p\n"), *(static_cast<DWORD*>(lpvStatusInformation)), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_SECURE_FAILURE:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        const DWORD dwStatusInformation = *(static_cast<DWORD*>(lpvStatusInformation));
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_CERT_REV_FAILED)
        {
          ATLTRACE(_T("Certification revocation checking has been enabled, but the revocation check failed to verify whether a certificate has been revoked, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_INVALID_CERT)
        {
          ATLTRACE(_T("SSL certificate is invalid, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_CERT_REVOKED)
        {
          ATLTRACE(_T("SSL certificate was revoked, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_INVALID_CA)
        {
          ATLTRACE(_T("The function is unfamiliar with the Certificate Authority that generated the server's certificate, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_CERT_CN_INVALID)
        {
          ATLTRACE(_T("SSL certificate common name (host name field) is incorrect, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_CERT_DATE_INVALID)
        {
          ATLTRACE(_T("SSL certificate date that was received from the server is bad. The certificate is expired, Handle:%p\n"), hInternet);
        }
        if (dwStatusInformation & WINHTTP_CALLBACK_STATUS_FLAG_SECURITY_CHANNEL_ERROR)
        {
          ATLTRACE(_T("The application experienced an internal error loading the SSL libraries, Handle:%p\n"), hInternet);
        }
        break;
      }
      case WINHTTP_CALLBACK_STATUS_SENDING_REQUEST:
      {
        ATLTRACE(_T("Sending the information request to the server, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE:
      {
        ATLTRACE(_T("The request completed successfully, Handle:%p\n"), hInternet);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE:
      {
#pragma warning(suppress: 26477)
        ATLASSUME(lpvStatusInformation != nullptr);
        ATLTRACE(_T("Data was successfully written to the server, Data Written:%u, Handle:%p\n"), *(static_cast<DWORD*>(lpvStatusInformation)), hInternet);
        break;
      }
      default:
      {
        ATLTRACE(_T("Unknown status:%08X, Handle:%p\n"), dwInternetStatus, hInternet);
        break;
      }
    }
  }
#endif //#ifdef _DEBUG

#pragma warning(suppress: 26440)
  virtual HRESULT OnCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus,  _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    return S_FALSE; //S_FALSE means not handled in our callback
  }

#ifdef _DEBUG
  static void TraceCallbackComplete(_In_ HRESULT hr, _In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus,  _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    ATLTRACE(_T("CWinHTTPHandle::TraceCallbackComplete, HRESULT:%08X, InternetStatus:%08X, Handle:%p\n"), hr, dwInternetStatus, hInternet);
  }
#endif //#ifdef _DEBUG

#pragma warning(suppress: 26440)
  virtual HRESULT OnCallbackComplete(_In_ HRESULT hr, _In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(hr);
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    return E_NOTIMPL;
  }
};


//Wrapper for a WinHttp resolver HINTERNET handle
class CWINHTTPWRAPPERS_EXT_CLASS CResolver : public CHandle
{
public:
//Constructors / Destructors
  CResolver() = default;
  CResolver(_In_ const CResolver&) = delete;
  CResolver(_In_ CResolver&& resolver) noexcept : CHandle(std::move(resolver))
  {
  }
  explicit CResolver(_In_ HINTERNET h) noexcept : CHandle(h)
  {
  }
  ~CResolver() = default; //NOLINT(modernize-use-override)

//Methods
  CResolver& operator=(_In_ const CResolver&) = delete;

#pragma warning(suppress: 26456)
  CResolver& operator=(_In_ CResolver&& resolver) noexcept
  {
    __super::operator=(std::move(resolver));
    return *this;
  }

__if_exists(WinHttpGetProxyForUrlEx)
{
  DWORD GetProxyForUrlEx(_In_ PCWSTR pcwszUrl, _In_ WINHTTP_AUTOPROXY_OPTIONS* pAutoProxyOptions, _In_opt_ DWORD_PTR pContext) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpGetProxyForUrlEx(m_h, pcwszUrl, pAutoProxyOptions, pContext);
  }
}

__if_exists(WinHttpGetProxyForUrlEx2)
{
  DWORD GetProxyForUrlEx2(_In_ PCWSTR pcwszUrl, _In_ WINHTTP_AUTOPROXY_OPTIONS* pAutoProxyOptions, _In_ DWORD cbInterfaceSelectionContext,
                          _In_reads_bytes_opt_(cbInterfaceSelectionContext) BYTE* pInterfaceSelectionContext, _In_opt_ DWORD_PTR pContext) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpGetProxyForUrlEx2(m_h, pcwszUrl, pAutoProxyOptions, cbInterfaceSelectionContext, pInterfaceSelectionContext, pContext);
  }
}

__if_exists(WinHttpGetProxyResult)
{
  DWORD GetProxyResult(_Out_ WINHTTP_PROXY_RESULT* pProxyResult) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpGetProxyResult(m_h, pProxyResult);
  }
}

__if_exists(WinHttpGetProxyResultEx)
{
  DWORD GetProxyResultEx(_Out_ _Out_ WINHTTP_PROXY_RESULT_EX* pProxyResultEx) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpGetProxyResultEx(m_h, pProxyResultEx);
  }
}

};


//Wrapper for a WinHttp web socket handle
class CWINHTTPWRAPPERS_EXT_CLASS CWebSocket : public CHandle
{
public:
//Constructors / Destructors
  CWebSocket() = default;
  CWebSocket(_In_ const CWebSocket&) = delete;
#pragma warning(suppress: 26495)
  CWebSocket(_In_ CWebSocket&& socket) noexcept : CHandle(std::move(socket))
  {
  }
#pragma warning(suppress: 26495)
  explicit CWebSocket(_In_ HINTERNET h) noexcept : CHandle(h)
  {
  }
  ~CWebSocket() = default; //NOLINT(modernize-use-override)

//Methods
  CWebSocket& operator=(_In_ const CWebSocket&) = delete;

#pragma warning(suppress: 26456)
  CWebSocket& operator=(_In_ CWebSocket&& socket) noexcept
  {
    __super::operator=(std::move(socket));
    return *this;
  }

#pragma warning(suppress: 26812)
  DWORD Send(_In_ WINHTTP_WEB_SOCKET_BUFFER_TYPE eBufferType, _In_reads_opt_(dwBufferLength) PVOID pvBuffer, _In_ DWORD dwBufferLength) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpWebSocketSend(m_h, eBufferType, pvBuffer, dwBufferLength);
  }

  DWORD Receive(_Out_writes_bytes_to_(dwBufferLength, *pdwBytesRead) PVOID pvBuffer, _In_ DWORD dwBufferLength, _Out_range_(0, dwBufferLength) DWORD* pdwBytesRead, _Out_ WINHTTP_WEB_SOCKET_BUFFER_TYPE* peBufferType) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpWebSocketReceive(m_h, pvBuffer, dwBufferLength, pdwBytesRead, peBufferType);
  }

  DWORD Shutdown(_In_ USHORT usStatus, _In_reads_bytes_opt_(dwReasonLength) PVOID pvReason, _In_range_(0, WINHTTP_WEB_SOCKET_MAX_CLOSE_REASON_LENGTH) DWORD dwReasonLength) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpWebSocketShutdown(m_h, usStatus, pvReason, dwReasonLength);
  }

  DWORD WebSocketClose(_In_ USHORT usStatus, _In_reads_bytes_opt_(dwReasonLength) PVOID pvReason, _In_range_(0, WINHTTP_WEB_SOCKET_MAX_CLOSE_REASON_LENGTH) DWORD dwReasonLength) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpWebSocketClose(m_h, usStatus, pvReason, dwReasonLength);
  }

  DWORD QueryCloseStatus(_Out_ USHORT* pusStatus, _Out_writes_bytes_to_opt_(dwReasonLength, *pdwReasonLengthConsumed) PVOID pvReason,
                         _In_range_(0, WINHTTP_WEB_SOCKET_MAX_CLOSE_REASON_LENGTH) DWORD dwReasonLength, _Out_range_(0, WINHTTP_WEB_SOCKET_MAX_CLOSE_REASON_LENGTH) DWORD* pdwReasonLengthConsumed) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return WinHttpWebSocketQueryCloseStatus(m_h, pusStatus, pvReason, dwReasonLength, pdwReasonLengthConsumed);
  }

//Member variables
  HINTERNET m_h;
};


//Wrapper for a WinHttp Session HINTERNET handle
class CWINHTTPWRAPPERS_EXT_CLASS CSession : public CHandle
{
public:
//Constructors / Destructors
  CSession() = default;
  CSession(_In_ const CSession&) = delete;
  CSession(_In_ CSession&& session) noexcept : CHandle(std::move(session))
  {
  }
  explicit CSession(_In_ HINTERNET h) noexcept : CHandle(h)
  {
  }
  ~CSession() = default; //NOLINT(modernize-use-override)

//Methods
  CSession& operator=(_In_ const CSession&) = delete;

#pragma warning(suppress: 26456)
  CSession& operator=(_In_ CSession&& session) noexcept
  {
    __super::operator=(std::move(session));
    return *this;
  }

  HRESULT Initialize(_In_opt_z_ LPCWSTR pwszUserAgent, _In_ DWORD dwAccessType = WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, _In_opt_z_ LPCWSTR pwszProxyName = WINHTTP_NO_PROXY_NAME, 
                     _In_opt_z_ LPCWSTR pwszProxyBypass = WINHTTP_NO_PROXY_BYPASS, _In_ DWORD dwFlags = WINHTTP_FLAG_ASYNC) noexcept
  {
    HINTERNET hSession = WinHttpOpen(pwszUserAgent, dwAccessType, pwszProxyName, pwszProxyBypass, dwFlags);
    if (hSession == nullptr)
      return ATL::AtlHresultFromLastError();
    Attach(hSession);

    return S_OK;
  }

  HRESULT GetProxyForUrl(_In_ LPCWSTR lpcwszUrl, _In_ WINHTTP_AUTOPROXY_OPTIONS& AutoProxyOptions, _Out_ DWORD& dwAccessType, _Out_ String& sProxy, _Out_ String& sProxyBypass)
  {
    WINHTTP_PROXY_INFO proxyInfo{};
    if (!WinHttpGetProxyForUrl(m_h, lpcwszUrl, &AutoProxyOptions, &proxyInfo))
      return ATL::AtlHresultFromLastError();

    //Update the output parameters
    dwAccessType = proxyInfo.dwAccessType;
#pragma warning(suppress: 6387)
    sProxy = proxyInfo.lpszProxy;
#pragma warning(suppress: 6387)
    sProxyBypass = proxyInfo.lpszProxyBypass;

    //Free up the allocated memory
    if (proxyInfo.lpszProxy != nullptr)
      GlobalFree(proxyInfo.lpszProxy);
    if (proxyInfo.lpszProxyBypass != nullptr)
      GlobalFree(proxyInfo.lpszProxyBypass);
     
    return S_OK;
  }

  HRESULT SetTimeouts(_In_ int dwResolveTimeout, _In_ int dwConnectTimeout, _In_ int dwSendTimeout, _In_ int dwReceiveTimeout) noexcept
  {
    if (!WinHttpSetTimeouts(m_h, dwResolveTimeout, dwConnectTimeout, dwSendTimeout, dwReceiveTimeout))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

__if_exists(WinHttpCreateProxyResolver)
{
  HRESULT CreateProxyResolver(_Inout_ CResolver& resolver) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(resolver.operator HANDLE() == nullptr);
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return ATL::AtlHresultFromWin32(WinHttpCreateProxyResolver(m_h, &resolver.m_h));
  }
}

__if_exists(WinHttpResetAutoProxy)
{
  HRESULT ResetAutoProxy(_In_ DWORD dwFlags) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return ATL::AtlHresultFromWin32(WinHttpResetAutoProxy(m_h, dwFlags));
  }
}

__if_exists(WinHttpWriteProxySettings)
{
  HRESULT WriteProxySettings(_In_ BOOL fForceUpdate, _In_ WINHTTP_PROXY_SETTINGS* pWinHttpProxySettings) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return ATL::AtlHresultFromWin32(WinHttpWriteProxySettings(m_h, fForceUpdate, pWinHttpProxySettings));
  }
}

__if_exists(WinHttpReadProxySettings)
{
  HRESULT ReadProxySettings(_In_opt_ PCWSTR pcwszConnectionName, _In_ BOOL fFallBackToDefaultSettings, _In_ BOOL fSetAutoDiscoverForDefaultSettings,
                            _Out_ DWORD* pdwSettingsVersion, _Out_ BOOL *pfDefaultSettingsAreReturned, _Out_ WINHTTP_PROXY_SETTINGS* pWinHttpProxySettings) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    return ATL::AtlHresultFromWin32(WinHttpReadProxySettings(m_h, pcwszConnectionName, fFallBackToDefaultSettings, fSetAutoDiscoverForDefaultSettings, pdwSettingsVersion, pfDefaultSettingsAreReturned, pWinHttpProxySettings));
  }
}

__if_exists(WinHttpGetProxySettingsVersion)
{
  HRESULT GetProxySettingsVersion(_Out_ DWORD* pdwProxySettingsVersion) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSERT(m_h != nullptr);

    if (!WinHttpGetProxySettingsVersion(m_h, pdwProxySettingsVersion))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }
}

};


//Wrapper for a WinHttp connection HINTERNET handle
class CWINHTTPWRAPPERS_EXT_CLASS CConnection : public CHandle
{
public:
//Constructors / Destructors
  CConnection() = default;
  CConnection(_In_ const CConnection&) = delete;
  CConnection(_In_ CConnection&& connection) noexcept : CHandle(std::move(connection))
  {
  }
  explicit CConnection(_In_ HINTERNET h) noexcept : CHandle(h)
  {
  }
  ~CConnection() = default; //NOLINT(modernize-use-override)

//Methods
  CConnection& operator=(_In_ const CConnection&) = delete;

#pragma warning(suppress: 26456)
  CConnection& operator=(_In_ CConnection&& connection) noexcept
  {
    __super::operator=(std::move(connection));
    return *this;
  }

  HRESULT Initialize(_In_ const CSession& session, _In_z_ LPCWSTR pwszServerName, _In_ INTERNET_PORT nServerPort = INTERNET_DEFAULT_PORT) noexcept
  {
    HINTERNET hConnection = WinHttpConnect(session, pwszServerName, nServerPort, 0);
    if (hConnection == nullptr)
      return ATL::AtlHresultFromLastError();
    Attach(hConnection);

    return S_OK;
  }
};


//Wrapper for a WinHttp request HINTERNET handle
class CWINHTTPWRAPPERS_EXT_CLASS CRequest : public CHandle
{
public:
//Constructors / Destructors
  CRequest() = default;
  CRequest(_In_ const CRequest&) = delete;
  CRequest(_In_ CRequest&& request) noexcept : CHandle(std::move(request))
  {
  }
  explicit CRequest(_In_ HINTERNET h) noexcept : CHandle(h)
  {
  }
  ~CRequest() = default; //NOLINT(modernize-use-override)

//Methods
  CRequest& operator=(_In_ const CRequest&) = delete;

#pragma warning(suppress: 26456)
  CRequest& operator=(_In_ CRequest&& request) noexcept
  {
    __super::operator=(std::move(request));
    return *this;
  }

  HRESULT Initialize(_In_ const CConnection& connection, _In_z_ LPCWSTR pwszObjectName, _In_opt_z_ LPCWSTR pwszVerb = nullptr, _In_opt_z_ LPCWSTR pwszVersion = nullptr, _In_opt_z_ LPCWSTR pwszReferrer = WINHTTP_NO_REFERER,
                     _In_opt_ LPCWSTR* ppwszAcceptTypes = WINHTTP_DEFAULT_ACCEPT_TYPES, _In_ DWORD dwFlags = 0) noexcept
  {
    HINTERNET hRequest = WinHttpOpenRequest(connection, pwszVerb, pwszObjectName, pwszVersion, pwszReferrer, ppwszAcceptTypes, dwFlags); 
    if (hRequest == nullptr)
      return ATL::AtlHresultFromLastError();
    Attach(hRequest);

    return S_OK;
  }

  HRESULT AddHeaders(
                   #ifdef _When_
                     _When_(dwHeadersLength == (DWORD)-1, _In_z_)
                     _When_(dwHeadersLength != (DWORD)-1, _In_reads_(dwHeadersLength)) LPCWSTR pwszHeaders,
                   #else
                     _In_ LPCWSTR pwszHeaders,
                   #endif
                     _In_ DWORD dwHeadersLength, _In_ DWORD dwModifiers) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSUME(m_h != nullptr);

    if (!WinHttpAddRequestHeaders(m_h, pwszHeaders, dwHeadersLength, dwModifiers))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

__if_exists(WinHttpAddRequestHeadersEx)
{
  DWORD AddRequestHeaders(_In_ DWORD dwModifiers, _In_ ULONGLONG ullFlags, _In_ ULONGLONG ullExtra,
                          _In_ DWORD cHeaders, _In_reads_(cHeaders) WINHTTP_EXTENDED_HEADER* pHeaders) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSUME(m_h != nullptr);

    return WinHttpAddRequestHeadersEx(m_h, dwModifiers, ullFlags, ullExtra, cHeaders, pHeaders);
  }
}

  HRESULT QueryAuthSchemes(_Out_ DWORD& dwSupportedSchemes, _Out_ DWORD& dwFirstScheme, _Out_ DWORD& dwAuthTarget) noexcept
  {
    if (!WinHttpQueryAuthSchemes(m_h, &dwSupportedSchemes, &dwFirstScheme, &dwAuthTarget))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT QueryDataAvailable(__out_data_source(NETWORK) DWORD* lpdwNumberOfBytesAvailable) noexcept
  {
    if (!WinHttpQueryDataAvailable(m_h, lpdwNumberOfBytesAvailable))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT QueryHeaders(IN DWORD dwInfoLevel, IN LPCWSTR pwszName OPTIONAL, _Out_writes_bytes_to_opt_(dwBufferLength, dwBufferLength) __out_data_source(NETWORK) LPVOID lpBuffer, IN OUT DWORD& dwBufferLength, IN OUT DWORD* lpdwIndex OPTIONAL) noexcept
  {
    if (!WinHttpQueryHeaders(m_h, dwInfoLevel, pwszName, lpBuffer, &dwBufferLength, lpdwIndex))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

__if_exists(WinHttpQueryHeadersEx)
{
#pragma warning(suppress: 26476)
  HRESULT QueryHeadersEx(_In_ DWORD dwInfoLevel, _In_ ULONGLONG ullFlags, _In_ UINT uiCodePage, _Inout_opt_ PDWORD pdwIndex, _In_opt_ PWINHTTP_HEADER_NAME pHeaderName, _Out_writes_bytes_to_opt_(*pdwBufferLength, *pdwBufferLength) PVOID pBuffer,
                         _Inout_ PDWORD pdwBufferLength, _Out_writes_opt_(*pdwHeadersCount) PWINHTTP_EXTENDED_HEADER* ppHeaders, _Out_ PDWORD pdwHeadersCount) noexcept
  {
#pragma warning(suppress: 6001)
    if (!WinHttpQueryHeadersEx(m_h, dwInfoLevel, ullFlags, uiCodePage, pdwIndex, pHeaderName, pBuffer, pdwBufferLength, ppHeaders, pdwHeadersCount))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }
}

  HRESULT ReadData(_Out_writes_bytes_to_(dwNumberOfBytesToRead, *lpdwNumberOfBytesRead) __out_data_source(NETWORK) LPVOID lpBuffer, IN DWORD dwNumberOfBytesToRead, OUT DWORD* lpdwNumberOfBytesRead) noexcept
  {
    if (!WinHttpReadData(m_h, lpBuffer, dwNumberOfBytesToRead, lpdwNumberOfBytesRead))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

__if_exists(WinHttpReadDataEx)
{
  HRESULT ReadDataEx(_Out_writes_bytes_to_(dwNumberOfBytesToRead, *lpdwNumberOfBytesRead) __out_data_source(NETWORK) LPVOID lpBuffer, IN DWORD dwNumberOfBytesToRead, OUT DWORD* lpdwNumberOfBytesRead,
                     IN ULONGLONG ullFlags, IN DWORD cbProperty, _In_reads_bytes_opt_(cbProperty) PVOID pvProperty) noexcept
  {
    if (!WinHttpReadDataEx(m_h, lpBuffer, dwNumberOfBytesToRead, lpdwNumberOfBytesRead, ullFlags, cbProperty, pvProperty))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }
}

    HRESULT ReceiveResponse() noexcept
  {
    if (!WinHttpReceiveResponse(m_h, nullptr))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT SendRequest(_In_reads_opt_(dwHeadersLength) LPCWSTR pwszHeaders = WINHTTP_NO_ADDITIONAL_HEADERS, _In_ DWORD dwHeadersLength = 0, _In_reads_bytes_opt_(dwOptionalLength) LPVOID lpOptional = WINHTTP_NO_REQUEST_DATA, _In_ DWORD dwOptionalLength = 0, _In_ DWORD dwTotalLength = 0, _In_ DWORD_PTR dwContext = 0) noexcept
  {
    if (!WinHttpSendRequest(m_h, pwszHeaders, dwHeadersLength, lpOptional, dwOptionalLength, dwTotalLength, dwContext))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT SetCredentials(_In_ DWORD AuthTargets, _In_ DWORD AuthScheme, _In_ LPCWSTR pwszUserName, _In_ LPCWSTR pwszPassword) noexcept
  {
    if (!WinHttpSetCredentials(m_h, AuthTargets, AuthScheme, pwszUserName, pwszPassword, nullptr))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT SetTimeouts(_In_ int dwResolveTimeout, _In_ int dwConnectTimeout, _In_ int dwSendTimeout, _In_ int dwReceiveTimeout) noexcept
  {
    if (!WinHttpSetTimeouts(m_h, dwResolveTimeout, dwConnectTimeout, dwSendTimeout, dwReceiveTimeout))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  HRESULT WriteData(_In_reads_bytes_opt_(dwNumberOfBytesToWrite) LPCVOID lpBuffer, _In_ DWORD dwNumberOfBytesToWrite, _Out_opt_ DWORD* lpdwNumberOfBytesWritten) noexcept
  {
    if (!WinHttpWriteData(m_h, lpBuffer, dwNumberOfBytesToWrite, lpdwNumberOfBytesWritten))
      return ATL::AtlHresultFromLastError();

    return S_OK;
  }

  CWebSocket WebSocketCompleteUpgrade(_In_opt_ DWORD_PTR pContext) noexcept
  {
    //Validate our parameters
#pragma warning(suppress: 26477)
    ATLASSUME(m_h != nullptr);

    return CWebSocket(WinHttpWebSocketCompleteUpgrade(m_h, pContext));
  }
};


//Wrapper for a simple WinHttp async download
class CWINHTTPWRAPPERS_EXT_CLASS CAsyncDownloader : public CRequest
{
public:
//Constructors / Destructors
  CAsyncDownloader() noexcept : m_dwProxyPreauthenticationScheme(WINHTTP_AUTH_SCHEME_NEGOTIATE),
                                m_dwHTTPPreauthenticationScheme(WINHTTP_AUTH_SCHEME_NEGOTIATE),
                                m_bProxyPreauthentication(true),
                                m_bHTTPPreauthentication(true),
                                m_nDownloadStartPos(0),
                                m_bNoURLRedirect(false),
                                m_lpRequest(nullptr),
                                m_dwRequestSize(0),
                                m_dbLimit(0),
                                m_dwReadBufferLength(0),
                                m_dwWriteBufferLength(0),
                                m_nFileToUploadSize(0),
                                m_nFileToUploadIndex(0),
                                m_dwLastStatusCode(0),
                                m_bValidLastStatusCode(false),
                                m_nContentLength(-1),
                                m_pOptionalBuffer(nullptr),
                                m_dwOptionalBufferLength(0),
                                m_dwProxyAuthScheme(0),
                                m_nTotalBytesRead(0),
                                m_dwStartTicksDownload(0),
                                m_bUsingObjectStatusCallback(false)
  {
  }
  CAsyncDownloader(const CAsyncDownloader&) = delete;
  CAsyncDownloader(CAsyncDownloader&&) = delete;

  ~CAsyncDownloader() //NOLINT(modernize-use-override)
  {
    if (m_bUsingObjectStatusCallback)
    {
      SetStatusCallback(nullptr, WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS);
      m_bUsingObjectStatusCallback = false;
    }
  }

//Methods
  CAsyncDownloader& operator=(const CAsyncDownloader&) = delete;
  CAsyncDownloader& operator=(CAsyncDownloader&&) = delete;

#pragma warning(suppress: 26434 26477 26487)
  HRESULT Initialize(_In_ const CConnection& connection, _In_z_ LPCWSTR pwszObjectName, _In_opt_z_ LPCWSTR pwszVerb = nullptr, _In_opt_z_ LPCWSTR pwszVersion = nullptr, _In_opt_z_ LPCWSTR pwszReferrer = WINHTTP_NO_REFERER,
                     _In_opt_ LPCWSTR* ppwszAcceptTypes = WINHTTP_DEFAULT_ACCEPT_TYPES, _In_ DWORD dwFlags = 0, _In_ DWORD dwBufferLength = 8096, _In_ DWORD dwShareMode = 0)
  {
    //Initialize the critical section
    HRESULT hr = m_cs.Init();
    if (FAILED(hr))
      return hr;

    //Let the base class do its thing
    hr = __super::Initialize(connection, pwszObjectName, pwszVerb, pwszVersion, pwszReferrer, ppwszAcceptTypes, dwFlags);
    if (FAILED(hr))
      return hr;

    //Disable redirects if required
    if (m_bNoURLRedirect)
    {
      DWORD dwOptionValue = WINHTTP_DISABLE_REDIRECTS;
      hr = SetOption(WINHTTP_DISABLE_REDIRECTS, &dwOptionValue, sizeof(dwOptionValue));
      if (FAILED(hr))
        return hr;
    }

    //Hook up the callback function
    if (SetStatusCallback() == WINHTTP_INVALID_STATUS_CALLBACK)
      return ATL::AtlHresultFromLastError();

    //Release our resources if currently in use
    ReleaseResources();

    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    m_bUsingObjectStatusCallback = true;

    //Allocate the receive buffer
    if (!m_ReadBuffer.Allocate(dwBufferLength))
      return E_OUTOFMEMORY;
    m_dwReadBufferLength = dwBufferLength;

    //Open up the file for downloading if necessary
    if (m_sFileToDownloadInto.length())
    {
    #ifdef _UNICODE
      hr = m_fileToDownloadInto.Create(m_sFileToDownloadInto.c_str(), GENERIC_WRITE, dwShareMode, OPEN_ALWAYS);
    #else
      hr = m_fileToDownloadInto.Create(ATL::CW2T(m_sFileToDownloadInto.c_str()), GENERIC_WRITE, dwShareMode, OPEN_ALWAYS);
    #endif //#ifdef _UNICODE
      if (FAILED(hr))
        return hr;

      //Seek to the start position of the download
      hr = m_fileToDownloadInto.Seek(m_nDownloadStartPos, FILE_BEGIN);
      if (FAILED(hr))
        return hr;
      hr = m_fileToDownloadInto.SetSize(m_nDownloadStartPos);
      if (FAILED(hr))
        return hr;
    }

    //Also open up the file to upload if necessary
    if (m_sFileToUpload.length())
    {
      //Allocate the send buffer
      if (!m_WriteBuffer.Allocate(dwBufferLength))
        return E_OUTOFMEMORY;
      m_dwWriteBufferLength = dwBufferLength;

      //Open up the file for downloading into
    #ifdef _UNICODE
      hr = m_fileToUpload.Create(m_sFileToUpload.c_str(), GENERIC_READ, dwShareMode, OPEN_EXISTING);
    #else
      hr = m_fileToUpload.Create(ATL::CW2T(m_sFileToUpload.c_str()), GENERIC_READ, dwShareMode, OPEN_EXISTING);
    #endif //#ifdef _UNICODE
      if (FAILED(hr))
        return hr;

      //Remember the size of the file to upload
      hr = m_fileToUpload.GetSize(m_nFileToUploadSize);
      if (FAILED(hr))
        return hr;
    }

    return S_OK;
  }

  void ReleaseResources()
  {
    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //Ensure the files are closed and the buffers are deleted
    m_fileToDownloadInto.Close();
    if (m_ReadBuffer.m_pData != nullptr)
      m_ReadBuffer.Free();
    m_dwReadBufferLength = 0;
    m_fileToUpload.Close();
    if (m_WriteBuffer.m_pData != nullptr)
      m_WriteBuffer.Free();
    m_dwWriteBufferLength = 0;
    m_nFileToUploadSize = 0;
    m_nFileToUploadIndex = 0;
    m_pOptionalBuffer = nullptr;
    m_dwOptionalBufferLength = 0;
  }

#pragma warning(suppress: 26165)
  HRESULT DeleteDownloadedFile()
  {
    //What will be the return value from this method (assume the best)
    HRESULT hr = S_OK;

    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //Delete the partially downloaded file if unsuccessful
  #ifdef _UNICODE
    if (m_sFileToDownloadInto.length() && !DeleteFile(m_sFileToDownloadInto.c_str()))
  #else
    if (m_sFileToDownloadInto.length() && !DeleteFile(ATL::CW2T(m_sFileToDownloadInto.c_str())))
  #endif //#ifdef _UNICODE
      hr = ATL::AtlHresultFromLastError();

    return hr;
  }

  _NODISCARD ULONGLONG TimeSinceStartDownload() const noexcept
  {
    return GetTickCount64() - m_dwStartTicksDownload;
  }

  virtual String GetHeaders()
  {
    //What will be the return value from this method
    String sHeaders;

    //Create the Range header here if required
    if (m_nDownloadStartPos != 0)  //we will build the range request
    {
      ATL::CAtlStringW sHeader;
      sHeader.Format(L"Range: bytes=%I64u-\r\n", m_nDownloadStartPos);
      sHeaders += sHeader;
    }

    //Update the content length if we have a file to upload
    if (m_fileToUpload.operator HANDLE())
    {
      ATL::CAtlStringW sHeader;
      sHeader.Format(L"Content-Length: %I64u\r\n", m_nFileToUploadSize);
      sHeaders += sHeader;
    }

    return sHeaders;
  }

  virtual DWORD GetContentLength() noexcept
  {
    if (m_fileToUpload.operator HANDLE())
    {
      if (m_nFileToUploadSize > UINT_MAX)
        return WINHTTP_IGNORE_REQUEST_TOTAL_LENGTH;
      else
      {
#pragma warning(suppress: 26472)
        return static_cast<DWORD>(m_nFileToUploadSize);
      }
    }
    else
      return m_dwRequestSize;
  }

  DWORD GetLastStatusCode(_Out_ bool& bValid) noexcept
  {
    bValid = m_bValidLastStatusCode;
    return m_dwLastStatusCode;
  }

#pragma warning(suppress: 26434 26477)
  HRESULT SendRequest(_In_reads_bytes_opt_(dwOptionalLength) LPVOID lpOptional = WINHTTP_NO_REQUEST_DATA, _In_ DWORD dwOptionalLength = 0)
  {
    //Remember the parameters passed in case we need to resend the request
    m_dwOptionalBufferLength = dwOptionalLength;
    m_pOptionalBuffer = lpOptional;

    //Reset the last proxy auth scheme used before we send the request
    m_dwProxyAuthScheme = 0;

    //Do preauthentication if required
    if (m_bProxyPreauthentication)
    {
      const HRESULT hr = DoAuthentication(m_dwProxyPreauthenticationScheme, 0, WINHTTP_AUTH_TARGET_PROXY);
      if (FAILED(hr))
        return hr;
    }
    if (m_bHTTPPreauthentication)
    {
      const HRESULT hr = DoAuthentication(m_dwHTTPPreauthenticationScheme, 0, WINHTTP_AUTH_TARGET_SERVER);
      if (FAILED(hr))
        return hr;
    }

    //Form the headers we are sending
    String sHeaders(GetHeaders());
#pragma warning(suppress: 26472)
    const auto dwHeadersLength = static_cast<DWORD>(sHeaders.length());

    //Reset the total bytes read in the response
    m_nTotalBytesRead = 0;

    //Remember the time we started the download at
    m_dwStartTicksDownload = GetTickCount64();

    //Reset the last status code
    m_bValidLastStatusCode = false;
    m_dwLastStatusCode = 0;

    //Call the base class using the this pointer as the context value
#pragma warning(suppress: 26477 26490)
    return __super::SendRequest(dwHeadersLength ? sHeaders.c_str() : WINHTTP_NO_ADDITIONAL_HEADERS, dwHeadersLength, lpOptional, dwOptionalLength, dwOptionalLength + GetContentLength(), reinterpret_cast<DWORD_PTR>(this));
  }

  virtual HRESULT DoAuthentication(_In_ DWORD dwAuthenticationScheme, _In_ DWORD /*dwFirstScheme*/, _In_ DWORD dwAuthTarget) noexcept
  {
    //What will be the return value from this method
    HRESULT hr = S_FALSE;

    switch (dwAuthTarget)
    {
      case WINHTTP_AUTH_TARGET_SERVER:
      {
        if (m_sHTTPUserName.length())
          hr = SetCredentials(dwAuthTarget, dwAuthenticationScheme, m_sHTTPUserName.c_str(), m_sHTTPPassword.c_str());
        break;
      }
      case WINHTTP_AUTH_TARGET_PROXY:
      {
        if (m_sProxyUserName.length())
          hr = SetCredentials(dwAuthTarget, dwAuthenticationScheme, m_sProxyUserName.c_str(), m_sProxyPassword.c_str());
        break;
      }
      default:
      {
        hr = E_UNEXPECTED;
        break;
      }
    }

    return hr;
  }

  virtual DWORD ChooseAuthScheme(_In_ DWORD dwSupportedSchemes, _In_ DWORD /*dwFirstScheme*/, _In_ DWORD /*dwAuthTarget*/) noexcept
  {
    //This default implementation will allow any authentication scheme support
    //and will pick in order of "decreasing strength"
    if (dwSupportedSchemes & WINHTTP_AUTH_SCHEME_NEGOTIATE)
      return WINHTTP_AUTH_SCHEME_NEGOTIATE;
    else if (dwSupportedSchemes & WINHTTP_AUTH_SCHEME_NTLM)
      return WINHTTP_AUTH_SCHEME_NTLM;
    else if (dwSupportedSchemes & WINHTTP_AUTH_SCHEME_PASSPORT)
      return WINHTTP_AUTH_SCHEME_PASSPORT;
    else if (dwSupportedSchemes & WINHTTP_AUTH_SCHEME_DIGEST)
      return WINHTTP_AUTH_SCHEME_DIGEST;
    else if (dwSupportedSchemes & WINHTTP_AUTH_SCHEME_BASIC)
      return WINHTTP_AUTH_SCHEME_BASIC;
    else
      return 0;
  }

//Member variables
  String m_sHTTPUserName;                 //The username to use for HTTP authentication
  String m_sHTTPPassword;                 //the password to use for HTTP authentication
  String m_sProxyUserName;                //The username to use for Proxy authentication
  String m_sProxyPassword;                //The password to use for Proxy authentication
  DWORD m_dwProxyPreauthenticationScheme; //The authentication scheme to use for proxy preauthentication
  DWORD m_dwHTTPPreauthenticationScheme;  //The authentication scheme to use for HTTP server preauthentication
  bool m_bProxyPreauthentication;         //Should we supply credentials on the first request for the Proxy rather than starting out with anonymous credentials
                                          //and only authenticating when challenged
  bool m_bHTTPPreauthentication;          //Should we supply credentials on the first request for the HTTP Server rather than starting out with anonymous credentials
                                          //and only authenticating when challenged
  ULONGLONG m_nDownloadStartPos;          //Offset to resume the download at
  bool m_bNoURLRedirect;                  //Set to true if you want to disable URL redirection following
  String m_sFileToUpload;                 //The path of the file to upload
  String m_sFileToDownloadInto;           //The path of the file to download into
  ByteArray m_Response;                   //The in memory copy of the HTTP response
  LPCVOID m_lpRequest;                    //The in memory data to send in the HTTP request
  DWORD m_dwRequestSize;                  //The size in bytes of m_lpRequest
  double m_dbLimit;                       //For bandwidth throtling, The value in KB/Second to limit the connection to

protected:
//Methods
  HRESULT OnCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength) override
  {
    //What will be the return value from this function (S_FALSE means not handled in our callback)
    HRESULT hr = S_FALSE;

    switch (dwInternetStatus)
    {
      case WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE:
      case WINHTTP_CALLBACK_STATUS_SENDREQUEST_COMPLETE:
      {
        hr = OnWriteCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE:
      {
        hr = OnHeadersAvailableCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_READ_COMPLETE:
      {
        hr = OnReadCompleteCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
        break;
      }
      case WINHTTP_CALLBACK_STATUS_REQUEST_ERROR:
      {
        hr = OnRequestErrorCallback(hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
        break;
      }
      default:
      {
        break;
      }
    }

    return hr;
  }

  virtual HRESULT OnReadCompleteCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    //What will be the return value from the function
    HRESULT hr = E_UNEXPECTED;

    if (dwStatusInformationLength > 0)
    {
      //Serialize access to our member variables
      ATL::CCritSecLock sl(m_cs.m_sec, true);
   
      //Call the virtual OnReadData method if we have received some response data
      hr = OnReadData(m_ReadBuffer.m_pData, dwStatusInformationLength);
      if (FAILED(hr))
        return hr;

      //Continue to read the HTTP response
      hr = ReadData(m_ReadBuffer.m_pData, m_dwReadBufferLength, nullptr);
      if (FAILED(hr))
        return hr;
    }
    else
    {
      //A dwStatusInformationLength of 0 indicates that the response is complete, call
      //the OnResponseComplete method to indicate that the download is complete
      hr = OnCallbackComplete(S_OK, hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
    }

    return hr;
  }

  virtual HRESULT OnWriteCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    //Call the virtual OnWriteData method to allow this class a chance to send additional request data
    HRESULT hr = OnWriteData();
    if (FAILED(hr))
      return hr;

    //When the request was sent successfully, lets kick off reading the response
    if (hr == S_FALSE)
      hr = ReceiveResponse();
    
    return hr;
  }

  virtual HRESULT On407Response()
  {
    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //Fail the request if we got the same status code a second time
    if (m_dwLastStatusCode == HTTP_STATUS_PROXY_AUTH_REQ)
      return ATL::AtlHresultFromWin32(ERROR_WINHTTP_LOGIN_FAILURE);

    //Check what authentication schemes the server supports 
    DWORD dwSupportedSchemes = 0;
    DWORD dwFirstScheme = 0;
    DWORD dwAuthTarget = 0;
    HRESULT hr = QueryAuthSchemes(dwSupportedSchemes, dwFirstScheme, dwAuthTarget);
    if (FAILED(hr))
      return hr;

    //Pick an authentication scheme
    m_dwProxyAuthScheme = ChooseAuthScheme(dwSupportedSchemes, dwFirstScheme, dwAuthTarget);
    if (m_dwProxyAuthScheme == 0)
      return ATL::AtlHresultFromWin32(ERROR_WINHTTP_LOGIN_FAILURE);

    //Do the authentication
    hr = DoAuthentication(m_dwProxyAuthScheme, dwFirstScheme, dwAuthTarget);
    if (FAILED(hr))
      return hr;

    //Remember the last status code
    m_bValidLastStatusCode = true;
    m_dwLastStatusCode = HTTP_STATUS_PROXY_AUTH_REQ;

    //Form the headers we are sending
    String sHeaders(GetHeaders());
#pragma warning(suppress: 26472)
    const auto dwHeadersLength = static_cast<DWORD>(sHeaders.length());

    //Call the base class using the this pointer as the context value
#pragma warning(suppress: 26477 26490)
    return __super::SendRequest(dwHeadersLength ? sHeaders.c_str() : WINHTTP_NO_ADDITIONAL_HEADERS, dwHeadersLength, m_pOptionalBuffer, m_dwOptionalBufferLength, m_dwOptionalBufferLength + GetContentLength(), reinterpret_cast<DWORD_PTR>(this));
  }

  virtual HRESULT On401Response()
  {
    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //Fail the request if we got the same status code a second time
    if (m_dwLastStatusCode == HTTP_STATUS_DENIED)
      return ATL::AtlHresultFromWin32(ERROR_WINHTTP_LOGIN_FAILURE);

    //Check what authentication schemes the server supports 
    DWORD dwSupportedSchemes = 0;
    DWORD dwFirstScheme = 0;
    DWORD dwAuthTarget = 0;
    HRESULT hr = QueryAuthSchemes(dwSupportedSchemes, dwFirstScheme, dwAuthTarget);
    if (FAILED(hr))
      return hr;

    //Pick an authentication scheme
    const DWORD dwAuthenticationScheme = ChooseAuthScheme(dwSupportedSchemes, dwFirstScheme, dwAuthTarget);
    if (dwAuthenticationScheme == 0)
      return ATL::AtlHresultFromWin32(ERROR_WINHTTP_LOGIN_FAILURE);

    //Do the authentication
    hr = DoAuthentication(dwAuthenticationScheme, dwFirstScheme, dwAuthTarget);
    if (FAILED(hr))
      return hr;

    //Resend the Proxy authentication details also if used before, otherwise we could end up in a 407-401-407-401 loop
    if (m_dwProxyAuthScheme != 0)
    {
      //Do the authentication
      hr = DoAuthentication(m_dwProxyAuthScheme, 0, WINHTTP_AUTH_TARGET_PROXY);
      if (FAILED(hr))
        return hr;
    }

    //Remember the last status code
    m_bValidLastStatusCode = true;
    m_dwLastStatusCode = HTTP_STATUS_DENIED;

    //Form the headers we are sending
    String sHeaders(GetHeaders());
#pragma warning(suppress: 26472)
    const auto dwHeadersLength = static_cast<DWORD>(sHeaders.length());

    //Call the base class using the this pointer as the context value
#pragma warning(suppress: 26477 26490)
    return __super::SendRequest(dwHeadersLength ? sHeaders.c_str() : WINHTTP_NO_ADDITIONAL_HEADERS, dwHeadersLength, m_pOptionalBuffer, m_dwOptionalBufferLength, m_dwOptionalBufferLength + GetContentLength(), reinterpret_cast<DWORD_PTR>(this));
  }

  HRESULT QueryStatusCode(_Out_ DWORD& dwStatusCode) noexcept
  {
    dwStatusCode = 0;
    DWORD dwStatusCodeSize = sizeof(dwStatusCode);
#pragma warning(suppress: 26477)
    return QueryHeaders(WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX, &dwStatusCode, dwStatusCodeSize, WINHTTP_NO_HEADER_INDEX);
  }

  HRESULT QueryContentLength(_Out_ LONGLONG& nContentLength)
  {
    nContentLength = -1;

    //First call to get the size of the buffer we need
    ATL::CAtlStringW sContentLength;
    DWORD dwBufferSize = 32;
#pragma warning(suppress: 26477)
    const HRESULT hr = QueryHeaders(WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_HEADER_NAME_BY_INDEX, sContentLength.GetBufferSetLength(dwBufferSize), dwBufferSize, WINHTTP_NO_HEADER_INDEX);
    sContentLength.ReleaseBuffer();
    if (FAILED(hr))
      return hr;

    //Update the output parameter
    nContentLength = _wtoi64(sContentLength);

    return hr;
  }

  virtual HRESULT OnHeadersAvailableCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    //Get the HTTP status code
    DWORD dwStatusCode = 0;
    HRESULT hr = QueryStatusCode(dwStatusCode);
    if (FAILED(hr))
      return hr;

    //Check what status code we have got
    if (dwStatusCode == HTTP_STATUS_PROXY_AUTH_REQ)
      return On407Response();
    else if (dwStatusCode == HTTP_STATUS_DENIED)
      return On401Response(); 
    else if ((dwStatusCode / 100) != 2) //Any 2XX is Success
    {
      m_bValidLastStatusCode = true;
      m_dwLastStatusCode = dwStatusCode;
      return ATL::AtlHresultFromWin32(ERROR_WINHTTP_INVALID_HEADER);
    }
    else
    {
      m_bValidLastStatusCode = true;
      m_dwLastStatusCode = dwStatusCode;
    }

    //Cache the content length header also if we can
    QueryContentLength(m_nContentLength);

    //Lets begin reading the response
    hr = ReadData(m_ReadBuffer.m_pData, m_dwReadBufferLength, nullptr);
    if (FAILED(hr))
      return hr;

    return hr;
  }

  virtual HRESULT OnRequestErrorCallback(_In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    //Pull out the WINHTTP_ASYNC_RESULT
    const auto pResult = static_cast<const WINHTTP_ASYNC_RESULT*>(lpvStatusInformation);

    if (pResult == nullptr)
    {
#pragma warning(suppress: 26477)
      ATLASSERT(FALSE);
      return E_UNEXPECTED;
    }

    //Resend the request if required
    if (pResult->dwError == ERROR_WINHTTP_RESEND_REQUEST)
    {
      //Form the headers we are sending
      String sHeaders(GetHeaders());
#pragma warning(suppress: 26472)
      const auto dwHeadersLength = static_cast<DWORD>(sHeaders.length());

      //Serialize access to our member variables
      ATL::CCritSecLock sl(m_cs.m_sec, true);

      //Call the base class using the this pointer as the context value
#pragma warning(suppress: 26477 26490)
      return __super::SendRequest(dwHeadersLength ? sHeaders.c_str() : WINHTTP_NO_ADDITIONAL_HEADERS, dwHeadersLength, m_pOptionalBuffer, m_dwOptionalBufferLength, m_dwOptionalBufferLength + GetContentLength(), reinterpret_cast<DWORD_PTR>(this));
    }

    //Call the OnCallbackComplete method with the async HRESULT
    return OnCallbackComplete(ATL::AtlHresultFromWin32(pResult->dwError), hInternet, dwInternetStatus, lpvStatusInformation, dwStatusInformationLength);
  }

#pragma warning(suppress: 26165)
  virtual HRESULT OnReadData(_In_reads_bytes_(dwBytesRead) const void* lpvBuffer, _In_ DWORD dwBytesRead)
  {
    //What will be the return value from this method (assume the best)
    HRESULT hr = S_OK;

    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //Increment the total number of bytes read
    m_nTotalBytesRead += dwBytesRead;

    //Write out the buffer to the download file if it is open
    if (m_fileToDownloadInto.operator HANDLE())
      hr = m_fileToDownloadInto.Write(lpvBuffer, dwBytesRead);
    else
    {
      //Otherwise build up the response in the in-memory response array

      //Preallocate the response buffer if nothing has been read yet
      const std::vector<BYTE>::size_type nResponseSize = m_Response.size();
      if (nResponseSize == 0)
      {
        if (m_nContentLength != -1)
#pragma warning(suppress: 26472)
          m_Response.reserve(static_cast<std::vector<BYTE>::size_type>(m_nContentLength));
        else
          m_Response.reserve(m_dwReadBufferLength);
      }

      //Add the data read to the response array
#pragma warning(suppress: 26444 26481)
      m_Response.insert(m_Response.end(), static_cast<const BYTE*>(lpvBuffer), static_cast<const BYTE*>(lpvBuffer) + dwBytesRead);
    }

    //Call the method to handle bandwidth throttling
    DoBandwidthThrottling();

    return hr;
  }

  virtual void DoBandwidthThrottling() noexcept
  {
    //Do the bandwidth throttling
    if (m_dbLimit > 0)
    {
      const auto fTimeSinceStartDownload = static_cast<double>(TimeSinceStartDownload());
      if (fTimeSinceStartDownload)
      {
        const double q = static_cast<double>(m_nTotalBytesRead) / fTimeSinceStartDownload;
        if (q > m_dbLimit)
#pragma warning(suppress: 26467)
          Sleep(static_cast<DWORD>(((q*fTimeSinceStartDownload)/m_dbLimit) - fTimeSinceStartDownload));
      }
    }
  }

  virtual HRESULT OnWriteData()
  {
    //Serialize access to our member variables
    ATL::CCritSecLock sl(m_cs.m_sec, true);

    //If we have a file to upload?
    if (m_fileToUpload.operator HANDLE())
    {
      //Read in the next blob of data to write from the upload file
      DWORD dwBytesRead = 0;
      HRESULT hr = m_fileToUpload.Read(m_WriteBuffer.m_pData, m_dwWriteBufferLength, dwBytesRead);
      if (FAILED(hr))
        return hr;

      //Write the data to the server
      hr = WriteData(m_WriteBuffer.m_pData, dwBytesRead, nullptr);
      if (FAILED(hr))
        return hr; 

      //Update the current position
      m_nFileToUploadIndex += dwBytesRead;

      //Return S_FALSE to conclude the writing if we have reached the end of the file
      return (m_nFileToUploadIndex >= m_nFileToUploadSize) ? S_FALSE : S_OK;
    }
    else
    {
      //Upload the in-memory data if specified
      if (m_dwRequestSize)
      {
#pragma warning(suppress: 26477)
        ATLASSERT(m_lpRequest != nullptr); //m_pbyRequest should be provided if m_dwRequestSize is non-zero
        const HRESULT hr = WriteData(m_lpRequest, m_dwRequestSize, nullptr);
        if (FAILED(hr))
          return hr;
      }

      //There's nothing more to upload so return S_FALSE
      return S_FALSE;
    }
  }

#ifdef _DEBUG
  static void TraceCallbackComplete(_In_ HRESULT hr, _In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength)
  {
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    if (hr == S_OK)
      ATLTRACE(_T("CAsyncWinHttpDownloader::TraceCallbackComplete, The file was successfully downloaded\n"));
    else
      ATLTRACE(_T("CAsyncWinHttpDownloader::TraceCallbackComplete, The file was not downloaded correctly, Error:%08X\n"), hr);
  }
#endif //#ifdef _DEBUG

#pragma warning(suppress: 26433)
  HRESULT OnCallbackComplete(_In_ HRESULT hr, _In_ HINTERNET hInternet, _In_ DWORD dwInternetStatus, _In_opt_ LPVOID lpvStatusInformation, _In_ DWORD dwStatusInformationLength) override
  {
    UNREFERENCED_PARAMETER(hInternet);
    UNREFERENCED_PARAMETER(dwInternetStatus);
    UNREFERENCED_PARAMETER(lpvStatusInformation);
    UNREFERENCED_PARAMETER(dwStatusInformationLength);

    //Delegate the cleanup to the helper method
    ReleaseResources();

    //Delete the file if it was partially downloaded
    if (hr != S_OK)
      DeleteDownloadedFile();

    return S_OK;
  }

//Member variables
  ATL::CHeapPtr<BYTE> m_ReadBuffer;   //The buffer we will read into
  DWORD m_dwReadBufferLength;         //The size in bytes of "m_ReadBuffer"
  ATL::CAtlFile m_fileToDownloadInto; //The file we will download into
  ATL::CHeapPtr<BYTE> m_WriteBuffer;  //The buffer we will write from
  DWORD m_dwWriteBufferLength;        //The size in bytes of "m_WriteBuffer"
  ATL::CAtlFile m_fileToUpload;       //The file we will upload
  ULONGLONG m_nFileToUploadSize;      //The size of the file to upload
  ULONGLONG m_nFileToUploadIndex;     //The current index of the file upload writing
  ATL::CComCriticalSection m_cs;      //Used to serialize access to our member variables
  DWORD m_dwLastStatusCode;           //Used to avoid multiple status codes of the same value being returned
  bool m_bValidLastStatusCode;        //Is "m_dwLastStatusCode" actually valid. Will only be true after QueryStatusCode is called during the download process
  LONGLONG m_nContentLength;          //The content length header value (-1 implies not provided)
  LPVOID m_pOptionalBuffer;           //A pointer to the optional data (Note that we do not need to make a copy of it due to the usage requirements in WinHttpSendRequest
  DWORD m_dwOptionalBufferLength;     //The size in bytes of "m_pOptionalBuffer"
  DWORD m_dwProxyAuthScheme;          //The auth scheme used for the last proxy authentication
  ULONGLONG m_nTotalBytesRead;        //The total bytes read in the current response
  ULONGLONG m_dwStartTicksDownload;   //The Tick count when we started the download
  bool m_bUsingObjectStatusCallback;  //true if SetStatusCallback has been called to setup CHandle::_Callback as the WinHTTP callback
};


//Wrapper for a simple WinHttp sync download
class CWINHTTPWRAPPERS_EXT_CLASS CSyncDownloader : public CAsyncDownloader
{
public:
//Methods
#pragma warning(suppress: 26477)
  virtual HRESULT SendRequestSync(_In_reads_bytes_opt_(dwOptionalLength) LPVOID lpOptional = WINHTTP_NO_REQUEST_DATA, _In_ DWORD dwOptionalLength = 0)
  {
    //Use the base class to send the request initially
    HRESULT hr = __super::SendRequest(lpOptional, dwOptionalLength);
    if (FAILED(hr))
    {
      ReleaseResources();
      DeleteDownloadedFile();
      return hr;
    }

    bool bDownloaded = false;
    while (!bDownloaded)
    {
      //Loop around calling the OnWriteData virtual method until it returns S_FALSE
      do
      {
        hr = OnWriteData();
      }
      while (SUCCEEDED(hr) && (hr != S_FALSE));
      if (FAILED(hr))
      {
        ReleaseResources();
        DeleteDownloadedFile();
        return hr;
      }

      //Wait for the status code and response headers to be received
      hr = ReceiveResponse();
      if (FAILED(hr))
      {
        ReleaseResources();
        DeleteDownloadedFile();
        return hr;
      }

      //Get the HTTP status code
      DWORD dwStatusCode = 0;
      hr = QueryStatusCode(dwStatusCode);
      if (FAILED(hr))
      {
        ReleaseResources();
        DeleteDownloadedFile();
        return hr;
      }

      //Check what status code we have got
      if (dwStatusCode == HTTP_STATUS_PROXY_AUTH_REQ)
      {
        hr = On407Response();
        if (FAILED(hr))
        {
          ReleaseResources();
          DeleteDownloadedFile();
          return hr;
        }
        continue;
      }
      else if (dwStatusCode == HTTP_STATUS_DENIED)
      {
        hr = On401Response();
        if (FAILED(hr))
        {
          ReleaseResources();
          DeleteDownloadedFile();
          return hr;
        }
        continue;
      }
      else if ((dwStatusCode != HTTP_STATUS_OK) && (dwStatusCode != HTTP_STATUS_PARTIAL_CONTENT))
      {
        m_bValidLastStatusCode = true;
        m_dwLastStatusCode = dwStatusCode;
        ReleaseResources();
        DeleteDownloadedFile();
        return ATL::AtlHresultFromWin32(ERROR_WINHTTP_INVALID_HEADER);
      }
      else
      {
        m_bValidLastStatusCode = true;
        m_dwLastStatusCode = dwStatusCode;
      }

      //Cache the content length header also if we can
      QueryContentLength(m_nContentLength);

      //Read the response
      DWORD dwBytesRead = 0;
      do
      {
        hr = ReadData(m_ReadBuffer.m_pData, m_dwReadBufferLength, &dwBytesRead);
        if (FAILED(hr))
        {
          ReleaseResources();
          DeleteDownloadedFile();
          return hr;
        }

        if (dwBytesRead)
        {
          //Call the virtual OnReadData method
          hr = OnReadData(m_ReadBuffer.m_pData, dwBytesRead);

          //Handle OnReadData failing
          if (FAILED(hr))
          {
            ReleaseResources();
            DeleteDownloadedFile();
            return hr;
          }
        }
      }
      while (dwBytesRead);

      bDownloaded = true;
    }

    ReleaseResources();
    return S_OK;
  }
};


}; //namespace WinHttpWrappers

#endif //#ifndef __WINHTTPWRAPPERS_H__
