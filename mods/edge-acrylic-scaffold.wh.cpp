// ==WindhawkMod==
// @id              edge-acrylic-scaffold
// @name            Edge Acrylic (discovery scaffold)
// @description     Experimental: logs Edge's native View classes and clears chosen backgrounds. Requires Translucent Windows.
// @version         0.8
// @author          scaffold based on the approach of Brave Acrylic by bardelyne
// @license         MIT
// @architecture    x86-64
// @include         msedge.exe
// @compilerOptions -lshell32
// ==/WindhawkMod==

// ==WindhawkModReadme==
/*
# Edge Acrylic (discovery scaffold)

Untested starting point. It does three things:

1. Waits for `msedge.dll` and resolves a few Chromium symbols with Windhawk's `HookSymbols`.
2. **Discovery mode:** logs every distinct `views::View` class name that paints a background.
3. Clears the background of the classes you list in settings (comma-separated), so Translucent
   Windows can show through.

## How to use

1. Enable Translucent Windows for `msedge.exe`.
2. Enable this mod with Discovery mode on. Leave the class list empty.
3. Enable Windhawk debug logging, restart Edge fully (close all windows and background Edge).
4. Check the log: it prints which symbols resolved and every class name seen.
5. Put class names for the toolbar, tab strip, etc. into the class list. Settings changes apply
   to new paints, no rebuild needed. Restart Edge if the window stays opaque.

## Limits

- Edge UI that is WebUI (HTML) cannot be cleared by this.
- If the "transparent window" symbol does not resolve, cleared areas may render black or opaque.
- Symbols come from Microsoft's symbol server through Windhawk; msedge.dll is large, so the first
  run can take a while. If no symbols are available for your build, nothing is hooked.
- No menu/popup blur, no address-bar blur, no caption-button workaround in this scaffold.
*/
// ==/WindhawkModReadme==

// ==WindhawkModSettings==
/*
- discoveryMode: true
  $name: Discovery mode
  $description: Log each distinct View class name that paints a background (capped at 500).
- clearClasses: ""
  $name: View classes to make transparent
  $description: "Comma-separated exact class names from the log, e.g. ToolbarView,TopContainerView"
- clearAll: false
  $name: TEST - clear every View background
  $description: Pipeline test. Clears all backgrounds that go through the hooked paint function. The UI may look broken while on.
- tweakTabs: true
  $name: Make tabs translucent
  $description: Lowers the opacity of Edge's tab background color. Turn off if tabs look wrong.
- tabInactiveOpacity: 0
  $name: Unselected tab opacity (%)
  $description: 0 = fully transparent, 100 = unchanged.
- tabActiveOpacity: 100
  $name: Selected tab opacity (%)
  $description: 0 = fully transparent, 100 = unchanged. Check the log if selected and unselected look swapped.
- hideNativeCaptionButtons: true
  $name: Hide duplicate Windows buttons
  $description: "Removes the real Windows system-menu style so Windows does not draw its own minimize/maximize/close buttons under Edge's. Can affect Alt+Space and taskbar menus. Applies immediately."
- dumpSymbols: false
  $name: Dump matching symbol names to the log
  $description: Background scan of msedge.dll symbols for the class-name and transparent-window functions. Takes effect on next Edge start. Turn off once done.
*/
// ==/WindhawkModSettings==

#include <windows.h>
#include <windhawk_api.h>
#include <windhawk_utils.h>
#include <shellapi.h>

#include <atomic>
#include <cstring>
#include <string>
#include <string_view>
#include <unordered_set>
#include <vector>

#ifndef _M_X64
BOOL Wh_ModInit() {
    return FALSE;
}
#else

