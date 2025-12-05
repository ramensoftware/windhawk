/*
 * Copyright 2017 - 2018 Justas Masiulis
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#ifndef WOW64PP_HPP
#define WOW64PP_HPP

#if !(defined _M_IX86) && !(defined __i386__)
#error wow64pp is designed for x86 only
#endif

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <cstring>  // memcpy
#include <expected>
#include <memory>
#include <optional>
#include <string_view>
#include <system_error>

// The following macros are used to initialize static variables once in a
// thread-safe manner while avoiding TLS, which is what MSVC uses for static
// variables.
#ifdef WOW64PP_AVOID_TLS
//  Similar to:
//  static T var_name = initializer;
#define WOW64PP_STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer) \
    static constinit T var_name;                                   \
    do {                                                           \
        static_assert(std::is_trivially_destructible_v<T>);        \
        static std::once_flag static_init_once_flag_;              \
        std::call_once(static_init_once_flag_,                     \
                       []() { var_name = initializer; });          \
    } while (0)
#else
#define WOW64PP_STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer) \
    static T var_name = initializer;
#endif

namespace wow64pp {

typedef LONG NTSTATUS;

namespace defs {

using NtWow64QueryInformationProcess64T =
    NTSTATUS(__stdcall*)(HANDLE ProcessHandle,
                         std::uint32_t ProcessInformationClass,
                         void* ProcessInformation,
                         std::uint32_t ProcessInformationLength,
                         std::uint32_t* ReturnLength);

using NtWow64ReadVirtualMemory64T =
    NTSTATUS(__stdcall*)(HANDLE ProcessHandle,
                         std::uint64_t BaseAddress,
                         void* Buffer,
                         std::uint64_t Size,
                         std::uint64_t* NumberOfBytesRead);

struct LIST_ENTRY_64 {
    std::uint64_t Flink;
    std::uint64_t Blink;
};

struct UNICODE_STRING_64 {
    std::uint16_t Length;
    std::uint16_t MaximumLength;
    std::uint64_t Buffer;
};

struct PROCESS_BASIC_INFORMATION_64 {
    std::uint64_t unused_1_;
    std::uint64_t PebBaseAddress;
    std::uint64_t unused_2_[4];
};

struct PEB_64 {
    std::uint8_t unused_1_[4];
    std::uint64_t unused_2_[2];
    std::uint64_t Ldr;
};

struct PEB_LDR_DATA_64 {
    std::uint32_t Length;
    std::uint32_t Initialized;
    std::uint64_t SsHandle;
    LIST_ENTRY_64 InLoadOrderModuleList;
};

struct LDR_DATA_TABLE_ENTRY_64 {
    LIST_ENTRY_64 InLoadOrderLinks;
    LIST_ENTRY_64 InMemoryOrderLinks;
    LIST_ENTRY_64 InInitializationOrderLinks;
    std::uint64_t DllBase;
    std::uint64_t EntryPoint;
    union {
        std::uint32_t SizeOfImage;
        std::uint64_t dummy_;
    };
    UNICODE_STRING_64 FullDllName;
    UNICODE_STRING_64 BaseDllName;
};

}  // namespace defs

namespace detail {

inline std::error_code get_last_error() noexcept {
    return std::error_code(static_cast<int>(GetLastError()),
                           std::system_category());
}

[[noreturn]] inline void throw_error_code(const std::error_code& ec) {
    throw std::system_error(ec);
}

[[noreturn]] inline void throw_error_code(const std::error_code& ec,
                                          const char* message) {
    throw std::system_error(ec, message);
}

[[noreturn]] inline void throw_last_error(const char* message) {
    throw std::system_error(get_last_error(), message);
}

inline void throw_if_failed(const char* message, NTSTATUS status) {
    if (status < 0) {
        throw std::system_error(std::error_code(status, std::system_category()),
                                message);
    }
}

inline HANDLE self_handle() {
    HANDLE h;

    if (!DuplicateHandle(GetCurrentProcess(), GetCurrentProcess(),
                         GetCurrentProcess(), &h, 0, 0,
                         DUPLICATE_SAME_ACCESS)) {
        throw_last_error("failed to duplicate current process handle");
    }

    return h;
}

inline HANDLE self_handle(std::error_code& ec) noexcept {
    HANDLE h;

    if (!DuplicateHandle(GetCurrentProcess(), GetCurrentProcess(),
                         GetCurrentProcess(), &h, 0, 0,
                         DUPLICATE_SAME_ACCESS)) {
        ec = get_last_error();
        return nullptr;
    }

    ec.clear();
    return h;
}

template <typename F>
inline F native_ntdll_function(const char* name) {
    const auto ntdll_addr = GetModuleHandleW(L"ntdll.dll");
    if (!ntdll_addr) {
        throw_last_error("GetModuleHandle() failed");
    }

    auto f = reinterpret_cast<F>(GetProcAddress(ntdll_addr, name));
    if (!f) {
        throw_last_error("failed to get address of ntdll function");
    }

    return f;
}

template <typename F>
inline F native_ntdll_function(const char* name, std::error_code& ec) noexcept {
    const auto ntdll_addr = GetModuleHandleW(L"ntdll.dll");
    if (!ntdll_addr) {
        ec = get_last_error();
        return nullptr;
    }

    const auto f = reinterpret_cast<F>(GetProcAddress(ntdll_addr, name));
    if (!f) {
        ec = get_last_error();
        return nullptr;
    }

    ec.clear();
    return f;
}

template <typename FunctionType, const char* FunctionName>
inline FunctionType get_cached_native_ntdll_function(
    std::error_code& ec) noexcept {
    using function_result_t = std::expected<FunctionType, std::error_code>;
    WOW64PP_STATIC_INIT_ONCE_TRIVIAL(
        function_result_t, function_result, ([]() -> function_result_t {
            std::error_code ec;
            const auto function =
                native_ntdll_function<FunctionType>(FunctionName, ec);
            if (ec)
                return std::unexpected(ec);
            return function;
        }()));
    if (!function_result.has_value()) {
        ec = function_result.error();
        return nullptr;
    }

    ec.clear();
    return *function_result;
}

inline defs::NtWow64QueryInformationProcess64T
get_cached_nt_wow64_query_information_process_64(std::error_code& ec) noexcept {
    static constexpr char function_name[] = "NtWow64QueryInformationProcess64";
    return get_cached_native_ntdll_function<
        defs::NtWow64QueryInformationProcess64T, function_name>(ec);
}

inline defs::NtWow64ReadVirtualMemory64T
get_cached_nt_wow64_read_virtual_memory_64(std::error_code& ec) noexcept {
    static constexpr char function_name[] = "NtWow64ReadVirtualMemory64";
    return get_cached_native_ntdll_function<defs::NtWow64ReadVirtualMemory64T,
                                            function_name>(ec);
}

inline std::uint64_t peb_address() {
    std::error_code ec;
    const auto NtWow64QueryInformationProcess64 =
        get_cached_nt_wow64_query_information_process_64(ec);
    if (ec) {
        throw_error_code(ec);
    }

    defs::PROCESS_BASIC_INFORMATION_64 pbi;
    const auto hres =
        NtWow64QueryInformationProcess64(GetCurrentProcess(),
                                         0,  // ProcessBasicInformation
                                         &pbi, sizeof(pbi), nullptr);
    throw_if_failed("NtWow64QueryInformationProcess64() failed", hres);

    return pbi.PebBaseAddress;
}

inline std::uint64_t peb_address(std::error_code& ec) noexcept {
    const auto NtWow64QueryInformationProcess64 =
        get_cached_nt_wow64_query_information_process_64(ec);
    if (ec) {
        return 0;
    }

    defs::PROCESS_BASIC_INFORMATION_64 pbi;
    const auto hres =
        NtWow64QueryInformationProcess64(GetCurrentProcess(),
                                         0,  // ProcessBasicInformation
                                         &pbi, sizeof(pbi), nullptr);
    if (hres < 0) {
        ec = get_last_error();
    }

    return pbi.PebBaseAddress;
}

template <typename P>
inline void read_memory(std::uint64_t address,
                        P* buffer,
                        std::size_t size = sizeof(P)) {
    if (address + size - 1 <= std::numeric_limits<std::uint32_t>::max()) {
        std::memcpy(
            buffer,
            reinterpret_cast<const void*>(static_cast<std::uint32_t>(address)),
            size);
        return;
    }

    std::error_code ec;
    const auto NtWow64ReadVirtualMemory64 =
        get_cached_nt_wow64_read_virtual_memory_64(ec);
    if (ec) {
        throw_error_code(ec);
    }

    HANDLE h_self = self_handle();
    auto hres =
        NtWow64ReadVirtualMemory64(h_self, address, buffer, size, nullptr);
    CloseHandle(h_self);
    throw_if_failed("NtWow64ReadVirtualMemory64() failed", hres);
}

template <typename P>
inline void read_memory(std::uint64_t address,
                        P* buffer,
                        std::size_t size,
                        std::error_code& ec) noexcept {
    if (address + size - 1 <= std::numeric_limits<std::uint32_t>::max()) {
        std::memcpy(
            buffer,
            reinterpret_cast<const void*>(static_cast<std::uint32_t>(address)),
            size);
        return;
    }

    const auto NtWow64ReadVirtualMemory64 =
        get_cached_nt_wow64_read_virtual_memory_64(ec);
    if (ec) {
        return;
    }

    HANDLE h_self = self_handle(ec);
    if (ec) {
        return;
    }

    auto hres =
        NtWow64ReadVirtualMemory64(h_self, address, buffer, size, nullptr);
    CloseHandle(h_self);
    if (hres < 0) {
        ec = get_last_error();
    }
}

template <typename T>
inline T read_memory(std::uint64_t address) {
    alignas(T) std::byte buffer[sizeof(T)];
    read_memory(address, &buffer, sizeof(T));
    return *static_cast<T*>(static_cast<void*>(&buffer));
}

template <typename T>
inline T read_memory(std::uint64_t address, std::error_code& ec) noexcept {
    alignas(T) std::byte buffer[sizeof(T)];
    read_memory(address, &buffer, sizeof(T), ec);
    return *static_cast<T*>(static_cast<void*>(&buffer));
}

}  // namespace detail

/** \brief An equivalent of winapi GetModuleHandle function.
 *   \param[in] module_name The name of the module to get the handle of.
 *   \return    The handle to the module as a 64 bit integer.
 *   \exception Throws std::system_error on failure.
 */
