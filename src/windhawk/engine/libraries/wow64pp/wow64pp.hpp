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

// Check _MSVC_LANG on MSVC, whose __cplusplus stays at 199711L without
// /Zc:__cplusplus.
#if (defined(_MSVC_LANG) ? _MSVC_LANG : __cplusplus) < 202302L
#error wow64pp requires C++23 or later
#endif

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <algorithm>  // std::equal
#include <cstddef>    // offsetof, std::byte
#include <cstdint>    // std::uint*_t
#include <cstring>    // memcpy
#include <expected>
#include <iterator>  // std::begin, std::end, std::size
#include <limits>    // std::numeric_limits
#include <memory>
#include <optional>
#include <stdexcept>  // std::runtime_error
#include <string_view>
#include <system_error>
#include <type_traits>  // std::is_trivially_copyable_v

// The following macros are used to initialize static variables once in a
// thread-safe manner while avoiding TLS, which is what MSVC uses for static
// variables.
#ifdef WOW64PP_AVOID_TLS
#include <mutex>        // std::call_once, std::once_flag
#include <new>          // placement new, std::launder
#include <type_traits>  // std::is_trivially_destructible_v
// Similar to:
// static T var_name(...);
#define WOW64PP_STATIC_INIT_ONCE(T, var_name, ...)                         \
    T* var_name;                                                           \
    do {                                                                   \
        static alignas(T) char static_init_once_storage_[sizeof(T)];       \
        static std::once_flag static_init_once_flag_;                      \
        std::call_once(static_init_once_flag_, []() {                      \
            ::new (static_init_once_storage_) T(__VA_ARGS__);              \
            if constexpr (!std::is_trivially_destructible_v<T>) {          \
                std::atexit([]() {                                         \
                    std::launder(                                          \
                        reinterpret_cast<T*>(static_init_once_storage_))   \
                        ->~T();                                            \
                });                                                        \
            }                                                              \
        });                                                                \
        var_name =                                                         \
            std::launder(reinterpret_cast<T*>(static_init_once_storage_)); \
    } while (0)

// Similar to:
// static T var_name = initializer;
// Like WOW64PP_STATIC_INIT_ONCE, but enforces that T is trivially destructible.
#define WOW64PP_STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer) \
    static constinit T var_name;                                   \
    do {                                                           \
        static_assert(std::is_trivially_destructible_v<T>);        \
        static std::once_flag static_init_once_flag_;              \
        std::call_once(static_init_once_flag_,                     \
                       []() { var_name = initializer; });          \
    } while (0)
#else
#define WOW64PP_STATIC_INIT_ONCE(T, var_name, ...)       \
    T* var_name;                                         \
    do {                                                 \
        static T static_init_once_var_ = T(__VA_ARGS__); \
        var_name = &static_init_once_var_;               \
    } while (0)
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

// Widens a 32-bit pointer to 64 bits for use by 64-bit code. Casting through
// uint32_t zero-extends the pointer; a direct cast to uint64_t sign extends it,
// which produces an invalid address with /LARGEADDRESSAWARE.
template <typename T>
inline std::uint64_t ptr_to_uint64(T* ptr) noexcept {
    static_assert(sizeof(ptr) == sizeof(std::uint32_t),
                  "expecting 32-bit pointers");
    return static_cast<std::uint64_t>(reinterpret_cast<std::uint32_t>(ptr));
}

[[noreturn]] inline void throw_error_code(const std::error_code& ec) {
    throw std::system_error(ec);
}

