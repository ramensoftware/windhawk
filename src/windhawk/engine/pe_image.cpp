#include "stdafx.h"

#include "pe_image.h"
#include "var_init_once.h"

namespace Functions {

namespace {

// Upper bound on a mapped image's PE headers: they start at the image base,
// and a page from there is always mapped.
constexpr DWORD kMaxHeadersSize = 0x1000;

// As many T as fit between the RVA and the end of the image, for an array which
// ends at a terminator instead of carrying a count.
template <typename T>
std::span<const T> ArrayToImageEnd(const PeImage& image, ULONG rva) {
    if (rva >= image.imageSize()) {
        return {};
    }

    return PeImageArray<T>(image, rva, (image.imageSize() - rva) / sizeof(T));
}

}  // namespace

void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName) {
    auto image = PeImage::FromBase(hFindInModule);
    if (!image) {
        return nullptr;
    }

    const IMAGE_DATA_DIRECTORY* importDir =
        image->DataDirectory(IMAGE_DIRECTORY_ENTRY_IMPORT);
    if (!importDir || !importDir->VirtualAddress) {
        return nullptr;
    }

    // The slot is the caller's to patch, the walk which reaches it is read
    // only.
    auto importSlot = [](const IMAGE_THUNK_DATA& thunk) {
        return const_cast<void**>(
            reinterpret_cast<const void* const*>(&thunk.u1.Function));
    };

    bool wantOrdinal = ((ULONG_PTR)pImportName & ~0xFFFF) == 0;

    for (const auto& descriptor : ArrayToImageEnd<IMAGE_IMPORT_DESCRIPTOR>(
             *image, importDir->VirtualAddress)) {
        // The array ends at an all-zero descriptor. A descriptor which leaves
        // OriginalFirstThunk zero is a legal shape, so it doesn't end the walk.
        if (!descriptor.OriginalFirstThunk && !descriptor.FirstThunk &&
            !descriptor.Name) {
            break;
        }

        const char* moduleName = image->String(descriptor.Name);
        if (!moduleName || _stricmp(moduleName, pModuleName) != 0) {
            continue;
        }

        // The original thunks name the imports, the first thunks hold the
        // addresses the loader wrote for them, one for one. A descriptor
        // without original thunks names its imports through the first thunks,
        // which hold addresses by the time the module is loaded, leaving no
        // names to match against.
        if (!descriptor.OriginalFirstThunk) {
            continue;
        }

        auto nameThunks = ArrayToImageEnd<IMAGE_THUNK_DATA>(
            *image, descriptor.OriginalFirstThunk);
        auto addressThunks =
            ArrayToImageEnd<IMAGE_THUNK_DATA>(*image, descriptor.FirstThunk);

        for (size_t i = 0; i < nameThunks.size() && i < addressThunks.size();
             i++) {
            ULONG_PTR entry = nameThunks[i].u1.Ordinal;
            if (!entry) {
                break;
            }

            if (IMAGE_SNAP_BY_ORDINAL(entry)) {
                if (wantOrdinal &&
                    IMAGE_ORDINAL(entry) == (ULONG_PTR)pImportName) {
                    return importSlot(addressThunks[i]);
                }

                continue;
            }

            // An RVA to an IMAGE_IMPORT_BY_NAME: the hint, then the name.
            if (wantOrdinal || entry > ULONG_MAX - sizeof(WORD)) {
                continue;
            }

            const char* importName =
                image->String(static_cast<ULONG>(entry + sizeof(WORD)));
            if (importName && strcmp(importName, pImportName) == 0) {
                return importSlot(addressThunks[i]);
            }
        }
    }

    return nullptr;
}

std::optional<PeImage> PeImage::FromBase(const void* base) {
    if (!base) {
        return std::nullopt;
    }

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
    // addressed by RVA and so has nothing for a walk to follow. Neither bit is
    // a module the loader already had loaded, which it hands back in place of a
    // mapping and which is laid out as an image.
    ULONG_PTR handle = reinterpret_cast<ULONG_PTR>(module);
    if (handle & 1) {
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
// The signature GUID and age of the PDB built alongside the module, out of the
// last CodeView entry of its debug directory, which is the entry debuggers and
// profilers go by. Together they name the PDB to a symbol server.
bool ModuleGetPDBInfo(HANDLE hOsHandle,
                      _Out_ GUID* pGuidSignature,
                      _Out_ DWORD* pdwAge) {
    ZeroMemory(pGuidSignature, sizeof(*pGuidSignature));
    *pdwAge = 0;

    auto image = PeImage::FromBase(hOsHandle);
    if (!image) {
        return false;
    }

    const IMAGE_DATA_DIRECTORY* debugDir =
        image->DataDirectory(IMAGE_DIRECTORY_ENTRY_DEBUG);
    if (!debugDir || !debugDir->VirtualAddress) {
        return false;
    }

    // CodeView RSDS debug information -> PDB 7.00
    struct CV_INFO_PDB70 {
        DWORD magic;
        GUID signature;                 // unique identifier
        DWORD age;                      // an always-incrementing value
        _Field_z_ char path[MAX_PATH];  // zero terminated string with the name
                                        // of the PDB file
    };

    constexpr DWORD kCvSignatureRsds = 0x53445352;

    // An entry which doesn't parse is skipped, leaving the last one which does.
    const CV_INFO_PDB70* pdb70Last = nullptr;

    for (const auto& entry : PeImageArray<IMAGE_DEBUG_DIRECTORY>(
             *image, debugDir->VirtualAddress,
             debugDir->Size / sizeof(IMAGE_DEBUG_DIRECTORY))) {
        if (entry.Type != IMAGE_DEBUG_TYPE_CODEVIEW) {
            continue;
        }

        // The data of a mapped image is reached by RVA, so AddressOfRawData is
        // the field to follow, not PointerToRawData.
        ULONG size = entry.SizeOfData;

        // The path is what an entry can cut short, holding only the characters
        // it needs rather than all of MAX_PATH; what precedes it has to be
        // there in full, and nothing past the structure belongs to it.
        if (!entry.AddressOfRawData || size <= offsetof(CV_INFO_PDB70, path) ||
            size > sizeof(CV_INFO_PDB70)) {
            continue;
        }

        auto* pdb70 = static_cast<const CV_INFO_PDB70*>(
            image->At(entry.AddressOfRawData, size));
        if (!pdb70 || pdb70->magic != kCvSignatureRsds) {
            continue;
        }

        // The path has to end inside the entry's own data.
        size_t pathMaxIncludingNul = size - offsetof(CV_INFO_PDB70, path);
        if (strnlen(pdb70->path, pathMaxIncludingNul) == pathMaxIncludingNul) {
            continue;
        }

        pdb70Last = pdb70;
    }

    if (!pdb70Last) {
        return false;
    }

    *pGuidSignature = pdb70Last->signature;
    *pdwAge = pdb70Last->age;
    return true;
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