inline std::uint64_t module_handle(std::string_view module_name) {
    const auto ldr_base =
        detail::read_memory<defs::PEB_64>(detail::peb_address()).Ldr;

    const auto last_entry =
        ldr_base + offsetof(defs::PEB_LDR_DATA_64, InLoadOrderModuleList);

    defs::LDR_DATA_TABLE_ENTRY_64 head;
    head.InLoadOrderLinks.Flink =
        detail::read_memory<defs::PEB_LDR_DATA_64>(ldr_base)
            .InLoadOrderModuleList.Flink;

    do {
        try {
            detail::read_memory(head.InLoadOrderLinks.Flink, &head);
        } catch (std::system_error&) {
            continue;
        }

        const auto other_module_name_len =
            head.BaseDllName.Length / sizeof(wchar_t);
        if (other_module_name_len != module_name.length()) {
            continue;
        }

        auto other_module_name =
            std::make_unique<wchar_t[]>(other_module_name_len);
        detail::read_memory(head.BaseDllName.Buffer, other_module_name.get(),
                            head.BaseDllName.Length);

        if (std::equal(begin(module_name), end(module_name),
                       other_module_name.get())) {
            return head.DllBase;
        }
    } while (head.InLoadOrderLinks.Flink != last_entry);

    throw std::system_error(
        std::error_code(ERROR_MOD_NOT_FOUND, std::system_category()),
        "Could not get x64 module handle");
}

/** \brief An equivalent of winapi GetModuleHandle function.
 *   \param[in] module_name The name of the module to get the handle of.
 *   \param[out] ec An error code that will be set in case of failure
 *   \return    The handle to the module as a 64 bit integer.
 *   \exception Does not throw.
 */
