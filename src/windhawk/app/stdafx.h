#pragma once

// Change these values to use different versions
#define WINVER _WIN32_WINNT_WIN7
#define _WIN32_WINNT _WIN32_WINNT_WIN7
#define _WIN32_IE _WIN32_IE_IE80
#define _RICHEDIT_VER 0x0500

#define WIN32_LEAN_AND_MEAN  // Exclude rarely-used stuff from Windows headers
#define NOMINMAX

//////////////////////////////////////////////////////////////////////////
// WTL

#define _WTL_NO_CSTRING
#define _WTL_NO_WTYPES
#define _WTL_NO_UNION_CLASSES
#define _ATL_CSTRING_EXPLICIT_CONSTRUCTORS

#include <atlbase.h>
#include <atlfile.h>
#include <atlstr.h>
#include <atltypes.h>
#include <atlutil.h>

#include <atlapp.h>
extern CAppModule _Module;

#include <atlwin.h>

#include <atlcrack.h>
#include <atlctrls.h>
#include <atlctrlx.h>
// #include <atldlgs.h>
#include <atlframe.h>
// #include <atlmisc.h>

//////////////////////////////////////////////////////////////////////////
// Windows

#include <comutil.h>
#include <intsafe.h>
#include <objbase.h>
#include <sddl.h>
#include <shobjidl.h>
#include <taskschd.h>
#include <tlhelp32.h>
#include <userenv.h>
#include <winevt.h>
#include <winhttp.h>
#include <wtsapi32.h>

//////////////////////////////////////////////////////////////////////////
// STL

#include <array>
#include <atomic>
#include <exception>
#include <filesystem>
#include <fstream>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <ranges>
#include <string>
#include <variant>

//////////////////////////////////////////////////////////////////////////
// Libraries

// https://github.com/nlohmann/json#implicit-conversions
#define JSON_USE_IMPLICIT_CONVERSIONS 0

#include <winhttpwrappers/WinHTTPWrappers.h>
#include <nlohmann/json.hpp>

#include <wil/stl.h>  // must be included before other wil includes

#include <wil/com.h>
#include <wil/resource.h>
#include <wil/result.h>
#include <wil/safecast.h>
#include <wil/win32_helpers.h>
