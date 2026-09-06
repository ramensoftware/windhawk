#pragma once

namespace Functions {

// The import address table slot the module calls the named import of the named
// module through, or null if it imports no such thing. pImportName is an
// ordinal when its value fits in 16 bits, a name otherwise.
void** FindImportPtr(HMODULE hFindInModule,
                     PCSTR pModuleName,
                     PCSTR pImportName);

// A PE mapped so that RVAs can be followed from the base: a loaded module, or a
// mapping made with LOAD_LIBRARY_AS_IMAGE_RESOURCE. Construction validates the
// headers and every read is bounded by SizeOfImage, so a malformed header can't
// send one outside the image. Nothing here throws or logs, leaving each caller
// its own policy for an image that doesn't parse.
class PeImage {
   public:
    static std::optional<PeImage> FromBase(const void* base);
    // Decodes the mapping kind the loader tags into a LoadLibraryEx datafile
    // handle's low bits. Null for a flat mapping of the file's bytes, which
    // isn't addressed by RVA. An untagged handle is a module the loader already
    // had loaded and handed back in place of a mapping.
    static std::optional<PeImage> FromLoadLibraryExHandle(HMODULE module);

    const BYTE* base() const { return m_base; }
    ULONG imageSize() const { return m_imageSize; }
    bool is64Bit() const { return m_is64Bit; }

    // Null when the entry is past NumberOfRvaAndSizes.
    const IMAGE_DATA_DIRECTORY* DataDirectory(ULONG entry) const;
    // Null when the span isn't wholly inside the image.
    const void* At(ULONG rva, size_t size) const;
    // Null when no terminator is reached inside the image.
    const char* String(ULONG rva) const;
    // For a pointer field the loader already relocated, such as
    // CHPEMetadataPointer. Null when it doesn't point into the image.
    std::optional<ULONG> RvaFromVa(ULONGLONG va) const;

   private:
    const BYTE* m_base;
    ULONG m_imageSize;
    bool m_is64Bit;
    const IMAGE_DATA_DIRECTORY* m_dataDirectory;
    ULONG m_dataDirectoryCount;
};

// What the helpers below need from an image: a bounds-checked read of a span of
// bytes at an RVA, and one of a NUL-terminated string there. PeImage is one
// such reader; a parser over a flat file mapping, where RVAs go through the
// section table, is another.
template <typename T>
concept PeImageReader = requires(const T& image, ULONG rva, size_t size) {
    { image.At(rva, size) } -> std::convertible_to<const void*>;
    { image.String(rva) } -> std::convertible_to<const char*>;
};

template <typename T, PeImageReader TImage>
const T* PeImageAt(const TImage& image, ULONG rva) {
    return static_cast<const T*>(image.At(rva, sizeof(T)));
}

template <typename T, PeImageReader TImage>
std::span<const T> PeImageArray(const TImage& image, ULONG rva, ULONG count) {
    // Bound the count before multiplying, since size_t is 32-bit in the 32-bit
    // build and a count out of a malformed header can wrap it.
    if (count > SIZE_MAX / sizeof(T)) {
        return {};
    }

    auto* ptr = static_cast<const T*>(image.At(rva, count * sizeof(T)));
    if (!ptr) {
        return {};
    }

    return {ptr, count};
}

// Walks the image's named exports, calling fn(exports, index, name) for each
// name that lies inside the image and skipping any that doesn't. Stops as soon
// as fn returns true, and returns whether it did.
template <PeImageReader TImage, typename TFn>
bool ForEachExportName(const TImage& image,
                       const IMAGE_DATA_DIRECTORY& exportDir,
                       TFn&& fn) {
    if (!exportDir.VirtualAddress) {
        return false;
    }

    auto* exports =
        PeImageAt<IMAGE_EXPORT_DIRECTORY>(image, exportDir.VirtualAddress);
    if (!exports || !exports->AddressOfNames) {
        return false;
    }

    auto nameRvas = PeImageArray<DWORD>(image, exports->AddressOfNames,
                                        exports->NumberOfNames);

    for (ULONG i = 0; i < nameRvas.size(); i++) {
        // A null RVA names nothing.
        if (!nameRvas[i]) {
            continue;
        }

        const char* name = image.String(nameRvas[i]);
        if (!name) {
            continue;
        }

        if (fn(*exports, i, std::string_view(name))) {
            return true;
        }
    }

    return false;
}

// Whether the PE file exports any of the given names. Mapped as data, so
// nothing in it runs and its architecture needn't be this one. Throws if it
// can't be mapped; a file with no export table to speak of is not a match.
bool DoesFileExportAnyName(const std::filesystem::path& path,
                           std::span<const std::string_view> names);

bool ModuleGetPDBInfo(HANDLE hOsHandle,
                      _Out_ GUID* pGuidSignature,
                      _Out_ DWORD* pdwAge);
std::string GetModuleVersion(HMODULE hModule);

}  // namespace Functions