inline std::uint64_t module_handle(std::string_view module_name,
                                   std::error_code& ec) noexcept {
    const auto ldr_base =
        detail::read_memory<defs::PEB_64>(detail::peb_address(ec), ec).Ldr;
    if (ec) {
        return 0;
    }

    const auto last_entry =
        ldr_base + offsetof(defs::PEB_LDR_DATA_64, InLoadOrderModuleList);

    defs::LDR_DATA_TABLE_ENTRY_64 head;
    head.InLoadOrderLinks.Flink =
        detail::read_memory<defs::PEB_LDR_DATA_64>(ldr_base, ec)
            .InLoadOrderModuleList.Flink;
    if (ec) {
        return 0;
    }

    do {
        detail::read_memory(head.InLoadOrderLinks.Flink, &head, sizeof(head),
                            ec);
        if (ec) {
            continue;
        }

        const auto other_module_name_len =
            head.BaseDllName.Length / sizeof(wchar_t);
        if (other_module_name_len != module_name.length()) {
            continue;
        }

        auto other_module_name =
            std::make_unique<wchar_t[]>(other_module_name_len);
        detail::read_memory(head.BaseDllName.Buffer, other_module_name.get(),
                            head.BaseDllName.Length, ec);
        if (ec) {
            continue;
        }

        if (std::equal(begin(module_name), end(module_name),
                       other_module_name.get())) {
            ec.clear();
            return head.DllBase;
        }
    } while (head.InLoadOrderLinks.Flink != last_entry);

    if (!ec) {
        ec = std::error_code(ERROR_MOD_NOT_FOUND, std::system_category());
    }

    return 0;
}

namespace detail {

inline IMAGE_EXPORT_DIRECTORY image_export_dir(std::uint64_t ntdll_base) {
    const auto e_lfanew = read_memory<IMAGE_DOS_HEADER>(ntdll_base).e_lfanew;

    const auto idd_virtual_addr =
        read_memory<IMAGE_NT_HEADERS64>(ntdll_base + e_lfanew)
            .OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT]
            .VirtualAddress;

    if (!idd_virtual_addr) {
        throw std::runtime_error(
            "IMAGE_EXPORT_DIRECTORY::VirtualAddress was 0");
    }

    return read_memory<IMAGE_EXPORT_DIRECTORY>(ntdll_base + idd_virtual_addr);
}

inline IMAGE_EXPORT_DIRECTORY image_export_dir(std::uint64_t ntdll_base,
                                               std::error_code& ec) noexcept {
    const auto e_lfanew =
        read_memory<IMAGE_DOS_HEADER>(ntdll_base, ec).e_lfanew;
    if (ec) {
        return {};
    }

    const auto idd_virtual_addr =
        read_memory<IMAGE_NT_HEADERS64>(ntdll_base + e_lfanew, ec)
            .OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT]
            .VirtualAddress;
    if (ec) {
        return {};
    }

    if (idd_virtual_addr == 0) {
        ec = std::error_code(ERROR_PROC_NOT_FOUND, std::system_category());
        return {};
    }

    return read_memory<IMAGE_EXPORT_DIRECTORY>(ntdll_base + idd_virtual_addr,
                                               ec);
}

inline std::uint64_t ldr_procedure_address() {
    const auto ntdll_base = module_handle("ntdll.dll");

    const auto ied = image_export_dir(ntdll_base);

    auto rva_table = std::make_unique<std::uint32_t[]>(ied.NumberOfFunctions);
    read_memory(ntdll_base + ied.AddressOfFunctions, rva_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfFunctions);

    auto ord_table = std::make_unique<std::uint16_t[]>(ied.NumberOfFunctions);
    read_memory(ntdll_base + ied.AddressOfNameOrdinals, ord_table.get(),
                sizeof(std::uint16_t) * ied.NumberOfFunctions);

    auto name_table = std::make_unique<std::uint32_t[]>(ied.NumberOfNames);
    read_memory(ntdll_base + ied.AddressOfNames, name_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfNames);

    const char to_find[] = "LdrGetProcedureAddress";
    char buffer[std::size(to_find)] = "";

    const std::size_t n =
        (ied.NumberOfFunctions > ied.NumberOfNames ? ied.NumberOfNames
                                                   : ied.NumberOfFunctions);
    for (std::size_t i = 0; i < n; ++i) {
        read_memory(ntdll_base + name_table[i], &buffer);

        if (std::equal(std::begin(to_find), std::end(to_find), buffer)) {
            return ntdll_base + rva_table[ord_table[i]];
        }
    }

    throw std::system_error(
        std::error_code(ERROR_PROC_NOT_FOUND, std::system_category()),
        "could find x64 LdrGetProcedureAddress()");
}

inline std::uint64_t ldr_procedure_address(std::error_code& ec) noexcept {
    const auto ntdll_base = module_handle("ntdll.dll", ec);
    if (ec) {
        return 0;
    }

    const auto ied = image_export_dir(ntdll_base, ec);
    if (ec) {
        return 0;
    }

    auto rva_table = std::make_unique<std::uint32_t[]>(ied.NumberOfFunctions);
    read_memory(ntdll_base + ied.AddressOfFunctions, rva_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfFunctions, ec);
    if (ec) {
        return 0;
    }

    auto ord_table = std::make_unique<std::uint16_t[]>(ied.NumberOfFunctions);
    read_memory(ntdll_base + ied.AddressOfNameOrdinals, ord_table.get(),
                sizeof(std::uint16_t) * ied.NumberOfFunctions, ec);
    if (ec) {
        return 0;
    }

    auto name_table = std::make_unique<std::uint32_t[]>(ied.NumberOfNames);
    read_memory(ntdll_base + ied.AddressOfNames, name_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfNames, ec);
    if (ec) {
        return 0;
    }

    const char to_find[] = "LdrGetProcedureAddress";
    char buffer[std::size(to_find)] = "";

    const std::size_t n = ied.NumberOfFunctions > ied.NumberOfNames
                              ? ied.NumberOfNames
                              : ied.NumberOfFunctions;

    for (std::size_t i = 0; i < n; ++i) {
        read_memory(ntdll_base + name_table[i], &buffer, sizeof(buffer), ec);
        if (ec) {
            continue;
        }

        if (std::equal(std::begin(to_find), std::end(to_find), buffer)) {
            ec.clear();
            return ntdll_base + rva_table[ord_table[i]];
        }
    }

    ec = std::error_code(ERROR_PROC_NOT_FOUND, std::system_category());
    return 0;
}

