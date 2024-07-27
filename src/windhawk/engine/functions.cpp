#include "stdafx.h"

#include "functions.h"
#include "var_init_once.h"

namespace Functions {

namespace {

// Source:
// https://github.com/dotnet-bot/corert/blob/8928dfd66d98f40017ec7435df1fbada113656a8/src/Native/Runtime/windows/PalRedhawkCommon.cpp#L78
//
// Given the OS handle of a loaded module, compute the upper and lower virtual
// address bounds (inclusive).
void PalGetModuleBounds(HANDLE hOsHandle,
                        _Out_ BYTE** ppLowerBound,
                        _Out_ BYTE** ppUpperBound) {
    BYTE* pbModule = (BYTE*)hOsHandle;
    DWORD cbModule;

    IMAGE_NT_HEADERS* pNtHeaders =
        (IMAGE_NT_HEADERS*)(pbModule +
                            ((IMAGE_DOS_HEADER*)hOsHandle)->e_lfanew);
    if (pNtHeaders->OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC)
        cbModule = ((IMAGE_OPTIONAL_HEADER32*)&pNtHeaders->OptionalHeader)
                       ->SizeOfImage;
    else
        cbModule = ((IMAGE_OPTIONAL_HEADER64*)&pNtHeaders->OptionalHeader)
                       ->SizeOfImage;

    *ppLowerBound = pbModule;
    *ppUpperBound = pbModule + cbModule - 1;
}

}  // namespace

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
        SYSTEM_INFO siSystemInfo;
        GetNativeSystemInfo(&siSystemInfo);
        if (siSystemInfo.wProcessorArchitecture !=
            PROCESSOR_ARCHITECTURE_INTEL) {
            // Replace %ProgramFiles% with %ProgramW6432% to get the native
            // Program Files path regardless of the current process
            // architecture.
            constexpr WCHAR kEnvVar[] = L"%ProgramFiles%";
            constexpr size_t kEnvVarLength = ARRAYSIZE(kEnvVar) - 1;

            if (patternPart.length() >= kEnvVarLength) {
                constexpr WCHAR kEnvVarReplacement[] = L"%ProgramW6432%";
                constexpr size_t kEnvVarReplacementLength =
                    ARRAYSIZE(kEnvVarReplacement) - 1;

                for (size_t i = 0;
                     i < patternPart.length() - kEnvVarLength + 1;) {
                    if (_wcsnicmp(patternPart.c_str() + i, kEnvVar,
                                  kEnvVarLength) == 0) {
                        patternPart.replace(i, kEnvVarLength,
                                            kEnvVarReplacement,
                                            kEnvVarReplacementLength);
                        i += kEnvVarReplacementLength;
                    } else {
                        i++;
                    }
                }
            }
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

// Based on:
// https://github.com/dotnet-bot/corert/blob/8928dfd66d98f40017ec7435df1fbada113656a8/src/Native/Runtime/windows/PalRedhawkCommon.cpp#L109
//
// Reads through the PE header of the specified module, and returns
// the module's matching PDB's signature GUID and age by
// fishing them out of the last IMAGE_DEBUG_DIRECTORY of type
// IMAGE_DEBUG_TYPE_CODEVIEW.  Used when sending the ModuleLoad event
// to help profilers find matching PDBs for loaded modules.
//
// Arguments:
//
// [in] hOsHandle - OS Handle for module from which to get PDB info
// [out] pGuidSignature - PDB's signature GUID to be placed here
// [out] pdwAge - PDB's age to be placed here
//
// This is a simplification of similar code in desktop CLR's GetCodeViewInfo
// in eventtrace.cpp.
bool ModuleGetPDBInfo(HANDLE hOsHandle,
                      _Out_ GUID* pGuidSignature,
                      _Out_ DWORD* pdwAge) {
    // Zero-init [out]-params
    ZeroMemory(pGuidSignature, sizeof(*pGuidSignature));
    *pdwAge = 0;

    BYTE* pbModule = (BYTE*)hOsHandle;

    IMAGE_NT_HEADERS const* pNtHeaders =
        (IMAGE_NT_HEADERS*)(pbModule +
                            ((IMAGE_DOS_HEADER*)hOsHandle)->e_lfanew);
    IMAGE_DATA_DIRECTORY const* rgDataDirectory = NULL;
    if (pNtHeaders->OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC)
        rgDataDirectory =
            ((IMAGE_OPTIONAL_HEADER32 const*)&pNtHeaders->OptionalHeader)
                ->DataDirectory;
    else
        rgDataDirectory =
            ((IMAGE_OPTIONAL_HEADER64 const*)&pNtHeaders->OptionalHeader)
                ->DataDirectory;

    IMAGE_DATA_DIRECTORY const* pDebugDataDirectory =
        &rgDataDirectory[IMAGE_DIRECTORY_ENTRY_DEBUG];

    // In Redhawk, modules are loaded as MAPPED, so we don't have to worry about
    // dealing with FLAT files (with padding missing), so header addresses can
    // be used as is
    IMAGE_DEBUG_DIRECTORY const* rgDebugEntries =
        (IMAGE_DEBUG_DIRECTORY const*)(pbModule +
                                       pDebugDataDirectory->VirtualAddress);
    DWORD cbDebugEntries = pDebugDataDirectory->Size;
    if (cbDebugEntries < sizeof(IMAGE_DEBUG_DIRECTORY))
        return false;

    // Since rgDebugEntries is an array of IMAGE_DEBUG_DIRECTORYs,
    // cbDebugEntries should be a multiple of sizeof(IMAGE_DEBUG_DIRECTORY).
    if (cbDebugEntries % sizeof(IMAGE_DEBUG_DIRECTORY) != 0)
        return false;

    // CodeView RSDS debug information -> PDB 7.00
    struct CV_INFO_PDB70 {
        DWORD magic;
        GUID signature;                 // unique identifier
        DWORD age;                      // an always-incrementing value
        _Field_z_ char path[MAX_PATH];  // zero terminated string with the name
                                        // of the PDB file
    };

    // Temporary storage for a CV_INFO_PDB70 and its size (which could be less
    // than sizeof(CV_INFO_PDB70); see below).
    struct PdbInfo {
        CV_INFO_PDB70* m_pPdb70;
        ULONG m_cbPdb70;
    };

    // Grab module bounds so we can do some rough sanity checking before we
    // follow any RVAs
    BYTE* pbModuleLowerBound = NULL;
    BYTE* pbModuleUpperBound = NULL;
    PalGetModuleBounds(hOsHandle, &pbModuleLowerBound, &pbModuleUpperBound);

    // Iterate through all debug directory entries. The convention is that
    // debuggers & profilers typically just use the very last
    // IMAGE_DEBUG_TYPE_CODEVIEW entry.  Treat raw bytes we read as untrusted.
    PdbInfo pdbInfoLast = {0};
    int cEntries = cbDebugEntries / sizeof(IMAGE_DEBUG_DIRECTORY);
    for (int i = 0; i < cEntries; i++) {
        if ((BYTE*)(&rgDebugEntries[i]) + sizeof(rgDebugEntries[i]) >=
            pbModuleUpperBound) {
            // Bogus pointer
            return false;
        }

        if (rgDebugEntries[i].Type != IMAGE_DEBUG_TYPE_CODEVIEW)
            continue;

        // Get raw data pointed to by this IMAGE_DEBUG_DIRECTORY

        // AddressOfRawData is generally set properly for Redhawk modules, so we
        // don't have to worry about using PointerToRawData and converting it to
        // an RVA
        if (rgDebugEntries[i].AddressOfRawData == NULL)
            continue;

        DWORD rvaOfRawData = rgDebugEntries[i].AddressOfRawData;
        ULONG cbDebugData = rgDebugEntries[i].SizeOfData;
        if (cbDebugData < size_t(&((CV_INFO_PDB70*)0)->magic) +
                              sizeof(((CV_INFO_PDB70*)0)->magic)) {
            // raw data too small to contain magic number at expected spot, so
            // its format is not recognizeable. Skip
            continue;
        }

        // Verify the magic number is as expected
        const DWORD CV_SIGNATURE_RSDS = 0x53445352;
        CV_INFO_PDB70* pPdb70 = (CV_INFO_PDB70*)(pbModule + rvaOfRawData);
        if ((BYTE*)(pPdb70) + cbDebugData >= pbModuleUpperBound) {
            // Bogus pointer
            return false;
        }

        if (pPdb70->magic != CV_SIGNATURE_RSDS) {
            // Unrecognized magic number.  Skip
            continue;
        }

        // From this point forward, the format should adhere to the expected
        // layout of CV_INFO_PDB70. If we find otherwise, then assume the
        // IMAGE_DEBUG_DIRECTORY is outright corrupt.

        // Verify sane size of raw data
        if (cbDebugData > sizeof(CV_INFO_PDB70))
            return false;

        // cbDebugData actually can be < sizeof(CV_INFO_PDB70), since the "path"
        // field can be truncated to its actual data length (i.e., fewer than
        // MAX_PATH chars may be present in the PE file). In some cases, though,
        // cbDebugData will include all MAX_PATH chars even though path gets
        // null-terminated well before the MAX_PATH limit.

        // Gotta have at least one byte of the path
        if (cbDebugData < offsetof(CV_INFO_PDB70, path) + sizeof(char))
            return false;

        // How much space is available for the path?
        size_t cchPathMaxIncludingNullTerminator =
            (cbDebugData - offsetof(CV_INFO_PDB70, path)) / sizeof(char);
        assert(cchPathMaxIncludingNullTerminator >= 1);  // Guaranteed above

        // Verify path string fits inside the declared size
        size_t cchPathActualExcludingNullTerminator =
            strnlen_s(pPdb70->path, cchPathMaxIncludingNullTerminator);
        if (cchPathActualExcludingNullTerminator ==
            cchPathMaxIncludingNullTerminator) {
            // This is how strnlen indicates failure--it couldn't find the null
            // terminator within the buffer size specified
            return false;
        }

        // Looks valid.  Remember it.
        pdbInfoLast.m_pPdb70 = pPdb70;
        pdbInfoLast.m_cbPdb70 = cbDebugData;
    }

    // Take the last IMAGE_DEBUG_TYPE_CODEVIEW entry we saw, and return it to
    // the caller
    if (pdbInfoLast.m_pPdb70 != NULL) {
        memcpy(pGuidSignature, &pdbInfoLast.m_pPdb70->signature, sizeof(GUID));
        *pdwAge = pdbInfoLast.m_pPdb70->age;
        return true;
    }

    return false;
}

}  // namespace Functions