namespace {

struct ClassName {
    const char *data;
    size_t length;
};

using PaintFn = void (*)(void *, void *);
using DrawColorFn = void (*)(void *, unsigned int, int);
using GetClassFn = ClassName *(*)(const void *, ClassName *);
using TransparentFn = bool (*)(const void *);

PaintFn g_origPaint = nullptr;
TransparentFn g_origTransparent = nullptr;
DrawColorFn g_drawColor = nullptr;
GetClassFn g_getClass = nullptr;

std::atomic<bool> g_ready{false};
std::atomic<bool> g_discovery{true};
std::atomic<bool> g_anyClear{false};
std::atomic<bool> g_clearAll{false};
std::atomic<bool> g_dump{false};
std::atomic<bool> g_hideCaption{true};
std::atomic<bool> g_tweakTabs{true};
std::atomic<int> g_tabInactivePct{0};
std::atomic<int> g_tabActivePct{100};
std::atomic<unsigned> g_paintCalls{0};
HANDLE g_dumpThread = nullptr;
std::atomic<bool> g_dumpStop{false};

SRWLOCK g_lock = SRWLOCK_INIT;
std::vector<std::string> g_clear;
std::unordered_set<std::string> g_seen;

SRWLOCK g_initLock = SRWLOCK_INIT;
bool g_attempted = false, g_stopping = false, g_applyAfterInit = false, g_browser = false;

using LoadLibraryExW_t = decltype(&LoadLibraryExW);
LoadLibraryExW_t g_origLoadLibraryExW = nullptr;

void LoadSettings() {
    g_discovery.store(Wh_GetIntSetting(L"discoveryMode") != 0);
    g_clearAll.store(Wh_GetIntSetting(L"clearAll") != 0);
    g_dump.store(Wh_GetIntSetting(L"dumpSymbols") != 0);
    g_hideCaption.store(Wh_GetIntSetting(L"hideNativeCaptionButtons") != 0);
    g_tweakTabs.store(Wh_GetIntSetting(L"tweakTabs") != 0);
    g_tabInactivePct.store(Wh_GetIntSetting(L"tabInactiveOpacity"));
    g_tabActivePct.store(Wh_GetIntSetting(L"tabActiveOpacity"));

    PCWSTR raw = Wh_GetStringSetting(L"clearClasses");
    std::vector<std::string> list;
    std::string current;
    for (const wchar_t *p = raw;; ++p) {
        if (*p == L',' || !*p) {
            if (!current.empty())
                list.push_back(current);
            current.clear();
            if (!*p)
                break;
        } else if (*p < 128 && *p != L' ') {
            current.push_back(static_cast<char>(*p));
        }
    }
    Wh_FreeStringSetting(raw);

    const size_t count = list.size();
    g_anyClear.store(!list.empty());
    AcquireSRWLockExclusive(&g_lock);
    g_clear = std::move(list);
    ReleaseSRWLockExclusive(&g_lock);
    Wh_Log(L"Settings loaded: clearAll=%d, %u class name(s) in clear list",
           g_clearAll.load() ? 1 : 0, static_cast<unsigned>(count));
}

bool ShouldClear(std::string_view name) {
    bool match = false;
    AcquireSRWLockShared(&g_lock);
    for (const auto &entry : g_clear)
        if (entry == name) {
            match = true;
            break;
        }
    ReleaseSRWLockShared(&g_lock);
    return match;
}

void LogOnce(std::string_view name) {
    bool isNew = false;
    AcquireSRWLockExclusive(&g_lock);
    if (g_seen.size() < 500)
        isNew = g_seen.emplace(name).second;
    ReleaseSRWLockExclusive(&g_lock);
    if (isNew) {
        std::wstring wide(name.begin(), name.end());
        Wh_Log(L"View class: %s", wide.c_str());
    }
}

void LogClearOnce(std::string_view name) {
    bool isNew = false;
    AcquireSRWLockExclusive(&g_lock);
    if (g_seen.size() < 600)
        isNew = g_seen.emplace("clear:" + std::string(name)).second;
    ReleaseSRWLockExclusive(&g_lock);
    if (isNew) {
        std::wstring wide(name.begin(), name.end());
        Wh_Log(L"Clearing class: %s", wide.c_str());
    }
}

void OnPaintBackground_Hook(void *view, void *canvas) {
    if (view && canvas && g_ready.load(std::memory_order_acquire)) {
        if (g_paintCalls.fetch_add(1, std::memory_order_relaxed) == 0)
            Wh_Log(L"OnPaintBackground hook is live");
        if (g_clearAll.load(std::memory_order_relaxed)) {
            g_drawColor(canvas, 0, 0 /* SkBlendMode::kClear */);
            return;
        }
    }
    if (view && canvas && g_ready.load(std::memory_order_acquire) && g_getClass) {
        ClassName name{};
        g_getClass(view, &name);
        if (name.data && name.length && name.length < 128) {
            const std::string_view sv(name.data, name.length);
            if (g_discovery.load(std::memory_order_relaxed))
                LogOnce(sv);
            if (g_anyClear.load(std::memory_order_relaxed) && ShouldClear(sv)) {
                LogClearOnce(sv);
                g_drawColor(canvas, 0, 0 /* SkBlendMode::kClear */);
                return;
            }
        }
    }
    g_origPaint(view, canvas);
}

bool ShouldWindowContentsBeTransparent_Hook(const void *host) {
    const bool force =
        g_ready.load(std::memory_order_acquire) &&
        (g_anyClear.load(std::memory_order_relaxed) || g_clearAll.load(std::memory_order_relaxed));
    const bool original = g_origTransparent ? g_origTransparent(host) : false;
    static std::atomic<unsigned> calls{0};
    if (calls.fetch_add(1) < 8)
        Wh_Log(L"ShouldWindowContentsBeTransparent called: original=%d forced=%d", original ? 1 : 0,
               force ? 1 : 0);
    return force ? true : original;
}

bool InsideModule(HMODULE module, void *address) {
    MEMORY_BASIC_INFORMATION info{};
    return address && VirtualQuery(address, &info, sizeof(info)) &&
           info.AllocationBase == reinterpret_cast<void *>(module);
}

template <typename Function>
bool Hook(void *address, Function callback, Function *original) {
    return address && WindhawkUtils::SetFunctionHook(reinterpret_cast<Function>(address), callback,
                                                     original);
}

// Enumerates every symbol in msedge.dll on a background thread and logs the ones we care about.
DWORD WINAPI DumpSymbolsThread(void *param) {
    HMODULE module = static_cast<HMODULE>(param);
    // Looking for the code that paints context menus and the main menu.
    static const wchar_t *filters[] = {L"MenuScrollViewContainer::", L"MenuItemView::OnPaint",
                                       L"EdgeMenu"};
    unsigned counts[ARRAYSIZE(filters)]{};
    WH_FIND_SYMBOL found{};
    HANDLE search = Wh_FindFirstSymbol(module, nullptr, &found);
    if (!search) {
        Wh_Log(L"Symbol enumeration unavailable");
        return 0;
    }
    Wh_Log(L"Symbol enumeration started");
    unsigned total = 0;
    do {
        if (g_dumpStop.load())
            break;
        ++total;
        if (!found.symbol)
            continue;
        for (unsigned i = 0; i < ARRAYSIZE(filters); ++i)
            if (counts[i] < 80 && wcsstr(found.symbol, filters[i])) {
                ++counts[i];
                Wh_Log(L"Match: %s", found.symbol);
            }
    } while (Wh_FindNextSymbol(search, &found));
    Wh_FindCloseSymbol(search);
    Wh_Log(L"Symbol enumeration finished (%u symbols)", total);
    return 0;
}

// EdgeTabStyleViews::EdgeGetTabBackgroundColor(TabSelectionState, bool) const -> SkColor (ARGB).
using TabColorFn = unsigned int (*)(const void *, int, bool);
TabColorFn g_origTabColor = nullptr;

unsigned int TabBackgroundColor_Hook(const void *self, int state, bool flag) {
    const unsigned int color = g_origTabColor(self, state, flag);
    static std::atomic<unsigned> calls{0};
    if (calls.fetch_add(1) < 12)
        Wh_Log(L"EdgeGetTabBackgroundColor: state=%d flag=%d color=%08X", state, flag ? 1 : 0, color);
    if (!g_tweakTabs.load(std::memory_order_relaxed) || !g_ready.load(std::memory_order_acquire))
        return color;
    int pct = state == 0 ? g_tabActivePct.load() : g_tabInactivePct.load();
    if (pct >= 100)
        return color;
    if (pct < 0)
        pct = 0;
    const unsigned alpha = ((color >> 24) * static_cast<unsigned>(pct) + 50) / 100;
    return (color & 0x00ffffffu) | (alpha << 24);
}

bool ResolveAndHook(HMODULE module) {
    void *a[5]{};

    // Class-name and namespace spellings may differ in Edge builds. Alternatives are
    // listed per symbol; whichever resolves is used. Check the log to see which did.
    WindhawkUtils::SYMBOL_HOOK hooks[] = {
        {{L"protected: virtual void __cdecl views::View::OnPaintBackground(class gfx::Canvas *)"},
         &a[0], nullptr, true},
        {{L"public: void __cdecl gfx::Canvas::DrawColor(unsigned int,enum SkBlendMode)"},
         &a[1], nullptr, true},
        {{L"public: class std::__Cr::basic_string_view<char,struct std::__Cr::char_traits<char> > __cdecl views::View::GetClassNameW(void)const ",
          L"public: class std::__Cr::basic_string_view<char,struct std::__Cr::char_traits<char> > __cdecl views::View::GetClassNameW(void)const",
          L"public: class std::__Cr::basic_string_view<char,struct std::__Cr::char_traits<char> > __cdecl views::View::GetClassName(void)const"},
         &a[2], nullptr, true},
        // Edge's PDB lists const methods with a trailing space after "const".
        // Only the browser-window class is hooked, not the shared base class.
        {{L"private: virtual bool __cdecl BrowserDesktopWindowTreeHostWin::ShouldWindowContentsBeTransparent(void)const ",
          L"public: virtual bool __cdecl BrowserDesktopWindowTreeHostWin::ShouldWindowContentsBeTransparent(void)const ",
          L"private: virtual bool __cdecl BrowserDesktopWindowTreeHostWin::ShouldWindowContentsBeTransparent(void)const",
          L"public: virtual bool __cdecl BrowserDesktopWindowTreeHostWin::ShouldWindowContentsBeTransparent(void)const"},
         &a[3], nullptr, true},
        {{L"private: virtual unsigned int __cdecl EdgeTabStyleViews::EdgeGetTabBackgroundColor(enum TabStyle::TabSelectionState,bool)const ",
          L"private: virtual unsigned int __cdecl EdgeTabStyleViews::EdgeGetTabBackgroundColor(enum TabStyle::TabSelectionState,bool)const"},
         &a[4], nullptr, true},
    };

    if (!WindhawkUtils::HookSymbols(module, hooks, ARRAYSIZE(hooks))) {
        Wh_Log(L"HookSymbols failed (no symbols for this msedge.dll build?)");
        return false;
    }

    const wchar_t *labels[] = {L"View::OnPaintBackground", L"Canvas::DrawColor",
                               L"View::GetClassName", L"ShouldWindowContentsBeTransparent",
                               L"EdgeGetTabBackgroundColor"};
    for (unsigned i = 0; i < 5; ++i)
        Wh_Log(L"Symbol %s: %s", labels[i], a[i] ? L"resolved" : L"NOT found");

    for (unsigned i = 0; i < 5; ++i)
        if (a[i] && !InsideModule(module, a[i])) {
            Wh_Log(L"Symbol %u resolved outside msedge.dll; aborting", i);
            return false;
        }

    if (g_dump.load() && !g_dumpThread)
        g_dumpThread = CreateThread(nullptr, 0, DumpSymbolsThread, module, 0, nullptr);

    // Minimum: paint + DrawColor. Without GetClassName we can only use the clearAll test.
    if (!a[0] || !a[1])
        return false;
    if (!a[2])
        Wh_Log(L"GetClassName not found: class names unavailable, only clearAll works");

    g_drawColor = reinterpret_cast<DrawColorFn>(a[1]);
    g_getClass = reinterpret_cast<GetClassFn>(a[2]);

    if (!Hook(a[0], OnPaintBackground_Hook, &g_origPaint))
        return false;
    if (a[3] && !Hook(a[3], ShouldWindowContentsBeTransparent_Hook, &g_origTransparent))
        Wh_Log(L"Could not hook the transparent-window function");
    if (a[4] && !Hook(a[4], TabBackgroundColor_Hook, &g_origTabColor))
        Wh_Log(L"Could not hook the tab background color function");
    return true;
}

bool TryInstall(HMODULE module, bool applyNow) {
    if (!module)
        return false;
    AcquireSRWLockExclusive(&g_initLock);
    if (g_attempted || g_stopping) {
        ReleaseSRWLockExclusive(&g_initLock);
        return false;
    }
    g_attempted = true;
    bool installed = ResolveAndHook(module);
    if (installed) {
        if (applyNow) {
            installed = Wh_ApplyHookOperations() != FALSE;
            g_ready.store(installed, std::memory_order_release);
        } else {
            g_applyAfterInit = true;
        }
    }
    ReleaseSRWLockExclusive(&g_initLock);
    return installed;
}

// ---- Native caption button workaround (adapted from the Brave Acrylic approach) ----
// With a transparent window, Windows draws its own caption buttons beneath Edge's. Hide only
// WS_SYSMENU for the real style; Chromium still reads its original logical style.
constexpr wchar_t kNativeCaptionProperty[] = L"Windhawk.EdgeAcrylic.NativeCaption";
std::atomic<bool> g_captionFixActive{false};
std::atomic<bool> g_captionStopping{false};
std::atomic<bool> g_captionPropertyUsed{false};
bool g_captionHooksInstalled = false;
decltype(&CreateWindowExW) pOrigCreateWindowExW = nullptr;
decltype(&GetWindowLongPtrW) pOrigGetWindowLongPtrW = nullptr;
decltype(&SetWindowLongPtrW) pOrigSetWindowLongPtrW = nullptr;
decltype(&GetWindowLongW) pOrigGetWindowLongW = nullptr;
decltype(&SetWindowLongW) pOrigSetWindowLongW = nullptr;
thread_local HWND g_captionStyleWrite = nullptr;

bool TransparencyWanted() {
    return g_ready.load(std::memory_order_acquire) &&
           (g_anyClear.load(std::memory_order_relaxed) || g_clearAll.load(std::memory_order_relaxed));
}

bool IsBrowserCaptionFrame(HWND window, LONG_PTR style) {
    constexpr LONG_PTR required = WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
    if (!window || (style & required) != required || (style & WS_CHILD))
        return false;
    DWORD process = 0;
    GetWindowThreadProcessId(window, &process);
    if (process != GetCurrentProcessId() || GetAncestor(window, GA_ROOT) != window ||
        GetWindow(window, GW_OWNER))
        return false;
    const auto exStyle = pOrigGetWindowLongPtrW(window, GWL_EXSTYLE);
    if (exStyle & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_DLGMODALFRAME))
        return false;
    wchar_t name[32]{};
    return GetClassNameW(window, name, ARRAYSIZE(name)) && !wcscmp(name, L"Chrome_WidgetWin_1");
}