#pragma code_seg(push, r1, ".text")
__declspec(allocate(".text"))  //
inline static const std::uint8_t call_function_x64_shellcode[] = {
    // clang-format off

    0x55,             // push ebp
    0x89, 0xE5,       // mov ebp, esp

    0x83, 0xE4, 0xF0, // and esp, 0xFFFFFFF0

    // enter 64 bit mode
    0x6A, 0x33, 0xE8, 0x00, 0x00, 0x00, 0x00, 0x83, 0x04, 0x24, 0x05, 0xCB,

    0x67, 0x48, 0x8B, 0x4D, 16, // mov rcx, [ebp + 16]
    0x67, 0x48, 0x8B, 0x55, 24, // mov rdx, [ebp + 24]
    0x67, 0x4C, 0x8B, 0x45, 32, // mov r8,  [ebp + 32]
    0x67, 0x4C, 0x8B, 0x4D, 40, // mov r9,  [ebp + 40]

    0x67, 0x48, 0x8B, 0x45, 48, // mov rax, [ebp + 48] args count

    0xA8, 0x01,             // test al, 1
    0x75, 0x04,             // jne _no_adjust
    0x48, 0x83, 0xEC, 0x08, // sub rsp, 8
    // _no adjust:
        0x57,                                     // push rdi
        0x67, 0x48, 0x8B, 0x7D, 0x38,             // mov rdi, [ebp + 56]
        0x48, 0x85, 0xC0,                         // je _ls_e
        0x74, 0x16, 0x48, 0x8D, 0x7C, 0xC7, 0xF8, // lea rdi, [rdi+rax*8-8]
    // _ls:
        0x48, 0x85, 0xC0,       // test rax, rax
        0x74, 0x0C,             // je _ls_e
        0xFF, 0x37,             // push [rdi]
        0x48, 0x83, 0xEF, 0x08, // sub rdi, 8
        0x48, 0x83, 0xE8, 0x01, // sub rax, 1
        0xEB, 0xEF,             // jmp _ls
    // _ls_e:
    0x67, 0x8B, 0x7D, 0x40,       // mov edi, [ebp + 64]
    0x48, 0x83, 0xEC, 0x20,       // sub rsp, 0x20
    0x67, 0xFF, 0x55, 0x08,       // call [ebp + 0x8]
    0x67, 0x48, 0x89, 0x07,       // mov [edi], rax
    0x67, 0x48, 0x8B, 0x4D, 0x30, // mov rcx, [ebp+48]
    0x48, 0x8D, 0x64, 0xCC, 0x20, // lea rsp, [rsp+rcx*8+0x20]
    0x5F,                         // pop rdi

    // exit 64 bit mode
    0xE8, 0, 0, 0, 0, 0xC7, 0x44, 0x24, 4, 0x23, 0, 0, 0, 0x83, 4, 0x24, 0xD, 0xCB,

    0x66, 0x8C, 0xD8, // mov ax, ds
    0x8E, 0xD0,       // mov ss, eax

    0x89, 0xEC, // mov esp, ebp
    0x5D,       // pop ebp
    0xC3        // ret

    // clang-format on
};
#pragma code_seg(pop, r1)

template <class... Args>
inline std::uint64_t call_function_x64(std::uint64_t func,
                                       Args... args) noexcept {
    std::uint64_t arr_args[sizeof...(args) > 4 ? sizeof...(args) : 4] = {
        (std::uint64_t)(args)...};

    using my_fn_sig = void(__cdecl*)(
        std::uint64_t, std::uint64_t, std::uint64_t, std::uint64_t,
        std::uint64_t, std::uint64_t, std::uint64_t, std::uint32_t);

    std::uint64_t ret;
    reinterpret_cast<my_fn_sig>(&call_function_x64_shellcode)(
        func, arr_args[0], arr_args[1], arr_args[2], arr_args[3],
        sizeof...(Args) > 4 ? (sizeof...(Args) - 4) : 0,
        reinterpret_cast<std::uint64_t>(arr_args + 4),
        reinterpret_cast<std::uint32_t>(&ret));

    return ret;
}

inline std::uint64_t* find_import_ptr_64(HMODULE module,
                                         const char* module_name,
                                         const char* import_name) noexcept {
    IMAGE_DOS_HEADER* dos_header = reinterpret_cast<IMAGE_DOS_HEADER*>(module);
    IMAGE_NT_HEADERS64* nt_header = reinterpret_cast<IMAGE_NT_HEADERS64*>(
        reinterpret_cast<std::byte*>(dos_header) + dos_header->e_lfanew);

    if (!nt_header->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
             .VirtualAddress) {
        return nullptr;
    }

    std::byte* image_base = reinterpret_cast<std::byte*>(module);
    IMAGE_IMPORT_DESCRIPTOR* import_descriptor =
        reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR*>(
            image_base + nt_header->OptionalHeader
                             .DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
                             .VirtualAddress);

    while (import_descriptor->OriginalFirstThunk) {
        if (_stricmp(reinterpret_cast<const char*>(image_base +
                                                   import_descriptor->Name),
                     module_name) == 0) {
            IMAGE_THUNK_DATA64* original_first_think =
                reinterpret_cast<IMAGE_THUNK_DATA64*>(
                    image_base + import_descriptor->OriginalFirstThunk);
            IMAGE_THUNK_DATA64* first_think =
                reinterpret_cast<IMAGE_THUNK_DATA64*>(
                    image_base + import_descriptor->FirstThunk);

            while (std::uint64_t iter = original_first_think->u1.Function) {
                if (!IMAGE_SNAP_BY_ORDINAL64(iter)) {
                    if (reinterpret_cast<std::uint64_t>(import_name) &
                        ~0xFFFF) {
                        if (strcmp(
                                reinterpret_cast<const char*>(
                                    image_base + iter + sizeof(std::uint16_t)),
                                import_name) == 0) {
                            return &first_think->u1.Function;
                        }
                    }
                } else if ((reinterpret_cast<std::uint64_t>(import_name) &
                            ~0xFFFF) == 0 &&
                           IMAGE_ORDINAL64(iter) ==
                               IMAGE_ORDINAL64(reinterpret_cast<std::uint64_t>(
                                   import_name))) {
                    return &first_think->u1.Function;
                }

                original_first_think++;
                first_think++;
            }
        }

        import_descriptor++;
    }

    return nullptr;
}

