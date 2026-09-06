#include "stdafx.h"

#include "path_matching.h"
#include "shared_functions.h"

namespace Functions {

// https://github.com/tidwall/match.c
//
// Whether str matches pat, where '*' stands for any run of characters and '?'
// for any single one. Nothing is a separator here: a '*' crosses a backslash
// like any other character, which is what lets a pattern holding a path match
// one.
//
// Unlike the linked implementation, the lengths are unsigned and must be exact,
// a negative length isn't a "call wcslen" sentinel.
//
// The match is iterative because retrying every '*' at every subject position
// costs O(slen^k) for k stars, enough to stall on an ordinary path. Only the
// most recent '*' has to be retried: matching each part at its earliest
// position leaves the most room for the rest, so stretching an earlier '*' can
// never turn a failure into a match. That one backtrack point bounds the work
// at O(plen * slen).
bool wcsmatch(PCWSTR pat, size_t plen, PCWSTR str, size_t slen) {
    size_t patPos = 0;
    size_t strPos = 0;

    // The '*' to resume from on a mismatch, and how far into str it's
    // stretched. plen means that no '*' was seen.
    size_t starPatPos = plen;
    size_t starStrPos = 0;

    while (strPos < slen) {
        if (patPos < plen && pat[patPos] == L'*') {
            if (patPos + 1 == plen) {
                return true;
            }
            starPatPos = patPos;
            starStrPos = strPos;
            patPos++;
        } else if (patPos < plen &&
                   (pat[patPos] == L'?' || pat[patPos] == str[strPos])) {
            patPos++;
            strPos++;
        } else if (starPatPos < plen) {
            patPos = starPatPos + 1;
            strPos = ++starStrPos;
        } else {
            return false;
        }
    }

    while (patPos < plen && pat[patPos] == L'*') {
        patPos++;
    }

    return patPos == plen;
}

bool DoesPathMatchPattern(std::wstring_view path,
                          std::wstring_view pattern,
                          bool explicitOnly) {
    if (pattern.empty()) {
        return false;
    }

    // A case-insensitive comparison as recommended here:
    // https://stackoverflow.com/q/410502

    std::wstring pathUpper{path};

    // Don't use CharUpperBuff to avoid depending on user32.dll. Use
    // LCMapStringEx just like it's called internally by CharUpperBuff.
    // CharUpperBuff(&pathUpper[0], wil::safe_cast<DWORD>(pathUpper.length()));
    LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE, &pathUpper[0],
                  wil::safe_cast<int>(pathUpper.length()), &pathUpper[0],
                  wil::safe_cast<int>(pathUpper.length()), nullptr, nullptr, 0);

    std::wstring_view pathFileNameUpper = pathUpper;
    if (size_t i = pathFileNameUpper.rfind(L'\\');
        i != pathFileNameUpper.npos) {
        pathFileNameUpper.remove_prefix(i + 1);
    }

    for (const auto& patternPartView : SplitStringToViews(pattern, L'|')) {
        if (explicitOnly) {
            bool patternIsWildcard =
                patternPartView.find_first_of(L"*?") != patternPartView.npos;
            if (patternIsWildcard) {
                // If the pattern contains wildcards, it's not an explicit
                // match.
                continue;
            }
        }

        auto patternPart = std::wstring{patternPartView};

#ifndef _WIN64
        BOOL isWow64;
        if (IsWow64Process(GetCurrentProcess(), &isWow64) && isWow64) {
            // Get the native Program Files path regardless of the current
            // process architecture.
            patternPart = ReplaceAll(patternPart, L"%ProgramFiles%",
                                     L"%ProgramW6432%", /*ignoreCase=*/true);
        }
#endif  // _WIN64

        auto patternPartNormalized =
            wil::ExpandEnvironmentStrings<std::wstring>(patternPart.c_str());

        // CharUpperBuff(&patternPartNormalized[0],
        //               wil::safe_cast<DWORD>(patternPartNormalized.length()));
        LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE,
                      &patternPartNormalized[0],
                      wil::safe_cast<int>(patternPartNormalized.length()),
                      &patternPartNormalized[0],
                      wil::safe_cast<int>(patternPartNormalized.length()),
                      nullptr, nullptr, 0);

        std::wstring_view match = pathUpper;

        // If there's no backslash in the pattern part, match only against the
        // file name, not the full path.
        if (patternPartNormalized.find(L'\\') == patternPartNormalized.npos) {
            match = pathFileNameUpper;
        }

        if (wcsmatch(patternPartNormalized.data(),
                     patternPartNormalized.length(), match.data(),
                     match.length())) {
            return true;
        }
    }

    return false;
}

}  // namespace Functions
