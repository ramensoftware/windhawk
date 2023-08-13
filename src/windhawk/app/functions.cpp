#include "stdafx.h"

#include "functions.h"

namespace Functions {

// SetPrivilege enables/disables process token privilege.
// https://docs.microsoft.com/en-us/windows-hardware/drivers/debugger/debug-privilege
BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege) {
    LUID luid;
    BOOL bRet = FALSE;

    if (LookupPrivilegeValue(nullptr, lpszPrivilege, &luid)) {
        TOKEN_PRIVILEGES tp;

        tp.PrivilegeCount = 1;
        tp.Privileges[0].Luid = luid;
        tp.Privileges[0].Attributes =
            (bEnablePrivilege) ? SE_PRIVILEGE_ENABLED : 0;

        // Enable the privilege or disable all privileges.
        if (AdjustTokenPrivileges(hToken, FALSE, &tp, 0, nullptr, nullptr)) {
            // Check to see if you have proper access.
            // You may get "ERROR_NOT_ALL_ASSIGNED".
            bRet = (GetLastError() == ERROR_SUCCESS);
        }
    }

    return bRet;
}

BOOL SetDebugPrivilege(BOOL bEnablePrivilege) {
    wil::unique_handle token;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES,
                          &token)) {
        return FALSE;
    }

    return SetPrivilege(token.get(), SE_DEBUG_NAME, bEnablePrivilege);
}

HANDLE CreateEventForMediumIntegrity(PCWSTR eventName, BOOL manualReset) {
    // Allow only EVENT_MODIFY_STATE (0x0002), only for medium integrity.
    PCWSTR pszStringSecurityDescriptor = L"D:(A;;0x0002;;;WD)S:(ML;;NW;;;ME)";

    wil::unique_hlocal secDesc;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptor(
            pszStringSecurityDescriptor, SDDL_REVISION_1, &secDesc, nullptr)) {
        return nullptr;
    }

    SECURITY_ATTRIBUTES secAttr = {sizeof(SECURITY_ATTRIBUTES)};
    secAttr.lpSecurityDescriptor = secDesc.get();
    secAttr.bInheritHandle = FALSE;

    return CreateEvent(&secAttr, manualReset, FALSE, eventName);
}

//
// FUNCTION: IsRunAsAdmin()
//
// PURPOSE: The function checks whether the current process is run as
// administrator. In other words, it dictates whether the primary access
// token of the process belongs to user account that is a member of the
// local Administrators group and it is elevated.
//
// RETURN VALUE: Returns TRUE if the primary access token of the process
// belongs to user account that is a member of the local Administrators
// group and it is elevated. Returns FALSE if the token does not. Returns
// FALSE on failure. To get extended error information, call GetLastError.
//
BOOL IsRunAsAdmin() {
    BOOL fIsRunAsAdmin = FALSE;
    DWORD dwError = ERROR_SUCCESS;
    PSID pAdministratorsGroup = nullptr;
    SID_IDENTIFIER_AUTHORITY NtAuthority = SECURITY_NT_AUTHORITY;

    // Allocate and initialize a SID of the administrators group.
    if (AllocateAndInitializeSid(&NtAuthority, 2, SECURITY_BUILTIN_DOMAIN_RID,
                                 DOMAIN_ALIAS_RID_ADMINS, 0, 0, 0, 0, 0, 0,
                                 &pAdministratorsGroup)) {
        // Determine whether the SID of administrators group is enabled in
        // the primary access token of the process.
        if (!CheckTokenMembership(nullptr, pAdministratorsGroup,
                                  &fIsRunAsAdmin)) {
            dwError = GetLastError();
        }

        FreeSid(pAdministratorsGroup);

        if (dwError != ERROR_SUCCESS) {
            SetLastError(dwError);
        }
    }

    return fIsRunAsAdmin;
}

PCWSTR LoadStrFromRsrc(UINT uStrId) {
    PCWSTR pStr;
    if (!LoadString(nullptr, uStrId, (WCHAR*)&pStr, 0)) {
        pStr = L"(Could not load resource)";
    }

    return pStr;
}

std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim) {
    // https://stackoverflow.com/a/48403210
    auto view =
        s | std::ranges::views::split(delim) |
        std::ranges::views::transform([](auto&& rng) {
            return std::wstring_view(&*rng.begin(), std::ranges::distance(rng));
        });
    return std::vector<std::wstring>(view.begin(), view.end());
}

}  // namespace Functions