// The native 64-bit code and data are placed in a separate section to make sure
// they're separated from the rest of the code. This has several benefits:
// * VirtualProtect can be used upon initialization without affecting running
//   code.
// * Better control of the order of the contents of the section.
// * Having the contents aligned to the page size is important for ARM64 due to
//   the compiler-generated IPC code.

#pragma code_seg(push, r1, ".text64")
#pragma warning(push)
#pragma warning(disable : 4200)  // Structures with zero length arrays.
__declspec(allocate(".text64"))  //
inline static struct {
    std::uint64_t original;
    std::uint8_t hook[];
} wow64_system_service_ex = {
    0xD4200000D4200000,
    {
        // clang-format off

        // Native ARM64 hook, compiled from the native_64_shellcode project.
        0xff, 0x43, 0x00, 0xd1, 0xfd, 0x7b, 0xbf, 0xa9, 0xfd, 0x03, 0x00, 0x91, 0xff, 0x83, 0x01, 0xd1,
        0x08, 0x00, 0x00, 0x90, 0x02, 0x21, 0x00, 0x91, 0x1f, 0xa8, 0x3f, 0x71, 0xe0, 0x00, 0x00, 0x54,
        0x48, 0x80, 0x5f, 0xf8, 0x00, 0x01, 0x3f, 0xd6, 0xff, 0x83, 0x01, 0x91, 0xfd, 0x7b, 0xc1, 0xa8,
        0xff, 0x43, 0x00, 0x91, 0xc0, 0x03, 0x5f, 0xd6, 0xf3, 0x3b, 0x00, 0xf9, 0x33, 0x00, 0x40, 0xb9,
        0x49, 0x20, 0x00, 0x58, 0x68, 0x02, 0x40, 0xf9, 0x1f, 0x01, 0x09, 0xeb, 0x20, 0x01, 0x00, 0x54,
        0x48, 0x80, 0x5f, 0xf8, 0x40, 0xfd, 0x81, 0x52, 0x00, 0x01, 0x3f, 0xd6, 0xf3, 0x3b, 0x40, 0xf9,
        0xff, 0x83, 0x01, 0x91, 0xfd, 0x7b, 0xc1, 0xa8, 0xff, 0x43, 0x00, 0x91, 0xc0, 0x03, 0x5f, 0xd6,
        0x69, 0xaa, 0x40, 0xa9, 0x00, 0x00, 0x80, 0x92, 0x68, 0x0e, 0x40, 0xf9, 0x6a, 0x00, 0x00, 0xb5,
        0x20, 0x01, 0x3f, 0xd6, 0xe8, 0x00, 0x00, 0x14, 0x5f, 0x05, 0x00, 0xf1, 0x81, 0x00, 0x00, 0x54,
        0x00, 0x01, 0x40, 0xf9, 0x20, 0x01, 0x3f, 0xd6, 0xe3, 0x00, 0x00, 0x14, 0x5f, 0x09, 0x00, 0xf1,
        0x81, 0x00, 0x00, 0x54, 0x00, 0x05, 0x40, 0xa9, 0x20, 0x01, 0x3f, 0xd6, 0xde, 0x00, 0x00, 0x14,
        0x5f, 0x0d, 0x00, 0xf1, 0xa1, 0x00, 0x00, 0x54, 0x02, 0x09, 0x40, 0xf9, 0x00, 0x05, 0x40, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0xd8, 0x00, 0x00, 0x14, 0x5f, 0x11, 0x00, 0xf1, 0xa1, 0x00, 0x00, 0x54,
        0x02, 0x0d, 0x41, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0x20, 0x01, 0x3f, 0xd6, 0xd2, 0x00, 0x00, 0x14,
        0x5f, 0x15, 0x00, 0xf1, 0xc1, 0x00, 0x00, 0x54, 0x04, 0x11, 0x40, 0xf9, 0x02, 0x0d, 0x41, 0xa9,
        0x00, 0x05, 0x40, 0xa9, 0x20, 0x01, 0x3f, 0xd6, 0xcb, 0x00, 0x00, 0x14, 0x5f, 0x19, 0x00, 0xf1,
        0xc1, 0x00, 0x00, 0x54, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0x00, 0x05, 0x40, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0xc4, 0x00, 0x00, 0x14, 0x5f, 0x1d, 0x00, 0xf1, 0xe1, 0x00, 0x00, 0x54,
        0x06, 0x19, 0x40, 0xf9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0x00, 0x05, 0x40, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0xbc, 0x00, 0x00, 0x14, 0x5f, 0x21, 0x00, 0xf1, 0xe1, 0x00, 0x00, 0x54,
        0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0x00, 0x05, 0x40, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0xb4, 0x00, 0x00, 0x14, 0x5f, 0x25, 0x00, 0xf1, 0x21, 0x01, 0x00, 0x54,
        0x0a, 0x21, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0x00, 0x05, 0x40, 0xa9, 0xea, 0x03, 0x00, 0xf9, 0x20, 0x01, 0x3f, 0xd6, 0xaa, 0x00, 0x00, 0x14,
        0x5f, 0x29, 0x00, 0xf1, 0x21, 0x01, 0x00, 0x54, 0x0a, 0x2d, 0x44, 0xa9, 0x06, 0x1d, 0x43, 0xa9,
        0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0xa0, 0x00, 0x00, 0x14, 0x5f, 0x2d, 0x00, 0xf1, 0x61, 0x01, 0x00, 0x54,
        0x0a, 0x29, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x0b, 0x00, 0xf9, 0x0a, 0x2d, 0x44, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x94, 0x00, 0x00, 0x14, 0x5f, 0x31, 0x00, 0xf1, 0x61, 0x01, 0x00, 0x54,
        0x0a, 0x2d, 0x45, 0xa9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x88, 0x00, 0x00, 0x14, 0x5f, 0x35, 0x00, 0xf1, 0xa1, 0x01, 0x00, 0x54,
        0x0a, 0x31, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x13, 0x00, 0xf9, 0x0a, 0x2d, 0x45, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x01, 0xa9,
        0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9, 0x20, 0x01, 0x3f, 0xd6, 0x7a, 0x00, 0x00, 0x14,
        0x5f, 0x39, 0x00, 0xf1, 0xa1, 0x01, 0x00, 0x54, 0x0a, 0x2d, 0x46, 0xa9, 0x06, 0x1d, 0x43, 0xa9,
        0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0xea, 0x2f, 0x02, 0xa9, 0x0a, 0x2d, 0x45, 0xa9,
        0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x6c, 0x00, 0x00, 0x14, 0x5f, 0x3d, 0x00, 0xf1, 0xe1, 0x01, 0x00, 0x54,
        0x0a, 0x39, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x1b, 0x00, 0xf9, 0x0a, 0x2d, 0x46, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x02, 0xa9,
        0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x5c, 0x00, 0x00, 0x14, 0x5f, 0x41, 0x00, 0xf1, 0xe1, 0x01, 0x00, 0x54,
        0x0a, 0x2d, 0x47, 0xa9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x2f, 0x03, 0xa9, 0x0a, 0x2d, 0x46, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x02, 0xa9,
        0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x4c, 0x00, 0x00, 0x14, 0x5f, 0x45, 0x00, 0xf1, 0x21, 0x02, 0x00, 0x54,
        0x0a, 0x41, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x23, 0x00, 0xf9, 0x0a, 0x2d, 0x47, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x03, 0xa9,
        0x0a, 0x2d, 0x46, 0xa9, 0xea, 0x2f, 0x02, 0xa9, 0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9,
        0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9, 0x20, 0x01, 0x3f, 0xd6, 0x3a, 0x00, 0x00, 0x14,
        0x5f, 0x49, 0x00, 0xf1, 0x21, 0x02, 0x00, 0x54, 0x0a, 0x2d, 0x48, 0xa9, 0x06, 0x1d, 0x43, 0xa9,
        0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9, 0xea, 0x2f, 0x04, 0xa9, 0x0a, 0x2d, 0x47, 0xa9,
        0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x03, 0xa9, 0x0a, 0x2d, 0x46, 0xa9, 0xea, 0x2f, 0x02, 0xa9,
        0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x28, 0x00, 0x00, 0x14, 0x5f, 0x4d, 0x00, 0xf1, 0x61, 0x02, 0x00, 0x54,
        0x0a, 0x49, 0x40, 0xf9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x2b, 0x00, 0xf9, 0x0a, 0x2d, 0x48, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x04, 0xa9,
        0x0a, 0x2d, 0x47, 0xa9, 0xea, 0x2f, 0x03, 0xa9, 0x0a, 0x2d, 0x46, 0xa9, 0xea, 0x2f, 0x02, 0xa9,
        0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x14, 0x00, 0x00, 0x14, 0x5f, 0x51, 0x00, 0xf1, 0x41, 0x02, 0x00, 0x54,
        0x0a, 0x2d, 0x49, 0xa9, 0x06, 0x1d, 0x43, 0xa9, 0x04, 0x15, 0x42, 0xa9, 0x02, 0x0d, 0x41, 0xa9,
        0xea, 0x2f, 0x05, 0xa9, 0x0a, 0x2d, 0x48, 0xa9, 0x00, 0x05, 0x40, 0xa9, 0xea, 0x2f, 0x04, 0xa9,
        0x0a, 0x2d, 0x47, 0xa9, 0xea, 0x2f, 0x03, 0xa9, 0x0a, 0x2d, 0x46, 0xa9, 0xea, 0x2f, 0x02, 0xa9,
        0x0a, 0x2d, 0x45, 0xa9, 0xea, 0x2f, 0x01, 0xa9, 0x0a, 0x2d, 0x44, 0xa9, 0xea, 0x2f, 0x00, 0xa9,
        0x20, 0x01, 0x3f, 0xd6, 0x28, 0x00, 0x80, 0xd2, 0x68, 0x02, 0x02, 0xa9, 0x00, 0x00, 0x80, 0x52,
        0xf3, 0x3b, 0x40, 0xf9, 0xff, 0x83, 0x01, 0x91, 0xfd, 0x7b, 0xc1, 0xa8, 0xff, 0x43, 0x00, 0x91,
        0xc0, 0x03, 0x5f, 0xd6, 0x1f, 0x20, 0x03, 0xd5, 0x23, 0x82, 0x90, 0x43, 0xbe, 0xe9, 0xe3, 0x89,

        // clang-format on
    },
};
#pragma warning(pop)
#pragma code_seg(pop, r1)