[[noreturn]] inline void throw_error_code(const std::error_code& ec,
                                          const char* message) {
    throw std::system_error(ec, message);
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

inline HANDLE self_handle() {
    std::error_code ec;
    const auto h = self_handle(ec);
    if (ec) {
        throw_error_code(ec, "failed to get a handle to the current process");
    }

    return h;
}

struct handle_closer {
    void operator()(HANDLE handle) const noexcept { CloseHandle(handle); }
};

using unique_handle = std::unique_ptr<void, handle_closer>;

inline HANDLE get_cached_self_handle(std::error_code& ec) noexcept {
    using handle_result_t = std::expected<unique_handle, std::error_code>;
    WOW64PP_STATIC_INIT_ONCE(handle_result_t, handle_result,
                             ([]() -> handle_result_t {
                                 std::error_code ec;
                                 const HANDLE h = self_handle(ec);
                                 if (ec)
                                     return std::unexpected(ec);
                                 return unique_handle(h);
                             }()));
    if (!handle_result->has_value()) {
        ec = handle_result->error();
        return nullptr;
    }

    ec.clear();
    return (*handle_result)->get();
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

template <typename F>
inline F native_ntdll_function(const char* name) {
    std::error_code ec;
    const auto f = native_ntdll_function<F>(name, ec);
    if (ec) {
        throw_error_code(ec, "failed to get address of ntdll function");
    }

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
        ec = std::error_code(hres, std::system_category());
        return 0;
    }

    return pbi.PebBaseAddress;
}

inline std::uint64_t peb_address() {
    std::error_code ec;
    const auto address = peb_address(ec);
    if (ec) {
        throw_error_code(ec, "failed to get the x64 PEB address");
    }

    return address;
}

inline std::uint64_t get_cached_peb_address(std::error_code& ec) noexcept {
    using peb_result_t = std::expected<std::uint64_t, std::error_code>;
    WOW64PP_STATIC_INIT_ONCE_TRIVIAL(peb_result_t, peb_result,
                                     ([]() -> peb_result_t {
                                         std::error_code ec;
                                         const auto address = peb_address(ec);
                                         if (ec)
                                             return std::unexpected(ec);
                                         return address;
                                     }()));
    if (!peb_result.has_value()) {
        ec = peb_result.error();
        return 0;
    }

    ec.clear();
    return *peb_result;
}

template <typename P>
inline void read_memory(std::uint64_t address,
                        P* buffer,
                        std::size_t size,
                        std::error_code& ec) noexcept {
    if (size == 0) {
        return;
    }

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

    const HANDLE h_self = get_cached_self_handle(ec);
    if (ec) {
        return;
    }

    const auto hres =
        NtWow64ReadVirtualMemory64(h_self, address, buffer, size, nullptr);
    if (hres < 0) {
        ec = std::error_code(hres, std::system_category());
    }
}

template <typename P>
inline void read_memory(std::uint64_t address,
                        P* buffer,
                        std::size_t size = sizeof(P)) {
    std::error_code ec;
    read_memory(address, buffer, size, ec);
    if (ec) {
        throw_error_code(ec, "failed to read memory");
    }
}

template <typename T>
inline T read_memory(std::uint64_t address, std::error_code& ec) noexcept {
    static_assert(std::is_trivially_copyable_v<T>);
    T value{};
    read_memory(address, &value, sizeof(T), ec);
    return value;
}

template <typename T>
inline T read_memory(std::uint64_t address) {
    std::error_code ec;
    const auto value = read_memory<T>(address, ec);
    if (ec) {
        throw_error_code(ec, "failed to read memory");
    }

    return value;
}

template <typename T>
inline std::unique_ptr<T[]> make_unique_nothrow(std::size_t count) noexcept {
    try {
        return std::make_unique<T[]>(count);
    } catch (...) {
        return nullptr;
    }
}

// Walks the 64-bit loader's InLoadOrderModuleList, invoking fn on each module
// entry until fn returns true or the list ends. A read that fails stops the
// walk with ec set, so a broken link can't spin the loop. Returns whether fn
// stopped the walk, i.e. found what it was looking for.
template <typename Fn>
inline bool for_each_module_64(Fn&& fn, std::error_code& ec) noexcept {
    const auto peb = get_cached_peb_address(ec);
    if (ec) {
        return false;
    }

    const auto ldr_base = read_memory<defs::PEB_64>(peb, ec).Ldr;
    if (ec) {
        return false;
    }

    const auto last_entry =
        ldr_base + offsetof(defs::PEB_LDR_DATA_64, InLoadOrderModuleList);

    auto entry_addr = read_memory<defs::PEB_LDR_DATA_64>(ldr_base, ec)
                          .InLoadOrderModuleList.Flink;
    if (ec) {
        return false;
    }

    while (entry_addr != last_entry) {
        defs::LDR_DATA_TABLE_ENTRY_64 head;
        read_memory(entry_addr, &head, sizeof(head), ec);
        if (ec) {
            return false;
        }

        if (fn(head)) {
            return true;
        }

        entry_addr = head.InLoadOrderLinks.Flink;
    }

    return false;
}

}  // namespace detail

