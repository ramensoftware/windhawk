#include "stdafx.h"

#include "pe_image.h"
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

// Upper bound on a mapped image's PE headers: they start at the image base,
// and a page from there is always mapped.
constexpr DWORD kMaxHeadersSize = 0x1000;

}  // namespace

void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName) {
    IMAGE_DOS_HEADER* pDosHeader = (IMAGE_DOS_HEADER*)hFindInModule;
    IMAGE_NT_HEADERS* pNtHeader =
        (IMAGE_NT_HEADERS*)((char*)pDosHeader + pDosHeader->e_lfanew);

    if (pNtHeader->OptionalHeader.NumberOfRvaAndSizes <=
            IMAGE_DIRECTORY_ENTRY_IMPORT ||
        !pNtHeader->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
             .VirtualAddress) {
        return nullptr;
    }

    ULONG_PTR ImageBase = (ULONG_PTR)hFindInModule;
    IMAGE_IMPORT_DESCRIPTOR* pImportDescriptor =
        (IMAGE_IMPORT_DESCRIPTOR*)(ImageBase +
                                   pNtHeader->OptionalHeader
                                       .DataDirectory
                                           [IMAGE_DIRECTORY_ENTRY_IMPORT]
                                       .VirtualAddress);

    while (pImportDescriptor->OriginalFirstThunk) {
        if (_stricmp((char*)(ImageBase + pImportDescriptor->Name),
                     pModuleName) == 0) {
            IMAGE_THUNK_DATA* pOriginalFirstThunk =
                (IMAGE_THUNK_DATA*)(ImageBase +
                                    pImportDescriptor->OriginalFirstThunk);
            IMAGE_THUNK_DATA* pFirstThunk =
                (IMAGE_THUNK_DATA*)(ImageBase + pImportDescriptor->FirstThunk);

            while (ULONG_PTR ImageImportByName =
                       pOriginalFirstThunk->u1.Function) {
                if (!IMAGE_SNAP_BY_ORDINAL(ImageImportByName)) {
                    if ((ULONG_PTR)pImportName & ~0xFFFF) {
                        ImageImportByName += sizeof(WORD);

                        if (strcmp((char*)(ImageBase + ImageImportByName),
                                   pImportName) == 0) {
                            return (void**)pFirstThunk;
                        }
                    }
                } else {
                    if (((ULONG_PTR)pImportName & ~0xFFFF) == 0) {
                        if (IMAGE_ORDINAL(ImageImportByName) ==
                            (ULONG_PTR)pImportName) {
                            return (void**)pFirstThunk;
                        }
                    }
                }

                pOriginalFirstThunk++;
                pFirstThunk++;
            }
        }

        pImportDescriptor++;
    }

    return nullptr;
}

std::optional<PeImage> PeImage::FromBase(const void* base) {
    auto* bytes = static_cast<const BYTE*>(base);

    auto* dosHeader = reinterpret_cast<const IMAGE_DOS_HEADER*>(bytes);
    if (dosHeader->e_magic != IMAGE_DOS_SIGNATURE ||
        dosHeader->e_lfanew < static_cast<LONG>(sizeof(IMAGE_DOS_HEADER)) ||
        dosHeader->e_lfanew >
            static_cast<LONG>(kMaxHeadersSize - sizeof(IMAGE_NT_HEADERS64))) {
        return std::nullopt;
    }

    // The signature, the file header and the optional header's Magic sit at the
    // same offsets in both layouts.
    auto* ntHeaders = reinterpret_cast<const IMAGE_NT_HEADERS32*>(
        bytes + dosHeader->e_lfanew);
    if (ntHeaders->Signature != IMAGE_NT_SIGNATURE) {
        return std::nullopt;
    }

    PeImage image;
    image.m_base = bytes;

    // What precedes the fields below differs between the two optional header
    // layouts, so each is read through its own.
    switch (ntHeaders->OptionalHeader.Magic) {
        case IMAGE_NT_OPTIONAL_HDR32_MAGIC: {
            const auto& optionalHeader = ntHeaders->OptionalHeader;
            image.m_is64Bit = false;
            image.m_imageSize = optionalHeader.SizeOfImage;
            image.m_dataDirectory = optionalHeader.DataDirectory;
            image.m_dataDirectoryCount = optionalHeader.NumberOfRvaAndSizes;
            break;
        }

        case IMAGE_NT_OPTIONAL_HDR64_MAGIC: {
            const auto& optionalHeader =
                reinterpret_cast<const IMAGE_NT_HEADERS64*>(ntHeaders)
                    ->OptionalHeader;
            image.m_is64Bit = true;
            image.m_imageSize = optionalHeader.SizeOfImage;
            image.m_dataDirectory = optionalHeader.DataDirectory;
            image.m_dataDirectoryCount = optionalHeader.NumberOfRvaAndSizes;
            break;
        }

        default:
            return std::nullopt;
    }

    // The array itself is only as long as the header declares it, whatever the
    // count claims.
    if (image.m_dataDirectoryCount > IMAGE_NUMBEROF_DIRECTORY_ENTRIES) {
        image.m_dataDirectoryCount = IMAGE_NUMBEROF_DIRECTORY_ENTRIES;
    }

    return image;
}