#pragma code_seg(push, r1, ".text")
__declspec(allocate(".text"))  //
inline static const std::uint8_t shellcode_syscall_via_fastcall[] = {
    // clang-format off
    0x89, 0xC8,        // mov eax, ecx
    0xFF, 0xD2,        // call edx
    0xC2, 0x04, 0x00,  // ret 4
    // clang-format on
};
#pragma code_seg(pop, r1)

struct CALL_FUNCTION_ARM64_DATA {
    std::error_code ec;
    void** pp_wow64_transition = nullptr;
    std::uint64_t* pp_wow64_system_service_ex = nullptr;
    std::uint64_t p_wow64_system_service_ex_original = 0;
    CRITICAL_SECTION critical_section;
    int call_count = 0;
};

inline CALL_FUNCTION_ARM64_DATA
make_initial_call_function_arm64_data() noexcept {
    std::error_code ec;
    void** pp_wow64_transition =
        native_ntdll_function<void**>("Wow64Transition", ec);
    if (ec) {
        return CALL_FUNCTION_ARM64_DATA{.ec = ec};
    }

    const auto wow64cpu_base = module_handle("xtajit.dll", ec);
    if (ec) {
        return CALL_FUNCTION_ARM64_DATA{.ec = ec};
    }

    // Looks like the module is always mapped in the 32-bit address space.
    //
    // "[...] the address of wow64cpu!KiFastSystemCall is held in the 32-bit TEB
    // (Thread Environment Block) via member WOW32Reserved"
    // https://cloud.google.com/blog/topics/threat-intelligence/wow64-subsystem-internals-and-hooking-techniques/
    if (wow64cpu_base > std::numeric_limits<std::uint32_t>::max()) {
        return CALL_FUNCTION_ARM64_DATA{
            .ec = std::error_code(ERROR_INDEX_OUT_OF_BOUNDS,
                                  std::system_category())};
    }

    std::uint64_t* pp_wow64_system_service_ex =
        find_import_ptr_64(reinterpret_cast<HMODULE>(wow64cpu_base),
                           "wow64.dll", "Wow64SystemServiceEx");
    if (!pp_wow64_system_service_ex) {
        return CALL_FUNCTION_ARM64_DATA{
            .ec =
                std::error_code(ERROR_PROC_NOT_FOUND, std::system_category())};
    }

    std::uint64_t p_wow64_system_service_ex_original =
        *pp_wow64_system_service_ex;

    DWORD dwOldProtect;
    VirtualProtect(&wow64_system_service_ex.original,
                   sizeof(wow64_system_service_ex.original), PAGE_READWRITE,
                   &dwOldProtect);
    wow64_system_service_ex.original = p_wow64_system_service_ex_original;
    VirtualProtect(&wow64_system_service_ex.original,
                   sizeof(wow64_system_service_ex.original), dwOldProtect,
                   &dwOldProtect);

    CRITICAL_SECTION critical_section;
    InitializeCriticalSection(&critical_section);

    return CALL_FUNCTION_ARM64_DATA{
        .pp_wow64_transition = pp_wow64_transition,
        .pp_wow64_system_service_ex = pp_wow64_system_service_ex,
        .p_wow64_system_service_ex_original =
            p_wow64_system_service_ex_original,
        .critical_section = critical_section,
    };
}

