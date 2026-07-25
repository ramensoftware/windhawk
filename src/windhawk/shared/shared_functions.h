#pragma once

namespace Functions {

std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim);
std::vector<std::wstring_view> SplitStringToViews(std::wstring_view s,
                                                  WCHAR delim);
std::wstring ReplaceAll(std::wstring_view source,
                        std::wstring_view from,
                        std::wstring_view to,
                        bool ignoreCase = false);
void GetNtVersionNumbers(ULONG* pNtMajorVersion,
                         ULONG* pNtMinorVersion,
                         ULONG* pNtBuildNumber);
bool IsWindowsVersionOrGreaterWithBuildNumber(WORD wMajorVersion,
                                              WORD wMinorVersion,
                                              WORD wBuildNumber);
HRESULT SetThreadDescriptionIfAvailable(HANDLE hThread,
                                        PCWSTR lpThreadDescription);

}  // namespace Functions