void RefreshNativeCaption(HWND window) {
    SetWindowPos(window, nullptr, 0, 0, 0, 0,
                 SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
}

void RestoreNativeCaption(HWND window) {
    if (!GetPropW(window, kNativeCaptionProperty))
        return;
    RemovePropW(window, kNativeCaptionProperty);
    const auto style = GetWindowLongPtrW(window, GWL_STYLE);
    SetWindowLongPtrW(window, GWL_STYLE, style | WS_SYSMENU);
    RefreshNativeCaption(window);
}

template <typename Value>
Value ReadCaptionStyle(HWND window, int index, Value(WINAPI *original)(HWND, int)) {
    if (index != GWL_STYLE ||
        (!g_captionFixActive.load(std::memory_order_acquire) &&
         !g_captionPropertyUsed.load(std::memory_order_relaxed)))
        return original(window, index);
    auto value = original(window, index);
    const DWORD error = GetLastError();
    if (GetPropW(window, kNativeCaptionProperty))
        value |= WS_SYSMENU;
    SetLastError(error);
    return value;
}

LONG_PTR WINAPI GetWindowLongPtrW_Hook(HWND window, int index) {
    return ReadCaptionStyle(window, index, pOrigGetWindowLongPtrW);
}

LONG WINAPI GetWindowLongW_Hook(HWND window, int index) {
    return ReadCaptionStyle(window, index, pOrigGetWindowLongW);
}

template <typename Value>
Value WriteCaptionStyle(HWND window, int index, Value value,
                        Value(WINAPI *original)(HWND, int, Value)) {
    if (index != GWL_STYLE || g_captionStyleWrite == window ||
        (!g_captionFixActive.load(std::memory_order_acquire) &&
         !g_captionPropertyUsed.load(std::memory_order_relaxed)))
        return original(window, index, value);
    const bool owned = GetPropW(window, kNativeCaptionProperty) != nullptr;
    const bool hide = g_captionFixActive.load(std::memory_order_acquire) && (value & WS_SYSMENU) &&
                      (owned || IsBrowserCaptionFrame(window, value));
    bool marked = owned;
    if (hide && !owned) {
        g_captionPropertyUsed.store(true, std::memory_order_relaxed);
        marked = SetPropW(window, kNativeCaptionProperty, reinterpret_cast<HANDLE>(1)) != FALSE;
    } else if (!hide && owned) {
        RemovePropW(window, kNativeCaptionProperty);
        marked = false;
    }
    const HWND previousWrite = g_captionStyleWrite;
    g_captionStyleWrite = window;
    SetLastError(ERROR_SUCCESS);
    const auto previous =
        original(window, index, hide && marked ? value & ~static_cast<Value>(WS_SYSMENU) : value);
    const DWORD error = GetLastError();
    g_captionStyleWrite = previousWrite;
    if (!previous && error) {
        if (owned)
            SetPropW(window, kNativeCaptionProperty, reinterpret_cast<HANDLE>(1));
        else
            RemovePropW(window, kNativeCaptionProperty);
    } else if (hide && marked && !g_captionFixActive.load(std::memory_order_acquire)) {
        RestoreNativeCaption(window);
    }
    SetLastError(error);
    return previous && owned ? previous | WS_SYSMENU : previous;
}

LONG_PTR WINAPI SetWindowLongPtrW_Hook(HWND window, int index, LONG_PTR value) {
    return WriteCaptionStyle(window, index, value, pOrigSetWindowLongPtrW);
}

LONG WINAPI SetWindowLongW_Hook(HWND window, int index, LONG value) {
    return WriteCaptionStyle(window, index, value, pOrigSetWindowLongW);
}

void ApplyNativeCaption(HWND window) {
    if (!g_captionFixActive.load(std::memory_order_acquire) ||
        GetPropW(window, kNativeCaptionProperty))
        return;
    const auto style = pOrigGetWindowLongPtrW(window, GWL_STYLE);
    if (!(style & WS_SYSMENU) || !IsBrowserCaptionFrame(window, style))
        return;
    SetWindowLongPtrW_Hook(window, GWL_STYLE, style);
    if (GetPropW(window, kNativeCaptionProperty))
        RefreshNativeCaption(window);
}

HWND WINAPI CreateWindowExW_Hook(DWORD exStyle, LPCWSTR className, LPCWSTR name, DWORD style, int x,
                                 int y, int width, int height, HWND parent, HMENU menu,
                                 HINSTANCE instance, LPVOID param) {
    const auto window = pOrigCreateWindowExW(exStyle, className, name, style, x, y, width, height,
                                             parent, menu, instance, param);
    const DWORD error = GetLastError();
    if (window)
        ApplyNativeCaption(window);
    SetLastError(error);
    return window;
}

BOOL CALLBACK SyncNativeCaptionWindow(HWND window, LPARAM) {
    DWORD process = 0;
    GetWindowThreadProcessId(window, &process);
    if (process == GetCurrentProcessId()) {
        if (g_captionFixActive.load(std::memory_order_acquire))
            ApplyNativeCaption(window);
        else
            RestoreNativeCaption(window);
    }
    return TRUE;
}

void SyncNativeCaptions() {
    const bool active = g_captionHooksInstalled && !g_captionStopping.load(std::memory_order_acquire) &&
                        TransparencyWanted() && g_hideCaption.load(std::memory_order_relaxed);
    g_captionFixActive.store(active, std::memory_order_release);
    if (g_captionHooksInstalled)
        EnumWindows(SyncNativeCaptionWindow, 0);
}

void InstallNativeCaptionHooks() {
    g_captionHooksInstalled =
        WindhawkUtils::SetFunctionHook(CreateWindowExW, CreateWindowExW_Hook,
                                       &pOrigCreateWindowExW) &&
        WindhawkUtils::SetFunctionHook(GetWindowLongPtrW, GetWindowLongPtrW_Hook,
                                       &pOrigGetWindowLongPtrW) &&
        WindhawkUtils::SetFunctionHook(SetWindowLongPtrW, SetWindowLongPtrW_Hook,
                                       &pOrigSetWindowLongPtrW) &&
        WindhawkUtils::SetFunctionHook(GetWindowLongW, GetWindowLongW_Hook, &pOrigGetWindowLongW) &&
        WindhawkUtils::SetFunctionHook(SetWindowLongW, SetWindowLongW_Hook, &pOrigSetWindowLongW);
    if (!g_captionHooksInstalled)
        Wh_Log(L"Native caption fix unavailable");
}
// ---- end caption workaround ----

bool IsExecutableLoad(DWORD flags) {
    return !(flags & (DONT_RESOLVE_DLL_REFERENCES | LOAD_LIBRARY_AS_DATAFILE |
                      LOAD_LIBRARY_AS_DATAFILE_EXCLUSIVE | LOAD_LIBRARY_AS_IMAGE_RESOURCE));
}

HMODULE WINAPI LoadLibraryExW_Hook(LPCWSTR name, HANDLE file, DWORD flags) {
    const HMODULE module = g_origLoadLibraryExW(name, file, flags);
    const DWORD error = GetLastError();
    if (module && IsExecutableLoad(flags) && module == GetModuleHandleW(L"msedge.dll")) {
        TryInstall(module, true);
        SyncNativeCaptions();
    }
    SetLastError(error);
    return module;
}

// Only the main browser process has no --type= argument.
bool IsBrowserProcess() {
    int count = 0;
    auto *args = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!args)
        return false;
    bool browser = true;
    for (int i = 1; i < count; ++i) {
        if (!wcscmp(args[i], L"--"))
            break;
        if (!wcsncmp(args[i], L"--type=", 7))
            browser = false;
    }
    LocalFree(args);
    return browser;
}

} // namespace