inline CALL_FUNCTION_ARM64_DATA* get_call_function_arm64_data() noexcept {
    WOW64PP_STATIC_INIT_ONCE_TRIVIAL(std::optional<CALL_FUNCTION_ARM64_DATA>,
                                     function_result,
                                     make_initial_call_function_arm64_data());
    return &*function_result;
}

template <class... Args>
inline std::uint64_t call_function_arm64(std::error_code& ec,
                                         std::uint64_t func,
                                         Args... args) noexcept {
    CALL_FUNCTION_ARM64_DATA* data = get_call_function_arm64_data();
    ec = data->ec;
    if (ec) {
        return 0xFFFFFFFFFFFFFFFF;
    }

    // Some unique SystemCallNumber (bits 1-12), zero ServiceTableIndex (13-16
    // bits), zero TurboThunkNumber (bits 17-21).
    std::uint32_t syscall_num = 0x0FEA;

    std::uint64_t arr_args[sizeof...(args) > 1 ? sizeof...(args) : 1] = {
        (std::uint64_t)(args)...};

    struct {
        std::uint64_t signature;
        std::uint64_t func;
        std::uint64_t args_count;
        std::uint64_t args;
        std::uint64_t called;
        std::uint64_t ret;
    } wow64_system_service_ex_param{
        .signature = 0x89E3E9BE43908223,
        .func = func,
        .args_count = sizeof...(Args),
        .args = reinterpret_cast<std::uint64_t>(arr_args),
    };

    void** pp_wow64_transition = data->pp_wow64_transition;
    std::uint64_t* pp_wow64_system_service_ex =
        data->pp_wow64_system_service_ex;
    std::uint64_t p_wow64_system_service_ex_original =
        data->p_wow64_system_service_ex_original;

    EnterCriticalSection(&data->critical_section);
    if (data->call_count == 0) {
        DWORD dwOldProtect;
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), PAGE_READWRITE,
                       &dwOldProtect);
        *pp_wow64_system_service_ex =
            reinterpret_cast<std::uint64_t>(wow64_system_service_ex.hook);
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), dwOldProtect,
                       &dwOldProtect);
    }
    data->call_count++;
    LeaveCriticalSection(&data->critical_section);

    using shellcode_syscall_via_fastcall_sig =
        void(__fastcall*)(std::uint32_t, void*, void*);

    reinterpret_cast<shellcode_syscall_via_fastcall_sig>(
        &shellcode_syscall_via_fastcall)(syscall_num, *pp_wow64_transition,
                                         &wow64_system_service_ex_param);

    EnterCriticalSection(&data->critical_section);
    data->call_count--;
    if (data->call_count == 0) {
        DWORD dwOldProtect;
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), PAGE_READWRITE,
                       &dwOldProtect);
        *pp_wow64_system_service_ex = p_wow64_system_service_ex_original;
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), dwOldProtect,
                       &dwOldProtect);
    }
    LeaveCriticalSection(&data->critical_section);

    if (!wow64_system_service_ex_param.called) {
        __fastfail(FAST_FAIL_FATAL_APP_EXIT);
    }

    return wow64_system_service_ex_param.ret;
}

inline std::uint16_t get_native_machine(std::error_code& ec) noexcept {
    using native_machine_result_t =
        std::expected<std::uint16_t, std::error_code>;
    WOW64PP_STATIC_INIT_ONCE_TRIVIAL(
        native_machine_result_t, native_machine,
        ([]() -> native_machine_result_t {
            using is_wow64_process2_t =
                BOOL(WINAPI*)(HANDLE hProcess, USHORT * pProcessMachine,
                              USHORT * pNativeMachine);

            is_wow64_process2_t is_wow64_process2 = nullptr;
            const auto kernel32_addr = GetModuleHandleW(L"kernel32.dll");
            if (kernel32_addr) {
                is_wow64_process2 = reinterpret_cast<is_wow64_process2_t>(
                    GetProcAddress(kernel32_addr, "IsWow64Process2"));
            }

            if (is_wow64_process2) {
                std::uint16_t process_machine = 0;
                std::uint16_t native_machine = 0;
                if (is_wow64_process2(GetCurrentProcess(), &process_machine,
                                      &native_machine)) {
                    return native_machine;
                }

                return std::unexpected(get_last_error());
            }

            BOOL is_wow64_process = FALSE;
            if (IsWow64Process(GetCurrentProcess(), &is_wow64_process)) {
                // Assume AMD64 if WOW64 process, not sure if it can be anything
                // else in this case.
                return is_wow64_process ? IMAGE_FILE_MACHINE_AMD64
                                        : IMAGE_FILE_MACHINE_I386;
            }

            return std::unexpected(get_last_error());
        }()));
    if (!native_machine.has_value()) {
        ec = native_machine.error();
        return IMAGE_FILE_MACHINE_UNKNOWN;
    }

    ec.clear();
    return *native_machine;
}

}  // namespace detail

