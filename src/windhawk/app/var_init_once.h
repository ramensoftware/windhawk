#pragma once

// The engine mirrors these macros with a TLS-free, thread-safe implementation
// because MSVC initializes function-local statics via TLS, which the injection
// engine can't use. The app has no such restriction, so here each macro is a
// thin wrapper over a plain function-local static. Each one declares the
// variable with the same name and type as its engine counterpart, so code
// shared between the app and the engine compiles against whichever
// var_init_once.h its project provides.

// Similar to:
// static T var_name(...);
// Declares a T*, as the engine does, so the variable is used as *var_name. The
// initializer is spelled T(...) rather than passed to a constructor call so
// that an empty __VA_ARGS__ value-initializes instead of declaring a function.
#define STATIC_INIT_ONCE(T, var_name, ...)               \
    T* var_name;                                         \
    do {                                                 \
        static T static_init_once_var_ = T(__VA_ARGS__); \
        var_name = &static_init_once_var_;               \
    } while (0)

// static T var_name = initializer;
#define STATIC_INIT_ONCE_TRIVIAL(T, var_name, initializer) \
    static T var_name = initializer

// static T ptr = (T)GetProcAddress(GetModuleHandle(module_name), proc_name);
#define GET_PROC_ADDRESS_ONCE(T, ptr, module_name, proc_name) \
    static T ptr = (T)GetProcAddress(GetModuleHandle(module_name), proc_name)

// static T ptr =
//     (T)GetProcAddress(LoadLibraryEx(module_name, nullptr, flags), proc_name);
#define LOAD_LIBRARY_GET_PROC_ADDRESS_ONCE(T, ptr, module_name, flags, \
                                           proc_name)                  \
    static T ptr = (T)GetProcAddress(                                  \
        LoadLibraryEx(module_name, nullptr, flags), proc_name)