/** \brief An equivalent of winapi GetModuleHandle function.
 *   \param[in] module_name The name of the module to get the handle of.
 *   \param[out] ec An error code that will be set in case of failure
 *   \return    The handle to the module as a 64 bit integer.
 *   \exception Does not throw.
 */
inline std::uint64_t module_handle(std::string_view module_name,
                                   std::error_code& ec) noexcept {
    std::uint64_t module_base = 0;
    std::error_code last_read_ec;

    const bool found = detail::for_each_module_64(
        [&](const defs::LDR_DATA_TABLE_ENTRY_64& entry) {
            const auto other_module_name_len =
                entry.BaseDllName.Length / sizeof(wchar_t);
            if (other_module_name_len != module_name.length()) {
                return false;
            }

            auto other_module_name =
                detail::make_unique_nothrow<wchar_t>(other_module_name_len);
            if (!other_module_name) {
                ec = std::error_code(ERROR_NOT_ENOUGH_MEMORY,
                                     std::system_category());
                return true;
            }

            std::error_code read_ec;
            detail::read_memory(
                entry.BaseDllName.Buffer, other_module_name.get(),
                other_module_name_len * sizeof(wchar_t), read_ec);
            if (read_ec) {
                last_read_ec = read_ec;
                return false;
            }

            auto names_equal = [](char a, wchar_t b) {
                auto fold = [](wchar_t c) -> wchar_t {
                    return (c >= L'A' && c <= L'Z') ? c - L'A' + L'a' : c;
                };
                // Cast through unsigned char so a high byte isn't
                // sign-extended.
                return fold(static_cast<unsigned char>(a)) == fold(b);
            };

            if (std::equal(std::begin(module_name), std::end(module_name),
                           other_module_name.get(), names_equal)) {
                module_base = entry.DllBase;
                return true;
            }

            return false;
        },
        ec);

    if (ec) {
        return 0;
    }

    if (found) {
        return module_base;
    }

    ec = last_read_ec
             ? last_read_ec
             : std::error_code(ERROR_MOD_NOT_FOUND, std::system_category());
    return 0;
}

/** \brief An equivalent of winapi GetModuleHandle function.
 *   \param[in] module_name The name of the module to get the handle of.
 *   \return    The handle to the module as a 64 bit integer.
 *   \exception Throws std::system_error on failure.
 */
inline std::uint64_t module_handle(std::string_view module_name) {
    std::error_code ec;
    const auto module_base = module_handle(module_name, ec);
    if (ec) {
        detail::throw_error_code(ec, "Could not get x64 module handle");
    }

    return module_base;
}