/** \brief Calls a 64 bit function from 32 bit process
 *   \param[out] ec An error code that will be set in case of failure.
 *   \param[in] func The address of 64 bit function to be called.
 *   \param[in] args... The arguments for the function to be called.
 *   \return    The return value of the called function.
 *   \exception Does not throw.
 */
template <class... Args>
inline std::uint64_t call_function(std::error_code& ec,
                                   std::uint64_t func,
                                   Args... args) noexcept {
    auto native_machine = detail::get_native_machine(ec);
    if (ec) {
        return 0;
    }

    switch (native_machine) {
        case IMAGE_FILE_MACHINE_AMD64:
            return detail::call_function_x64(func, args...);
        case IMAGE_FILE_MACHINE_ARM64:
            return detail::call_function_arm64(ec, func, args...);
        default:
            ec = std::error_code(ERROR_NOT_SUPPORTED, std::system_category());
            return 0;
    }
}

/** \brief Calls a 64 bit function from 32 bit process
 *   \param[in] func The address of 64 bit function to be called.
 *   \param[in] args... The arguments for the function to be called.
 *   \return    The return value of the called function.
 *   \exception Throws std::system_error on failure.
 */
template <class... Args>
inline std::uint64_t call_function(std::uint64_t func, Args... args) {
    std::error_code ec;
    std::uint64_t result = call_function(ec, func, args...);
    if (ec) {
        detail::throw_error_code(ec);
    }

    return result;
}

/** \brief Use to pass pointers as arguments to call_function.
 *   \param[in] ptr The pointer.
 *   \return    The 64 bit integer argument.
 *   \exception Does not throw.
 */
template <typename T>
inline std::uint64_t ptr_to_uint64(T* ptr) noexcept {
    static_assert(sizeof(ptr) == sizeof(std::uint32_t),
                  "expecting 32-bit pointers");

    // Without the double casting, the pointer is sign extended, not zero
    // extended, which leads to invalid addresses with /LARGEADDRESSAWARE.
    return static_cast<std::uint64_t>(reinterpret_cast<std::uint32_t>(ptr));
}

/** \brief Use to pass handles as arguments to call_function.
 *   \param[in] ptr The handle.
 *   \return    The 64 bit integer argument.
 *   \exception Does not throw.
 */
inline std::uint64_t handle_to_uint64(HANDLE handle) noexcept {
    static_assert(sizeof(handle) == sizeof(std::int32_t),
                  "expecting 32-bit handles");

    // Sign-extension is required for pseudo handles such as the handle returned
    // from GetCurrentProcess().
    // "64-bit versions of Windows use 32-bit handles for interoperability [...]
    // it is safe to [...] sign-extend the handle (when passing it from 32-bit
    // to 64-bit)."
    // https://docs.microsoft.com/en-us/windows/win32/winprog64/interprocess-communication
    return static_cast<std::uint64_t>(reinterpret_cast<std::int32_t>(handle));
}

namespace detail {

inline std::uint64_t get_cached_ldr_procedure_address(
    std::error_code& ec) noexcept {
    using ldr_result_t = std::expected<std::uint64_t, std::error_code>;
    WOW64PP_STATIC_INIT_ONCE_TRIVIAL(
        ldr_result_t, ldr_result, ([]() -> ldr_result_t {
            std::error_code ec;
            const auto ldr_result = ldr_procedure_address(ec);
            if (ec)
                return std::unexpected(ec);
            return ldr_result;
        }()));
    if (!ldr_result.has_value()) {
        ec = ldr_result.error();
        return 0;
    }

    ec.clear();
    return *ldr_result;
}

}  // namespace detail

/** \brief An equivalent of winapi GetProcAddress function.
 *   \param[in] hmodule The handle to the module in which to search for the
                procedure.
 *   \param[in] procedure_name The name of the procedure to be searched for.
 *   \return    The address of the exported function or variable.
 *   \exception Throws std::system_error on failure.
 */
inline std::uint64_t import(std::uint64_t hmodule,
                            std::string_view procedure_name) {
    std::error_code ec;
    const auto ldr_procedure_address_base =
        detail::get_cached_ldr_procedure_address(ec);
    if (ec) {
        detail::throw_error_code(ec);
    }

    defs::UNICODE_STRING_64 unicode_fun_name{
        .Length = static_cast<std::uint16_t>(procedure_name.size()),
        .MaximumLength = static_cast<std::uint16_t>(procedure_name.size()),
        .Buffer = ptr_to_uint64(procedure_name.data()),
    };

    std::uint64_t ret = 0;
    auto fn_ret =
        call_function(ldr_procedure_address_base, hmodule,
                      ptr_to_uint64(&unicode_fun_name), 0, ptr_to_uint64(&ret));
    if (fn_ret) {
        throw std::system_error(
            std::error_code(static_cast<int>(fn_ret), std::system_category()),
            "call_function(ldr_procedure_address_base...) failed");
    }

    return ret;
}

/** \brief An equivalent of winapi GetProcAddress function.
 *   \param[in]  hmodule The handle to the module in which to search for the
                 procedure.
 *   \param[in]  procedure_name The name of the procedure to be searched for.
 *   \param[out] ec An error code that will be set in case of failure
 *   \return     The address of the exported function or variable.
 *   \exception  Does not throw.
 */
inline std::uint64_t import(std::uint64_t hmodule,
                            std::string_view procedure_name,
                            std::error_code& ec) noexcept {
    const auto ldr_procedure_address_base =
        detail::get_cached_ldr_procedure_address(ec);
    if (ec) {
        return 0;
    }

    defs::UNICODE_STRING_64 unicode_fun_name{
        .Length = static_cast<std::uint16_t>(procedure_name.size()),
        .MaximumLength = static_cast<std::uint16_t>(procedure_name.size()),
        .Buffer = ptr_to_uint64(procedure_name.data()),
    };

    std::uint64_t ret = 0;
    auto fn_ret =
        call_function(ec, ldr_procedure_address_base, hmodule,
                      ptr_to_uint64(&unicode_fun_name), 0, ptr_to_uint64(&ret));
    if (ec) {
        return 0;
    }

    if (fn_ret) {
        ec = std::error_code(static_cast<int>(fn_ret), std::system_category());
        return 0;
    }

    return ret;
}

}  // namespace wow64pp

#endif  // WOW64PP_HPP
