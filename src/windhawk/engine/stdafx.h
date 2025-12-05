#pragma once

#define WINVER _WIN32_WINNT_WIN7
#define _WIN32_WINNT _WIN32_WINNT_WIN7
#define _WIN32_IE _WIN32_IE_IE80
#define NTDDI_VERSION NTDDI_WIN7

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <dbghelp.h>
#include <ntsecapi.h>
#include <sddl.h>
#include <shlobj.h>
#include <tlhelp32.h>
#include <winhttp.h>

// STL

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <filesystem>
#include <functional>
#include <memory>
#include <mutex>
#include <new>
#include <optional>
#include <ranges>
#include <stdexcept>
#include <string>
#include <tuple>
#include <type_traits>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <variant>
#include <vector>

using namespace std::string_view_literals;

// Libraries

#include <dia/dia2.h>
#include <dia/diacreate.h>

#include <thread-call-stack-scanner/ThreadsCallStackWaitForRegions.h>

#define TLS_NO_DEBUG
#include <ThreadLocal.h>

#include <wil/stl.h>  // must be included before other wil includes

#include <wil/com.h>
#include <wil/resource.h>
#include <wil/result.h>
#include <wil/safecast.h>
#include <wil/win32_helpers.h>

#ifdef _M_IX86
#include <wow64pp/wow64pp.hpp>
#endif

// Disasm engine

#if defined(_M_IX86) || defined(_M_X64)
#include <Zydis/Zydis.h>
#elif defined(_M_ARM64)
#include <binaryninja-arm64-disassembler/decompose_and_disassemble.h>
#endif

// Hooking engine

#if 0
#define WH_HOOKING_ENGINE_MINHOOK
#include <MinHook/include/MinHook.h>
#elif 1
#define WH_HOOKING_ENGINE_MINHOOK
#define WH_HOOKING_ENGINE_MINHOOK_DETOURS
#include <MinHook-Detours/MinHook.h>
#else
#error "This option is only for testing"
#endif