namespace detail {

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

inline IMAGE_EXPORT_DIRECTORY image_export_dir(std::uint64_t ntdll_base) {
    std::error_code ec;
    const auto ied = image_export_dir(ntdll_base, ec);
    if (ec) {
        throw_error_code(ec, "failed to read x64 ntdll export directory");
    }

    return ied;
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

    auto rva_table = make_unique_nothrow<std::uint32_t>(ied.NumberOfFunctions);
    if (!rva_table) {
        ec = std::error_code(ERROR_NOT_ENOUGH_MEMORY, std::system_category());
        return 0;
    }
    read_memory(ntdll_base + ied.AddressOfFunctions, rva_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfFunctions, ec);
    if (ec) {
        return 0;
    }

    auto ord_table = make_unique_nothrow<std::uint16_t>(ied.NumberOfNames);
    if (!ord_table) {
        ec = std::error_code(ERROR_NOT_ENOUGH_MEMORY, std::system_category());
        return 0;
    }
    read_memory(ntdll_base + ied.AddressOfNameOrdinals, ord_table.get(),
                sizeof(std::uint16_t) * ied.NumberOfNames, ec);
    if (ec) {
        return 0;
    }

    auto name_table = make_unique_nothrow<std::uint32_t>(ied.NumberOfNames);
    if (!name_table) {
        ec = std::error_code(ERROR_NOT_ENOUGH_MEMORY, std::system_category());
        return 0;
    }
    read_memory(ntdll_base + ied.AddressOfNames, name_table.get(),
                sizeof(std::uint32_t) * ied.NumberOfNames, ec);
    if (ec) {
        return 0;
    }

    const char to_find[] = "LdrGetProcedureAddress";
    char buffer[std::size(to_find)] = "";

    for (std::size_t i = 0; i < ied.NumberOfNames; ++i) {
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

inline std::uint64_t ldr_procedure_address() {
    std::error_code ec;
    const auto address = ldr_procedure_address(ec);
    if (ec) {
        throw_error_code(ec, "could not find x64 LdrGetProcedureAddress()");
    }

    return address;
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

// Calling a shellcode array indirectly fails fast under CFG: it is data, so the
// image's function table does not list it as a valid target. A thunk is a real
// function, so it is listed.
__declspec(naked) inline void call_function_x64_shellcode_thunk() {
    __asm {
        mov eax, offset call_function_x64_shellcode
        jmp eax
    }
}

template <class... Args>
inline std::uint64_t call_function_x64(std::uint64_t func,
                                       Args... args) noexcept {
    std::uint64_t arr_args[sizeof...(args) > 4 ? sizeof...(args) : 4] = {
        (std::uint64_t)(args)...};

    using my_fn_sig = void(__cdecl*)(
        std::uint64_t, std::uint64_t, std::uint64_t, std::uint64_t,
        std::uint64_t, std::uint64_t, std::uint64_t, std::uint32_t);

    std::uint64_t ret;
    reinterpret_cast<my_fn_sig>(&call_function_x64_shellcode_thunk)(
        func, arr_args[0], arr_args[1], arr_args[2], arr_args[3],
        sizeof...(Args) > 4 ? (sizeof...(Args) - 4) : 0,
        ptr_to_uint64(arr_args + 4), reinterpret_cast<std::uint32_t>(&ret));

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
            IMAGE_THUNK_DATA64* original_first_thunk =
                reinterpret_cast<IMAGE_THUNK_DATA64*>(
                    image_base + import_descriptor->OriginalFirstThunk);
            IMAGE_THUNK_DATA64* first_thunk =
                reinterpret_cast<IMAGE_THUNK_DATA64*>(
                    image_base + import_descriptor->FirstThunk);

            while (std::uint64_t iter = original_first_thunk->u1.Function) {
                if (!IMAGE_SNAP_BY_ORDINAL64(iter)) {
                    if (reinterpret_cast<std::uint64_t>(import_name) &
                        ~0xFFFF) {
                        if (strcmp(
                                reinterpret_cast<const char*>(
                                    image_base + iter + sizeof(std::uint16_t)),
                                import_name) == 0) {
                            return &first_thunk->u1.Function;
                        }
                    }
                } else if ((reinterpret_cast<std::uint64_t>(import_name) &
                            ~0xFFFF) == 0 &&
                           IMAGE_ORDINAL64(iter) ==
                               IMAGE_ORDINAL64(reinterpret_cast<std::uint64_t>(
                                   import_name))) {
                    return &first_thunk->u1.Function;
                }

                original_first_thunk++;
                first_thunk++;
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
struct wow64_system_service_ex_t {
    std::uint64_t original;
    std::uint8_t hook[];
};
// External linkage (a plain inline variable, not inline static) so every
// translation unit shares one instance. The hook pointer swap and the original
// dispatcher address written here must act on a single object.
__declspec(allocate(".text64"))  //
inline wow64_system_service_ex_t wow64_system_service_ex = {
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

__declspec(naked) inline void __fastcall
shellcode_syscall_via_fastcall_thunk() {
    __asm {
        mov eax, offset shellcode_syscall_via_fastcall
        jmp eax
    }
}

struct CALL_FUNCTION_ARM64_DATA {
    std::error_code ec;
    void** pp_wow64_transition = nullptr;
    std::uint64_t* pp_wow64_system_service_ex = nullptr;
    std::uint64_t p_wow64_system_service_ex_original = 0;
    SRWLOCK lock = SRWLOCK_INIT;
    int call_count = 0;
};

// find_import_ptr_64 walks an image's headers with raw pointers, so a module
// that isn't a 64-bit PE has to be kept away from it.
inline bool is_pe64_image(std::uint64_t module_base) noexcept {
    auto* base = reinterpret_cast<const std::byte*>(
        static_cast<std::uintptr_t>(module_base));

    // The loader always commits at least a page for an image's headers, which
    // bounds how far e_lfanew may point.
    constexpr LONG max_headers_size = 0x1000;

    auto* dos_header = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if (dos_header->e_magic != IMAGE_DOS_SIGNATURE ||
        dos_header->e_lfanew < static_cast<LONG>(sizeof(IMAGE_DOS_HEADER)) ||
        dos_header->e_lfanew >
            max_headers_size - static_cast<LONG>(sizeof(IMAGE_NT_HEADERS64))) {
        return false;
    }

    auto* nt_header = reinterpret_cast<const IMAGE_NT_HEADERS64*>(
        base + dos_header->e_lfanew);
    return nt_header->Signature == IMAGE_NT_SIGNATURE &&
           nt_header->OptionalHeader.Magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC &&
           nt_header->OptionalHeader.NumberOfRvaAndSizes >
               IMAGE_DIRECTORY_ENTRY_IMPORT;
}

// Finds the import table entry, in the emulation CPU module, that the syscall
// hook below swaps out.
//
// The module is identified by what it is rather than by its name. It was
// xtajit.dll for years, but Windows 11 24H2 added variants of it (xtajitf.dll,
// xtajitse.dll, and xtajitte.dll on 26H1) and picks between them through a
// feature-staging flag, so which one a process gets can differ between machines
// on the same build and can change on one machine without an update. The
// registry value naming the CPU module keeps saying xtajit.dll either way, so
// it doesn't answer the question. Two properties do hold for every variant:
//
// * It is mapped in the 32-bit address space, since 32-bit code has to reach
//   it. "[...] the address of wow64cpu!KiFastSystemCall is held in the 32-bit
//   TEB (Thread Environment Block) via member WOW32Reserved"
//   https://cloud.google.com/blog/topics/threat-intelligence/wow64-subsystem-internals-and-hooking-techniques/
// * It imports the dispatcher this needs, which no other module in a WOW64
//   process does.
inline std::uint64_t* find_wow64_system_service_ex_ptr(
    std::error_code& ec) noexcept {
    // The process image is mapped low too and is the one other entry that can
    // be, so it's skipped rather than parsed as a 64-bit image.
    const auto process_image_base = static_cast<std::uint64_t>(
        reinterpret_cast<std::uint32_t>(GetModuleHandleW(nullptr)));

    std::uint64_t* import_ptr = nullptr;
    bool candidate_seen = false;

    const bool found = for_each_module_64(
        [&](const defs::LDR_DATA_TABLE_ENTRY_64& entry) {
            const auto module_base = entry.DllBase;
            if (!module_base ||
                module_base > std::numeric_limits<std::uint32_t>::max() ||
                module_base == process_image_base ||
                !is_pe64_image(module_base)) {
                return false;
            }

            candidate_seen = true;

            import_ptr = find_import_ptr_64(
                reinterpret_cast<HMODULE>(
                    static_cast<std::uintptr_t>(module_base)),
                "wow64.dll", "Wow64SystemServiceEx");
            return import_ptr != nullptr;
        },
        ec);

    if (found) {
        return import_ptr;
    }

    if (ec) {
        return nullptr;
    }

    // Telling the two apart keeps the failure diagnosable: either nothing that
    // could be the CPU module was mapped, or one was and it dispatches syscalls
    // some other way.
    ec = std::error_code(
        candidate_seen ? ERROR_PROC_NOT_FOUND : ERROR_MOD_NOT_FOUND,
        std::system_category());
    return nullptr;
}

inline CALL_FUNCTION_ARM64_DATA
make_initial_call_function_arm64_data() noexcept {
    std::error_code ec;
    void** pp_wow64_transition =
        native_ntdll_function<void**>("Wow64Transition", ec);
    if (ec) {
        return CALL_FUNCTION_ARM64_DATA{.ec = ec};
    }

    std::uint64_t* pp_wow64_system_service_ex =
        find_wow64_system_service_ex_ptr(ec);
    if (ec) {
        return CALL_FUNCTION_ARM64_DATA{.ec = ec};
    }

    std::uint64_t p_wow64_system_service_ex_original =
        *pp_wow64_system_service_ex;

    DWORD dwOldProtect;
    if (!VirtualProtect(&wow64_system_service_ex.original,
                        sizeof(wow64_system_service_ex.original),
                        PAGE_READWRITE, &dwOldProtect)) {
        return CALL_FUNCTION_ARM64_DATA{.ec = get_last_error()};
    }
    wow64_system_service_ex.original = p_wow64_system_service_ex_original;
    VirtualProtect(&wow64_system_service_ex.original,
                   sizeof(wow64_system_service_ex.original), dwOldProtect,
                   &dwOldProtect);

    return CALL_FUNCTION_ARM64_DATA{
        .pp_wow64_transition = pp_wow64_transition,
        .pp_wow64_system_service_ex = pp_wow64_system_service_ex,
        .p_wow64_system_service_ex_original =
            p_wow64_system_service_ex_original,
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
        .args = ptr_to_uint64(arr_args),
    };

    void** pp_wow64_transition = data->pp_wow64_transition;
    std::uint64_t* pp_wow64_system_service_ex =
        data->pp_wow64_system_service_ex;
    std::uint64_t p_wow64_system_service_ex_original =
        data->p_wow64_system_service_ex_original;

    AcquireSRWLockExclusive(&data->lock);
    if (data->call_count == 0) {
        DWORD dwOldProtect;
        if (!VirtualProtect(pp_wow64_system_service_ex,
                            sizeof(*pp_wow64_system_service_ex), PAGE_READWRITE,
                            &dwOldProtect)) {
            ec = get_last_error();
            ReleaseSRWLockExclusive(&data->lock);
            return 0xFFFFFFFFFFFFFFFF;
        }
        *pp_wow64_system_service_ex =
            ptr_to_uint64(wow64_system_service_ex.hook);
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), dwOldProtect,
                       &dwOldProtect);
    }
    data->call_count++;
    ReleaseSRWLockExclusive(&data->lock);

    using shellcode_syscall_via_fastcall_sig =
        void(__fastcall*)(std::uint32_t, void*, void*);

    reinterpret_cast<shellcode_syscall_via_fastcall_sig>(
        &shellcode_syscall_via_fastcall_thunk)(
        syscall_num, *pp_wow64_transition, &wow64_system_service_ex_param);

    AcquireSRWLockExclusive(&data->lock);
    data->call_count--;
    if (data->call_count == 0) {
        DWORD dwOldProtect;
        if (!VirtualProtect(pp_wow64_system_service_ex,
                            sizeof(*pp_wow64_system_service_ex), PAGE_READWRITE,
                            &dwOldProtect)) {
            // The dispatcher pointer stays redirected to our hook. Once this
            // module unloads the pointer dangles and any WOW64 syscall in the
            // process crashes, so there is no safe way to continue.
            __fastfail(FAST_FAIL_FATAL_APP_EXIT);
        }
        *pp_wow64_system_service_ex = p_wow64_system_service_ex_original;
        VirtualProtect(pp_wow64_system_service_ex,
                       sizeof(*pp_wow64_system_service_ex), dwOldProtect,
                       &dwOldProtect);
    }
    ReleaseSRWLockExclusive(&data->lock);

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
    return detail::ptr_to_uint64(ptr);
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

    // LdrGetProcedureAddress takes an ANSI_STRING, whose layout matches
    // UNICODE_STRING_64.
    defs::UNICODE_STRING_64 ansi_fun_name{
        .Length = static_cast<std::uint16_t>(procedure_name.size()),
        .MaximumLength = static_cast<std::uint16_t>(procedure_name.size()),
        .Buffer = ptr_to_uint64(procedure_name.data()),
    };

    std::uint64_t ret = 0;
    auto fn_ret =
        call_function(ec, ldr_procedure_address_base, hmodule,
                      ptr_to_uint64(&ansi_fun_name), 0, ptr_to_uint64(&ret));
    if (ec) {
        return 0;
    }

    if (fn_ret) {
        ec = std::error_code(static_cast<int>(fn_ret), std::system_category());
        return 0;
    }

    return ret;
}

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
    const auto ret = import(hmodule, procedure_name, ec);
    if (ec) {
        detail::throw_error_code(ec, "failed to get x64 procedure address");
    }

    return ret;
}

}  // namespace wow64pp

#endif  // WOW64PP_HPP
