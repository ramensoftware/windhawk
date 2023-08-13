#pragma once

namespace Functions {

BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege);
BOOL SetDebugPrivilege(BOOL bEnablePrivilege);
HANDLE CreateEventForMediumIntegrity(PCWSTR eventName,
                                     BOOL manualReset = FALSE);
BOOL IsRunAsAdmin();
PCWSTR LoadStrFromRsrc(UINT uStrId);
std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim);

}  // namespace Functions
