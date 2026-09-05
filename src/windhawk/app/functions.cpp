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
    // IU cannot be narrowed to BA: callers run unelevated, and an unelevated
    // administrator's filtered token holds the Administrators SID as deny-only,
    // so a BA ACE would grant no one. ME blocks sandboxed, low-integrity
    // callers.
    PCWSTR pszStringSecurityDescriptor =
        L"D:(A;;0x0002;;;IU)(A;;0x0002;;;SY)S:(ML;;NW;;;ME)";

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

// FALSE for an admin whose token is UAC-filtered: this asks whether the
// process runs as an administrator, not whether the user is one.
BOOL IsRunAsAdmin() {
    bool isRunAsAdmin;
    if (FAILED(wil::test_token_membership_nothrow(
            &isRunAsAdmin, nullptr, SECURITY_NT_AUTHORITY,
            SECURITY_BUILTIN_DOMAIN_RID, DOMAIN_ALIAS_RID_ADMINS))) {
        return FALSE;
    }

    return isRunAsAdmin;
}

bool WriteFileContentAtomically(const std::filesystem::path& path,
                                std::string_view content) {
    // Unique temp name per writer, so no two concurrent writers ever touch the
    // same temp file - not two threads here, not another Windhawk process, and
    // not the core, all of which derive the temp name the same way. The process
    // and thread ids suffice: a thread runs only one write at a time, so the
    // pair is unique across every writer live at once. The single shared step
    // is the atomic MoveFileEx below, which resolves to a clean
    // last-writer-wins without torn files.
    std::filesystem::path tempPath = path;
    tempPath += L"." + std::to_wstring(GetCurrentProcessId()) + L"." +
                std::to_wstring(GetCurrentThreadId()) + L".tmp";

    {
        // Open exclusively (no sharing) so nothing can touch our private temp
        // file between creation and the rename.
        wil::unique_hfile tempFile(CreateFile(tempPath.c_str(), GENERIC_WRITE,
                                              0, nullptr, CREATE_ALWAYS,
                                              FILE_ATTRIBUTE_NORMAL, nullptr));
        if (!tempFile) {
            return false;
        }

        DWORD bytesWritten;
        bool succeeded = WriteFile(tempFile.get(), content.data(),
                                   wil::safe_cast<DWORD>(content.size()),
                                   &bytesWritten, nullptr) &&
                         bytesWritten == content.size() &&
                         FlushFileBuffers(tempFile.get());
        if (!succeeded) {
            tempFile.reset();
            DeleteFile(tempPath.c_str());
            return false;
        }
    }

    if (!MoveFileEx(tempPath.c_str(), path.c_str(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)) {
        DeleteFile(tempPath.c_str());
        return false;
    }

    return true;
}

}  // namespace Functions
