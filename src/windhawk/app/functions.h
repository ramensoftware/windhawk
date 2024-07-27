#pragma once

namespace Functions {

BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege);
BOOL SetDebugPrivilege(BOOL bEnablePrivilege);
HANDLE CreateEventForMediumIntegrity(PCWSTR eventName,
                                     BOOL manualReset = FALSE);
BOOL IsRunAsAdmin();
PCWSTR LoadStrFromRsrc(UINT uStrId);
std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim);
std::vector<std::wstring_view> SplitStringToViews(std::wstring_view s,
                                                  WCHAR delim);
UINT GetDpiForWindowWithFallback(HWND hWnd);
int GetSystemMetricsForDpiWithFallback(int nIndex, UINT dpi);
int GetSystemMetricsForWindow(HWND hWnd, int nIndex);

// Returns true for suspended UWP processes.
// https://stackoverflow.com/a/50173965
bool IsProcessFrozen(HANDLE hProcess);

void GetNtVersionNumbers(ULONG* pNtMajorVersion,
                         ULONG* pNtMinorVersion,
                         ULONG* pNtBuildNumber);
bool IsWindowsVersionOrGreaterWithBuildNumber(WORD wMajorVersion,
                                              WORD wMinorVersion,
                                              WORD wBuildNumber);
NTSTATUS CreateExecutionRequiredRequest(_In_ HANDLE ProcessHandle,
                                        _Out_ PHANDLE PowerRequestHandle);

}  // namespace Functions
