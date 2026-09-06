#pragma once

// The following macros are used to initialize static variables once in a
// thread-safe manner while avoiding TLS, which is what MSVC uses for static
// variables.

// Similar to:
// static T var_name(...);
#define STATIC_INIT_ONCE(T, var_name, ...)                                 \
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
#define STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer)  \
    static constinit T var_name;                            \
    do {                                                    \
        static_assert(std::is_trivially_destructible_v<T>); \
        static std::once_flag static_init_once_flag_;       \
        std::call_once(static_init_once_flag_,              \
                       []() { var_name = initializer; });   \
    } while (0)

// Similar to:
// static T ptr = (T)GetProcAddress(GetModuleHandle(module_name), proc_name);
//
// The lookups below are hooked by mods, and a hook is free to call back into
// the engine and reach this macro on the same thread. The resolving thread is
// recorded so that such a call resolves to null instead of blocking on a once
// flag its own thread is inside. Only that nested call gets null, which every
// caller handles already, and the resolution it interrupted serves the calls
// that follow.
#define GET_PROC_ADDRESS_ONCE(T, ptr, module_name, proc_name)                 \
    static T ptr;                                                             \
    do {                                                                      \
        static_assert(std::is_trivially_destructible_v<T>);                   \
        static constinit std::atomic<DWORD> get_proc_address_once_owner_{0};  \
        static std::once_flag get_proc_address_once_flag_;                    \
        if (get_proc_address_once_owner_.load(std::memory_order_relaxed) ==   \
            GetCurrentThreadId()) {                                           \
            break;                                                            \
        }                                                                     \
        std::call_once(get_proc_address_once_flag_, []() {                    \
            get_proc_address_once_owner_.store(GetCurrentThreadId(),          \
                                               std::memory_order_relaxed);    \
            HMODULE get_proc_address_once_module_ =                           \
                GetModuleHandle(module_name);                                 \
            if (get_proc_address_once_module_) {                              \
                ptr = (T)GetProcAddress(get_proc_address_once_module_,        \
                                        proc_name);                           \
            }                                                                 \
            get_proc_address_once_owner_.store(0, std::memory_order_relaxed); \
        });                                                                   \
    } while (0)

// Similar to:
// static T ptr =
//     (T)GetProcAddress(LoadLibraryEx(module_name, nullptr, flags), proc_name);
// The module reference is kept once the function is resolved, and the module
// stays loaded for as long as the process lives. Releasing it would have to
// happen while this dll is being unloaded, i.e. from DllMain, where unloading
// another module and its dependencies is unsafe.
//
// The nested call is refused for the reason given above GET_PROC_ADDRESS_ONCE.
// Here it also keeps the lookup from recursing: loading the module runs the
// hook which led back here, which would load it again.
#define LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(T, ptr, module_name, flags,        \
                                           proc_name)                         \
    static T ptr;                                                             \
    do {                                                                      \
        static_assert(std::is_trivially_destructible_v<T>);                   \
        static constinit std::atomic<DWORD> get_proc_address_once_owner_{0};  \
        static std::once_flag get_proc_address_once_flag_;                    \
        if (get_proc_address_once_owner_.load(std::memory_order_relaxed) ==   \
            GetCurrentThreadId()) {                                           \
            break;                                                            \
        }                                                                     \
        std::call_once(get_proc_address_once_flag_, []() {                    \
            get_proc_address_once_owner_.store(GetCurrentThreadId(),          \
                                               std::memory_order_relaxed);    \
            HMODULE get_proc_address_once_module_ =                           \
                LoadLibraryEx(module_name, nullptr, flags);                   \
            if (get_proc_address_once_module_) {                              \
                ptr = (T)GetProcAddress(get_proc_address_once_module_,        \
                                        proc_name);                           \
                if (!ptr) {                                                   \
                    FreeLibrary(get_proc_address_once_module_);               \
                }                                                             \
            }                                                                 \
            get_proc_address_once_owner_.store(0, std::memory_order_relaxed); \
        });                                                                   \
    } while (0)
