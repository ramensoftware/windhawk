#pragma once

namespace Functions {

BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege);
BOOL SetDebugPrivilege(BOOL bEnablePrivilege);
HANDLE CreateEventForMediumIntegrity(PCWSTR eventName,
                                     BOOL manualReset = FALSE);
BOOL IsRunAsAdmin();

// Writes content to a file via a temporary file and a rename, so that the
// target file is never left with partial content.
bool WriteFileContentAtomically(const std::filesystem::path& path,
                                std::string_view content);

}  // namespace Functions
