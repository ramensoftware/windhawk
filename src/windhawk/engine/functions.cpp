#include "stdafx.h"

#include "functions.h"
#include "var_init_once.h"

namespace Functions {

// https://github.com/tidwall/match.c
//
// match returns true if str matches pattern. This is a very
// simple wildcard match where '*' matches on any number characters
// and '?' matches on any one character.
//
// pattern:
//   { term }
// term:
// 	 '*'         matches any sequence of non-Separator characters
// 	 '?'         matches any single non-Separator character
// 	 c           matches character c (c != '*', '?')
bool wcsmatch(PCWSTR pat, size_t plen, PCWSTR str, size_t slen) {
    if (plen < 0)
        plen = wcslen(pat);
    if (slen < 0)
        slen = wcslen(str);
    while (plen > 0) {
        if (pat[0] == L'*') {
            if (plen == 1)
                return true;
            if (pat[1] == L'*') {
                pat++;
                plen--;
                continue;
            }
            if (wcsmatch(pat + 1, plen - 1, str, slen))
                return true;
            if (slen == 0)
                return false;
            str++;
            slen--;
            continue;
        }
        if (slen == 0)
            return false;
        if (pat[0] != L'?' && str[0] != pat[0])
            return false;
        pat++;
        plen--;
        str++;
        slen--;
    }
    return slen == 0 && plen == 0;
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

bool DoesPathMatchPattern(std::wstring_view path, std::wstring_view pattern) {
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
    if (size_t i = pathFileNameUpper.rfind(L'\\'); i != std::wstring::npos) {
        pathFileNameUpper.remove_prefix(i + 1);
    }

    for (const auto& patternPart : SplitString(pattern, L'|')) {
        auto patternPartExpanded =
            wil::ExpandEnvironmentStrings<std::wstring>(patternPart.c_str());

        // CharUpperBuff(&patternPartExpanded[0],
        //               wil::safe_cast<DWORD>(patternPartExpanded.length()));
        LCMapStringEx(LOCALE_NAME_USER_DEFAULT, LCMAP_UPPERCASE,
                      &patternPartExpanded[0],
                      wil::safe_cast<int>(patternPartExpanded.length()),
                      &patternPartExpanded[0],
                      wil::safe_cast<int>(patternPartExpanded.length()),
                      nullptr, nullptr, 0);

        std::wstring_view match = pathUpper;

        // If there's no backslash in the pattern part, match only against the
        // file name, not the full path.
        if (patternPartExpanded.find(L'\\') == std::wstring::npos) {
            match = pathFileNameUpper;
        }

        if (wcsmatch(patternPartExpanded.data(), patternPartExpanded.length(),
                     match.data(), match.length())) {
            return true;
        }
    }

    return false;
}

void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName) {
    IMAGE_DOS_HEADER* pDosHeader;
    IMAGE_NT_HEADERS* pNtHeader;
    ULONG_PTR ImageBase;
    IMAGE_IMPORT_DESCRIPTOR* pImportDescriptor;
    ULONG_PTR* pOriginalFirstThunk;
    ULONG_PTR* pFirstThunk;
    ULONG_PTR ImageImportByName;

    // Init
    pDosHeader = (IMAGE_DOS_HEADER*)hFindInModule;
    pNtHeader = (IMAGE_NT_HEADERS*)((char*)pDosHeader + pDosHeader->e_lfanew);

    if (!pNtHeader->OptionalHeader.DataDirectory[1].VirtualAddress)
        return nullptr;

    ImageBase = (ULONG_PTR)hFindInModule;
    pImportDescriptor =
        (IMAGE_IMPORT_DESCRIPTOR*)(ImageBase +
                                   pNtHeader->OptionalHeader.DataDirectory[1]
                                       .VirtualAddress);

    // Search!
    while (pImportDescriptor->OriginalFirstThunk) {
        if (lstrcmpiA((char*)(ImageBase + pImportDescriptor->Name),
                      pModuleName) == 0) {
            pOriginalFirstThunk =
                (ULONG_PTR*)(ImageBase + pImportDescriptor->OriginalFirstThunk);
            ImageImportByName = *pOriginalFirstThunk;

            pFirstThunk =
                (ULONG_PTR*)(ImageBase + pImportDescriptor->FirstThunk);

            while (ImageImportByName) {
                if (!(ImageImportByName & IMAGE_ORDINAL_FLAG)) {
                    if ((ULONG_PTR)pImportName & ~0xFFFF) {
                        ImageImportByName += sizeof(WORD);

                        if (lstrcmpA((char*)(ImageBase + ImageImportByName),
                                     pImportName) == 0)
                            return (void**)pFirstThunk;
                    }
                } else {
                    if (((ULONG_PTR)pImportName & ~0xFFFF) == 0)
                        if ((ImageImportByName & 0xFFFF) ==
                            (ULONG_PTR)pImportName)
                            return (void**)pFirstThunk;
                }

                pOriginalFirstThunk++;
                ImageImportByName = *pOriginalFirstThunk;

                pFirstThunk++;
            }
        }

        pImportDescriptor++;
    }

    return nullptr;
}

BOOL GetFullAccessSecurityDescriptor(
    _Outptr_ PSECURITY_DESCRIPTOR* SecurityDescriptor,
    _Out_opt_ PULONG SecurityDescriptorSize) {
    // http://rsdn.org/forum/winapi/7510772.flat
    //
    // For full access maniacs :)
    // Full access for the "Everyone" group and for the "All [Restricted] App
    // Packages" groups. The integrity label is Untrusted (lowest level).
    //
    // D - DACL
    // P - Protected
    // A - Access Allowed
    // GA - GENERIC_ALL
    // WD - 'All' Group (World)
    // S-1-15-2-1 - All Application Packages
    // S-1-15-2-2 - All Restricted Application Packages
    //
    // S - SACL
    // ML - Mandatory Label
    // NW - No Write-Up policy
    // S-1-16-0 - Untrusted Mandatory Level
    PCWSTR pszStringSecurityDescriptor =
        L"D:P(A;;GA;;;WD)(A;;GA;;;S-1-15-2-1)(A;;GA;;;S-1-15-2-2)S:(ML;;NW;;;S-"
        L"1-16-0)";

    return ConvertStringSecurityDescriptorToSecurityDescriptor(
        pszStringSecurityDescriptor, SDDL_REVISION_1, SecurityDescriptor,
        SecurityDescriptorSize);
}

// Based on:
// http://securityxploded.com/ntcreatethreadex.php
// Another reference:
// https://github.com/winsiderss/systeminformer/blob/25846070780183848dc8d8f335a54fa6e636e281/phlib/basesup.c#L217
HANDLE MyCreateRemoteThread(HANDLE hProcess,
                            LPTHREAD_START_ROUTINE lpStartAddress,
                            LPVOID lpParameter,
                            ULONG createFlags) {
    using NtCreateThreadEx_t = NTSTATUS(WINAPI*)(
        _Out_ PHANDLE ThreadHandle, _In_ ACCESS_MASK DesiredAccess,
        _In_opt_ LPVOID ObjectAttributes,  // POBJECT_ATTRIBUTES
        _In_ HANDLE ProcessHandle,
        _In_ PVOID StartRoutine,  // PUSER_THREAD_START_ROUTINE
        _In_opt_ PVOID Argument,
        _In_ ULONG CreateFlags,  // THREAD_CREATE_FLAGS_*
        _In_ SIZE_T ZeroBits, _In_ SIZE_T StackSize,
        _In_ SIZE_T MaximumStackSize,
        _In_opt_ LPVOID AttributeList  // PPS_ATTRIBUTE_LIST
    );

    GET_PROC_ADDRESS_ONCE(NtCreateThreadEx_t, pNtCreateThreadEx, L"ntdll.dll",
                          "NtCreateThreadEx");

    if (!pNtCreateThreadEx) {
        SetLastError(ERROR_PROC_NOT_FOUND);
        return nullptr;
    }

    HANDLE hThread;
    NTSTATUS result = pNtCreateThreadEx(&hThread, THREAD_ALL_ACCESS, nullptr,
                                        hProcess, lpStartAddress, lpParameter,
                                        createFlags, 0, 0, 0, nullptr);
    if (result < 0) {
        SetLastError(LsaNtStatusToWinError(result));
        return nullptr;
    }

    return hThread;
}

}  // namespace Functions
