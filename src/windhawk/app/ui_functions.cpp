#include "stdafx.h"

#include "shared_functions.h"
#include "ui_functions.h"

namespace Functions {

PCWSTR LoadStrFromRsrc(UINT uStrId) {
    PCWSTR pStr;
    if (!LoadString(nullptr, uStrId, (WCHAR*)&pStr, 0)) {
        pStr = L"(Could not load resource)";
    }

    return pStr;
}

UINT GetDpiForWindowWithFallback(HWND hWnd) {
    using GetDpiForWindow_t = UINT(WINAPI*)(HWND hwnd);
    static GetDpiForWindow_t pGetDpiForWindow = []() {
        HMODULE hUser32 = GetModuleHandle(L"user32.dll");
        if (hUser32) {
            return (GetDpiForWindow_t)GetProcAddress(hUser32,
                                                     "GetDpiForWindow");
        }

        return (GetDpiForWindow_t) nullptr;
    }();

    int iDpi = 96;
    if (pGetDpiForWindow) {
        iDpi = pGetDpiForWindow(hWnd);
    } else {
        CDC hdc = ::GetDC(nullptr);
        if (hdc) {
            iDpi = hdc.GetDeviceCaps(LOGPIXELSX);
        }
    }

    return iDpi;
}

int GetSystemMetricsForDpiWithFallback(int nIndex, UINT dpi) {
    using GetSystemMetricsForDpi_t = int(WINAPI*)(int nIndex, UINT dpi);
    static GetSystemMetricsForDpi_t pGetSystemMetricsForDpi = []() {
        HMODULE hUser32 = GetModuleHandle(L"user32.dll");
        if (hUser32) {
            return (GetSystemMetricsForDpi_t)GetProcAddress(
                hUser32, "GetSystemMetricsForDpi");
        }

        return (GetSystemMetricsForDpi_t) nullptr;
    }();

    if (pGetSystemMetricsForDpi) {
        return pGetSystemMetricsForDpi(nIndex, dpi);
    } else {
        return GetSystemMetrics(nIndex);
    }
}

int GetSystemMetricsForWindow(HWND hWnd, int nIndex) {
    return GetSystemMetricsForDpiWithFallback(
        nIndex, GetDpiForWindowWithFallback(hWnd));
}

bool IsRightToLeftLanguage(LANGID langId) {
    switch (PRIMARYLANGID(langId)) {
        case LANG_ARABIC:
        case LANG_FARSI:
        case LANG_HEBREW:
        case LANG_URDU:
            return true;

        default:
            return false;
    }
}

void ApplyDialogLayoutRtl(CWindow wnd, bool isLayoutRtl) {
    bool modified = wnd.ModifyStyleEx(isLayoutRtl ? 0 : WS_EX_LAYOUTRTL,
                                      isLayoutRtl ? WS_EX_LAYOUTRTL : 0);
    if (!modified) {
        // No change, so no need to update child controls.
        return;
    }

    ::EnumChildWindows(
        wnd,
        [](HWND hWnd, LPARAM lParam) {
            bool isLayoutRtl = lParam != 0;

            CWindow control(hWnd);
            CWindow parent = control.GetParent();

            CRect rcParent;
            parent.GetClientRect(rcParent);

            CRect rcControl;
            control.GetWindowRect(rcControl);
            ::MapWindowPoints(NULL, parent, (POINT*)&rcControl, 2);

            rcControl.MoveToX(rcParent.Width() - rcControl.right);

            control.SetWindowPos(NULL, rcControl, SWP_NOZORDER);

            if (isLayoutRtl) {
                control.ModifyStyleEx(0, WS_EX_LAYOUTRTL);
            } else {
                // Sometimes (e.g. for Edit controls), when setting
                // WS_EX_LAYOUTRTL, the flag is not actually set. Other flags
                // are being set instead (e.g. WS_EX_RTLREADING). Below, we try
                // to handle such cases.

                DWORD dwExStyle = control.GetExStyle();
                if (dwExStyle & WS_EX_LAYOUTRTL) {
                    control.ModifyStyleEx(WS_EX_LAYOUTRTL, 0);
                } else if (dwExStyle & (WS_EX_RTLREADING | WS_EX_RIGHT |
                                        WS_EX_LEFTSCROLLBAR)) {
                    control.ModifyStyleEx(
                        WS_EX_RTLREADING | WS_EX_RIGHT | WS_EX_LEFTSCROLLBAR,
                        0);

                    WCHAR szClassName[64];
                    if (::GetClassName(control, szClassName,
                                       _countof(szClassName))) {
                        if (_wcsicmp(szClassName, L"Edit") == 0)
                            control.ModifyStyle(ES_RIGHT, 0);
                    }
                }
            }

            control.InvalidateRect(NULL);

            return TRUE;
        },
        isLayoutRtl);

    wnd.InvalidateRect(NULL);
}

// Undocumented uxtheme.dll dark mode controls, resolved by ordinal.
// https://github.com/ysc3839/win32-darkmode
void EnableDarkModeMenus() {
    // Note: Before 1903, `BOOL __stdcall AllowDarkModeForApp(BOOL)` (same
    // ordinal) only accepts TRUE or FALSE. TRUE means dark mode is allowed and
    // vice versa. After 1903, `PreferredMode __stdcall
    // SetPreferredAppMode(PreferredMode)` accepts 4 valid values. Calling it
    // with TRUE (1) is valid in both cases.
    enum PreferredAppMode {
        PreferredAppModeDefault,
        PreferredAppModeAllowDark,
        PreferredAppModeForceDark,
        PreferredAppModeForceLight,
        PreferredAppModeMax,
    };

    using SetPreferredAppMode_t =
        PreferredAppMode(WINAPI*)(PreferredAppMode appMode);
    static SetPreferredAppMode_t pSetPreferredAppMode = []() {
        // The ordinal only holds this function starting with Windows 10 1809,
        // the first version with dark mode support. On older versions it may
        // resolve to an unrelated export with a different signature.
        if (!IsWindowsVersionOrGreaterWithBuildNumber(10, 0, 17763)) {
            return (SetPreferredAppMode_t) nullptr;
        }

        HMODULE hUxtheme = LoadLibraryEx(L"uxtheme.dll", nullptr,
                                         LOAD_LIBRARY_SEARCH_SYSTEM32);
        if (hUxtheme) {
            return (SetPreferredAppMode_t)GetProcAddress(hUxtheme,
                                                         MAKEINTRESOURCEA(135));
        }

        return (SetPreferredAppMode_t) nullptr;
    }();

    if (pSetPreferredAppMode) {
        pSetPreferredAppMode(PreferredAppModeAllowDark);
    }
}

}  // namespace Functions