std::optional<PeImage> PeImage::FromLoadLibraryExHandle(HMODULE module) {
    // The loader tags the mapping's kind in the handle's low bits: bit 1 for an
    // image mapping, bit 0 for a flat mapping of the file's bytes, which isn't
    // addressed by RVA and so has nothing for a walk to follow.
    ULONG_PTR handle = reinterpret_cast<ULONG_PTR>(module);
    if (!(handle & 2)) {
        return std::nullopt;
    }

    return FromBase(
        reinterpret_cast<const void*>(handle & ~static_cast<ULONG_PTR>(3)));
}

const IMAGE_DATA_DIRECTORY* PeImage::DataDirectory(ULONG entry) const {
    if (entry >= m_dataDirectoryCount) {
        return nullptr;
    }

    return &m_dataDirectory[entry];
}

const void* PeImage::At(ULONG rva, size_t size) const {
    if (rva >= m_imageSize || size > m_imageSize - rva) {
        return nullptr;
    }

    return m_base + rva;
}

const char* PeImage::String(ULONG rva) const {
    auto* str = static_cast<const char*>(At(rva, 1));
    if (!str) {
        return nullptr;
    }

    // The string runs to its terminator, which the end of the image bounds.
    size_t available = m_imageSize - rva;
    return strnlen(str, available) < available ? str : nullptr;
}

std::optional<ULONG> PeImage::RvaFromVa(ULONGLONG va) const {
    auto baseVa = reinterpret_cast<ULONGLONG>(m_base);
    if (va < baseVa || va - baseVa >= m_imageSize) {
        return std::nullopt;
    }

    return static_cast<ULONG>(va - baseVa);
}

bool DoesFileExportAnyName(const std::filesystem::path& path,
                           std::span<const std::string_view> names) {
    // Mapped as an image resource: laid out as if loaded, so RVAs can be
    // followed, but nothing in it runs and no imports are resolved.
    wil::unique_hmodule module(LoadLibraryEx(
        path.c_str(), nullptr,
        LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE | LOAD_LIBRARY_AS_IMAGE_RESOURCE));
    THROW_LAST_ERROR_IF_NULL(module);

    // A file that isn't laid out as an image, or has no export table to speak
    // of, answers the same as one without the name.
    auto image = PeImage::FromLoadLibraryExHandle(module.get());
    if (!image) {
        return false;
    }

    const IMAGE_DATA_DIRECTORY* exportDir =
        image->DataDirectory(IMAGE_DIRECTORY_ENTRY_EXPORT);
    if (!exportDir) {
        return false;
    }

    return ForEachExportName(
        *image, *exportDir,
        [names](const IMAGE_EXPORT_DIRECTORY&, ULONG, std::string_view name) {
            for (std::string_view wantedName : names) {
                if (name == wantedName) {
                    return true;
                }
            }

            return false;
        });
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
    DWORD cDataDirectory = 0;
    if (pNtHeaders->OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC) {
        IMAGE_OPTIONAL_HEADER32 const* pOptionalHeader =
            (IMAGE_OPTIONAL_HEADER32 const*)&pNtHeaders->OptionalHeader;
        rgDataDirectory = pOptionalHeader->DataDirectory;
        cDataDirectory = pOptionalHeader->NumberOfRvaAndSizes;
    } else {
        IMAGE_OPTIONAL_HEADER64 const* pOptionalHeader =
            (IMAGE_OPTIONAL_HEADER64 const*)&pNtHeaders->OptionalHeader;
        rgDataDirectory = pOptionalHeader->DataDirectory;
        cDataDirectory = pOptionalHeader->NumberOfRvaAndSizes;
    }

    if (cDataDirectory <= IMAGE_DIRECTORY_ENTRY_DEBUG)
        return false;

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
            // its format is not recognizable. Skip
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
        // assert(cchPathMaxIncludingNullTerminator >= 1);  // Guaranteed above

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

std::string GetModuleVersion(HMODULE hModule) {
    // Avoid having version.dll in the import table, since it might not be
    // available in all cases, e.g. sandboxed processes.
    using VerQueryValueW_t = decltype(&VerQueryValueW);

    LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(
        VerQueryValueW_t, pVerQueryValueW, L"version.dll",
        LOAD_LIBRARY_SEARCH_SYSTEM32, "VerQueryValueW");

    if (!pVerQueryValueW) {
        return {};
    }

    HRSRC hResource =
        FindResource(hModule, MAKEINTRESOURCE(VS_VERSION_INFO), VS_FILE_INFO);
    if (!hResource) {
        return {};
    }

    HGLOBAL hGlobal = LoadResource(hModule, hResource);
    if (!hGlobal) {
        return {};
    }

    void* pData = LockResource(hGlobal);
    if (!pData) {
        return {};
    }

    VS_FIXEDFILEINFO* pFixedFileInfo = nullptr;
    UINT uPtrLen = 0;
    if (!pVerQueryValueW(pData, L"\\",
                         reinterpret_cast<void**>(&pFixedFileInfo), &uPtrLen) ||
        uPtrLen == 0) {
        return {};
    }

    WORD nMajor = HIWORD(pFixedFileInfo->dwFileVersionMS);
    WORD nMinor = LOWORD(pFixedFileInfo->dwFileVersionMS);
    WORD nBuild = HIWORD(pFixedFileInfo->dwFileVersionLS);
    WORD nQFE = LOWORD(pFixedFileInfo->dwFileVersionLS);

    std::string result;
    result += std::to_string(nMajor);
    result += '.';
    result += std::to_string(nMinor);
    result += '.';
    result += std::to_string(nBuild);
    result += '.';
    result += std::to_string(nQFE);

    return result;
}

}  // namespace Functions
