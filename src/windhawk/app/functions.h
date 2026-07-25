#pragma once

#include "shared_functions.h"

namespace Functions {

BOOL SetPrivilege(HANDLE hToken, LPCTSTR lpszPrivilege, BOOL bEnablePrivilege);
BOOL SetDebugPrivilege(BOOL bEnablePrivilege);
HANDLE CreateEventForMediumIntegrity(PCWSTR eventName,
                                     BOOL manualReset = FALSE);
BOOL IsRunAsAdmin();
PCWSTR LoadStrFromRsrc(UINT uStrId);
UINT GetDpiForWindowWithFallback(HWND hWnd);
int GetSystemMetricsForDpiWithFallback(int nIndex, UINT dpi);
int GetSystemMetricsForWindow(HWND hWnd, int nIndex);

// Returns true for suspended UWP processes.
// https://stackoverflow.com/a/50173965
bool IsProcessFrozen(HANDLE hProcess);

NTSTATUS CreateExecutionRequiredRequest(_In_ HANDLE ProcessHandle,
                                        _Out_ PHANDLE PowerRequestHandle);
bool IsRightToLeftLanguage(LANGID langId);
void ApplyDialogLayoutRtl(CWindow wnd, bool isLayoutRtl);

// Opts the process into following the system dark/light setting for popup
// (context) menus. Relies on undocumented uxtheme exports.
void EnableDarkModeMenus();

// Writes content to a file via a temporary file and a rename, so that the
// target file is never left with partial content.
bool WriteFileContentAtomically(const std::filesystem::path& path,
                                std::string_view content);

}  // namespace Functions
