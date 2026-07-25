#include "stdafx.h"

#include "shared_functions.h"
#include "var_init_once.h"

namespace Functions {

std::vector<std::wstring> SplitString(std::wstring_view s, WCHAR delim) {
    // https://stackoverflow.com/a/48403210
    auto view =
        s | std::views::split(delim) | std::views::transform([](auto&& rng) {
            return std::wstring_view(rng.data(), rng.size());
        });
    return std::vector<std::wstring>(view.begin(), view.end());
}

std::vector<std::wstring_view> SplitStringToViews(std::wstring_view s,
                                                  WCHAR delim) {
    // https://stackoverflow.com/a/48403210
    auto view =
        s | std::views::split(delim) | std::views::transform([](auto&& rng) {
            return std::wstring_view(rng.data(), rng.size());
        });
    return std::vector<std::wstring_view>(view.begin(), view.end());
}

// https://stackoverflow.com/a/29752943
std::wstring ReplaceAll(std::wstring_view source,
                        std::wstring_view from,
                        std::wstring_view to,
                        bool ignoreCase) {
    // An empty needle matches at every position without consuming input, which
    // would make the loop below spin forever.
    if (from.empty()) {
        return std::wstring(source);
    }

    auto findString = [ignoreCase](std::wstring_view haystack,
                                   std::wstring_view needle,
                                   size_t pos) -> size_t {
        if (!ignoreCase) {
            return haystack.find(needle, pos);
        }

        auto it = std::search(
            haystack.begin() + pos, haystack.end(), needle.begin(),
            needle.end(), [](WCHAR ch1, WCHAR ch2) {
                LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &ch1,
                              1, &ch1, 1, nullptr, nullptr, 0);
                LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &ch2,
                              1, &ch2, 1, nullptr, nullptr, 0);
                return ch1 == ch2;
            });
        if (it == haystack.end()) {
            return haystack.npos;
        }

        return std::distance(haystack.begin(), it);
    };

    std::wstring newString;

    size_t lastPos = 0;
    size_t findPos;

    while ((findPos = findString(source, from, lastPos)) != source.npos) {
        newString.append(source, lastPos, findPos - lastPos);
        newString += to;
        lastPos = findPos + from.length();
    }

    // Care for the rest after last occurrence.
    newString += source.substr(lastPos);

    return newString;
}

void GetNtVersionNumbers(ULONG* pNtMajorVersion,
                         ULONG* pNtMinorVersion,
                         ULONG* pNtBuildNumber) {
    using RtlGetNtVersionNumbers_t =
        void(WINAPI*)(ULONG * pNtMajorVersion, ULONG * pNtMinorVersion,
                      ULONG * pNtBuildNumber);

    GET_PROC_ADDRESS_ONCE(RtlGetNtVersionNumbers_t, pRtlGetNtVersionNumbers,
                          L"ntdll.dll", "RtlGetNtVersionNumbers");

    if (pRtlGetNtVersionNumbers) {
        pRtlGetNtVersionNumbers(pNtMajorVersion, pNtMinorVersion,
                                pNtBuildNumber);
        // The upper 4 bits are reserved for the type of the OS build.
        // https://dennisbabkin.com/blog/?t=how-to-tell-the-real-version-of-windows-your-app-is-running-on
        *pNtBuildNumber &= ~0xF0000000;
        return;
    }

    // Use GetVersionEx as a fallback.
#pragma warning(push)
#pragma warning(disable : 4996)  // disable deprecation message
    OSVERSIONINFO versionInfo = {sizeof(OSVERSIONINFO)};
    if (GetVersionEx(&versionInfo)) {
        *pNtMajorVersion = versionInfo.dwMajorVersion;
        *pNtMinorVersion = versionInfo.dwMinorVersion;
        *pNtBuildNumber = versionInfo.dwBuildNumber;
        return;
    }
#pragma warning(pop)

    *pNtMajorVersion = 0;
    *pNtMinorVersion = 0;
    *pNtBuildNumber = 0;
}

bool IsWindowsVersionOrGreaterWithBuildNumber(WORD wMajorVersion,
                                              WORD wMinorVersion,
                                              WORD wBuildNumber) {
    ULONG majorVersion = 0;
    ULONG minorVersion = 0;
    ULONG buildNumber = 0;
    Functions::GetNtVersionNumbers(&majorVersion, &minorVersion, &buildNumber);

    if (majorVersion != wMajorVersion) {
        return majorVersion > wMajorVersion;
    }

    if (minorVersion != wMinorVersion) {
        return minorVersion > wMinorVersion;
    }

    return buildNumber >= wBuildNumber;
}

HRESULT SetThreadDescriptionIfAvailable(HANDLE hThread,
                                        PCWSTR lpThreadDescription) {
    using SetThreadDescription_t = decltype(&SetThreadDescription);
    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        SetThreadDescription_t, pSetThreadDescription, L"kernel32.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "SetThreadDescription");

    if (!pSetThreadDescription) {
        return E_NOTIMPL;
    }

    return pSetThreadDescription(hThread, lpThreadDescription);
}

}  // namespace Functions
