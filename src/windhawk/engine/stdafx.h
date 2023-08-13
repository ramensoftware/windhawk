#pragma once

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <dbghelp.h>
#include <ntsecapi.h>
#include <sddl.h>
#include <shlobj.h>
#include <tlhelp32.h>

// STL

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <filesystem>
#include <functional>
#include <memory>
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

// Libraries

#include <dia/dia2.h>
#include <dia/diacreate.h>

#include <MinHook/MinHook.h>

#define TLS_NO_DEBUG
#include <ThreadLocal.h>

#include <wil/stl.h>  // must be included before other wil includes

#include <wil/com.h>
#include <wil/resource.h>
#include <wil/result.h>
#include <wil/safecast.h>
#include <wil/win32_helpers.h>

#include <wow64ext/wow64ext.h>

#include <Zydis/Zydis.h>