BOOL Wh_ModInit() {
    if (!IsBrowserProcess())
        return FALSE;
    g_browser = true;
    LoadSettings();
    InstallNativeCaptionHooks();

    if (HMODULE edge = GetModuleHandleW(L"msedge.dll"))
        return TryInstall(edge, false);

    const HMODULE kernelBase = GetModuleHandleW(L"kernelbase.dll");
    const auto loader =
        kernelBase
            ? reinterpret_cast<LoadLibraryExW_t>(GetProcAddress(kernelBase, "LoadLibraryExW"))
            : nullptr;
    return loader && WindhawkUtils::SetFunctionHook(loader, LoadLibraryExW_Hook,
                                                    &g_origLoadLibraryExW);
}

void Wh_ModAfterInit() {
    if (!g_browser)
        return;
    AcquireSRWLockExclusive(&g_initLock);
    if (!g_stopping && g_applyAfterInit) {
        g_applyAfterInit = false;
        g_ready.store(true, std::memory_order_release);
    }
    ReleaseSRWLockExclusive(&g_initLock);
    // Cover a load that happened between ModInit's check and the loader hook going live.
    if (HMODULE edge = GetModuleHandleW(L"msedge.dll"))
        TryInstall(edge, true);
    SyncNativeCaptions();
}

void Wh_ModSettingsChanged() {
    if (g_browser) {
        LoadSettings();
        SyncNativeCaptions();
    }
}

void Wh_ModBeforeUninit() {
    g_captionStopping.store(true, std::memory_order_release);
    g_captionFixActive.store(false, std::memory_order_release);
    AcquireSRWLockExclusive(&g_initLock);
    g_stopping = true;
    g_applyAfterInit = false;
    g_ready.store(false, std::memory_order_release);
    ReleaseSRWLockExclusive(&g_initLock);
    g_dumpStop.store(true);
    if (g_dumpThread) {
        WaitForSingleObject(g_dumpThread, 10000);
        CloseHandle(g_dumpThread);
        g_dumpThread = nullptr;
    }
    // Restore the real WS_SYSMENU bit while the style hooks are still installed.
    if (g_browser)
        EnumWindows(SyncNativeCaptionWindow, 0);
}

#endif
