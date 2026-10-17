// ==WindhawkMod==
// @id              dimmer-overlay
// @name            Dimmer Overlay
// @description     Configurable Dimmer Overlay with a conditional transparent-shadow control panel.
// @version         1.7.1
// @author          rockinggreninja-bot
// @include         explorer.exe
// @compilerOptions -Wl,--export-all-symbols -luser32 -lgdi32 -ldwmapi
// @license         BSD-3-Clause
// ==/WindhawkMod==

// ==WindhawkModReadme==
/*
# Dimmer Overlay

A lightweight desktop dimming overlay for Windows with a compact native Desktop Acrylic glass floating display-control panel. It does not attempt hardware brightness control.

## Features

- Adjustable black-overlay opacity from 0% (no dimming) to 95% (near-black).
- Works across the full virtual desktop, including multiple monitors.
- Click-through and non-activating: normal mouse and keyboard interaction continues.
- Excludes applications by their exact executable path.
- Dimming returns automatically when an excluded application is minimized, closed,
  or no longer the foreground application.
- Ctrl+Alt+Up makes the screen brighter (reduces overlay opacity) in 5-point steps.
- Ctrl+Alt+Down makes the screen darker (increases overlay opacity) in 5-point steps.
- Ctrl+Alt+End toggles display effects on/off; Ctrl+Alt+F11 remains as a fallback toggle.
- Ctrl+Alt+B opens a compact floating flyout with brightness and temperature controls; Ctrl+Alt+Home restores Default temperature. All six global shortcuts are configurable in Windhawk Settings.
- When Follow Translucent Windows background settings is OFF and Transparent control panel with shadow is ON, the panel is fully transparent with a soft software drop shadow. If Follow is ON, Translucent Windows takes priority and supplies the panel background. When Follow is OFF and the transparent-shadow option is OFF, Dimmer can request its own Windows Desktop Acrylic background.
- Panel placement, taskbar-edge reveal, animation style and duration, icon transition style, icon/slider colors, brightness limits, temperature limits, and global shortcuts are configurable in Windhawk Settings. Ctrl+drag the panel to move it; the position is remembered when enabled. The panel can also be linked to Windows Quick Settings (Control Center).
- Reverting or changing settings does not modify the separate Translucent Windows mod.
- No separate Dimmer application or tray app is required; the overlay runs in the first eligible Explorer process.

## Configure

Open the mod's **Settings** in Windhawk to configure the background behavior, panel placement, whether it animates from behind the taskbar, Control Center integration, hotkeys, animation styles, colors, and brightness/temperature ranges. Follow Translucent Windows background settings takes priority when enabled; the transparent-shadow style is used only when Follow is disabled and Transparent control panel with shadow is enabled. Hold Ctrl and drag the panel with the left mouse button to move it; when Remember panel position is enabled, the coordinates are saved in this mod's local storage. The flame icon toggles Default tint; the configurable reset-temperature shortcut resets it immediately. Excluded executable paths use the application's actual `.exe` path, not a Start-menu shortcut (`.lnk`).

## Panel background mode priority

- **Follow Translucent Windows background settings = ON:** Translucent Windows owns the backdrop, regardless of the Transparent control panel with shadow setting.
- **Follow = OFF, Transparent control panel with shadow = ON:** no backdrop host is created; only transparent controls and a software shadow are drawn.
- **Follow = OFF, Transparent control panel with shadow = OFF:** Dimmer requests Windows Desktop Acrylic on supported Windows 11 builds.

## Important limitations

- The default panel background is fully transparent. A soft software shadow is rendered in the transparent padding outside the controls; the shadow is intentionally click-through. The old glass mode remains available as an option, but depends on the Windows/DWM backdrop rendering path.
- Five embedded sprite frames are used for each icon. The icon state changes every 20% of the configured slider range, with selectable match-and-appear, cross-fade, pop, or none transitions. Temperature values are displayed as 0–100% mapped from configured minimum to maximum Kelvin values.
- This is a user-mode overlay, not hardware brightness control. It does not reduce the
  monitor backlight or its power usage.
- Exclusive-fullscreen games, protected video paths, UAC's secure desktop, and some
  hardware-accelerated presentation modes may bypass or appear above the overlay.
- Packaged/UWP applications can expose a host process (for example, `ApplicationFrameHost.exe`)
  as the foreground process. In those cases, excluding the packaged app's own executable
  path may not match; exclude the actual foreground host path if appropriate.
- The overlay runs inside the first eligible Explorer process. A named mutex prevents
  duplicate overlays if Windows has multiple Explorer processes.
- Default global shortcuts are **Ctrl+Alt+Up** (brighter), **Ctrl+Alt+Down** (darker),
  **Ctrl+Alt+End** (toggle), **Ctrl+Alt+F11** (fallback toggle), **Ctrl+Alt+B** (panel),
  and **Ctrl+Alt+Home** (reset color temperature to Default); all can be changed in Settings.
- Optional Control Center integration opens the panel with Windows Quick Settings and closes it when Quick Settings closes. A panel opened/owned by Control Center ignores the auto-hide timeout. Opening it manually with the configured panel hotkey always uses the auto-hide timeout, even during a Control Center session.
- Brightness shortcuts adjust the current runtime level in 5-point steps, clamped to the configured brightness slider range. The configured Windhawk opacity setting is not changed by hotkeys; a mod reload restores it.
- If another app has already registered a shortcut, the mod log reports that shortcut
  could not be registered. Arrow-key shortcuts repeat when held down.

- The mod targets explorer.exe directly and does not use the dedicated tool-mod launcher.
*/
// ==/WindhawkModReadme==

// ==WindhawkModSettings==
/*
- Enabled: true
  $name: Enable dimming
  $description: Turns the overlay on or off. Ctrl+Alt+End toggles it temporarily; Ctrl+Alt+F11 is a fallback.
- DimAmount: 35
  $name: Black overlay opacity
  $description: 0 means no dimming; 95 is near-black. Higher values look darker.
  #! $min: 0
  #! $max: 95
  #! $format: slider
- ExcludedApps: [""]
  $name: Excluded executable paths
  $description: Add one full executable path per list item. Dimming is hidden only while that app is foreground.
- PanelPositionMode: aboveTaskbar
  $name: Panel position
  $description: Choose a taskbar-relative location, center screen, or custom coordinates. Ctrl+drag anywhere on the panel to place it freely.
  $options:
    - aboveTaskbar: Above the taskbar
    - overTaskbar: Overlap the taskbar
    - belowTaskbar: Below the taskbar edge
    - centerScreen: Center of screen
    - customCoordinates: Custom X/Y coordinates
- RememberPanelPosition: true
  $name: Remember panel position
  $description: Save the panel's Ctrl+drag position across reloads. Turning this off clears the saved position.
- PanelRevealFromTaskbar: false
  $name: Appear/disappear behind the taskbar
  $description: When enabled, the panel is revealed from behind the taskbar and slides back behind it when closing. If Windows does not allow the taskbar to cover the panel, Dimmer falls back to a taskbar-edge slide.
- ShowPanelWithControlCenter: true
  $name: Show panel with Control Center (Win+A)
  $description: Open the panel when Windows Quick Settings opens and close it when Quick Settings closes. While the panel is Control Center-managed, its auto-hide delay is ignored. Ctrl+Alt+B remains independent and uses the configured auto-hide delay.
- FollowTranslucentWindowsSettings: true
  $name: Follow Translucent Windows background settings
  $description: Requires the separate Translucent Windows Windhawk mod to be installed and enabled. When ON, this setting takes priority over Transparent control panel with shadow and lets Translucent Windows apply its selected background effect and color blend. When OFF, Transparent control panel with shadow selects transparent-shadow mode if enabled, or Dimmer's native Windows Desktop Acrylic if disabled. Changing this option recreates or removes the backdrop host as needed.
- PanelCustomX: 100
  $name: Custom panel X coordinate
  $description: Absolute desktop X coordinate, used when Panel position is Custom X/Y. Negative values are valid on left-side monitors.
- PanelCustomY: 100
  $name: Custom panel Y coordinate
  $description: Absolute desktop Y coordinate, used when Panel position is Custom X/Y. Negative values are valid on top-side monitors.
- PanelAnimation: dissolveRise
  $name: Panel opening/closing animation
  $options:
    - dissolveRise: Dissolve and rise / dissolve and slide down
    - fadeRise: Fade and rise / fade and slide down
    - slide: Slide only
    - fade: Fade only
    - none: No animation
- PanelAnimationDurationMs: 320
  $name: Panel animation duration (ms)
  #! $min: 100
  #! $max: 1000
  #! $format: slider
- PanelAutoHideMs: 2500
  $name: Panel auto-hide delay (ms)
  $description: Delay after the pointer leaves the panel. Minimum is 500 ms.
  #! $min: 500
  #! $max: 15000
  #! $format: slider
- BrighterHotkey: "ctrl+alt+38"
  $name: Brightness up shortcut
  $description: Global shortcut to make the screen brighter. Defaults to Ctrl+Alt+Up.
  #! $format: hotkey
- DarkerHotkey: "ctrl+alt+40"
  $name: Brightness down shortcut
  $description: Global shortcut to make the screen darker. Defaults to Ctrl+Alt+Down.
  #! $format: hotkey
- ToggleEffectsHotkey: "ctrl+alt+35"
  $name: Toggle display effects shortcut
  $description: Global shortcut to toggle the dimming/color effect. Defaults to Ctrl+Alt+End.
  #! $format: hotkey
- EmergencyToggleHotkey: "ctrl+alt+122"
  $name: Fallback toggle shortcut
  $description: Secondary global shortcut to toggle the dimming/color effect. Defaults to Ctrl+Alt+F11.
  #! $format: hotkey
- PanelHotkey: "ctrl+alt+66"
  $name: Open/close panel shortcut
  $description: Global shortcut to open or close the control panel. Defaults to Ctrl+Alt+B.
  #! $format: hotkey
- DefaultTemperatureHotkey: "ctrl+alt+36"
  $name: Reset temperature shortcut
  $description: Global shortcut to reset color temperature to Default. Defaults to Ctrl+Alt+Home.
  #! $format: hotkey
- IconAnimation: matchAppear
  $name: Icon state transition
  $options:
    - matchAppear: Match and appear (smooth frame-by-frame morph)
    - crossFade: Cross-fade
    - pop: Pop and settle
    - none: Instant state change
- IconAnimationDurationMs: 220
  $name: Icon transition duration (ms)
  #! $min: 100
  #! $max: 600
  #! $format: slider
- IconColorMode: stateBased
  $name: Icon colors
  $options:
    - stateBased: Follow brightness/warmth state colors
    - custom: Use one custom color for both icons
- CustomIconColor: "#FFFFFF"
  $name: Custom icon color (hex)
  $description: Enter a color such as #FFFFFF. On Windhawk 1.7.3 this is a hex text field.
  #! $format: colorRgb
- SliderFillColor: "#37D890"
  $name: Slider fill color (hex)
  #! $format: colorRgb
- SliderTrackColor: "#39423F"
  $name: Slider track color (hex)
  #! $format: colorRgb
- BrightnessMinPercent: 5
  $name: Minimum brightness slider value (%)
  $description: Simulated brightness cannot go below 5% because the dimming overlay is capped at 95% black.
  #! $min: 5
  #! $max: 99
  #! $format: slider
- BrightnessMaxPercent: 100
  $name: Maximum brightness slider value (%)
  #! $min: 6
  #! $max: 100
  #! $format: slider
- WarmthMinKelvin: 4500
  $name: Minimum color-temperature range (K)
  #! $min: 2500
  #! $max: 8999
  #! $format: slider
- WarmthMaxKelvin: 6000
  $name: Maximum color-temperature range (K)
  #! $min: 2501
  #! $max: 10000
  #! $format: slider
- TransparentShadowPanel: true
  $name: Transparent control panel with shadow
  $description: Used only when Follow Translucent Windows background settings is OFF. In that case, makes the panel background fully transparent and draws a soft floating shadow around it. When Follow is ON, Translucent Windows background takes priority regardless of this setting.
*/
// ==/WindhawkModSettings==

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <windowsx.h>
#include <shellapi.h>
#include <wingdi.h>
#include <dwmapi.h>
#include <cwchar>

#include <algorithm>
#include <atomic>
#include <cwctype>
#include <cmath>
#include <cstdint>
#include <mutex>
#include <string>
#include <utility>
#include <vector>

// Windhawk's loader requires the InternalWhModPtr data symbol and callback
// symbols to be visible in the compiled DLL. Explicit linker options in the
// metadata below ask the MinGW/LLD toolchain to export global symbols; the
// address reference in Wh_ModInit keeps the inline API pointer materialized.

namespace {

constexpr wchar_t kWindowClassName[] = L"WindhawkDimmerOverlayExclusions_3D872A1E";
constexpr wchar_t kPanelBackdropWindowClassName[] = L"WindhawkDimmerControlPanelBackdrop_3D872A1E";
constexpr wchar_t kPanelContentWindowClassName[] = L"WindhawkDimmerControlPanelContent_3D872A1E"; // crisp per-pixel control layer
constexpr int kPanelWidth = 280;
constexpr int kPanelHeight = 70;
// Extra transparent pixels around the logical panel let the software drop shadow
// extend beyond the control layout without painting any background fill.
constexpr int kPanelShadowPadding = 18;
constexpr int kPanelShadowBlurRadius = 14;
constexpr int kPanelSurfaceWidth = kPanelWidth + kPanelShadowPadding * 2;
constexpr int kPanelSurfaceHeight = kPanelHeight + kPanelShadowPadding * 2;
constexpr int kPanelSlideOffset = 10;
constexpr int kIconTileLeft = 10; // generous breathing room around the embedded icons
constexpr int kIconTileSize = 24;
constexpr int kBrightnessRowY = 21;
constexpr int kTemperatureRowY = 49;
constexpr int kValueTextRight = kPanelWidth - 12;
constexpr UINT kPanelOutsideClickMessage = WM_APP + 0xD21;
constexpr UINT kPanelSettingsChangedMessage = WM_APP + 0xD23;
constexpr UINT kHotkeysChangedMessage = WM_APP + 0xD24;
constexpr UINT_PTR kPanelAnimationTimerId = 0xD20;
constexpr UINT_PTR kIconAnimationTimerId = 0xD22;
constexpr UINT kPanelAnimationIntervalMs = 15;
constexpr UINT kPollIntervalMs = 125;
constexpr int kEmergencyHotkeyId = 0xD17;
constexpr int kBrighterHotkeyId = 0xD18;
constexpr int kDarkerHotkeyId = 0xD19;
constexpr int kToggleHotkeyId = 0xD1A;
constexpr int kPanelHotkeyId = 0xD1B;
constexpr int kDefaultTemperatureHotkeyId = 0xD1C;
constexpr int kDimStep = 5;
constexpr int kMaxExcludedApps = 128;

enum class PanelPositionMode { AboveTaskbar, OverTaskbar, BelowTaskbar, CenterScreen, CustomCoordinates };
enum class PanelAnimationMode { DissolveRise, FadeRise, Slide, Fade, None };
enum class IconAnimationMode { MatchAppear, CrossFade, Pop, None };
enum class IconColorMode { StateBased, Custom };

struct HotkeySpec {
    UINT modifiers = 0;
    UINT virtualKey = 0;
};

struct Settings {
    bool enabled = true;
    int dimAmount = 35;
    std::vector<std::wstring> excludedApps;
    PanelPositionMode panelPositionMode = PanelPositionMode::AboveTaskbar;
    bool rememberPanelPosition = true;
    bool panelRevealFromTaskbar = false;
    bool showPanelWithControlCenter = true;
    bool followTranslucentWindowsSettings = true;
    bool transparentShadowPanel = true;
    int panelCustomX = 100;
    int panelCustomY = 100;
    PanelAnimationMode panelAnimation = PanelAnimationMode::DissolveRise;
    int panelAnimationDurationMs = 320;
    int panelAutoHideMs = 2500;
    IconAnimationMode iconAnimation = IconAnimationMode::MatchAppear;
    int iconAnimationDurationMs = 220;
    IconColorMode iconColorMode = IconColorMode::StateBased;
    COLORREF customIconColor = RGB(255, 255, 255);
    COLORREF sliderFillColor = RGB(55, 216, 144);
    COLORREF sliderTrackColor = RGB(57, 66, 63);
    int brightnessMinPercent = 5;
    int brightnessMaxPercent = 100;
    int warmthMinKelvin = 4500;
    int warmthMaxKelvin = 6000;
    HotkeySpec brighterHotkey{};
    HotkeySpec darkerHotkey{};
    HotkeySpec toggleEffectsHotkey{};
    HotkeySpec emergencyToggleHotkey{};
    HotkeySpec panelHotkey{};
    HotkeySpec defaultTemperatureHotkey{};
};

std::mutex g_settingsMutex;
Settings g_settings;
std::atomic<HWND> g_overlayWindow{nullptr};
HANDLE g_stopEvent = nullptr;
HANDLE g_workerThread = nullptr;
HANDLE g_instanceMutex = nullptr;
std::atomic<bool> g_manualEnabled{true};
// Hotkeys adjust this runtime value without overwriting the persisted setting.
std::atomic<int> g_runtimeDimAmount{35};
std::atomic<int> g_configuredDimAmount{-1};
std::atomic<int> g_brightnessMinPercent{5};
std::atomic<int> g_brightnessMaxPercent{100};
std::atomic<int> g_warmthMinKelvin{4500};
std::atomic<int> g_warmthMaxKelvin{6000};
std::atomic<int> g_panelAnimationMode{static_cast<int>(PanelAnimationMode::DissolveRise)};
std::atomic<int> g_panelAnimationDurationMs{320};
std::atomic<int> g_panelAutoHideMs{2500};
std::atomic<int> g_iconAnimationMode{static_cast<int>(IconAnimationMode::MatchAppear)};
std::atomic<int> g_iconAnimationDurationMs{220};
std::atomic<bool> g_customIconColorEnabled{false};
std::atomic<COLORREF> g_customIconColor{RGB(255, 255, 255)};
std::atomic<COLORREF> g_sliderFillColor{RGB(55, 216, 144)};
std::atomic<COLORREF> g_sliderTrackColor{RGB(57, 66, 63)};
std::atomic<bool> g_hasSavedPanelPosition{false};
std::atomic<int> g_savedPanelX{0};
std::atomic<int> g_savedPanelY{0};
// 0 means Default/no tint; the selected range is an approximate software color tint.
std::atomic<int> g_runtimeTemperatureK{0};
std::atomic<int> g_lastNonDefaultTemperatureK{5000};
std::atomic<DWORD> g_overlayColor{RGB(0, 0, 0)};
HWND g_panelWindow = nullptr; // per-pixel layered control surface
HWND g_panelBackdropWindow = nullptr; // non-layered DWM/Translucent Windows material host
bool g_panelAcrylicAvailable = false;
ATOM g_panelBackdropClassAtom = 0;
bool g_panelBackdropFollowMode = true;
bool g_panelTransparentShadowMode = true;

// Translucent Windows must own the material whenever Follow is ON. The custom
// transparent-shadow surface is active only when Follow is OFF and its option is ON.
bool ShouldUseTransparentShadowPanel(const Settings& settings) {
    return settings.transparentShadowPanel && !settings.followTranslucentWindowsSettings;
}

int PanelSurfaceWidth() {
    return g_panelTransparentShadowMode ? kPanelSurfaceWidth : kPanelWidth;
}

int PanelSurfaceHeight() {
    return g_panelTransparentShadowMode ? kPanelSurfaceHeight : kPanelHeight;
}

int PanelSurfacePadding() {
    return g_panelTransparentShadowMode ? kPanelShadowPadding : 0;
}

bool GetLogicalPanelRect(HWND hwnd, RECT* rect) {
    if (!hwnd || !rect || !GetWindowRect(hwnd, rect)) return false;
    const int pad = PanelSurfacePadding();
    rect->left += pad;
    rect->top += pad;
    rect->right -= pad;
    rect->bottom -= pad;
    return true;
}

void ApplyControlPanelWindowRegion(HWND hwnd) {
    if (!hwnd || !IsWindow(hwnd)) return;
    const int width = PanelSurfaceWidth();
    const int height = PanelSurfaceHeight();
    const int corner = g_panelTransparentShadowMode ? 28 + kPanelShadowPadding * 2 : 28;
    HRGN region = CreateRoundRectRgn(0, 0, width, height, corner, corner);
    if (region && !SetWindowRgn(hwnd, region, TRUE)) {
        DeleteObject(region);
        Wh_Log(L"Control layer SetWindowRgn failed: %lu", GetLastError());
    }
}

HHOOK g_panelMouseHook = nullptr;


bool g_draggingBrightness = false;
bool g_draggingTemperature = false;
bool g_draggingPanel = false;
POINT g_panelDragStartCursor{};
int g_panelDragStartX = 0;
int g_panelDragStartY = 0;
bool g_panelAnimating = false;
bool g_panelHiding = false;
int g_panelTargetX = 0;
int g_panelTargetY = 0;
int g_panelStartX = 0;
int g_panelStartY = 0;
int g_panelHiddenX = 0;
int g_panelHiddenY = 0;
bool g_controlCenterOpen = false;
bool g_panelControlCenterManaged = false; // Control Center owns lifetime; auto-hide is suspended.
bool g_panelSuppressedForControlCenter = false; // Manual shortcut override for the current Control Center session.
bool g_panelCloseWhenControlCenterCloses = false;
bool g_loggedBehindTaskbarFallback = false;
ULONGLONG g_panelAnimationStart = 0;
int g_panelAnimationAlpha = 255;
int g_panelAnimationStartAlpha = 255;
std::atomic<ULONGLONG> g_lastPanelInteraction{0};

// Sprite-icon state ranges: five silhouettes per row, one for each 20% segment.
// Transitions cross-fade and scale at the same anchor; the icon never translates.
int g_brightnessIconState = -1;
int g_previousBrightnessIconState = -1;
int g_brightnessIconColorState = -1;
int g_previousBrightnessIconColorState = -1;
int g_temperatureIconState = -1;
int g_previousTemperatureIconState = -1;
int g_temperatureIconColorState = -1;
int g_previousTemperatureIconColorState = -1;
ULONGLONG g_brightnessIconTransitionStart = 0;
ULONGLONG g_temperatureIconTransitionStart = 0;

struct HotkeyBinding {
    int id;
    PCWSTR settingName;
    PCWSTR description;
    bool repeat;
    HotkeySpec Settings::*settingMember;
    bool registered;
};

HotkeyBinding g_hotkeyBindings[] = {
    {kEmergencyHotkeyId, L"EmergencyToggleHotkey", L"fallback toggle", false, &Settings::emergencyToggleHotkey, false},
    {kBrighterHotkeyId, L"BrighterHotkey", L"brightness up", true, &Settings::brighterHotkey, false},
    {kDarkerHotkeyId, L"DarkerHotkey", L"brightness down", true, &Settings::darkerHotkey, false},
    {kToggleHotkeyId, L"ToggleEffectsHotkey", L"toggle display effects", false, &Settings::toggleEffectsHotkey, false},
    {kPanelHotkeyId, L"PanelHotkey", L"open/close panel", false, &Settings::panelHotkey, false},
    {kDefaultTemperatureHotkeyId, L"DefaultTemperatureHotkey", L"reset temperature", false, &Settings::defaultTemperatureHotkey, false},
};

std::wstring Trim(std::wstring value) {
    size_t first = 0;
    while (first < value.size() && iswspace(value[first])) {
        ++first;
    }

    size_t last = value.size();
    while (last > first && iswspace(value[last - 1])) {
        --last;
    }

    return value.substr(first, last - first);
}

std::wstring ExpandEnvironment(std::wstring value) {
    if (value.empty()) {
        return value;
    }

    DWORD needed = ExpandEnvironmentStringsW(value.c_str(), nullptr, 0);
    if (needed == 0 || needed > 32768) {
        return value;
    }

    std::vector<wchar_t> buffer(needed);
    DWORD written = ExpandEnvironmentStringsW(value.c_str(), buffer.data(), needed);
    if (written == 0 || written > needed) {
        return value;
    }

    return std::wstring(buffer.data());
}

std::wstring NormalizePath(std::wstring path) {
    path = Trim(std::move(path));
    if (path.size() >= 2 && path.front() == L'"' && path.back() == L'"') {
        path = path.substr(1, path.size() - 2);
    }
    path = Trim(std::move(path));
    if (path.empty()) {
        return path;
    }

    path = ExpandEnvironment(std::move(path));
    std::replace(path.begin(), path.end(), L'/', L'\\');

    DWORD needed = GetFullPathNameW(path.c_str(), 0, nullptr, nullptr);
    if (needed > 0 && needed < 32768) {
        std::vector<wchar_t> buffer(needed);
        DWORD written = GetFullPathNameW(path.c_str(), needed, buffer.data(), nullptr);
        if (written > 0 && written < needed) {
            path.assign(buffer.data(), written);
        }
    }

    std::replace(path.begin(), path.end(), L'/', L'\\');
    return path;
}

std::wstring ReadStringSetting(PCWSTR name, PCWSTR fallback) {
    PCWSTR raw = Wh_GetStringSetting(name);
    std::wstring value = (raw && *raw) ? raw : (fallback ? fallback : L"");
    Wh_FreeStringSetting(raw);
    return value;
}

wchar_t LowerSettingChar(wchar_t c) {
    return static_cast<wchar_t>(towlower(c));
}

std::wstring LowerSetting(std::wstring value) {
    std::transform(value.begin(), value.end(), value.begin(), LowerSettingChar);
    return value;
}

PanelPositionMode ParsePanelPositionMode(const std::wstring& value) {
    const std::wstring v = LowerSetting(value);
    if (v == L"overtaskbar") return PanelPositionMode::OverTaskbar;
    if (v == L"belowtaskbar") return PanelPositionMode::BelowTaskbar;
    if (v == L"centerscreen") return PanelPositionMode::CenterScreen;
    if (v == L"customcoordinates") return PanelPositionMode::CustomCoordinates;
    return PanelPositionMode::AboveTaskbar;
}

PanelAnimationMode ParsePanelAnimation(const std::wstring& value) {
    const std::wstring v = LowerSetting(value);
    if (v == L"faderise") return PanelAnimationMode::FadeRise;
    if (v == L"slide") return PanelAnimationMode::Slide;
    if (v == L"fade") return PanelAnimationMode::Fade;
    if (v == L"none") return PanelAnimationMode::None;
    return PanelAnimationMode::DissolveRise;
}

IconAnimationMode ParseIconAnimation(const std::wstring& value) {
    const std::wstring v = LowerSetting(value);
    if (v == L"crossfade") return IconAnimationMode::CrossFade;
    if (v == L"pop") return IconAnimationMode::Pop;
    if (v == L"none") return IconAnimationMode::None;
    return IconAnimationMode::MatchAppear;
}

IconColorMode ParseIconColorMode(const std::wstring& value) {
    return LowerSetting(value) == L"custom" ? IconColorMode::Custom : IconColorMode::StateBased;
}

int HexDigit(wchar_t c) {
    if (c >= L'0' && c <= L'9') return c - L'0';
    c = LowerSettingChar(c);
    if (c >= L'a' && c <= L'f') return 10 + c - L'a';
    return -1;
}

COLORREF ParseHexColor(std::wstring value, COLORREF fallback) {
    value = Trim(std::move(value));
    if (!value.empty() && value.front() == L'#') value.erase(value.begin());
    if (value.size() != 6) return fallback;
    int channels[3]{};
    for (int i = 0; i < 3; ++i) {
        const int hi = HexDigit(value[static_cast<size_t>(i) * 2]);
        const int lo = HexDigit(value[static_cast<size_t>(i) * 2 + 1]);
        if (hi < 0 || lo < 0) return fallback;
        channels[i] = hi * 16 + lo;
    }
    return RGB(channels[0], channels[1], channels[2]);
}

HotkeySpec ParseHotkeySpec(std::wstring value) {
    HotkeySpec result{};
    value = LowerSetting(Trim(std::move(value)));
    if (value.empty()) return result;

    std::vector<std::wstring> parts;
    size_t start = 0;
    while (start <= value.size()) {
        const size_t plus = value.find(L'+', start);
        const size_t end = plus == std::wstring::npos ? value.size() : plus;
        std::wstring token = Trim(value.substr(start, end - start));
        if (!token.empty()) parts.push_back(std::move(token));
        if (plus == std::wstring::npos) break;
        start = plus + 1;
    }
    if (parts.empty()) return result;

    for (size_t i = 0; i + 1 < parts.size(); ++i) {
        const auto& token = parts[i];
        if (token == L"ctrl" || token == L"control") result.modifiers |= MOD_CONTROL;
        else if (token == L"alt") result.modifiers |= MOD_ALT;
        else if (token == L"shift") result.modifiers |= MOD_SHIFT;
        else if (token == L"win" || token == L"windows") result.modifiers |= MOD_WIN;
        else return HotkeySpec{};
    }

    const std::wstring& keyToken = parts.back();
    wchar_t* parseEnd = nullptr;
    unsigned long key = 0;
    if (keyToken.size() > 2 && keyToken[0] == L'0' && keyToken[1] == L'x') {
        key = std::wcstoul(keyToken.c_str() + 2, &parseEnd, 16);
        if (!parseEnd || *parseEnd != L'\0') return HotkeySpec{};
    } else {
        key = std::wcstoul(keyToken.c_str(), &parseEnd, 10);
        if (!parseEnd || parseEnd == keyToken.c_str() || *parseEnd != L'\0') {
            // Also accept a single letter for hand-edited settings; the Windhawk
            // hotkey picker normally stores a numeric virtual-key code.
            if (keyToken.size() == 1 && keyToken[0] >= L'a' && keyToken[0] <= L'z') {
                key = static_cast<unsigned long>(keyToken[0] - L'a' + L'A');
            } else {
                return HotkeySpec{};
            }
        }
    }
    if (key == 0 || key > 0xFF) return HotkeySpec{};
    result.virtualKey = static_cast<UINT>(key);
    return result;
}

HotkeySpec ReadHotkeySetting(PCWSTR name, PCWSTR fallback) {
    PCWSTR raw = Wh_GetStringSetting(name);
    std::wstring value = raw ? raw : L"";
    Wh_FreeStringSetting(raw);
    value = Trim(std::move(value));
    if (value.empty()) value = fallback ? fallback : L"";
    HotkeySpec parsed = ParseHotkeySpec(std::move(value));
    if (parsed.virtualKey == 0) {
        Wh_Log(L"Hotkey setting %ls is empty or invalid; shortcut disabled", name);
    }
    return parsed;
}

void LoadSettings() {
    Settings updated;
    updated.enabled = Wh_GetIntSetting(L"Enabled") != 0;
    updated.dimAmount = std::max(0, std::min(95, Wh_GetIntSetting(L"DimAmount")));

    updated.panelPositionMode = ParsePanelPositionMode(
        ReadStringSetting(L"PanelPositionMode", L"aboveTaskbar"));
    updated.rememberPanelPosition = Wh_GetIntSetting(L"RememberPanelPosition") != 0;
    updated.panelRevealFromTaskbar = Wh_GetIntSetting(L"PanelRevealFromTaskbar") != 0;
    updated.showPanelWithControlCenter = Wh_GetIntSetting(L"ShowPanelWithControlCenter") != 0;
    updated.followTranslucentWindowsSettings = Wh_GetIntSetting(L"FollowTranslucentWindowsSettings") != 0;
    updated.transparentShadowPanel = Wh_GetIntSetting(L"TransparentShadowPanel") != 0;
    updated.panelCustomX = std::max(-32768, std::min(32768, Wh_GetIntSetting(L"PanelCustomX")));
    updated.panelCustomY = std::max(-32768, std::min(32768, Wh_GetIntSetting(L"PanelCustomY")));
    updated.panelAnimation = ParsePanelAnimation(
        ReadStringSetting(L"PanelAnimation", L"dissolveRise"));
    updated.panelAnimationDurationMs = Wh_GetIntSetting(L"PanelAnimationDurationMs");
    if (updated.panelAnimationDurationMs <= 0) updated.panelAnimationDurationMs = 320;
    updated.panelAnimationDurationMs = std::max(100, std::min(1000, updated.panelAnimationDurationMs));
    updated.panelAutoHideMs = Wh_GetIntSetting(L"PanelAutoHideMs");
    if (updated.panelAutoHideMs <= 0) updated.panelAutoHideMs = 2500;
    updated.panelAutoHideMs = std::max(500, std::min(15000, updated.panelAutoHideMs));
    updated.iconAnimation = ParseIconAnimation(
        ReadStringSetting(L"IconAnimation", L"matchAppear"));
    updated.iconAnimationDurationMs = Wh_GetIntSetting(L"IconAnimationDurationMs");
    if (updated.iconAnimationDurationMs <= 0) updated.iconAnimationDurationMs = 220;
    updated.iconAnimationDurationMs = std::max(100, std::min(600, updated.iconAnimationDurationMs));
    updated.iconColorMode = ParseIconColorMode(
        ReadStringSetting(L"IconColorMode", L"stateBased"));
    updated.customIconColor = ParseHexColor(
        ReadStringSetting(L"CustomIconColor", L"#FFFFFF"), RGB(255, 255, 255));
    updated.sliderFillColor = ParseHexColor(
        ReadStringSetting(L"SliderFillColor", L"#37D890"), RGB(55, 216, 144));
    updated.sliderTrackColor = ParseHexColor(
        ReadStringSetting(L"SliderTrackColor", L"#39423F"), RGB(57, 66, 63));

    int brightnessMin = Wh_GetIntSetting(L"BrightnessMinPercent");
    int brightnessMax = Wh_GetIntSetting(L"BrightnessMaxPercent");
    if (brightnessMin <= 0) brightnessMin = 5;
    if (brightnessMax <= 0) brightnessMax = 100;
    updated.brightnessMinPercent = std::max(5, std::min(99, brightnessMin));
    updated.brightnessMaxPercent = std::max(6, std::min(100, brightnessMax));
    if (updated.brightnessMaxPercent <= updated.brightnessMinPercent) {
        updated.brightnessMaxPercent = std::min(100, updated.brightnessMinPercent + 1);
        if (updated.brightnessMaxPercent <= updated.brightnessMinPercent) {
            updated.brightnessMinPercent = 99;
            updated.brightnessMaxPercent = 100;
        }
    }

    int warmthMin = Wh_GetIntSetting(L"WarmthMinKelvin");
    int warmthMax = Wh_GetIntSetting(L"WarmthMaxKelvin");
    if (warmthMin <= 0) warmthMin = 4500;
    if (warmthMax <= 0) warmthMax = 6000;
    updated.warmthMinKelvin = std::max(2500, std::min(8999, warmthMin));
    updated.warmthMaxKelvin = std::max(2501, std::min(10000, warmthMax));
    if (updated.warmthMaxKelvin <= updated.warmthMinKelvin) {
        updated.warmthMaxKelvin = std::min(10000, updated.warmthMinKelvin + 1);
        if (updated.warmthMaxKelvin <= updated.warmthMinKelvin) {
            updated.warmthMinKelvin = 9999;
            updated.warmthMaxKelvin = 10000;
        }
    }

    // These are Windhawk's native hotkey-setting strings (for example,
    // "ctrl+alt+38"). An empty value falls back to the documented default.
    updated.emergencyToggleHotkey = ReadHotkeySetting(L"EmergencyToggleHotkey", L"ctrl+alt+122");
    updated.brighterHotkey = ReadHotkeySetting(L"BrighterHotkey", L"ctrl+alt+38");
    updated.darkerHotkey = ReadHotkeySetting(L"DarkerHotkey", L"ctrl+alt+40");
    updated.toggleEffectsHotkey = ReadHotkeySetting(L"ToggleEffectsHotkey", L"ctrl+alt+35");
    updated.panelHotkey = ReadHotkeySetting(L"PanelHotkey", L"ctrl+alt+66");
    updated.defaultTemperatureHotkey = ReadHotkeySetting(L"DefaultTemperatureHotkey", L"ctrl+alt+36");

    for (int index = 0; index < kMaxExcludedApps; ++index) {
        PCWSTR rawPath = Wh_GetStringSetting(L"ExcludedApps[%d]", index);
        std::wstring path = rawPath ? rawPath : L"";
        Wh_FreeStringSetting(rawPath);
        path = NormalizePath(std::move(path));
        if (path.empty()) break;

        bool duplicate = false;
        for (const auto& existing : updated.excludedApps) {
            if (CompareStringOrdinal(existing.c_str(), -1, path.c_str(), -1, TRUE) == CSTR_EQUAL) {
                duplicate = true;
                break;
            }
        }
        if (!duplicate) updated.excludedApps.push_back(std::move(path));
    }

    // A saved drag position remains active until the user changes Panel position.
    // This keeps the coordinates persistent without writing to Windhawk's settings.
    const int configuredPositionMode = static_cast<int>(updated.panelPositionMode);
    const int previouslyConfiguredMode = Wh_GetIntValue(L"PanelStoredPositionMode", -1);
    const int previouslyCustomX = Wh_GetIntValue(L"PanelStoredCustomX", 100);
    const int previouslyCustomY = Wh_GetIntValue(L"PanelStoredCustomY", 100);
    if (previouslyConfiguredMode != -1 &&
        (previouslyConfiguredMode != configuredPositionMode ||
         (updated.panelPositionMode == PanelPositionMode::CustomCoordinates &&
          (previouslyCustomX != updated.panelCustomX || previouslyCustomY != updated.panelCustomY)))) {
        Wh_SetIntValue(L"PanelHasSavedPosition", 0);
    }
    Wh_SetIntValue(L"PanelStoredPositionMode", configuredPositionMode);
    Wh_SetIntValue(L"PanelStoredCustomX", updated.panelCustomX);
    Wh_SetIntValue(L"PanelStoredCustomY", updated.panelCustomY);
    if (!updated.rememberPanelPosition) {
        Wh_SetIntValue(L"PanelHasSavedPosition", 0);
    }
    g_hasSavedPanelPosition.store(updated.rememberPanelPosition &&
        Wh_GetIntValue(L"PanelHasSavedPosition", 0) != 0);
    g_savedPanelX.store(Wh_GetIntValue(L"PanelSavedX", 0));
    g_savedPanelY.store(Wh_GetIntValue(L"PanelSavedY", 0));

    const int loadedEnabled = updated.enabled ? 1 : 0;
    const int loadedDimAmount = updated.dimAmount;
    const int loadedExclusionCount = static_cast<int>(updated.excludedApps.size());

    const int previousConfiguredAmount = g_configuredDimAmount.exchange(loadedDimAmount);
    if (previousConfiguredAmount < 0 || previousConfiguredAmount != loadedDimAmount) {
        g_runtimeDimAmount.store(loadedDimAmount);
    }

    g_brightnessMinPercent.store(updated.brightnessMinPercent);
    g_brightnessMaxPercent.store(updated.brightnessMaxPercent);
    g_warmthMinKelvin.store(updated.warmthMinKelvin);
    g_warmthMaxKelvin.store(updated.warmthMaxKelvin);
    g_panelAnimationMode.store(static_cast<int>(updated.panelAnimation));
    g_panelAnimationDurationMs.store(updated.panelAnimationDurationMs);
    g_panelAutoHideMs.store(updated.panelAutoHideMs);
    g_iconAnimationMode.store(static_cast<int>(updated.iconAnimation));
    g_iconAnimationDurationMs.store(updated.iconAnimationDurationMs);
    g_customIconColorEnabled.store(updated.iconColorMode == IconColorMode::Custom);
    g_customIconColor.store(updated.customIconColor);
    g_sliderFillColor.store(updated.sliderFillColor);
    g_sliderTrackColor.store(updated.sliderTrackColor);

    // Clamp current runtime values into the newly configured ranges.
    int brightness = 100 - g_runtimeDimAmount.load();
    brightness = std::max(updated.brightnessMinPercent,
                          std::min(updated.brightnessMaxPercent, brightness));
    g_runtimeDimAmount.store(100 - brightness);
    const int minKelvin = updated.warmthMinKelvin;
    const int maxKelvin = updated.warmthMaxKelvin;
    g_lastNonDefaultTemperatureK.store(std::max(minKelvin,
        std::min(maxKelvin, g_lastNonDefaultTemperatureK.load())));
    int runtimeKelvin = g_runtimeTemperatureK.load();
    if (runtimeKelvin > 0) {
        runtimeKelvin = std::max(minKelvin, std::min(maxKelvin, runtimeKelvin));
        g_runtimeTemperatureK.store(runtimeKelvin);
        g_lastNonDefaultTemperatureK.store(runtimeKelvin);
    }

    {
        std::lock_guard<std::mutex> lock(g_settingsMutex);
        g_settings = std::move(updated);
    }

    if (g_panelWindow && IsWindow(g_panelWindow)) {
        PostMessageW(g_panelWindow, kPanelSettingsChangedMessage, 0, 0);
    }
    HWND overlayWindow = g_overlayWindow.load();
    if (overlayWindow && IsWindow(overlayWindow)) {
        PostMessageW(overlayWindow, kHotkeysChangedMessage, 0, 0);
    }
    OutputDebugStringW(L"[DimmerWH] Settings loaded\r\n");
    Wh_Log(L"Settings loaded: enabled=%d, dimAmount=%d, exclusions=%d; pid=%lu",
           loadedEnabled, loadedDimAmount, loadedExclusionCount, GetCurrentProcessId());
}

Settings GetSettingsSnapshot() {
    std::lock_guard<std::mutex> lock(g_settingsMutex);
    return g_settings;
}

struct TintStop {
    int temperature;
    COLORREF color;
    int alpha;
};

void GetTemperatureTint(int temperature, COLORREF* tintColor, BYTE* tintAlpha) {
    if (!tintColor || !tintAlpha || temperature <= 0) {
        if (tintColor) *tintColor = RGB(0, 0, 0);
        if (tintAlpha) *tintAlpha = 0;
        return;
    }

    // The configured Kelvin range maps across the warm-neutral-cool tint palette.
    constexpr TintStop stops[] = {
        {4500, RGB(255, 170, 82), 34},
        {5000, RGB(255, 210, 158), 24},
        {5500, RGB(255, 239, 226), 11},
        {6000, RGB(218, 235, 255), 15},
    };
    const int rangeMin = g_warmthMinKelvin.load();
    const int rangeMax = std::max(rangeMin + 1, g_warmthMaxKelvin.load());
    temperature = std::max(rangeMin, std::min(rangeMax, temperature));
    const double rangeFraction = static_cast<double>(temperature - rangeMin) /
                                 static_cast<double>(rangeMax - rangeMin);
    temperature = 4500 + static_cast<int>(rangeFraction * 1500.0 + 0.5);
    const TintStop* lower = &stops[0];
    const TintStop* upper = &stops[3];
    for (size_t i = 0; i < 3; ++i) {
        if (temperature >= stops[i].temperature && temperature <= stops[i + 1].temperature) {
            lower = &stops[i];
            upper = &stops[i + 1];
            break;
        }
    }

    const double fraction = upper->temperature == lower->temperature ? 0.0 :
        static_cast<double>(temperature - lower->temperature) /
        static_cast<double>(upper->temperature - lower->temperature);
    const auto lerp = [fraction](int a, int b) {
        return static_cast<int>(a + (b - a) * fraction + 0.5);
    };
    *tintColor = RGB(lerp(GetRValue(lower->color), GetRValue(upper->color)),
                     lerp(GetGValue(lower->color), GetGValue(upper->color)),
                     lerp(GetBValue(lower->color), GetBValue(upper->color)));
    *tintAlpha = static_cast<BYTE>(std::max(0, std::min(255,
        lerp(lower->alpha, upper->alpha))));
}

void GetCompositeOverlay(int dimAmount, int temperature, BYTE* outAlpha, COLORREF* outColor) {
    if (!outAlpha || !outColor) return;
    const double dimAlpha = static_cast<double>(std::max(0, std::min(95, dimAmount))) / 100.0;
    COLORREF tintColor = RGB(0, 0, 0);
    BYTE tintAlphaByte = 0;
    GetTemperatureTint(temperature, &tintColor, &tintAlphaByte);
    const double tintAlpha = static_cast<double>(tintAlphaByte) / 255.0;
    const double combinedAlpha = 1.0 - (1.0 - dimAlpha) * (1.0 - tintAlpha);
    *outAlpha = static_cast<BYTE>(std::max(0, std::min(255,
        static_cast<int>(combinedAlpha * 255.0 + 0.5))));
    if (combinedAlpha <= 0.00001) {
        *outColor = RGB(0, 0, 0);
        return;
    }
    const auto channel = [tintAlpha, combinedAlpha](int value) {
        return std::max(0, std::min(255,
            static_cast<int>(value * tintAlpha / combinedAlpha + 0.5)));
    };
    *outColor = RGB(channel(GetRValue(tintColor)), channel(GetGValue(tintColor)),
                    channel(GetBValue(tintColor)));
}

int CurrentBrightnessPercent() {
    const int minValue = g_brightnessMinPercent.load();
    const int maxValue = g_brightnessMaxPercent.load();
    return std::max(minValue, std::min(maxValue, 100 - g_runtimeDimAmount.load()));
}

int BrightnessRangePercent() {
    const int minValue = g_brightnessMinPercent.load();
    const int maxValue = std::max(minValue + 1, g_brightnessMaxPercent.load());
    return std::max(0, std::min(100,
        ((CurrentBrightnessPercent() - minValue) * 100 + (maxValue - minValue) / 2) /
        (maxValue - minValue)));
}

int CurrentTemperatureSliderValue() {
    const int minValue = g_warmthMinKelvin.load();
    const int maxValue = std::max(minValue + 1, g_warmthMaxKelvin.load());
    const int temperature = g_runtimeTemperatureK.load();
    if (temperature > 0) return std::max(minValue, std::min(maxValue, temperature));
    return std::max(minValue, std::min(maxValue, g_lastNonDefaultTemperatureK.load()));
}

int CurrentTemperaturePercent() {
    // Linear mapping across the configurable range. The default 4500-6000K range
    // yields 0% at 4500K and 100% at 6000K.
    const int minValue = g_warmthMinKelvin.load();
    const int maxValue = std::max(minValue + 1, g_warmthMaxKelvin.load());
    const int temperature = CurrentTemperatureSliderValue();
    return std::max(0, std::min(100,
        ((temperature - minValue) * 100 + (maxValue - minValue) / 2) /
        (maxValue - minValue)));
}

int SliderLeft() {
    return 46;
}

int SliderRight() {
    return kPanelWidth - 50;
}

int SliderXToValue(int x, int minValue, int maxValue) {
    const int left = SliderLeft();
    const int right = SliderRight();
    x = std::max(left, std::min(right, x));
    const double fraction = static_cast<double>(x - left) / (right - left);
    return std::max(minValue, std::min(maxValue,
        static_cast<int>(minValue + fraction * (maxValue - minValue) + 0.5)));
}

int ValueToSliderX(int value, int minValue, int maxValue) {
    const int left = SliderLeft();
    const int right = SliderRight();
    value = std::max(minValue, std::min(maxValue, value));
    const double fraction = static_cast<double>(value - minValue) / (maxValue - minValue);
    return left + static_cast<int>(fraction * (right - left) + 0.5);
}

void FillPanelPill(HDC dc, int left, int top, int right, int bottom, COLORREF color) {
    if (right <= left || bottom <= top) return;
    HRGN region = CreateRoundRectRgn(left, top, right + 1, bottom + 1,
                                     bottom - top, bottom - top);
    if (!region) return;
    HBRUSH brush = CreateSolidBrush(color);
    if (brush) {
        FillRgn(dc, region, brush);
        DeleteObject(brush);
    }
    DeleteObject(region);
}



// Sprite masks extracted and cleaned from the icon sheet supplied by the user.

// Each state is embedded as a 24x24 alpha mask; no runtime image file is needed.

constexpr int kIconMaskSize = 24;
constexpr int kIconMaskPixelCount = kIconMaskSize * kIconMaskSize;

static constexpr BYTE kSunIconMasks[5][kIconMaskPixelCount] = {
    { // state 0 (20% range)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 141, 105, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 157, 119, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 115, 81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 17, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 42, 195, 246, 245, 165, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 18, 226, 129, 15, 31, 169, 185, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 8, 19, 2, 110, 175, 0, 0, 0, 7, 223, 55, 4, 19, 10, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 228, 249, 123, 151, 111, 0, 0, 0, 0, 172, 96, 135, 250, 230, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 23, 36, 9, 130, 149, 0, 0, 0, 0, 205, 74, 10, 37, 24, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 40, 236, 63, 0, 0, 113, 220, 8, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 90, 239, 203, 212, 221, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 20, 82, 72, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 103, 69, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 158, 118, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 138, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // state 1 (20% range)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 215, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 240, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 169, 38, 0, 0, 0, 75, 0, 0, 0, 39, 169, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 84, 209, 0, 33, 119, 141, 108, 22, 1, 213, 82, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 17, 86, 235, 161, 127, 174, 230, 62, 16, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 36, 237, 57, 0, 0, 0, 91, 231, 17, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 129, 147, 0, 0, 0, 0, 0, 185, 100, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 213, 232, 60, 169, 102, 0, 0, 0, 0, 0, 137, 135, 64, 233, 213, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 13, 18, 2, 139, 138, 0, 0, 0, 0, 0, 178, 105, 2, 18, 12, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 48, 235, 45, 0, 0, 0, 73, 237, 24, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 111, 235, 142, 108, 154, 234, 80, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 47, 198, 1, 49, 143, 172, 133, 33, 1, 196, 45, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 209, 75, 0, 0, 1, 79, 0, 0, 0, 75, 205, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 8, 237, 2, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 214, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // state 2 (20% range)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 111, 132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 129, 146, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 106, 78, 0, 0, 0, 96, 115, 0, 0, 0, 64, 122, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 73, 239, 76, 0, 9, 56, 57, 10, 0, 60, 236, 91, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 71, 70, 104, 233, 208, 209, 235, 114, 62, 82, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 107, 220, 54, 0, 0, 46, 214, 121, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 9, 233, 57, 0, 0, 0, 0, 45, 239, 19, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 114, 132, 103, 56, 222, 0, 0, 0, 0, 0, 0, 205, 72, 93, 130, 118, 0, 0, 0, 0,
        0, 0, 0, 0, 116, 132, 107, 56, 219, 0, 0, 0, 0, 0, 0, 200, 74, 97, 133, 121, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 10, 235, 53, 0, 0, 0, 0, 40, 240, 19, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 106, 221, 53, 0, 0, 43, 213, 125, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 74, 76, 106, 236, 211, 208, 238, 121, 62, 85, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 78, 239, 73, 0, 12, 61, 66, 13, 0, 56, 234, 94, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 103, 70, 0, 0, 0, 102, 123, 0, 0, 0, 56, 112, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 129, 147, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 107, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // state 3 (20% range)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 149, 132, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 170, 152, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 15, 48, 0, 0, 0, 0, 170, 152, 0, 0, 0, 0, 83, 20, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 81, 245, 86, 0, 0, 0, 117, 101, 0, 0, 0, 123, 248, 64, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 1, 135, 244, 72, 0, 30, 86, 84, 25, 0, 97, 249, 103, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 1, 119, 72, 141, 243, 197, 199, 240, 131, 65, 90, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 146, 206, 36, 0, 0, 43, 213, 128, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 33, 246, 37, 0, 0, 0, 0, 49, 243, 18, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 135, 153, 157, 101, 86, 199, 0, 0, 0, 0, 0, 0, 214, 69, 111, 155, 153, 135, 0, 0, 0,
        0, 0, 0, 144, 171, 171, 106, 87, 198, 0, 0, 0, 0, 0, 0, 216, 68, 117, 171, 171, 149, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 33, 245, 33, 0, 0, 0, 0, 52, 243, 16, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 153, 203, 36, 0, 0, 49, 221, 125, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 103, 68, 149, 241, 196, 201, 242, 132, 67, 89, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 125, 248, 81, 0, 31, 88, 83, 25, 0, 107, 250, 99, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 76, 248, 93, 0, 0, 0, 122, 107, 0, 0, 0, 127, 250, 64, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 15, 51, 0, 0, 0, 0, 170, 152, 0, 0, 0, 0, 78, 17, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 170, 152, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 148, 131, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // state 4 (20% range)
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 118, 177, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 150, 219, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 2, 70, 5, 0, 0, 0, 0, 149, 219, 0, 0, 0, 0, 2, 69, 5, 0, 0, 0, 0,
        0, 0, 0, 0, 36, 250, 166, 4, 0, 0, 0, 148, 215, 0, 0, 0, 2, 152, 250, 49, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 110, 251, 163, 4, 0, 0, 95, 133, 0, 0, 1, 149, 251, 115, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 111, 250, 112, 0, 49, 109, 108, 50, 0, 98, 251, 115, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 76, 38, 161, 242, 185, 189, 245, 167, 32, 77, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 159, 204, 32, 0, 0, 33, 200, 163, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 36, 244, 37, 0, 0, 0, 0, 34, 245, 43, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 141, 172, 173, 184, 111, 90, 199, 0, 0, 0, 0, 0, 0, 194, 101, 100, 177, 175, 173, 148, 0, 0,
        0, 0, 155, 187, 188, 187, 124, 93, 196, 0, 0, 0, 0, 0, 0, 194, 103, 108, 187, 187, 188, 159, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 39, 246, 38, 0, 0, 0, 0, 33, 245, 44, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 156, 209, 37, 0, 0, 38, 209, 160, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 85, 43, 158, 246, 203, 203, 247, 156, 37, 86, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 115, 250, 107, 0, 40, 100, 99, 40, 0, 98, 251, 122, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 112, 250, 152, 2, 0, 0, 120, 121, 0, 0, 1, 148, 251, 118, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 37, 250, 152, 2, 0, 0, 0, 183, 184, 0, 0, 0, 2, 145, 250, 43, 0, 0, 0, 0,
        0, 0, 0, 0, 1, 58, 2, 0, 0, 0, 0, 183, 185, 0, 0, 0, 0, 0, 58, 1, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 182, 185, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 146, 148, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    }
};

static constexpr BYTE kFlameIconMasks[5][kIconMaskPixelCount] = {
    { // sprite 0: smallest flame silhouette
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 174, 75, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 172, 241, 81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 203, 65, 218, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 56, 200, 0, 72, 195, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 216, 54, 0, 0, 179, 72, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 36, 214, 79, 0, 0, 0, 73, 166, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 15, 211, 73, 0, 0, 0, 0, 10, 223, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 135, 120, 0, 0, 0, 0, 45, 0, 224, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 215, 17, 0, 0, 0, 0, 113, 24, 229, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 234, 1, 0, 0, 0, 0, 161, 99, 181, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 193, 38, 0, 0, 0, 7, 228, 206, 72, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 69, 160, 1, 0, 0, 118, 250, 127, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 66, 90, 7, 90, 175, 70, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // sprite 1: small flame silhouette
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 103, 114, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 83, 246, 146, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 102, 130, 164, 130, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 60, 7, 199, 72, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 95, 181, 0, 0, 41, 215, 10, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 80, 204, 20, 0, 0, 0, 139, 116, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 57, 210, 27, 0, 0, 0, 0, 23, 219, 6, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 13, 213, 45, 0, 0, 15, 71, 0, 0, 166, 78, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 118, 131, 0, 0, 21, 199, 136, 0, 0, 79, 154, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 209, 29, 0, 33, 203, 135, 184, 0, 0, 18, 211, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 228, 0, 15, 207, 61, 2, 190, 98, 0, 0, 233, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 225, 8, 115, 125, 0, 0, 24, 216, 55, 5, 226, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 148, 107, 160, 61, 0, 0, 0, 97, 150, 81, 163, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 17, 204, 211, 109, 0, 0, 0, 78, 204, 215, 36, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 15, 141, 212, 2, 0, 0, 145, 176, 37, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // sprite 2: medium flame silhouette
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 210, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 236, 201, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 55, 174, 126, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 180, 71, 10, 216, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 120, 161, 0, 0, 151, 72, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 89, 193, 10, 0, 0, 101, 114, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 54, 208, 20, 0, 0, 0, 93, 121, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 6, 208, 46, 0, 0, 0, 0, 106, 109, 22, 145, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 97, 142, 0, 0, 53, 42, 0, 94, 130, 74, 250, 67, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 185, 40, 0, 36, 232, 36, 0, 11, 182, 205, 106, 166, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 224, 1, 3, 197, 218, 48, 0, 0, 0, 0, 6, 219, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 216, 0, 80, 160, 97, 155, 0, 0, 0, 0, 0, 228, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 220, 5, 150, 68, 3, 163, 154, 7, 0, 0, 6, 219, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 162, 71, 166, 47, 0, 1, 125, 172, 1, 0, 69, 162, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 50, 204, 136, 109, 0, 0, 0, 176, 60, 9, 199, 50, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 104, 230, 227, 24, 0, 0, 110, 145, 192, 112, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 59, 175, 199, 8, 0, 125, 202, 73, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // sprite 3: large flame silhouette
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 181, 82, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 133, 245, 105, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 116, 115, 195, 81, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 168, 63, 27, 216, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 209, 3, 0, 117, 121, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 40, 215, 52, 0, 0, 24, 200, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 66, 215, 58, 0, 0, 0, 0, 214, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 70, 209, 35, 0, 0, 1, 112, 1, 214, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 23, 215, 32, 0, 0, 7, 160, 177, 10, 208, 24, 199, 4, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 131, 109, 0, 0, 15, 192, 210, 115, 51, 168, 30, 251, 91, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 206, 19, 0, 8, 197, 99, 102, 109, 28, 210, 160, 179, 176, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 230, 0, 0, 147, 124, 0, 51, 185, 0, 66, 104, 8, 224, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 226, 2, 26, 210, 5, 0, 0, 169, 125, 1, 0, 0, 225, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 183, 49, 94, 129, 0, 0, 0, 10, 165, 153, 2, 6, 218, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 91, 157, 110, 104, 0, 0, 0, 0, 1, 164, 94, 72, 159, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 3, 193, 178, 142, 0, 0, 0, 0, 0, 51, 180, 202, 53, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 21, 201, 229, 11, 0, 0, 0, 0, 52, 250, 118, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 8, 124, 99, 0, 0, 0, 0, 95, 85, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    },
    { // sprite 4: largest flame silhouette
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 126, 148, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 81, 228, 190, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 84, 139, 116, 161, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 161, 77, 1, 192, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 75, 197, 3, 0, 93, 133, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 57, 213, 30, 0, 0, 55, 164, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 16, 215, 41, 0, 0, 0, 66, 156, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 50, 169, 117, 120, 0, 0, 0, 0, 94, 128, 0, 27, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 7, 215, 122, 175, 43, 0, 8, 86, 0, 96, 127, 4, 232, 57, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 97, 227, 129, 195, 23, 0, 10, 249, 113, 29, 209, 71, 227, 214, 9, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 176, 67, 209, 191, 1, 0, 19, 215, 197, 63, 68, 156, 60, 134, 105, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 219, 4, 5, 3, 0, 0, 82, 151, 45, 201, 1, 0, 0, 37, 193, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 228, 0, 0, 0, 0, 2, 199, 45, 0, 176, 52, 0, 0, 0, 225, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 226, 0, 0, 17, 111, 107, 149, 0, 0, 113, 106, 116, 56, 0, 219, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 197, 30, 0, 138, 242, 213, 14, 0, 0, 98, 123, 207, 135, 3, 224, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 130, 103, 0, 215, 74, 58, 0, 0, 0, 136, 222, 188, 164, 63, 167, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 35, 208, 12, 217, 0, 0, 0, 0, 4, 214, 133, 68, 157, 187, 61, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 134, 150, 222, 7, 0, 0, 0, 17, 27, 0, 138, 227, 152, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 3, 159, 250, 96, 0, 0, 0, 0, 0, 33, 238, 152, 4, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 1, 98, 208, 5, 0, 0, 0, 2, 134, 71, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    }
};

int BrightnessStateForCurrentValue() {
    const int value = BrightnessRangePercent();
    if (value <= 20) return 0;
    if (value <= 40) return 1;
    if (value <= 60) return 2;
    if (value <= 80) return 3;
    return 4;
}

int BrightnessColorStateForCurrentValue() {
    const int value = BrightnessRangePercent();
    if (value <= 20) return 0; // subdued slate-green
    if (value <= 70) return 1; // green
    return 2;                  // soft orange
}

int TemperatureStateForCurrentValue() {
    // Flame silhouettes run in reverse: low range = large flame, high range = small flame.
    const int percent = CurrentTemperaturePercent();
    if (percent <= 20) return 4;
    if (percent <= 40) return 3;
    if (percent <= 60) return 2;
    if (percent <= 80) return 1;
    return 0;
}

int TemperatureColorStateForCurrentValue() {
    if (g_runtimeTemperatureK.load() == 0) return 2; // Default is pale neutral yellow.
    const int percent = CurrentTemperaturePercent();
    if (percent <= 20) return 0; // warm amber
    if (percent <= 40) return 1; // golden amber
    if (percent <= 60) return 2; // pale yellow
    if (percent <= 80) return 3; // neutral ivory
    return 4;                   // cool blue
}

void StartIconTransitionIfVisible(ULONGLONG /*now*/) {
    if (g_iconAnimationMode.load() == static_cast<int>(IconAnimationMode::None)) {
        g_previousBrightnessIconState = g_brightnessIconState;
        g_previousBrightnessIconColorState = g_brightnessIconColorState;
        g_previousTemperatureIconState = g_temperatureIconState;
        g_previousTemperatureIconColorState = g_temperatureIconColorState;
        g_brightnessIconTransitionStart = 0;
        g_temperatureIconTransitionStart = 0;
        if (g_panelWindow && IsWindow(g_panelWindow)) InvalidateRect(g_panelWindow, nullptr, FALSE);
        return;
    }
    if (g_panelWindow && IsWindow(g_panelWindow) && IsWindowVisible(g_panelWindow) &&
        !g_panelHiding) {
        SetTimer(g_panelWindow, kIconAnimationTimerId, kPanelAnimationIntervalMs, nullptr);
    } else {
        // Collapse transitions that occur while hidden, so reopening never animates
        // an outdated icon state.
        g_previousBrightnessIconState = g_brightnessIconState;
        g_previousBrightnessIconColorState = g_brightnessIconColorState;
        g_previousTemperatureIconState = g_temperatureIconState;
        g_previousTemperatureIconColorState = g_temperatureIconColorState;
        g_brightnessIconTransitionStart = 0;
        g_temperatureIconTransitionStart = 0;
    }
}

void UpdateIconTransitionStates() {
    const ULONGLONG now = GetTickCount64();
    const int brightnessState = BrightnessStateForCurrentValue();
    const int brightnessColor = BrightnessColorStateForCurrentValue();
    const int temperatureState = TemperatureStateForCurrentValue();
    const int temperatureColor = TemperatureColorStateForCurrentValue();
    bool anyChanged = false;

    if (g_brightnessIconState < 0) {
        g_brightnessIconState = g_previousBrightnessIconState = brightnessState;
        g_brightnessIconColorState = g_previousBrightnessIconColorState = brightnessColor;
    } else if (brightnessState != g_brightnessIconState ||
               brightnessColor != g_brightnessIconColorState) {
        g_previousBrightnessIconState = g_brightnessIconState;
        g_previousBrightnessIconColorState = g_brightnessIconColorState;
        g_brightnessIconState = brightnessState;
        g_brightnessIconColorState = brightnessColor;
        g_brightnessIconTransitionStart = now;
        anyChanged = true;
    }

    if (g_temperatureIconState < 0) {
        g_temperatureIconState = g_previousTemperatureIconState = temperatureState;
        g_temperatureIconColorState = g_previousTemperatureIconColorState = temperatureColor;
    } else if (temperatureState != g_temperatureIconState ||
               temperatureColor != g_temperatureIconColorState) {
        g_previousTemperatureIconState = g_temperatureIconState;
        g_previousTemperatureIconColorState = g_temperatureIconColorState;
        g_temperatureIconState = temperatureState;
        g_temperatureIconColorState = temperatureColor;
        g_temperatureIconTransitionStart = now;
        anyChanged = true;
    }

    if (anyChanged) StartIconTransitionIfVisible(now);
}

double IconTransitionProgress(ULONGLONG start, ULONGLONG now) {
    if (!start) return 1.0;
    const int duration = std::max(100, g_iconAnimationDurationMs.load());
    const double t = std::min(1.0, static_cast<double>(now - start) /
                                      static_cast<double>(duration));
    // Ease-out curve: the new icon appears quickly and settles softly.
    const double inv = 1.0 - t;
    return 1.0 - inv * inv * inv;
}

COLORREF BrightnessIconColor(int colorState) {
    if (g_customIconColorEnabled.load()) return g_customIconColor.load();
    switch (colorState) {
        case 0: return RGB(142, 159, 151); // subdued for very dim
        case 1: return RGB(53, 216, 144);  // green normal state
        default: return RGB(255, 174, 101); // orange bright state
    }
}

COLORREF TemperatureIconColor(int colorState) {
    if (g_customIconColorEnabled.load()) return g_customIconColor.load();
    switch (colorState) {
        case 0: return RGB(255, 147, 64);   // warm amber
        case 1: return RGB(255, 183, 92);   // golden amber
        case 2: return RGB(250, 226, 142);  // pale yellow / Default
        case 3: return RGB(234, 238, 231);  // neutral ivory
        default: return RGB(104, 178, 255); // cool blue
    }
}

BYTE SampleIconMaskBilinear(const BYTE* mask, double x, double y) {
    x = std::max(0.0, std::min(static_cast<double>(kIconMaskSize - 1), x));
    y = std::max(0.0, std::min(static_cast<double>(kIconMaskSize - 1), y));
    const int x0 = static_cast<int>(x);
    const int y0 = static_cast<int>(y);
    const int x1 = std::min(kIconMaskSize - 1, x0 + 1);
    const int y1 = std::min(kIconMaskSize - 1, y0 + 1);
    const double fx = x - x0;
    const double fy = y - y0;
    const double p00 = mask[y0 * kIconMaskSize + x0];
    const double p10 = mask[y0 * kIconMaskSize + x1];
    const double p01 = mask[y1 * kIconMaskSize + x0];
    const double p11 = mask[y1 * kIconMaskSize + x1];
    const double top = p00 + (p10 - p00) * fx;
    const double bottom = p01 + (p11 - p01) * fx;
    return static_cast<BYTE>(std::max(0.0, std::min(255.0,
        top + (bottom - top) * fy + 0.5)));
}

void DrawTintedSprite(DWORD* pixels, int centerX, int centerY, const BYTE* mask,
                      COLORREF color, double scale, double opacity) {
    if (!pixels || !mask || opacity <= 0.003) return;
    const int surfaceWidth = PanelSurfaceWidth();
    const int surfaceHeight = PanelSurfaceHeight();
    const int destSize = std::max(1, static_cast<int>(kIconMaskSize * scale + 0.5));
    const int left = centerX - destSize / 2;
    const int top = centerY - destSize / 2;
    const int sourceRed = GetRValue(color);
    const int sourceGreen = GetGValue(color);
    const int sourceBlue = GetBValue(color);

    for (int y = 0; y < destSize; ++y) {
        const int destY = top + y;
        if (destY < 0 || destY >= surfaceHeight) continue;
        const double sourceY = (static_cast<double>(y) + 0.5) *
            kIconMaskSize / destSize - 0.5;
        for (int x = 0; x < destSize; ++x) {
            const int destX = left + x;
            if (destX < 0 || destX >= surfaceWidth) continue;
            const double sourceX = (static_cast<double>(x) + 0.5) *
                kIconMaskSize / destSize - 0.5;
            const double alpha = SampleIconMaskBilinear(mask, sourceX, sourceY) /
                255.0 * opacity;
            if (alpha <= 0.003) continue;

            const size_t index = static_cast<size_t>(destY) * surfaceWidth + destX;
            const DWORD background = pixels[index];
            const int br = (background >> 16) & 0xFF;
            const int bg = (background >> 8) & 0xFF;
            const int bb = background & 0xFF;
            const int r = static_cast<int>(br + (sourceRed - br) * alpha + 0.5);
            const int g = static_cast<int>(bg + (sourceGreen - bg) * alpha + 0.5);
            const int b = static_cast<int>(bb + (sourceBlue - bb) * alpha + 0.5);
            pixels[index] = (background & 0xFF000000u) |
                (static_cast<DWORD>(r) << 16) |
                (static_cast<DWORD>(g) << 8) |
                static_cast<DWORD>(b);
        }
    }
}

void DrawMorphingTintedSprite(DWORD* pixels, int centerX, int centerY,
                              const BYTE* previousMask, const BYTE* currentMask,
                              COLORREF previousColor, COLORREF currentColor,
                              double progress) {
    if (!pixels || !previousMask || !currentMask) return;
    progress = std::max(0.0, std::min(1.0, progress));
    // Smoothstep easing produces a real frame-by-frame shape interpolation at the
    // same anchor, rather than switching between two fully separate icon scenes.
    const double eased = progress * progress * (3.0 - 2.0 * progress);
    const int surfaceWidth = PanelSurfaceWidth();
    const int surfaceHeight = PanelSurfaceHeight();
    const int left = centerX - kIconMaskSize / 2;
    const int top = centerY - kIconMaskSize / 2;
    const int red = static_cast<int>(GetRValue(previousColor) +
        (static_cast<int>(GetRValue(currentColor)) - GetRValue(previousColor)) * eased + 0.5);
    const int green = static_cast<int>(GetGValue(previousColor) +
        (static_cast<int>(GetGValue(currentColor)) - GetGValue(previousColor)) * eased + 0.5);
    const int blue = static_cast<int>(GetBValue(previousColor) +
        (static_cast<int>(GetBValue(currentColor)) - GetBValue(previousColor)) * eased + 0.5);

    for (int y = 0; y < kIconMaskSize; ++y) {
        const int destY = top + y;
        if (destY < 0 || destY >= surfaceHeight) continue;
        for (int x = 0; x < kIconMaskSize; ++x) {
            const int destX = left + x;
            if (destX < 0 || destX >= surfaceWidth) continue;
            const size_t sourceIndex = static_cast<size_t>(y) * kIconMaskSize + x;
            const double maskAlpha = (previousMask[sourceIndex] * (1.0 - eased) +
                                      currentMask[sourceIndex] * eased) / 255.0;
            if (maskAlpha <= 0.003) continue;
            const size_t destIndex = static_cast<size_t>(destY) * surfaceWidth + destX;
            const DWORD background = pixels[destIndex];
            const int br = static_cast<int>((background >> 16) & 0xFF);
            const int bg = static_cast<int>((background >> 8) & 0xFF);
            const int bb = static_cast<int>(background & 0xFF);
            const int outR = static_cast<int>(br + (red - br) * maskAlpha + 0.5);
            const int outG = static_cast<int>(bg + (green - bg) * maskAlpha + 0.5);
            const int outB = static_cast<int>(bb + (blue - bb) * maskAlpha + 0.5);
            pixels[destIndex] = (background & 0xFF000000u) |
                (static_cast<DWORD>(outR) << 16) |
                (static_cast<DWORD>(outG) << 8) |
                static_cast<DWORD>(outB);
        }
    }
}

void DrawBrightnessSprite(DWORD* pixels, int cx, int cy) {
    const ULONGLONG now = GetTickCount64();
    const double p = IconTransitionProgress(g_brightnessIconTransitionStart, now);
    const int current = std::max(0, std::min(4, g_brightnessIconState));
    const int color = std::max(0, std::min(2, g_brightnessIconColorState));
    const IconAnimationMode mode = static_cast<IconAnimationMode>(g_iconAnimationMode.load());
    if (mode == IconAnimationMode::None || !g_brightnessIconTransitionStart || p >= 1.0) {
        DrawTintedSprite(pixels, cx, cy, kSunIconMasks[current], BrightnessIconColor(color), 1.0, 1.0);
        return;
    }

    const int previous = std::max(0, std::min(4, g_previousBrightnessIconState));
    const int previousColor = std::max(0, std::min(2, g_previousBrightnessIconColorState));
    const double t = std::max(0.0, std::min(1.0,
        static_cast<double>(now - g_brightnessIconTransitionStart) /
        static_cast<double>(std::max(100, g_iconAnimationDurationMs.load()))));
    const double pop = 0.045 * std::sin(3.141592653589793 * t);
    if (mode == IconAnimationMode::CrossFade) {
        DrawTintedSprite(pixels, cx, cy, kSunIconMasks[previous], BrightnessIconColor(previousColor), 1.0, 1.0 - p);
        DrawTintedSprite(pixels, cx, cy, kSunIconMasks[current], BrightnessIconColor(color), 1.0, p);
    } else if (mode == IconAnimationMode::Pop) {
        DrawTintedSprite(pixels, cx, cy, kSunIconMasks[previous], BrightnessIconColor(previousColor), 1.0, 1.0 - p);
        DrawTintedSprite(pixels, cx, cy, kSunIconMasks[current], BrightnessIconColor(color), 0.52 + 0.48 * p + pop, p);
    } else { // Match and appear: interpolate the mask pixel-by-pixel at a fixed anchor.
        DrawMorphingTintedSprite(pixels, cx, cy, kSunIconMasks[previous],
                                 kSunIconMasks[current],
                                 BrightnessIconColor(previousColor),
                                 BrightnessIconColor(color), p);
    }
}

void DrawTemperatureSprite(DWORD* pixels, int cx, int cy) {
    const ULONGLONG now = GetTickCount64();
    const double p = IconTransitionProgress(g_temperatureIconTransitionStart, now);
    const int current = std::max(0, std::min(4, g_temperatureIconState));
    const int color = std::max(0, std::min(4, g_temperatureIconColorState));
    const IconAnimationMode mode = static_cast<IconAnimationMode>(g_iconAnimationMode.load());
    if (mode == IconAnimationMode::None || !g_temperatureIconTransitionStart || p >= 1.0) {
        DrawTintedSprite(pixels, cx, cy, kFlameIconMasks[current], TemperatureIconColor(color), 1.0, 1.0);
        return;
    }

    const int previous = std::max(0, std::min(4, g_previousTemperatureIconState));
    const int previousColor = std::max(0, std::min(4, g_previousTemperatureIconColorState));
    const double t = std::max(0.0, std::min(1.0,
        static_cast<double>(now - g_temperatureIconTransitionStart) /
        static_cast<double>(std::max(100, g_iconAnimationDurationMs.load()))));
    const double pop = 0.045 * std::sin(3.141592653589793 * t);
    if (mode == IconAnimationMode::CrossFade) {
        DrawTintedSprite(pixels, cx, cy, kFlameIconMasks[previous], TemperatureIconColor(previousColor), 1.0, 1.0 - p);
        DrawTintedSprite(pixels, cx, cy, kFlameIconMasks[current], TemperatureIconColor(color), 1.0, p);
    } else if (mode == IconAnimationMode::Pop) {
        DrawTintedSprite(pixels, cx, cy, kFlameIconMasks[previous], TemperatureIconColor(previousColor), 1.0, 1.0 - p);
        DrawTintedSprite(pixels, cx, cy, kFlameIconMasks[current], TemperatureIconColor(color), 0.52 + 0.48 * p + pop, p);
    } else { // Match and appear: interpolate the mask pixel-by-pixel at a fixed anchor.
        DrawMorphingTintedSprite(pixels, cx, cy, kFlameIconMasks[previous],
                                 kFlameIconMasks[current],
                                 TemperatureIconColor(previousColor),
                                 TemperatureIconColor(color), p);
    }
}

void DrawPanelTrack(HDC dc, int y, int value, int minValue, int maxValue) {
    const int padding = PanelSurfacePadding();
    const int left = SliderLeft() + padding;
    const int right = SliderRight() + padding;
    const int fillRight = ValueToSliderX(value, minValue, maxValue) + padding;
    const int top = y + padding - 2;
    const int bottom = y + padding + 2;

    // Thin green-grey rail with no thumb: the filled endpoint itself indicates value.
    FillPanelPill(dc, left, top, right, bottom, g_sliderTrackColor.load());
    if (fillRight > left) {
        FillPanelPill(dc, left, top, std::min(right, fillRight + 1), bottom,
                      g_sliderFillColor.load());
    }
}

void DrawPanelValue(HDC dc, PCWSTR value, int rightEdge, int centerY,
                    bool temperature) {
    HFONT font = CreateFontW(-12, 0, 0, 0, FW_SEMIBOLD, FALSE, FALSE, FALSE,
        DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY, DEFAULT_PITCH | FF_DONTCARE, L"Segoe UI");
    if (!font) return;
    HGDIOBJ oldFont = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT);
    int temperatureReadoutState = TemperatureColorStateForCurrentValue();
    COLORREF valueColor = RGB(242, 247, 245);
    if (temperature) {
        switch (temperatureReadoutState) {
            case 0: valueColor = RGB(255, 171, 97); break;
            case 1: valueColor = RGB(255, 197, 128); break;
            case 2: valueColor = RGB(250, 226, 142); break;
            case 3: valueColor = RGB(234, 238, 231); break;
            default: valueColor = RGB(128, 190, 255); break;
        }
        if (g_runtimeTemperatureK.load() == 0) valueColor = RGB(242, 247, 245);
    }
    SetTextColor(dc, valueColor);
    SIZE extent{};
    GetTextExtentPoint32W(dc, value, static_cast<int>(wcslen(value)), &extent);
    // Direct, unboxed value text: no pill fill or outline behind the percentage.
    const int padding = PanelSurfacePadding();
    TextOutW(dc, rightEdge + padding - extent.cx,
             centerY + padding - extent.cy / 2 - 1,
             value, static_cast<int>(wcslen(value)));
    SelectObject(dc, oldFont);
    DeleteObject(font);
}

void FillPanelSurface(HDC dc) {
    if (g_panelAcrylicAvailable) {
        // Deliberately draw no frame: DWM/Translucent Windows supplies the backdrop,
        // and the user's request is for a borderless glass flyout.
        return;
    }

    // Safe fallback when DWM Acrylic is unavailable. Keep it borderless as well.
    HRGN panelRegion = CreateRoundRectRgn(0, 0, kPanelWidth, kPanelHeight, 28, 28);
    if (panelRegion) {
        HBRUSH blackBrush = CreateSolidBrush(RGB(4, 5, 6));
        if (blackBrush) {
            FillRgn(dc, panelRegion, blackBrush);
            DeleteObject(blackBrush);
        }
        DeleteObject(panelRegion);
    }
}

void DrawControlPanelPixels(HDC dc, DWORD* pixels) {
    SetBkMode(dc, TRANSPARENT);
    FillPanelSurface(dc);

    // Update icon states before drawing; sprite compositing happens after the GDI text/tracks.
    UpdateIconTransitionStates();

    DrawPanelTrack(dc, kBrightnessRowY, CurrentBrightnessPercent(),
                   g_brightnessMinPercent.load(), g_brightnessMaxPercent.load());
    wchar_t brightnessText[16]{};
    swprintf(brightnessText, sizeof(brightnessText) / sizeof(brightnessText[0]),
             L"%d%%", CurrentBrightnessPercent());
    DrawPanelValue(dc, brightnessText, kValueTextRight, kBrightnessRowY, false);

    wchar_t temperatureText[16]{};
    swprintf(temperatureText, sizeof(temperatureText) / sizeof(temperatureText[0]),
             L"%d%%", CurrentTemperaturePercent());
    DrawPanelTrack(dc, kTemperatureRowY, CurrentTemperatureSliderValue(),
                   g_warmthMinKelvin.load(), g_warmthMaxKelvin.load());
    DrawPanelValue(dc, temperatureText, kValueTextRight, kTemperatureRowY, true);

    // Flush GDI's writes to the selected DIB before blending the sprites directly into
    // its pixel buffer. This avoids transparent text/slider bleed-through and gives a
    // single fully composed frame to UpdateLayeredWindow.
    GdiFlush();
    const int pixelOffset = g_panelTransparentShadowMode ? kPanelShadowPadding : 0;
    DrawBrightnessSprite(pixels, kIconTileLeft + kIconTileSize / 2 + pixelOffset,
                         kBrightnessRowY + pixelOffset);
    DrawTemperatureSprite(pixels, kIconTileLeft + kIconTileSize / 2 + pixelOffset,
                          kTemperatureRowY + pixelOffset);
}

bool IsPixelInsideRoundedRect(int x, int y, int left, int top, int right, int bottom,
                              int radius) {
    if (x < left || x >= right || y < top || y >= bottom) return false;
    radius = std::max(0, std::min(radius,
        std::min((right - left) / 2, (bottom - top) / 2)));
    if (radius == 0) return true;
    const double px = x + 0.5;
    const double py = y + 0.5;
    const double cx = std::max(static_cast<double>(left + radius),
        std::min(px, static_cast<double>(right - radius)));
    const double cy = std::max(static_cast<double>(top + radius),
        std::min(py, static_cast<double>(bottom - radius)));
    const double dx = px - cx;
    const double dy = py - cy;
    return dx * dx + dy * dy <= static_cast<double>(radius * radius);
}

int MaxColorChannel(COLORREF color) {
    return std::max({static_cast<int>(GetRValue(color)),
                     static_cast<int>(GetGValue(color)),
                     static_cast<int>(GetBValue(color))});
}

int MaxPixelChannel(DWORD rgb) {
    return std::max({static_cast<int>((rgb >> 16) & 0xFF),
                     static_cast<int>((rgb >> 8) & 0xFF),
                     static_cast<int>(rgb & 0xFF)});
}

BYTE EstimatePremultipliedAlpha(DWORD rgb, int baseChannel) {
    const int maximum = MaxPixelChannel(rgb);
    if (maximum <= 0) return 1;
    baseChannel = std::max(1, std::min(255, baseChannel));
    return static_cast<BYTE>(std::max(1, std::min(255,
        (maximum * 255 + baseChannel / 2) / baseChannel)));
}

bool IsBrightnessIconPixel(int x, int y) {
    const int cx = kIconTileLeft + kIconTileSize / 2;
    return x >= cx - 14 && x <= cx + 14 &&
           y >= kBrightnessRowY - 14 && y <= kBrightnessRowY + 14;
}

bool IsTemperatureIconPixel(int x, int y) {
    const int cx = kIconTileLeft + kIconTileSize / 2;
    return x >= cx - 14 && x <= cx + 14 &&
           y >= kTemperatureRowY - 14 && y <= kTemperatureRowY + 14;
}

bool IsPanelValuePixel(int x, int y, int rowY) {
    return x >= kPanelWidth - 55 && x < kPanelWidth - 3 &&
           y >= rowY - 10 && y <= rowY + 10;
}

bool IsPanelTrackPixel(int x, int y, int rowY) {
    return x >= SliderLeft() - 1 && x <= SliderRight() + 1 &&
           y >= rowY - 2 && y <= rowY + 2;
}

int CurrentBrightnessIconMaxChannelForBlend() {
    int result = MaxColorChannel(BrightnessIconColor(
        std::max(0, std::min(2, g_brightnessIconColorState))));
    const ULONGLONG now = GetTickCount64();
    if (g_brightnessIconTransitionStart &&
        now - g_brightnessIconTransitionStart < static_cast<ULONGLONG>(std::max(100, g_iconAnimationDurationMs.load()))) {
        result = std::max(result, MaxColorChannel(BrightnessIconColor(
            std::max(0, std::min(2, g_previousBrightnessIconColorState)))));
    }
    return result;
}

int CurrentTemperatureIconMaxChannelForBlend() {
    int result = MaxColorChannel(TemperatureIconColor(
        std::max(0, std::min(4, g_temperatureIconColorState))));
    const ULONGLONG now = GetTickCount64();
    if (g_temperatureIconTransitionStart &&
        now - g_temperatureIconTransitionStart < static_cast<ULONGLONG>(std::max(100, g_iconAnimationDurationMs.load()))) {
        result = std::max(result, MaxColorChannel(TemperatureIconColor(
            std::max(0, std::min(4, g_previousTemperatureIconColorState)))));
    }
    return result;
}

BYTE DissolveThresholdForPixel(int x, int y) {
    // Stable 2x2-cell hash: the noise pattern never changes between frames, so
    // pixels reveal progressively instead of sparkling/flickering randomly.
    const uint32_t cx = static_cast<uint32_t>(x / 2);
    const uint32_t cy = static_cast<uint32_t>(y / 2);
    uint32_t v = cx * 0x9E3779B1u ^ cy * 0x85EBCA77u ^ 0xC2B2AE3Du;
    v ^= v >> 16;
    v *= 0x7FEB352Du;
    v ^= v >> 15;
    v *= 0x846CA68Bu;
    v ^= v >> 16;
    return static_cast<BYTE>(v & 0xFFu);
}

double RoundedRectSignedDistance(double x, double y, double width,
                                 double height, double radius) {
    const double dx = std::fabs(x - width * 0.5) - (width * 0.5 - radius);
    const double dy = std::fabs(y - height * 0.5) - (height * 0.5 - radius);
    const double outsideX = std::max(0.0, dx);
    const double outsideY = std::max(0.0, dy);
    const double outside = std::sqrt(outsideX * outsideX + outsideY * outsideY);
    return outside + std::min(std::max(dx, dy), 0.0) - radius;
}

BYTE TransparentPanelShadowAlpha(int logicalX, int logicalY, int revealAlpha) {
    // A real drop shadow lives outside the invisible rounded panel. The padded
    // per-pixel canvas lets it fade out naturally instead of drawing a card fill.
    // Offset the silhouette slightly down/right, then use a quadratic falloff.
    const double px = static_cast<double>(logicalX) + 0.5 - 2.0;
    const double py = static_cast<double>(logicalY) + 0.5 - 4.0;
    const double distance = RoundedRectSignedDistance(
        px, py, static_cast<double>(kPanelWidth),
        static_cast<double>(kPanelHeight), 14.0);
    if (distance < 0.0 || distance >= kPanelShadowBlurRadius) return 0;
    const double t = 1.0 - distance / kPanelShadowBlurRadius;
    const int shadow = static_cast<int>(78.0 * t * t + 0.5);
    return static_cast<BYTE>((shadow * std::max(0, std::min(255, revealAlpha)) + 127) / 255);
}

void ApplyAcrylicContentAlpha(DWORD* pixels) {
    if (!pixels) return;
    const int revealAlpha = std::max(0, std::min(255, g_panelAnimationAlpha));
    const bool transparentShadow = g_panelTransparentShadowMode;
    const int canvasWidth = PanelSurfaceWidth();
    const int canvasHeight = PanelSurfaceHeight();
    const int padding = PanelSurfacePadding();
    const Settings materialSettings = GetSettingsSnapshot();
    // The native Desktop Acrylic material provides the blur. For Dimmer-owned mode,
    // apply tint as black alpha in the layered control surface, above the blurred host
    // and below all visible control pixels. Keep this at 1 in Follow mode so the
    // separate Translucent Windows mod's own color blend remains authoritative.
    // Keep a fixed tint for Dimmer-owned Desktop Acrylic; the removed setting
    // did not reliably control the visible glass strength.
    constexpr int kDimmerGlassTintStrengthPercent = 46;
    const int glassTintAlpha = materialSettings.followTranslucentWindowsSettings
        ? 1
        : std::max(1, (kDimmerGlassTintStrengthPercent * 0x7F + 50) / 100);
    const int brightnessIconBase = CurrentBrightnessIconMaxChannelForBlend();
    const int temperatureIconBase = CurrentTemperatureIconMaxChannelForBlend();
    const int temperatureTextBase = MaxColorChannel(
        TemperatureIconColor(TemperatureColorStateForCurrentValue()));
    const int brightnessTextBase = MaxColorChannel(RGB(242, 247, 245));

    for (int y = 0; y < canvasHeight; ++y) {
        for (int x = 0; x < canvasWidth; ++x) {
            const size_t i = static_cast<size_t>(y) * canvasWidth + x;
            const DWORD rgb = pixels[i] & 0x00FFFFFFu;
            const int logicalX = x - padding;
            const int logicalY = y - padding;
            const bool dissolveMode = g_panelAnimationMode.load() ==
                static_cast<int>(PanelAnimationMode::DissolveRise);
            const BYTE dissolveThreshold = dissolveMode
                ? DissolveThresholdForPixel(logicalX, logicalY) : 0;
            if (!transparentShadow && !IsPixelInsideRoundedRect(
                    x, y, 1, 1, kPanelWidth - 1, kPanelHeight - 1, 14)) {
                // Glass mode retains its rounded corners. Transparent-shadow mode
                // instead uses the expanded canvas to render the external shadow.
                pixels[i] = 0;
                continue;
            }

            if (rgb == 0 && transparentShadow) {
                const BYTE shadowAlpha = TransparentPanelShadowAlpha(logicalX, logicalY, revealAlpha);
                pixels[i] = static_cast<DWORD>(shadowAlpha) << 24;
                continue;
            }

            if (!g_panelAcrylicAvailable) {
                pixels[i] = 0xFF000000u | rgb;
                continue;
            }

            if (rgb == 0) {
                // A stable dissolve mask reveals the Acrylic background in cells. The
                // hidden cells retain the opaque black film until their threshold is met.
                // At full reveal this is the adjustable glass tint; during an opening
                // animation the stronger black film recedes until the selected tint remains.
                int backgroundAlpha = std::max(glassTintAlpha, 255 - revealAlpha);
                if (dissolveMode && revealAlpha < 255 && dissolveThreshold > revealAlpha) {
                    backgroundAlpha = 255;
                }
                pixels[i] = static_cast<DWORD>(backgroundAlpha) << 24;
                continue;
            }

            if (dissolveMode && revealAlpha < 255 && dissolveThreshold > revealAlpha) {
                pixels[i] = 0;
                continue;
            }

            BYTE alpha = 255;
            if (IsBrightnessIconPixel(logicalX, logicalY)) {
                alpha = EstimatePremultipliedAlpha(rgb, brightnessIconBase);
            } else if (IsTemperatureIconPixel(logicalX, logicalY)) {
                alpha = EstimatePremultipliedAlpha(rgb, temperatureIconBase);
            } else if (IsPanelValuePixel(logicalX, logicalY, kBrightnessRowY)) {
                alpha = EstimatePremultipliedAlpha(rgb, brightnessTextBase);
            } else if (IsPanelValuePixel(logicalX, logicalY, kTemperatureRowY)) {
                alpha = EstimatePremultipliedAlpha(rgb, temperatureTextBase);
            } else if (IsPanelTrackPixel(logicalX, logicalY, kBrightnessRowY) ||
                       IsPanelTrackPixel(logicalX, logicalY, kTemperatureRowY)) {
                // Track interiors are flat GDI fills and must remain fully opaque.
                alpha = 255;
            }

            // RGB from the DIB is already premultiplied for antialiased text/icons.
            // Scale both RGB and alpha for the panel's fade animation. Keep the pixel
            // premultiplied so layered-window composition doesn't produce bright fringes.
            const int red = static_cast<int>((rgb >> 16) & 0xFF);
            const int green = static_cast<int>((rgb >> 8) & 0xFF);
            const int blue = static_cast<int>(rgb & 0xFF);
            const int scaledRed = (red * revealAlpha + 127) / 255;
            const int scaledGreen = (green * revealAlpha + 127) / 255;
            const int scaledBlue = (blue * revealAlpha + 127) / 255;
            const int scaledAlpha = (static_cast<int>(alpha) * revealAlpha + 127) / 255;
            pixels[i] = (static_cast<DWORD>(scaledAlpha) << 24) |
                        (static_cast<DWORD>(scaledRed) << 16) |
                        (static_cast<DWORD>(scaledGreen) << 8) |
                        static_cast<DWORD>(scaledBlue);
        }
    }
}

void PaintControlPanel(HWND hwnd) {
    // Draw into a top-down 32-bit DIB and submit a single premultiplied-alpha frame.
    // In transparent-shadow mode, empty pixels remain fully transparent except for
    // the soft shadow pixels generated around the invisible panel outline.
    PAINTSTRUCT ps{};
    BeginPaint(hwnd, &ps);
    EndPaint(hwnd, &ps);

    HDC screenDc = GetDC(nullptr);
    if (!screenDc) {
        Wh_Log(L"Control panel GetDC failed: %lu", GetLastError());
        return;
    }

    HDC memoryDc = CreateCompatibleDC(screenDc);
    if (!memoryDc) {
        ReleaseDC(nullptr, screenDc);
        Wh_Log(L"Control panel CreateCompatibleDC failed: %lu", GetLastError());
        return;
    }

    BITMAPINFO bitmapInfo{};
    bitmapInfo.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    const bool transparentShadow = g_panelTransparentShadowMode;
    const int surfaceWidth = PanelSurfaceWidth();
    const int surfaceHeight = PanelSurfaceHeight();
    bitmapInfo.bmiHeader.biWidth = surfaceWidth;
    bitmapInfo.bmiHeader.biHeight = -surfaceHeight; // top-down DIB
    bitmapInfo.bmiHeader.biPlanes = 1;
    bitmapInfo.bmiHeader.biBitCount = 32;
    bitmapInfo.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HBITMAP bitmap = CreateDIBSection(screenDc, &bitmapInfo, DIB_RGB_COLORS,
                                      &bits, nullptr, 0);
    if (!bitmap || !bits) {
        if (bitmap) DeleteObject(bitmap);
        DeleteDC(memoryDc);
        ReleaseDC(nullptr, screenDc);
        Wh_Log(L"Control panel CreateDIBSection failed: %lu", GetLastError());
        return;
    }

    HGDIOBJ oldBitmap = SelectObject(memoryDc, bitmap);
    const size_t pixelCount = static_cast<size_t>(surfaceWidth) * surfaceHeight;
    auto* pixels = static_cast<DWORD*>(bits);
    ZeroMemory(pixels, pixelCount * sizeof(DWORD));

    // Drawing helpers offset GDI controls into the expanded transparent canvas;
    // sprite drawing below uses the same offset for direct DIB-pixel composition.
    DrawControlPanelPixels(memoryDc, pixels);
    // Transparent mode emits only a soft shadow and the foreground controls; there
    // is no acrylic tint or card fill behind empty pixels.
    ApplyAcrylicContentAlpha(pixels);

    RECT windowRect{};
    if (GetWindowRect(hwnd, &windowRect)) {
        POINT destination{windowRect.left, windowRect.top};
        POINT source{0, 0};
        SIZE size{surfaceWidth, surfaceHeight};
        const BYTE constantAlpha = g_panelAcrylicAvailable
            ? 255
            : static_cast<BYTE>(std::max(0, std::min(255, g_panelAnimationAlpha)));
        BLENDFUNCTION blend{AC_SRC_OVER, 0, constantAlpha, AC_SRC_ALPHA};
        if (!UpdateLayeredWindow(hwnd, screenDc, &destination, &size,
                                 memoryDc, &source, 0, &blend, ULW_ALPHA)) {
            Wh_Log(L"Control panel UpdateLayeredWindow failed: %lu", GetLastError());
        }
    }

    SelectObject(memoryDc, oldBitmap);
    DeleteObject(bitmap);
    DeleteDC(memoryDc);
    ReleaseDC(nullptr, screenDc);
}
void SetBrightnessFromPanelX(int x) {
    const int brightness = SliderXToValue(x, g_brightnessMinPercent.load(),
                                          g_brightnessMaxPercent.load());
    g_runtimeDimAmount.store(100 - brightness);
    g_lastPanelInteraction.store(GetTickCount64());
    if (g_panelWindow) InvalidateRect(g_panelWindow, nullptr, FALSE);
}

void SetTemperatureFromPanelX(int x) {
    const int temperature = SliderXToValue(x, g_warmthMinKelvin.load(),
                                           g_warmthMaxKelvin.load());
    g_lastNonDefaultTemperatureK.store(temperature);
    g_runtimeTemperatureK.store(temperature);
    g_lastPanelInteraction.store(GetTickCount64());
    if (g_panelWindow) InvalidateRect(g_panelWindow, nullptr, FALSE);
}

void BeginPanelHideAnimation(HWND hwnd);
bool GetVirtualScreenRect(RECT* rect);
void ClampPanelPositionToVirtualScreen(int* x, int* y);
void CalculatePanelPosition(const Settings& settings, int* outX, int* outY);
void CalculatePanelHiddenPosition(const Settings& settings, int targetX, int targetY,
                                  int* hiddenX, int* hiddenY);
void SetPanelWindowsPosition(int x, int y, UINT flags, bool updateTarget = true);
void SavePanelPosition(int x, int y);
void ApplyPanelWindowZOrder(bool behindTaskbar);

// DWMWA_SYSTEMBACKDROP_TYPE (38), DWMWA_WINDOW_CORNER_PREFERENCE (33),
// DWMWA_USE_IMMERSIVE_DARK_MODE (20), and DWMSBT_TRANSIENTWINDOW (3). Numeric
// constants keep this source compatible with older MinGW headers bundled by some
// Windhawk installations. On supported Windows 11 builds, the native DWM Desktop
// Acrylic backdrop provides the actual background blur. The layered control surface
// applies a separate black tint to its otherwise-transparent pixels, so tint strength
// remains adjustable without competing with DWM's backdrop renderer.
void ConfigurePanelBackdropEligibility(HWND hwnd, bool followTranslucentWindows) {
    if (!hwnd) return;

    LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    LONG_PTR exStyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    LONG_PTR newStyle = style;
    LONG_PTR newExStyle = exStyle;

    if (followTranslucentWindows) {
        // Translucent Windows recognizes a top-level WS_POPUPWINDOW and rejects
        // WS_EX_NOACTIVATE / WS_EX_TRANSPARENT. WM_MOUSEACTIVATE and no-activate
        // position flags still prevent this background host from taking focus.
        newStyle |= static_cast<LONG_PTR>(WS_POPUPWINDOW);
        newExStyle &= ~static_cast<LONG_PTR>(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT);
    } else {
        // Keep Dimmer's own system-backdrop request out of the other mod's
        // per-window override path. The popup remains non-activating through both
        // its style and WM_MOUSEACTIVATE / SWP_NOACTIVATE handling.
        newStyle = (newStyle & ~static_cast<LONG_PTR>(WS_POPUPWINDOW)) |
                   static_cast<LONG_PTR>(WS_POPUP);
        newExStyle |= static_cast<LONG_PTR>(WS_EX_NOACTIVATE);
    }

    bool changed = false;
    if (newStyle != style) {
        SetWindowLongPtrW(hwnd, GWL_STYLE, newStyle);
        changed = true;
    }
    if (newExStyle != exStyle) {
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, newExStyle);
        changed = true;
    }
    if (changed) {
        SetWindowPos(hwnd, nullptr, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER |
                     SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
}

bool ApplyPanelAcrylicBackdrop(HWND hwnd) {
    if (!hwnd) return false;
    const Settings settings = GetSettingsSnapshot();
    ConfigurePanelBackdropEligibility(hwnd, settings.followTranslucentWindowsSettings);

    const DWORD immersiveDarkMode = 1;
    const DWORD cornerPreference = 2; // DWMWCP_ROUND
    const DWORD noBorderColor = 0xFFFFFFFEu; // DWMWA_COLOR_NONE
    const HRESULT darkHr = DwmSetWindowAttribute(hwnd, 20, &immersiveDarkMode,
                                                  sizeof(immersiveDarkMode));

    // Extend the full frame into the client area in both modes. This is the
    // glass-compatible drawing surface used by DWM's backdrop path and by the
    // separate Translucent Windows AccentBlurBehind handoff. In WM_ERASEBKGND and
    // WM_PAINT below, explicitly paint black into this glass surface instead of
    // claiming it was erased while drawing nothing.
    const MARGINS glassMargins{-1, -1, -1, -1};
    const HRESULT frameHr = DwmExtendFrameIntoClientArea(hwnd, &glassMargins);
    HRESULT backdropHr = S_OK;
    if (settings.followTranslucentWindowsSettings) {
        // Full handoff: Translucent Windows owns the selected material and ARGB tint.
        // Do not issue a competing system-backdrop or legacy accent-blur request.
        Wh_Log(L"Dimmer backdrop handed to Translucent Windows; material/tint remain externally owned");
    } else {
        // DWMSBT_TRANSIENTWINDOW (3) maps to Desktop Acrylic on Windows 11 build
        // 22621+. The foreground layered control surface stays separate and adds
        // the user-adjustable tint only to its transparent background pixels.
        const DWORD transientBackdrop = 3; // DWMSBT_TRANSIENTWINDOW (Desktop Acrylic)
        backdropHr = DwmSetWindowAttribute(hwnd, 38, &transientBackdrop,
                                           sizeof(transientBackdrop));
        if (FAILED(backdropHr)) {
            Wh_Log(L"Dimmer Desktop Acrylic request failed: HRESULT=%08X; requires Windows 11 build 22621+",
                   static_cast<unsigned>(backdropHr));
        }
    }

    DwmSetWindowAttribute(hwnd, 33, &cornerPreference, sizeof(cornerPreference));
    DwmSetWindowAttribute(hwnd, 34, &noBorderColor, sizeof(noBorderColor));

    Wh_Log(L"Panel backdrop setup: followTranslucent=%d dark=%08X desktopAcrylic=%08X frame=%08X",
           settings.followTranslucentWindowsSettings ? 1 : 0,
           static_cast<unsigned>(darkHr), static_cast<unsigned>(backdropHr),
           static_cast<unsigned>(frameHr));

    // Follow mode is successful when the host exists and is handed off to the
    // eligible Translucent Windows handler. Dimmer-owned mode requires DWM to accept
    // the native Desktop Acrylic system-backdrop request.
    return settings.followTranslucentWindowsSettings || SUCCEEDED(backdropHr);
}

bool RecreatePanelBackdropWindowForMode(const Settings& settings) {
    if (!g_panelBackdropClassAtom) {
        Wh_Log(L"Cannot recreate panel backdrop: backdrop window class is not registered");
        return false;
    }

    RECT rect{0, 0, kPanelWidth, kPanelHeight};
    bool show = false;
    if (g_panelWindow && IsWindow(g_panelWindow)) {
        rect.left = g_panelTargetX;
        rect.top = g_panelTargetY;
        rect.right = rect.left + kPanelWidth;
        rect.bottom = rect.top + kPanelHeight;
        show = IsWindowVisible(g_panelWindow) != FALSE;
    } else if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
        RECT backdropRect{};
        if (GetWindowRect(g_panelBackdropWindow, &backdropRect)) rect = backdropRect;
        show = IsWindowVisible(g_panelBackdropWindow) != FALSE;
    }

    if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
        ShowWindow(g_panelBackdropWindow, SW_HIDE);
        DestroyWindow(g_panelBackdropWindow);
        g_panelBackdropWindow = nullptr;
    }
    g_panelAcrylicAvailable = false;

    DWORD exStyle = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
    DWORD style = WS_POPUPWINDOW;
    if (!settings.followTranslucentWindowsSettings) {
        exStyle |= WS_EX_NOACTIVATE;
        style = WS_POPUP;
    }

    HINSTANCE instance = GetModuleHandleW(nullptr);
    HWND backdrop = CreateWindowExW(
        exStyle, kPanelBackdropWindowClassName, L"Dimmer Overlay Backdrop", style,
        rect.left, rect.top, kPanelWidth, kPanelHeight,
        nullptr, nullptr, instance, nullptr);
    if (!backdrop) {
        Wh_Log(L"Backdrop recreation failed: %lu", GetLastError());
        g_panelBackdropFollowMode = !settings.followTranslucentWindowsSettings;
        if (g_panelWindow && IsWindow(g_panelWindow)) {
            InvalidateRect(g_panelWindow, nullptr, FALSE);
            UpdateWindow(g_panelWindow);
        }
        return false;
    }

    g_panelBackdropWindow = backdrop;
    HRGN region = CreateRoundRectRgn(0, 0, kPanelWidth, kPanelHeight, 28, 28);
    if (region && !SetWindowRgn(backdrop, region, TRUE)) {
        DeleteObject(region);
        Wh_Log(L"Backdrop recreation SetWindowRgn failed: %lu", GetLastError());
    }

    g_panelAcrylicAvailable = ApplyPanelAcrylicBackdrop(backdrop);
    if (!g_panelAcrylicAvailable) {
        Wh_Log(L"Backdrop recreation could not initialize Dimmer Acrylic");
        DestroyWindow(backdrop);
        g_panelBackdropWindow = nullptr;
        g_panelBackdropFollowMode = !settings.followTranslucentWindowsSettings;
        if (g_panelWindow && IsWindow(g_panelWindow)) {
            InvalidateRect(g_panelWindow, nullptr, FALSE);
            UpdateWindow(g_panelWindow);
        }
        return false;
    }

    SetWindowPos(backdrop, nullptr, rect.left, rect.top, kPanelWidth, kPanelHeight,
                 SWP_NOZORDER | SWP_NOACTIVATE | (show ? SWP_SHOWWINDOW : 0));
    if (show) {
        ShowWindow(backdrop, SW_SHOWNOACTIVATE);
        ApplyPanelWindowZOrder(g_panelAnimating && settings.panelRevealFromTaskbar);
    }
    g_panelBackdropFollowMode = settings.followTranslucentWindowsSettings;
    InvalidateRect(backdrop, nullptr, TRUE);
    UpdateWindow(backdrop);
    if (g_panelWindow && IsWindow(g_panelWindow)) {
        InvalidateRect(g_panelWindow, nullptr, FALSE);
        UpdateWindow(g_panelWindow);
    }
    Wh_Log(L"Panel backdrop recreated for mode: followTranslucent=%d",
           settings.followTranslucentWindowsSettings ? 1 : 0);
    return true;
}

LRESULT CALLBACK PanelBackdropWindowProc(HWND hwnd, UINT message,
                                          WPARAM wParam, LPARAM lParam) {
    switch (message) {
        case WM_NCCALCSIZE:
            // WS_POPUPWINDOW is used so Translucent Windows recognizes this top-level
            // host as eligible. Keep the full window as client area so its style bits
            // do not create a visible border around the glass surface.
            return 0;
        case WM_NCPAINT:
            // The DWM frame/backdrop supplies the outer appearance; do not paint a
            // classic non-client frame over the translucent material.
            return 0;
        case WM_ERASEBKGND: {
            // The glass-frame path needs an explicit black client surface. Returning
            // 1 without painting suppresses the class brush and leaves an opaque/
            // undefined backing surface, which can hide the backdrop even when DWM
            // accepts the backdrop attribute. Black is the glass-compatible base.
            RECT clientRect{};
            GetClientRect(hwnd, &clientRect);
            FillRect(reinterpret_cast<HDC>(wParam), &clientRect,
                     static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));
            return 1;
        }
        case WM_PAINT: {
            PAINTSTRUCT ps{};
            HDC dc = BeginPaint(hwnd, &ps);
            RECT clientRect{};
            GetClientRect(hwnd, &clientRect);
            FillRect(dc, &clientRect,
                     static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH)));
            EndPaint(hwnd, &ps);
            return 0;
        }
        case WM_DWMCOMPOSITIONCHANGED:
            g_panelAcrylicAvailable = ApplyPanelAcrylicBackdrop(hwnd);
            InvalidateRect(hwnd, nullptr, TRUE);
            if (g_panelWindow && IsWindow(g_panelWindow)) {
                InvalidateRect(g_panelWindow, nullptr, FALSE);
            }
            return 0;
        case WM_MOUSEACTIVATE:
            return MA_NOACTIVATE;
        default:
            return DefWindowProcW(hwnd, message, wParam, lParam);
    }
}

LRESULT CALLBACK ControlPanelWindowProc(HWND hwnd, UINT message, WPARAM wParam, LPARAM lParam) {
    switch (message) {
        case WM_PAINT:
            PaintControlPanel(hwnd);
            return 0;
        case WM_ERASEBKGND:
            // No GDI background erase is needed; UpdateLayeredWindow paints the content layer.
            return 1;
        case WM_MOUSEACTIVATE:
            return MA_NOACTIVATE;
        case WM_NCHITTEST: {
            if (g_panelTransparentShadowMode) {
                const int screenX = GET_X_LPARAM(lParam);
                const int screenY = GET_Y_LPARAM(lParam);
                RECT rect{};
                if (GetWindowRect(hwnd, &rect)) {
                    const int logicalX = screenX - rect.left - kPanelShadowPadding;
                    const int logicalY = screenY - rect.top - kPanelShadowPadding;
                    if (logicalX < 0 || logicalY < 0 ||
                        logicalX >= kPanelWidth || logicalY >= kPanelHeight) {
                        return HTTRANSPARENT; // shadow passes clicks through to the app below
                    }
                }
            }
            return HTCLIENT;
        }
        case WM_MOUSEMOVE:
            g_lastPanelInteraction.store(GetTickCount64());
            if (g_draggingPanel) {
                POINT cursor{};
                if (GetCursorPos(&cursor)) {
                    const int newX = g_panelDragStartX + cursor.x - g_panelDragStartCursor.x;
                    const int newY = g_panelDragStartY + cursor.y - g_panelDragStartCursor.y;
                    int clampedX = newX, clampedY = newY;
                    ClampPanelPositionToVirtualScreen(&clampedX, &clampedY);
                    SetPanelWindowsPosition(clampedX, clampedY, 0);
                }
            } else if (g_draggingBrightness) {
                SetBrightnessFromPanelX(GET_X_LPARAM(lParam) - PanelSurfacePadding());
            } else if (g_draggingTemperature) {
                SetTemperatureFromPanelX(GET_X_LPARAM(lParam) - PanelSurfacePadding());
            }
            return 0;
        case WM_LBUTTONDOWN: {
            const int x = GET_X_LPARAM(lParam) - PanelSurfacePadding();
            const int y = GET_Y_LPARAM(lParam) - PanelSurfacePadding();
            g_lastPanelInteraction.store(GetTickCount64());
            // Ctrl+left-drag anywhere in the panel moves it. Without Ctrl, the icon
            // and sliders keep their ordinary click/drag behavior.
            if ((GetKeyState(VK_CONTROL) & 0x8000) != 0) {
                RECT panelRect{};
                POINT cursor{};
                if (GetLogicalPanelRect(hwnd, &panelRect) && GetCursorPos(&cursor)) {
                    g_draggingPanel = true;
                    g_panelDragStartCursor = cursor;
                    g_panelDragStartX = panelRect.left;
                    g_panelDragStartY = panelRect.top;
                    SetCapture(hwnd);
                }
                return 0;
            }
            if (x >= kIconTileLeft && x < kIconTileLeft + kIconTileSize &&
                y >= kTemperatureRowY - kIconTileSize / 2 &&
                y < kTemperatureRowY + kIconTileSize / 2) {
                const int currentTemperature = g_runtimeTemperatureK.load();
                if (currentTemperature == 0) {
                    g_runtimeTemperatureK.store(g_lastNonDefaultTemperatureK.load());
                } else {
                    g_lastNonDefaultTemperatureK.store(currentTemperature);
                    g_runtimeTemperatureK.store(0);
                }
                InvalidateRect(hwnd, nullptr, FALSE);
                return 0;
            }
            if (x >= SliderLeft() - 12 && x <= SliderRight() + 12 && y >= kBrightnessRowY - 14 && y <= kBrightnessRowY + 14) {
                g_draggingBrightness = true;
                SetCapture(hwnd);
                SetBrightnessFromPanelX(x);
                return 0;
            }
            if (x >= SliderLeft() - 12 && x <= SliderRight() + 12 && y >= kTemperatureRowY - 14 && y <= kTemperatureRowY + 14) {
                g_draggingTemperature = true;
                SetCapture(hwnd);
                SetTemperatureFromPanelX(x);
                return 0;
            }
            return 0;
        }
        case WM_LBUTTONUP:
            if (g_draggingPanel) {
                g_draggingPanel = false;
                SavePanelPosition(g_panelTargetX, g_panelTargetY);
            }
            g_draggingBrightness = false;
            g_draggingTemperature = false;
            if (GetCapture() == hwnd) ReleaseCapture();
            g_lastPanelInteraction.store(GetTickCount64());
            return 0;
        case WM_CAPTURECHANGED:
            g_draggingPanel = false;
            g_draggingBrightness = false;
            g_draggingTemperature = false;
            return 0;
        case kPanelSettingsChangedMessage: {
            const Settings settings = GetSettingsSnapshot();
            const bool useTransparentShadowPanel = ShouldUseTransparentShadowPanel(settings);
            if (useTransparentShadowPanel != g_panelTransparentShadowMode) {
                // Read the logical position using the old surface geometry before
                // changing the shadow padding / client-layer size.
                RECT logicalRect{};
                if (GetLogicalPanelRect(hwnd, &logicalRect)) {
                    g_panelTargetX = logicalRect.left;
                    g_panelTargetY = logicalRect.top;
                }
                g_panelTransparentShadowMode = useTransparentShadowPanel;
                if (g_panelTransparentShadowMode) {
                    if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
                        ShowWindow(g_panelBackdropWindow, SW_HIDE);
                        DestroyWindow(g_panelBackdropWindow);
                        g_panelBackdropWindow = nullptr;
                    }
                    g_panelAcrylicAvailable = true; // transparent per-pixel rendering, no glass host
                    g_panelBackdropFollowMode = false; // transparent-shadow mode can only be active when Follow is OFF
                    Wh_Log(L"Transparent shadow panel enabled; Follow Translucent Windows is OFF, so no background material window is used");
                } else {
                    // Follow mode always needs its own eligible host so Translucent
                    // Windows can apply its selected material. Own-glass mode uses it too.
                    g_panelAcrylicAvailable = false;
                    RecreatePanelBackdropWindowForMode(settings);
                }
                ApplyControlPanelWindowRegion(hwnd);
                SetPanelWindowsPosition(g_panelTargetX, g_panelTargetY, 0, false);
            } else if (!useTransparentShadowPanel) {
                if (settings.followTranslucentWindowsSettings != g_panelBackdropFollowMode) {
                    // Recreate host so Translucent Windows sees the selected mode at creation.
                    RecreatePanelBackdropWindowForMode(settings);
                } else if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
                    g_panelAcrylicAvailable = ApplyPanelAcrylicBackdrop(g_panelBackdropWindow);
                    InvalidateRect(g_panelBackdropWindow, nullptr, TRUE);
                    UpdateWindow(g_panelBackdropWindow);
                }
            } else {
                // The transparent-shadow style deliberately ignores glass-only settings.
                g_panelAcrylicAvailable = true;
            }
            if (IsWindowVisible(hwnd)) {
                if (g_panelAnimating && !g_panelHiding) {
                    // Preserve the current on-screen position as the new animation
                    // origin and retarget smoothly. Without this, placement changes
                    // made during a reveal could be dropped until the next opening.
                    RECT currentRect{};
                    if (GetLogicalPanelRect(hwnd, &currentRect)) {
                        CalculatePanelPosition(settings, &g_panelTargetX, &g_panelTargetY);
                        CalculatePanelHiddenPosition(settings, g_panelTargetX, g_panelTargetY,
                                                     &g_panelHiddenX, &g_panelHiddenY);
                        g_panelStartX = currentRect.left;
                        g_panelStartY = currentRect.top;
                        g_panelAnimationStart = GetTickCount64();
                        g_panelAnimationStartAlpha = g_panelAnimationAlpha;
                        if (settings.panelRevealFromTaskbar) {
                            ApplyPanelWindowZOrder(true);
                        }
                    }
                } else if (!g_panelAnimating) {
                    CalculatePanelPosition(settings, &g_panelTargetX, &g_panelTargetY);
                    SetPanelWindowsPosition(g_panelTargetX, g_panelTargetY, 0);
                    ApplyPanelWindowZOrder(false);
                }
            }
            InvalidateRect(hwnd, nullptr, FALSE);
            UpdateWindow(hwnd);
            return 0;
        }
        case WM_TIMER:
            if (wParam == kIconAnimationTimerId) {
                const ULONGLONG now = GetTickCount64();
                const ULONGLONG iconDuration = static_cast<ULONGLONG>(
                    std::max(100, g_iconAnimationDurationMs.load()));
                const bool brightnessAnimating = g_brightnessIconTransitionStart &&
                    now - g_brightnessIconTransitionStart < iconDuration;
                const bool temperatureAnimating = g_temperatureIconTransitionStart &&
                    now - g_temperatureIconTransitionStart < iconDuration;
                InvalidateRect(hwnd, nullptr, FALSE);
                UpdateWindow(hwnd);
                if (!brightnessAnimating && !temperatureAnimating) {
                    KillTimer(hwnd, kIconAnimationTimerId);
                    g_brightnessIconTransitionStart = 0;
                    g_temperatureIconTransitionStart = 0;
                }
                return 0;
            }
            if (wParam == kPanelAnimationTimerId) {
                const ULONGLONG elapsed = GetTickCount64() - g_panelAnimationStart;
                const int animationDuration = std::max(100, g_panelAnimationDurationMs.load());
                const double t = std::min(1.0, static_cast<double>(elapsed) /
                                                   static_cast<double>(animationDuration));
                // Quintic smoothstep: zero velocity/acceleration at both ends.
                const double eased = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
                const PanelAnimationMode mode = static_cast<PanelAnimationMode>(g_panelAnimationMode.load());
                const Settings animationSettings = GetSettingsSnapshot();
                const bool hasMotionStyle = mode == PanelAnimationMode::DissolveRise ||
                                            mode == PanelAnimationMode::FadeRise ||
                                            mode == PanelAnimationMode::Slide;
                const bool movement = mode != PanelAnimationMode::None &&
                                      (hasMotionStyle || animationSettings.panelRevealFromTaskbar);
                const bool alphaAnimation = mode == PanelAnimationMode::DissolveRise ||
                                            mode == PanelAnimationMode::FadeRise ||
                                            mode == PanelAnimationMode::Fade;
                const int targetX = (movement && g_panelHiding) ? g_panelHiddenX : g_panelTargetX;
                const int targetY = (movement && g_panelHiding) ? g_panelHiddenY : g_panelTargetY;
                const double positionProgress = movement ? eased : 1.0;
                const double xValue = g_panelStartX + (targetX - g_panelStartX) * positionProgress;
                const double yValue = g_panelStartY + (targetY - g_panelStartY) * positionProgress;
                const int newX = static_cast<int>(xValue >= 0 ? xValue + 0.5 : xValue - 0.5);
                const int newY = static_cast<int>(yValue >= 0 ? yValue + 0.5 : yValue - 0.5);
                const int targetAlpha = (alphaAnimation && g_panelHiding) ? 0 : 255;
                const double alphaValue = g_panelAnimationStartAlpha +
                    (targetAlpha - g_panelAnimationStartAlpha) * (alphaAnimation ? eased : 1.0);
                g_panelAnimationAlpha = std::max(0, std::min(255, static_cast<int>(alphaValue + 0.5)));

                // Use the same geometry-aware helper as drag/reposition operations.
                // It offsets the layered surface by shadow padding and keeps its
                // expanded dimensions stable on every frame, preventing jumps at the
                // beginning/end of reveal and hide animations.
                SetPanelWindowsPosition(newX, newY, SWP_NOACTIVATE, false);
                // Recompose the control layer each frame for a smooth alpha + rise motion.
                InvalidateRect(hwnd, nullptr, FALSE);
                UpdateWindow(hwnd);

                if (t >= 1.0) {
                    KillTimer(hwnd, kPanelAnimationTimerId);
                    g_panelAnimating = false;
                    if (g_panelHiding) {
                        g_panelAnimationAlpha = 0;
                        ShowWindow(hwnd, SW_HIDE);
                        if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
                            ShowWindow(g_panelBackdropWindow, SW_HIDE);
                        }
                        ApplyPanelWindowZOrder(false);
                    } else {
                        g_panelAnimationAlpha = 255;
                        SetPanelWindowsPosition(g_panelTargetX, g_panelTargetY,
                                                SWP_NOACTIVATE);
                        ApplyPanelWindowZOrder(false);
                        InvalidateRect(hwnd, nullptr, FALSE);
                        UpdateWindow(hwnd);
                        g_lastPanelInteraction.store(GetTickCount64());
                    }
                }
                return 0;
            }
            break;
        case kPanelOutsideClickMessage:
            BeginPanelHideAnimation(hwnd);
            return 0;
        case WM_CLOSE:
            BeginPanelHideAnimation(hwnd);
            return 0;
        case WM_NCDESTROY:
            KillTimer(hwnd, kPanelAnimationTimerId);
            KillTimer(hwnd, kIconAnimationTimerId);
            g_draggingPanel = false;
            g_draggingBrightness = false;
            g_draggingTemperature = false;
            g_panelAnimating = false;
            return DefWindowProcW(hwnd, message, wParam, lParam);
        default:
            return DefWindowProcW(hwnd, message, wParam, lParam);
    }

    // A WM_TIMER message with an unrecognized timer ID reaches this fallback.
    return DefWindowProcW(hwnd, message, wParam, lParam);
}

void ClampPanelPositionToVirtualScreen(int* x, int* y) {
    if (!x || !y) return;
    RECT screen{};
    if (!GetVirtualScreenRect(&screen)) {
        screen = {0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)};
    }
    const int minX = screen.left;
    const int minY = screen.top;
    const int screenRight = static_cast<int>(screen.right);
    const int screenBottom = static_cast<int>(screen.bottom);
    const int maxX = std::max(minX, screenRight - kPanelWidth);
    const int maxY = std::max(minY, screenBottom - kPanelHeight);
    *x = std::max(minX, std::min(maxX, *x));
    *y = std::max(minY, std::min(maxY, *y));
}

void CalculatePanelPosition(const Settings& settings, int* outX, int* outY) {
    if (!outX || !outY) return;
    if (settings.rememberPanelPosition && g_hasSavedPanelPosition.load()) {
        *outX = g_savedPanelX.load();
        *outY = g_savedPanelY.load();
        ClampPanelPositionToVirtualScreen(outX, outY);
        return;
    }

    RECT screen{};
    if (!GetVirtualScreenRect(&screen)) {
        screen = {0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)};
    }
    RECT work{};
    if (!SystemParametersInfoW(SPI_GETWORKAREA, 0, &work, 0)) work = screen;

    HWND taskbar = FindWindowW(L"Shell_TrayWnd", nullptr);
    RECT taskbarRect{};
    const bool haveTaskbar = taskbar && GetWindowRect(taskbar, &taskbarRect) &&
                             !IsRectEmpty(&taskbarRect);
    const int xCenter = work.left + ((work.right - work.left) - kPanelWidth) / 2;
    int x = xCenter;
    int y = std::max(work.top, work.bottom - kPanelHeight - 6);

    if (settings.panelPositionMode == PanelPositionMode::CustomCoordinates) {
        x = settings.panelCustomX;
        y = settings.panelCustomY;
    } else if (settings.panelPositionMode == PanelPositionMode::CenterScreen) {
        x = screen.left + ((screen.right - screen.left) - kPanelWidth) / 2;
        y = screen.top + ((screen.bottom - screen.top) - kPanelHeight) / 2;
    } else if (haveTaskbar) {
        const int taskbarWidth = taskbarRect.right - taskbarRect.left;
        const int taskbarHeight = taskbarRect.bottom - taskbarRect.top;
        const bool horizontal = taskbarWidth >= taskbarHeight;
        if (horizontal) {
            x = taskbarRect.left + (taskbarWidth - kPanelWidth) / 2;
            const bool taskbarAtTop = taskbarRect.top <= screen.top + 32;
            const bool taskbarAtBottom = taskbarRect.bottom >= screen.bottom - 32;
            if (settings.panelPositionMode == PanelPositionMode::OverTaskbar) {
                y = taskbarRect.top + (taskbarHeight - kPanelHeight) / 2;
            } else if (settings.panelPositionMode == PanelPositionMode::BelowTaskbar) {
                y = taskbarRect.bottom + 6;
                if (taskbarAtBottom) y = screen.bottom - kPanelHeight;
            } else { // Above taskbar; for a top-docked taskbar this means below it.
                y = taskbarAtTop ? taskbarRect.bottom + 6 : taskbarRect.top - kPanelHeight - 6;
                if (!taskbarAtTop && !taskbarAtBottom) {
                    y = taskbarRect.top - kPanelHeight - 6;
                }
            }
        } else {
            const bool taskbarAtLeft = taskbarRect.left <= screen.left + 32;
            const bool taskbarAtRight = taskbarRect.right >= screen.right - 32;
            y = taskbarRect.top + (taskbarHeight - kPanelHeight) / 2;
            if (settings.panelPositionMode == PanelPositionMode::OverTaskbar) {
                x = taskbarRect.left + (taskbarWidth - kPanelWidth) / 2;
            } else if (settings.panelPositionMode == PanelPositionMode::BelowTaskbar) {
                x = taskbarAtLeft ? taskbarRect.right + 6 : screen.right - kPanelWidth;
            } else {
                x = taskbarAtLeft ? taskbarRect.right + 6 : taskbarRect.left - kPanelWidth - 6;
                if (!taskbarAtLeft && !taskbarAtRight) x = taskbarRect.left - kPanelWidth - 6;
            }
        }
    }

    ClampPanelPositionToVirtualScreen(&x, &y);
    *outX = x;
    *outY = y;
}

void CalculatePanelHiddenPosition(const Settings& settings, int targetX, int targetY,
                                  int* hiddenX, int* hiddenY) {
    if (!hiddenX || !hiddenY) return;
    *hiddenX = targetX;
    *hiddenY = targetY + kPanelSlideOffset;
    if (!settings.panelRevealFromTaskbar) return;

    RECT screen{};
    if (!GetVirtualScreenRect(&screen)) {
        screen = {0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)};
    }
    HWND taskbar = FindWindowW(L"Shell_TrayWnd", nullptr);
    RECT taskbarRect{};
    if (!taskbar || !GetWindowRect(taskbar, &taskbarRect) || IsRectEmpty(&taskbarRect)) {
        return;
    }

    const int taskbarWidth = taskbarRect.right - taskbarRect.left;
    const int taskbarHeight = taskbarRect.bottom - taskbarRect.top;
    if (taskbarWidth >= taskbarHeight) {
        const bool taskbarAtTop = taskbarRect.top <= screen.top + 32;
        const bool taskbarAtBottom = taskbarRect.bottom >= screen.bottom - 32;
        if (taskbarAtBottom) {
            // The panel is taller than the taskbar. Aligning its top with the
            // taskbar top leaves a strip visible through a translucent taskbar and
            // can equal a remembered panel position, making the reveal look stuck.
            // Start just beyond the virtual-screen edge so the panel is truly
            // concealed, then animate it to the configured target position.
            *hiddenY = std::max(static_cast<int>(screen.bottom),
                                static_cast<int>(taskbarRect.bottom)) + 2;
        } else if (taskbarAtTop) {
            *hiddenY = std::min(static_cast<int>(screen.top),
                                static_cast<int>(taskbarRect.top)) - kPanelHeight - 2;
        } else if (targetY + kPanelHeight <= taskbarRect.top) {
            *hiddenY = taskbarRect.top - kPanelHeight - 2;
        } else {
            *hiddenY = taskbarRect.bottom + 2;
        }
    } else {
        const bool taskbarAtLeft = taskbarRect.left <= screen.left + 32;
        const bool taskbarAtRight = taskbarRect.right >= screen.right - 32;
        if (taskbarAtLeft) {
            *hiddenX = std::min(static_cast<int>(screen.left),
                                static_cast<int>(taskbarRect.left)) - kPanelWidth - 2;
        } else if (taskbarAtRight) {
            *hiddenX = std::max(static_cast<int>(screen.right),
                                static_cast<int>(taskbarRect.right)) + 2;
        } else if (targetX + kPanelWidth <= taskbarRect.left) {
            *hiddenX = taskbarRect.left - kPanelWidth - 2;
        } else {
            *hiddenX = taskbarRect.right + 2;
        }
    }
}

void ApplyPanelWindowZOrder(bool behindTaskbar) {
    HWND taskbar = FindWindowW(L"Shell_TrayWnd", nullptr);
    const bool taskbarCanCoverPanel = taskbar &&
        (GetWindowLongPtrW(taskbar, GWL_EXSTYLE) & WS_EX_TOPMOST) != 0;
    if (behindTaskbar && taskbarCanCoverPanel) {
        // Z order: taskbar -> controls -> backdrop. Both panel windows remain in
        // the topmost band but the taskbar covers the portion crossing its edge.
        if (g_panelWindow && IsWindow(g_panelWindow)) {
            SetWindowPos(g_panelWindow, taskbar, 0, 0, 0, 0,
                         SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
        if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
            HWND insertAfter = (g_panelWindow && IsWindow(g_panelWindow)) ? g_panelWindow : taskbar;
            SetWindowPos(g_panelBackdropWindow, insertAfter, 0, 0, 0, 0,
                         SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
        }
        return;
    }

    if (behindTaskbar && !g_loggedBehindTaskbarFallback) {
        Wh_Log(L"Taskbar is not in the topmost band; using edge animation without taskbar occlusion");
        g_loggedBehindTaskbarFallback = true;
    }
    if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) {
        SetWindowPos(g_panelBackdropWindow, HWND_TOPMOST, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
    if (g_panelWindow && IsWindow(g_panelWindow)) {
        SetWindowPos(g_panelWindow, HWND_TOPMOST, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
    }
}

void SetPanelWindowsPosition(int x, int y, UINT flags, bool updateTarget) {
    // Animation staging moves the windows without changing the final target.
    // Keep that distinction so an off-screen behind-taskbar start doesn't
    // overwrite the position the reveal animation must travel toward.
    if (updateTarget) {
        g_panelTargetX = x;
        g_panelTargetY = y;
    }

    const bool hasBackdrop = g_panelBackdropWindow && IsWindow(g_panelBackdropWindow);
    const bool hasPanel = g_panelWindow && IsWindow(g_panelWindow);
    if (!hasBackdrop && !hasPanel) return;

    const UINT positionFlags = flags | SWP_NOZORDER | SWP_NOACTIVATE;
    // Batch the backdrop and control surface into one z-order-preserving move so
    // they stay aligned throughout the open/close animation. Their sizes differ in
    // transparent-shadow mode because the control surface includes shadow padding.
    HDWP move = BeginDeferWindowPos(static_cast<int>(hasBackdrop) + static_cast<int>(hasPanel));
    if (move && hasBackdrop) {
        move = DeferWindowPos(move, g_panelBackdropWindow, nullptr,
                              x, y, kPanelWidth, kPanelHeight, positionFlags);
    }
    if (move && hasPanel) {
        const int pad = PanelSurfacePadding();
        move = DeferWindowPos(move, g_panelWindow, nullptr, x - pad, y - pad,
                              PanelSurfaceWidth(), PanelSurfaceHeight(), positionFlags);
    }
    if (move && EndDeferWindowPos(move)) return;

    // If the deferred transaction cannot be queued, fall back to individual moves.
    if (hasBackdrop) {
        SetWindowPos(g_panelBackdropWindow, nullptr, x, y, kPanelWidth, kPanelHeight,
                     positionFlags);
    }
    if (hasPanel) {
        const int pad = PanelSurfacePadding();
        SetWindowPos(g_panelWindow, nullptr, x - pad, y - pad,
                     PanelSurfaceWidth(), PanelSurfaceHeight(), positionFlags);
    }
}

void SavePanelPosition(int x, int y) {
    ClampPanelPositionToVirtualScreen(&x, &y);
    const Settings settings = GetSettingsSnapshot();
    if (!settings.rememberPanelPosition) {
        g_hasSavedPanelPosition.store(false);
        Wh_SetIntValue(L"PanelHasSavedPosition", 0);
        return;
    }
    g_savedPanelX.store(x);
    g_savedPanelY.store(y);
    g_hasSavedPanelPosition.store(true);
    Wh_SetIntValue(L"PanelSavedX", x);
    Wh_SetIntValue(L"PanelSavedY", y);
    Wh_SetIntValue(L"PanelHasSavedPosition", 1);
    Wh_SetIntValue(L"PanelStoredPositionMode", static_cast<int>(settings.panelPositionMode));
    Wh_SetIntValue(L"PanelStoredCustomX", settings.panelCustomX);
    Wh_SetIntValue(L"PanelStoredCustomY", settings.panelCustomY);
}

void StartPanelAnimation(HWND hwnd, bool hide) {
    if (!hwnd || !IsWindow(hwnd)) return;
    const Settings settings = GetSettingsSnapshot();
    const PanelAnimationMode mode = settings.panelAnimation;
    const bool hasMotionStyle = mode == PanelAnimationMode::DissolveRise ||
                                mode == PanelAnimationMode::FadeRise ||
                                mode == PanelAnimationMode::Slide;
    // The behind-taskbar option owns the reveal/exit movement independently
    // from the selected visual style (except explicit "No animation").
    const bool movement = mode != PanelAnimationMode::None &&
                          (hasMotionStyle || settings.panelRevealFromTaskbar);
    const bool alphaAnimation = mode == PanelAnimationMode::DissolveRise ||
                                mode == PanelAnimationMode::FadeRise ||
                                mode == PanelAnimationMode::Fade;
    RECT currentRect{};
    const bool wasVisible = IsWindowVisible(hwnd) != FALSE;

    if (!hide || !wasVisible) {
        CalculatePanelPosition(settings, &g_panelTargetX, &g_panelTargetY);
    } else if (GetLogicalPanelRect(hwnd, &currentRect)) {
        // Keep the current logical panel location for the closing trajectory,
        // excluding the transparent shadow padding around the window.
        g_panelTargetX = currentRect.left;
        g_panelTargetY = currentRect.top;
    }
    CalculatePanelHiddenPosition(settings, g_panelTargetX, g_panelTargetY,
                                  &g_panelHiddenX, &g_panelHiddenY);

    if (g_panelAnimating) KillTimer(hwnd, kPanelAnimationTimerId);

    if (mode == PanelAnimationMode::None) {
        g_panelAnimating = false;
        g_panelHiding = false;
        if (hide) {
            g_panelAnimationAlpha = 0;
            ShowWindow(hwnd, SW_HIDE);
            if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) ShowWindow(g_panelBackdropWindow, SW_HIDE);
            ApplyPanelWindowZOrder(false);
        } else {
            g_panelAnimationAlpha = 255;
            SetPanelWindowsPosition(g_panelTargetX, g_panelTargetY, SWP_SHOWWINDOW);
            if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) ShowWindow(g_panelBackdropWindow, SW_SHOWNOACTIVATE);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            ApplyPanelWindowZOrder(false);
            InvalidateRect(hwnd, nullptr, FALSE);
            UpdateWindow(hwnd);
            g_lastPanelInteraction.store(GetTickCount64());
        }
        return;
    }

    if (wasVisible && GetLogicalPanelRect(hwnd, &currentRect)) {
        g_panelStartX = currentRect.left;
        g_panelStartY = currentRect.top;
    } else if (movement) {
        g_panelStartX = g_panelHiddenX;
        g_panelStartY = g_panelHiddenY;
    } else {
        g_panelStartX = g_panelTargetX;
        g_panelStartY = g_panelTargetY;
    }

    if (hide) {
        if (!wasVisible) return;
        g_panelHiding = true;
        g_panelAnimationStartAlpha = g_panelAnimationAlpha;
        if (movement && settings.panelRevealFromTaskbar) {
            ApplyPanelWindowZOrder(true);
        }
    } else {
        g_panelHiding = false;
        if (!wasVisible) g_panelAnimationAlpha = alphaAnimation ? 0 : 255;
        g_panelAnimationStartAlpha = g_panelAnimationAlpha;
        if (movement && settings.panelRevealFromTaskbar) {
            // Set the Z order before showing so the taskbar can naturally occlude
            // the part of the panel that begins underneath it.
            ApplyPanelWindowZOrder(true);
        }
        const int startX = movement && !wasVisible ? g_panelHiddenX : g_panelStartX;
        const int startY = movement && !wasVisible ? g_panelHiddenY : g_panelStartY;
        const UINT showFlags = SWP_NOACTIVATE | (wasVisible ? 0 : SWP_SHOWWINDOW);
        SetPanelWindowsPosition(startX, startY, showFlags, false);
        if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow) && !IsWindowVisible(g_panelBackdropWindow)) {
            ShowWindow(g_panelBackdropWindow, SW_SHOWNOACTIVATE);
        }
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        InvalidateRect(hwnd, nullptr, FALSE);
        UpdateWindow(hwnd);
    }

    g_panelAnimationStart = GetTickCount64();
    g_panelAnimating = true;
    if (!SetTimer(hwnd, kPanelAnimationTimerId, kPanelAnimationIntervalMs, nullptr)) {
        g_panelAnimating = false;
        if (hide) {
            g_panelAnimationAlpha = 0;
            ShowWindow(hwnd, SW_HIDE);
            if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) ShowWindow(g_panelBackdropWindow, SW_HIDE);
            ApplyPanelWindowZOrder(false);
        } else {
            g_panelHiding = false;
            g_panelAnimationAlpha = 255;
            SetPanelWindowsPosition(g_panelTargetX, g_panelTargetY, SWP_SHOWWINDOW);
            if (g_panelBackdropWindow && IsWindow(g_panelBackdropWindow)) ShowWindow(g_panelBackdropWindow, SW_SHOWNOACTIVATE);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            ApplyPanelWindowZOrder(false);
            InvalidateRect(hwnd, nullptr, FALSE);
            UpdateWindow(hwnd);
            g_lastPanelInteraction.store(GetTickCount64());
        }
    }
}

void BeginPanelHideAnimation(HWND hwnd) {
    if (!hwnd || !IsWindow(hwnd) || !IsWindowVisible(hwnd) || g_panelHiding) return;
    if (GetCapture() == hwnd) ReleaseCapture();
    g_draggingBrightness = false;
    g_draggingTemperature = false;
    StartPanelAnimation(hwnd, true);
}

LRESULT CALLBACK PanelOutsideClickMouseHook(int code, WPARAM wParam, LPARAM lParam) {
    if (code == HC_ACTION &&
        (wParam == WM_LBUTTONDOWN || wParam == WM_RBUTTONDOWN ||
         wParam == WM_MBUTTONDOWN || wParam == WM_XBUTTONDOWN)) {
        HWND panel = g_panelWindow;
        const bool controlCenterOwnsPanel = g_panelControlCenterManaged &&
            g_controlCenterOpen && !g_panelSuppressedForControlCenter;
        if (!controlCenterOwnsPanel && panel && IsWindow(panel) &&
            IsWindowVisible(panel) && !g_panelHiding) {
            const auto* mouse = reinterpret_cast<const MSLLHOOKSTRUCT*>(lParam);
            RECT panelRect{};
            if (mouse && GetLogicalPanelRect(panel, &panelRect) &&
                !PtInRect(&panelRect, mouse->pt)) {
                // Post to the panel thread so this low-level hook stays quick and
                // the mouse click itself continues to the application underneath.
                PostMessageW(panel, kPanelOutsideClickMessage, 0, 0);
            }
        }
    }
    return CallNextHookEx(g_panelMouseHook, code, wParam, lParam);
}

void ToggleControlPanel() {
    if (!g_panelWindow || !IsWindow(g_panelWindow)) {
        Wh_Log(L"Display-control panel is not available");
        return;
    }

    const Settings settings = GetSettingsSnapshot();
    const bool linkedToOpenControlCenter = settings.showPanelWithControlCenter && g_controlCenterOpen;
    if (linkedToOpenControlCenter) {
        // Ctrl+Alt+B is always a manual invocation: auto-hide remains enabled.
        // Do not let the polling synchronizer immediately reclaim the panel.
        g_panelControlCenterManaged = false;
        g_panelSuppressedForControlCenter = true;
        g_panelCloseWhenControlCenterCloses = true;
    } else {
        g_panelControlCenterManaged = false;
        g_panelSuppressedForControlCenter = false;
        g_panelCloseWhenControlCenterCloses = false;
    }

    if (IsWindowVisible(g_panelWindow)) {
        if (g_panelHiding) {
            StartPanelAnimation(g_panelWindow, false);
        } else {
            BeginPanelHideAnimation(g_panelWindow);
        }
        return;
    }

    StartPanelAnimation(g_panelWindow, false);
    g_lastPanelInteraction.store(GetTickCount64());
}

void MaintainPanelVisibility() {
    HWND panel = g_panelWindow;
    if (g_panelControlCenterManaged && g_controlCenterOpen &&
        !g_panelSuppressedForControlCenter) {
        return; // Control Center owns the panel lifetime; do not auto-hide it.
    }
    if (!panel || !IsWindowVisible(panel) || g_panelAnimating || g_panelHiding ||
        g_draggingPanel || g_draggingBrightness || g_draggingTemperature || GetCapture() == panel) {
        return;
    }

    // Like the native volume flyout, remain open while the cursor is over the
    // panel even if the user pauses without moving the mouse.
    POINT cursor{};
    if (GetCursorPos(&cursor)) {
        HWND atCursor = WindowFromPoint(cursor);
        HWND rootAtCursor = atCursor ? GetAncestor(atCursor, GA_ROOT) : nullptr;
        if (rootAtCursor == panel || rootAtCursor == g_panelBackdropWindow) {
            g_lastPanelInteraction.store(GetTickCount64());
            return;
        }
    }

    if (GetTickCount64() - g_lastPanelInteraction.load() >=
        static_cast<ULONGLONG>(std::max(500, g_panelAutoHideMs.load()))) {
        BeginPanelHideAnimation(panel);
    }
}

std::wstring GetProcessBaseNameForWindow(HWND hwnd) {
    DWORD processId = 0;
    if (!hwnd || !GetWindowThreadProcessId(hwnd, &processId) || !processId) return L"";
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId);
    if (!process) return L"";
    wchar_t path[MAX_PATH]{};
    DWORD pathLength = ARRAYSIZE(path);
    std::wstring baseName;
    if (QueryFullProcessImageNameW(process, 0, path, &pathLength)) {
        std::wstring fullPath(path, pathLength);
        const size_t slash = fullPath.find_last_of(L"\\/");
        baseName = slash == std::wstring::npos ? fullPath : fullPath.substr(slash + 1);
        baseName = LowerSetting(std::move(baseName));
    }
    CloseHandle(process);
    return baseName;
}

bool IsUncloakedVisibleWindow(HWND hwnd) {
    if (!hwnd || !IsWindowVisible(hwnd)) return false;
    BOOL cloaked = FALSE;
    // DWMWA_CLOAKED is 14. Using the numeric value keeps compatibility with the
    // MinGW headers packaged with some Windhawk compiler builds.
    if (SUCCEEDED(DwmGetWindowAttribute(hwnd, 14, &cloaked, sizeof(cloaked))) && cloaked) {
        return false;
    }
    return true;
}

bool IsControlCenterWindow(HWND hwnd) {
    if (!IsUncloakedVisibleWindow(hwnd)) return false;

    wchar_t className[128]{};
    if (!GetClassNameW(hwnd, className, ARRAYSIZE(className))) return false;

    if (_wcsicmp(className, L"ControlCenterWindow") == 0) {
        const std::wstring processName = GetProcessBaseNameForWindow(hwnd);
        return processName == L"shellhost.exe" || processName == L"shellexperiencehost.exe";
    }

    // On Windows 11 21H2–23H2 Quick Settings is a CoreWindow hosted by
    // ShellExperienceHost. Its title is "Control Center"; Notification Center
    // has a different title and is intentionally not matched here.
    if (_wcsicmp(className, L"Windows.UI.Core.CoreWindow") != 0) return false;

    wchar_t title[256]{};
    const int titleLength = GetWindowTextW(hwnd, title, ARRAYSIZE(title));
    if (titleLength <= 0) return false;
    const bool quickSettingsTitle =
        _wcsicmp(title, L"Control Center") == 0 ||
        _wcsicmp(title, L"Control Centre") == 0 ||
        _wcsicmp(title, L"Quick Settings") == 0 ||
        _wcsicmp(title, L"Quick Actions") == 0;
    if (!quickSettingsTitle) return false;

    return GetProcessBaseNameForWindow(hwnd) == L"shellexperiencehost.exe";
}

BOOL CALLBACK FindControlCenterWindowProc(HWND hwnd, LPARAM lParam) {
    if (IsControlCenterWindow(hwnd)) {
        *reinterpret_cast<bool*>(lParam) = true;
        return FALSE;
    }
    return TRUE;
}

bool IsWindowsControlCenterOpen() {
    bool found = false;
    EnumWindows(FindControlCenterWindowProc, reinterpret_cast<LPARAM>(&found));
    return found;
}

void SynchronizeControlCenterPanel() {
    const Settings settings = GetSettingsSnapshot();
    const bool wasControlCenterOpen = g_controlCenterOpen;
    g_controlCenterOpen = IsWindowsControlCenterOpen();

    const bool shouldLinkPanel = settings.showPanelWithControlCenter && g_controlCenterOpen;
    if (shouldLinkPanel) {
        // If the user pressed the panel hotkey during this Control Center session,
        // honour that manual session: its own auto-hide timer applies and the panel
        // must not immediately reappear after the user dismisses it.
        if (g_panelSuppressedForControlCenter) {
            return;
        }

        if (!g_panelControlCenterManaged) {
            g_panelControlCenterManaged = true;
            g_panelCloseWhenControlCenterCloses = true;
            if (g_panelWindow && IsWindow(g_panelWindow) &&
                (!IsWindowVisible(g_panelWindow) || g_panelHiding)) {
                // Reverse a manual auto-hide animation if Quick Settings opens.
                StartPanelAnimation(g_panelWindow, false);
            }
        } else if (g_panelWindow && IsWindow(g_panelWindow) &&
                   !IsWindowVisible(g_panelWindow)) {
            StartPanelAnimation(g_panelWindow, false);
        }
    } else {
        const bool controlCenterSessionEnded = wasControlCenterOpen && !g_controlCenterOpen;
        const bool mustClose = g_panelControlCenterManaged ||
            (controlCenterSessionEnded && g_panelCloseWhenControlCenterCloses);

        g_panelControlCenterManaged = false;
        if (mustClose && g_panelWindow && IsWindow(g_panelWindow) && IsWindowVisible(g_panelWindow)) {
            BeginPanelHideAnimation(g_panelWindow);
        }

        if (controlCenterSessionEnded) {
            g_panelSuppressedForControlCenter = false;
            g_panelCloseWhenControlCenterCloses = false;
        }
    }
}

bool IsExcludedForegroundProcess(const std::vector<std::wstring>& exclusions) {
    if (exclusions.empty()) {
        return false;
    }

    HWND foregroundWindow = GetForegroundWindow();
    if (!foregroundWindow) {
        return false;
    }

    DWORD processId = 0;
    GetWindowThreadProcessId(foregroundWindow, &processId);
    if (processId == 0) {
        return false;
    }

    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, processId);
    if (!process) {
        return false;
    }

    std::vector<wchar_t> pathBuffer(32768);
    DWORD pathLength = static_cast<DWORD>(pathBuffer.size());
    std::wstring processPath;
    if (QueryFullProcessImageNameW(process, 0, pathBuffer.data(), &pathLength)) {
        processPath.assign(pathBuffer.data(), pathLength);
    }
    CloseHandle(process);

    if (processPath.empty()) {
        return false;
    }

    processPath = NormalizePath(std::move(processPath));
    for (const auto& excludedPath : exclusions) {
        if (CompareStringOrdinal(processPath.c_str(), -1,
                                 excludedPath.c_str(), -1, TRUE) == CSTR_EQUAL) {
            return true;
        }
    }

    return false;
}

bool GetVirtualScreenRect(RECT* rect) {
    if (!rect) {
        return false;
    }

    rect->left = GetSystemMetrics(SM_XVIRTUALSCREEN);
    rect->top = GetSystemMetrics(SM_YVIRTUALSCREEN);
    rect->right = rect->left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
    rect->bottom = rect->top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
    return rect->right > rect->left && rect->bottom > rect->top;
}

bool SameRect(const RECT& a, const RECT& b) {
    return a.left == b.left && a.top == b.top &&
           a.right == b.right && a.bottom == b.bottom;
}

void AdjustRuntimeDimAmount(int delta) {
    int previous = g_runtimeDimAmount.load();
    const int minDim = 100 - g_brightnessMaxPercent.load();
    const int maxDim = 100 - g_brightnessMinPercent.load();
    for (;;) {
        const int next = std::max(minDim, std::min(maxDim, previous + delta));
        if (g_runtimeDimAmount.compare_exchange_weak(previous, next)) {
            Wh_Log(L"Keyboard shortcut changed overlay opacity: %d%% -> %d%%",
                   previous, next);
            return;
        }
    }
}

bool RegisterDimmerHotkey(HWND hwnd, int id, UINT modifiers, UINT virtualKey,
                          PCWSTR description) {
    if (RegisterHotKey(hwnd, id, modifiers, virtualKey)) {
        Wh_Log(L"Registered global shortcut: %ls", description);
        return true;
    }

    Wh_Log(L"Could not register global shortcut %ls (error %lu)",
           description, GetLastError());
    return false;
}

void UnregisterConfiguredHotkeys(HWND hwnd) {
    for (auto& binding : g_hotkeyBindings) {
        if (binding.registered) {
            UnregisterHotKey(hwnd, binding.id);
            binding.registered = false;
        }
    }
}

void RegisterConfiguredHotkeys(HWND hwnd) {
    UnregisterConfiguredHotkeys(hwnd);
    const Settings settings = GetSettingsSnapshot();
    for (auto& binding : g_hotkeyBindings) {
        const HotkeySpec spec = settings.*(binding.settingMember);
        if (!spec.virtualKey) continue;
        UINT modifiers = spec.modifiers;
        if (!binding.repeat) modifiers |= MOD_NOREPEAT;
        binding.registered = RegisterDimmerHotkey(hwnd, binding.id, modifiers,
                                                   spec.virtualKey, binding.description);
        if (binding.registered) {
            Wh_Log(L"Hotkey source setting: %ls", binding.settingName);
        }
    }
}

LRESULT CALLBACK OverlayWindowProc(HWND hwnd, UINT message, WPARAM wParam, LPARAM lParam) {
    switch (message) {
        case WM_PAINT: {
            PAINTSTRUCT paint{};
            HDC dc = BeginPaint(hwnd, &paint);
            HBRUSH brush = CreateSolidBrush(static_cast<COLORREF>(g_overlayColor.load()));
            FillRect(dc, &paint.rcPaint, brush);
            DeleteObject(brush);
            EndPaint(hwnd, &paint);
            return 0;
        }

        case WM_ERASEBKGND: {
            RECT clientRect{};
            GetClientRect(hwnd, &clientRect);
            HBRUSH brush = CreateSolidBrush(static_cast<COLORREF>(g_overlayColor.load()));
            FillRect(reinterpret_cast<HDC>(wParam), &clientRect, brush);
            DeleteObject(brush);
            return 1;
        }

        case WM_NCHITTEST:
            // The layered + transparent extended styles pass pointer input through.
            return HTTRANSPARENT;

        case WM_MOUSEACTIVATE:
            return MA_NOACTIVATE;

        case WM_HOTKEY:
            if (wParam == kBrighterHotkeyId) {
                // A thinner black overlay means a brighter-looking screen.
                AdjustRuntimeDimAmount(-kDimStep);
                if (g_panelWindow && IsWindowVisible(g_panelWindow)) InvalidateRect(g_panelWindow, nullptr, FALSE);
                return 0;
            }
            if (wParam == kDarkerHotkeyId) {
                AdjustRuntimeDimAmount(kDimStep);
                if (g_panelWindow && IsWindowVisible(g_panelWindow)) InvalidateRect(g_panelWindow, nullptr, FALSE);
                return 0;
            }
            if (wParam == kEmergencyHotkeyId || wParam == kToggleHotkeyId) {
                bool enabled = !g_manualEnabled.load();
                g_manualEnabled.store(enabled);
                Wh_Log(L"Keyboard toggle: overlay %ls", enabled ? L"enabled" : L"disabled");
                return 0;
            }
            if (wParam == kPanelHotkeyId) {
                ToggleControlPanel();
                return 0;
            }
            if (wParam == kDefaultTemperatureHotkeyId) {
                g_runtimeTemperatureK.store(0);
                g_lastPanelInteraction.store(GetTickCount64());
                if (g_panelWindow && IsWindowVisible(g_panelWindow)) {
                    InvalidateRect(g_panelWindow, nullptr, FALSE);
                }
                Wh_Log(L"Color temperature reset to Default via Ctrl+Alt+Home");
                return 0;
            }
            break;

        case kHotkeysChangedMessage:
            RegisterConfiguredHotkeys(hwnd);
            return 0;

        case WM_DISPLAYCHANGE:
            // The worker refreshes the virtual-screen bounds on its next poll.
            return 0;

        default:
            break;
    }

    return DefWindowProcW(hwnd, message, wParam, lParam);
}

void UpdateOverlay(HWND hwnd, bool* visible, BYTE* appliedAlpha, COLORREF* appliedColor,
                   RECT* lastRect, bool* haveRect) {
    Settings settings = GetSettingsSnapshot();
    const int dimAmount = std::max(0, std::min(95, g_runtimeDimAmount.load()));
    const int temperature = g_runtimeTemperatureK.load();

    const bool manuallyEnabled = g_manualEnabled.load();
    const bool anyEffect = dimAmount > 0 || temperature > 0;
    const bool excluded = settings.enabled && anyEffect &&
                          IsExcludedForegroundProcess(settings.excludedApps);
    const bool shouldBeVisible = settings.enabled && anyEffect &&
                                 manuallyEnabled && !excluded;

    if (!shouldBeVisible) {
        if (*visible) {
            ShowWindow(hwnd, SW_HIDE);
            *visible = false;
        }
        return;
    }

    BYTE alpha = 0;
    COLORREF overlayColor = RGB(0, 0, 0);
    GetCompositeOverlay(dimAmount, temperature, &alpha, &overlayColor);
    if (alpha != *appliedAlpha) {
        if (!SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA)) {
            Wh_Log(L"SetLayeredWindowAttributes failed: %lu", GetLastError());
            if (*visible) {
                ShowWindow(hwnd, SW_HIDE);
                *visible = false;
            }
            return;
        }
        *appliedAlpha = alpha;
    }
    if (overlayColor != *appliedColor) {
        *appliedColor = overlayColor;
        g_overlayColor.store(static_cast<DWORD>(overlayColor));
        InvalidateRect(hwnd, nullptr, TRUE);
    }

    RECT screenRect{};
    if (!GetVirtualScreenRect(&screenRect)) {
        if (*visible) {
            ShowWindow(hwnd, SW_HIDE);
            *visible = false;
        }
        return;
    }

    int width = screenRect.right - screenRect.left;
    int height = screenRect.bottom - screenRect.top;
    BOOL positionResult = FALSE;
    const bool hadToShowOrResize = !*visible || !*haveRect || !SameRect(screenRect, *lastRect);

    if (hadToShowOrResize) {
        InvalidateRect(hwnd, nullptr, TRUE);
        positionResult = SetWindowPos(hwnd, HWND_TOPMOST,
                                      screenRect.left, screenRect.top, width, height,
                                      SWP_NOACTIVATE | SWP_SHOWWINDOW);
        if (positionResult) {
            *lastRect = screenRect;
            *haveRect = true;
            *visible = true;
        }
    } else {
        // No placement/z-order call is needed on ordinary polls. Repeatedly raising the
        // full-screen overlay then raising the panel above it made the flyout flicker.
        positionResult = TRUE;
    }

    if (!positionResult) {
        Wh_Log(L"SetWindowPos failed: %lu", GetLastError());
        ShowWindow(hwnd, SW_HIDE);
        *visible = false;
        return;
    }

    if (*visible) {
        UpdateWindow(hwnd);
        // Restore z-order only after the full-screen overlay was just shown/resized.
        // Acrylic host first, controls second, so neither is covered by the dim layer.
        if (hadToShowOrResize && g_panelWindow && IsWindowVisible(g_panelWindow)) {
            const Settings panelSettings = GetSettingsSnapshot();
            ApplyPanelWindowZOrder(g_panelAnimating && panelSettings.panelRevealFromTaskbar);
        }
    }
}

DWORD WINAPI OverlayThreadProc(void*) {
    OutputDebugStringW(L"[DimmerWH] OverlayThreadProc entered\r\n");
    // This code runs inside the first eligible Explorer process. A named mutex in
    // Wh_ModInit enforces a single overlay even if multiple Explorer processes exist.
    if (WaitForSingleObject(g_stopEvent, 0) == WAIT_OBJECT_0) {
        return 0;
    }

    HINSTANCE instance = GetModuleHandleW(nullptr);
    WNDCLASSEXW windowClass{};
    windowClass.cbSize = sizeof(windowClass);
    windowClass.lpfnWndProc = OverlayWindowProc;
    windowClass.hInstance = instance;
    windowClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    windowClass.hbrBackground = reinterpret_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    windowClass.lpszClassName = kWindowClassName;

    ATOM registeredClass = RegisterClassExW(&windowClass);
    if (!registeredClass) {
        OutputDebugStringW(L"[DimmerWH] RegisterClassExW failed\r\n");
        Wh_Log(L"RegisterClassExW failed: %lu", GetLastError());
        return 0;
    }

    RECT screenRect{};
    if (!GetVirtualScreenRect(&screenRect)) {
        screenRect = {0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)};
    }

    HWND hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT |
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        kWindowClassName, L"Dimmer Overlay", WS_POPUP,
        screenRect.left, screenRect.top,
        std::max(1L, screenRect.right - screenRect.left),
        std::max(1L, screenRect.bottom - screenRect.top),
        nullptr, nullptr, instance, nullptr);

    if (!hwnd) {
        OutputDebugStringW(L"[DimmerWH] CreateWindowExW failed\r\n");
        Wh_Log(L"CreateWindowExW failed: %lu", GetLastError());
        if (registeredClass) {
            UnregisterClassW(kWindowClassName, instance);
        }
        return 0;
    }

    g_overlayWindow.store(hwnd);
    OutputDebugStringW(L"[DimmerWH] Overlay window created\r\n");

    // Glass host: a normal non-layered popup lets DWM render native Desktop Acrylic
    // or the selected Translucent Windows material. The layered control surface is kept
    // separate so transparent pixels reveal/tint the material beneath while opaque
    // slider, icon and text pixels remain crisp.
    WNDCLASSEXW backdropClass{};
    backdropClass.cbSize = sizeof(backdropClass);
    backdropClass.lpfnWndProc = PanelBackdropWindowProc;
    backdropClass.hInstance = instance;
    backdropClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    backdropClass.hbrBackground = static_cast<HBRUSH>(GetStockObject(BLACK_BRUSH));
    backdropClass.lpszClassName = kPanelBackdropWindowClassName;
    ATOM backdropClassAtom = RegisterClassExW(&backdropClass);
    g_panelBackdropClassAtom = backdropClassAtom;
    // Keep this snapshot in the containing scope: it is used below to record
    // the mode that was actually used to initialize the backdrop host.
    const Settings initialPanelSettings = GetSettingsSnapshot();
    g_panelTransparentShadowMode = ShouldUseTransparentShadowPanel(initialPanelSettings);
    if (g_panelTransparentShadowMode) {
        // The requested style is intentionally backdrop-free: the pixels between
        // controls stay alpha-zero, and a software shadow is drawn around the panel.
        // This prevents the old opaque/acrylic host from covering the wallpaper.
        g_panelAcrylicAvailable = true;
        g_panelBackdropFollowMode = initialPanelSettings.followTranslucentWindowsSettings;
        Wh_Log(L"Transparent shadow panel active because Follow Translucent Windows is OFF; skipping the backdrop host");
    } else if (!backdropClassAtom) {
        Wh_Log(L"Acrylic host RegisterClassExW failed: %lu", GetLastError());
    } else {
        // Important for compatibility with Translucent Windows:
        // its IsWindowEligible() rejects WS_EX_NOACTIVATE / WS_EX_TRANSPARENT and
        // otherwise requires WS_POPUPWINDOW or WS_OVERLAPPEDWINDOW. This backdrop
        // host therefore uses WS_POPUPWINDOW without those two extended styles.
        // WM_MOUSEACTIVATE returns MA_NOACTIVATE and all show/position calls use
        // no-activate flags, so it remains a non-activating flyout in practice.
        // The window proc suppresses the non-client frame while preserving the style
        // bits that Translucent Windows checks.
        DWORD backdropExStyle = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        DWORD backdropWindowStyle = WS_POPUPWINDOW;
        if (!initialPanelSettings.followTranslucentWindowsSettings) {
            // Start ineligible so Translucent Windows cannot apply its material
            // during its window-created callback before Dimmer requests Acrylic.
            backdropExStyle |= WS_EX_NOACTIVATE;
            backdropWindowStyle = WS_POPUP;
        }
        g_panelBackdropWindow = CreateWindowExW(
            backdropExStyle,
            kPanelBackdropWindowClassName, L"Dimmer Acrylic Backdrop", backdropWindowStyle,
            0, 0, kPanelWidth, kPanelHeight,
            nullptr, nullptr, instance, nullptr);
        if (!g_panelBackdropWindow) {
            Wh_Log(L"Acrylic host CreateWindowExW failed: %lu", GetLastError());
            // Keep the class registered so a later settings change can retry the host.
        } else {
            HRGN backdropRegion = CreateRoundRectRgn(
                0, 0, kPanelWidth, kPanelHeight, 28, 28);
            if (backdropRegion && !SetWindowRgn(g_panelBackdropWindow, backdropRegion, TRUE)) {
                DeleteObject(backdropRegion);
                Wh_Log(L"Acrylic host SetWindowRgn failed: %lu", GetLastError());
            }
            g_panelAcrylicAvailable = ApplyPanelAcrylicBackdrop(g_panelBackdropWindow);
            if (!g_panelAcrylicAvailable) {
                Wh_Log(L"System Acrylic unavailable; using solid-black panel fallback");
                DestroyWindow(g_panelBackdropWindow);
                g_panelBackdropWindow = nullptr;
                // Keep the class registered so switching to follow mode can retry.
            } else {
                InvalidateRect(g_panelBackdropWindow, nullptr, TRUE);
                UpdateWindow(g_panelBackdropWindow);
                Wh_Log(L"DWM glass backdrop host created");
            }
        }
    }
    if (!g_panelTransparentShadowMode) {
        if (g_panelBackdropWindow) {
            g_panelBackdropFollowMode = initialPanelSettings.followTranslucentWindowsSettings;
        } else {
            g_panelBackdropFollowMode = !initialPanelSettings.followTranslucentWindowsSettings;
        }
    }

    // The foreground control layer is per-pixel layered. With a backdrop available,
    // empty pixels reveal the blurred host through the selected tint; visible controls
    // remain fully composed. If backdrop initialization fails, this layer falls back to solid black.
    WNDCLASSEXW panelContentClass{};
    panelContentClass.cbSize = sizeof(panelContentClass);
    panelContentClass.lpfnWndProc = ControlPanelWindowProc;
    panelContentClass.hInstance = instance;
    panelContentClass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    panelContentClass.hbrBackground = nullptr;
    panelContentClass.lpszClassName = kPanelContentWindowClassName;
    ATOM panelContentClassAtom = RegisterClassExW(&panelContentClass);
    if (!panelContentClassAtom) {
        Wh_Log(L"Control layer RegisterClassExW failed: %lu", GetLastError());
    } else {
        g_panelWindow = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            kPanelContentWindowClassName, L"Dimmer Display Controls", WS_POPUP,
            -PanelSurfacePadding(), -PanelSurfacePadding(),
            PanelSurfaceWidth(), PanelSurfaceHeight(),
            nullptr, nullptr, instance, nullptr);
        if (!g_panelWindow) {
            Wh_Log(L"Control layer CreateWindowExW failed: %lu", GetLastError());
            UnregisterClassW(kPanelContentWindowClassName, instance);
        } else {
            ApplyControlPanelWindowRegion(g_panelWindow);
            InvalidateRect(g_panelWindow, nullptr, FALSE);
            UpdateWindow(g_panelWindow);
            if (g_panelTransparentShadowMode) {
                Wh_Log(L"Transparent floating display-control panel with software drop shadow created");
            } else if (g_panelAcrylicAvailable) {
                Wh_Log(L"Compact floating display-control panel with glass backdrop created");
            } else {
                Wh_Log(L"Compact black floating display-control panel created (fallback)");
            }
        }
    }

    // Dismiss the flyout on an outside mouse click while allowing the click to
    // reach the window underneath. A low-level hook works even though the panel
    // deliberately does not activate and therefore does not own keyboard focus.
    if (g_panelWindow) {
        HMODULE hookModule = nullptr;
        const auto hookAddress = reinterpret_cast<LPCWSTR>(
            reinterpret_cast<ULONG_PTR>(&PanelOutsideClickMouseHook));
        if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               hookAddress, &hookModule)) {
            g_panelMouseHook = SetWindowsHookExW(WH_MOUSE_LL,
                                                 PanelOutsideClickMouseHook,
                                                 hookModule, 0);
        }
        if (!g_panelMouseHook) {
            Wh_Log(L"Outside-click hook install failed: %lu", GetLastError());
        }
    }

    // Register the current Windhawk hotkey settings. Changes are rebound live.
    RegisterConfiguredHotkeys(hwnd);

    bool visible = false;
    BYTE appliedAlpha = 255;
    COLORREF appliedColor = RGB(0, 0, 0);
    RECT lastRect{};
    bool haveRect = false;
    UpdateOverlay(hwnd, &visible, &appliedAlpha, &appliedColor, &lastRect, &haveRect);
    OutputDebugStringW(visible ? L"[DimmerWH] Overlay initially visible\r\n" : L"[DimmerWH] Overlay initially hidden; inspect settings/foreground exclusion\r\n");
    Wh_Log(L"Dimmer overlay started. Global shortcuts are configurable in Windhawk Settings; Ctrl+drag moves the panel. Control Center integration is configurable.");

    bool keepRunning = true;
    while (keepRunning) {
        DWORD waitResult = MsgWaitForMultipleObjects(1, &g_stopEvent, FALSE,
                                                     kPollIntervalMs, QS_ALLINPUT);
        if (waitResult == WAIT_OBJECT_0) {
            break;
        }

        if (waitResult == WAIT_OBJECT_0 + 1) {
            MSG message{};
            while (PeekMessageW(&message, nullptr, 0, 0, PM_REMOVE)) {
                if (message.message == WM_QUIT) {
                    keepRunning = false;
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        } else if (waitResult == WAIT_FAILED) {
            Wh_Log(L"MsgWaitForMultipleObjects failed: %lu", GetLastError());
            break;
        }

        if (keepRunning) {
            UpdateOverlay(hwnd, &visible, &appliedAlpha, &appliedColor, &lastRect, &haveRect);
            SynchronizeControlCenterPanel();
            MaintainPanelVisibility();
        }
    }

    UnregisterConfiguredHotkeys(hwnd);

    if (g_panelMouseHook) {
        UnhookWindowsHookEx(g_panelMouseHook);
        g_panelMouseHook = nullptr;
    }

    if (g_panelWindow) {
        ShowWindow(g_panelWindow, SW_HIDE);
        DestroyWindow(g_panelWindow);
        g_panelWindow = nullptr;
        UnregisterClassW(kPanelContentWindowClassName, instance);
    }
    if (g_panelBackdropWindow) {
        ShowWindow(g_panelBackdropWindow, SW_HIDE);
        DestroyWindow(g_panelBackdropWindow);
        g_panelBackdropWindow = nullptr;
    }
    if (backdropClassAtom) {
        UnregisterClassW(kPanelBackdropWindowClassName, instance);
        backdropClassAtom = 0;
        g_panelBackdropClassAtom = 0;
    }
    g_panelAcrylicAvailable = false;
    g_overlayWindow.store(nullptr);
    ShowWindow(hwnd, SW_HIDE);
    DestroyWindow(hwnd);
    if (registeredClass) {
        UnregisterClassW(kWindowClassName, instance);
    }

    Wh_Log(L"Dimmer overlay stopped.");
    return 0;
}

}  // namespace

__attribute__((dllexport)) BOOL Wh_ModInit() {
    // Keep the inline Windhawk API pointer materialized for the linker export.
#if !defined(WH_EDITING) && (defined(__clang__) || defined(__GNUC__))
    // The runtime build gets InternalWhModPtr from Windhawk's internal API
    // header. WH_EDITING intentionally hides that declaration from clangd.
    __asm__ __volatile__("" : : "r"(&InternalWhModPtr) : "memory");
#endif

    OutputDebugStringW(L"[DimmerWH] Wh_ModInit entered\r\n");
    Wh_Log(L"Wh_ModInit entered in Explorer process %lu", GetCurrentProcessId());

    DWORD sessionId = 0;
    if (!ProcessIdToSessionId(GetCurrentProcessId(), &sessionId) || sessionId == 0) {
        OutputDebugStringW(L"[DimmerWH] Not in an interactive session; skipping\r\n");
        return FALSE;
    }

    Wh_Log(L"Interactive session %lu confirmed", sessionId);

    // Windhawk can have several helper processes. Only one should own the overlay.
    SetLastError(ERROR_SUCCESS);
    g_instanceMutex = CreateMutexW(nullptr, FALSE,
        L"Local\\WindhawkDimmerOverlayExclusions_3D872A1E");
    if (!g_instanceMutex) {
        OutputDebugStringW(L"[DimmerWH] CreateMutexW failed\r\n");
        return FALSE;
    }
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        OutputDebugStringW(L"[DimmerWH] Another overlay instance already exists; skipping\r\n");
        CloseHandle(g_instanceMutex);
        g_instanceMutex = nullptr;
        return FALSE;
    }

    LoadSettings();

    g_stopEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (!g_stopEvent) {
        OutputDebugStringW(L"[DimmerWH] CreateEventW failed\r\n");
        Wh_Log(L"CreateEventW failed: %lu", GetLastError());
        CloseHandle(g_instanceMutex);
        g_instanceMutex = nullptr;
        return FALSE;
    }

    g_workerThread = CreateThread(nullptr, 0, OverlayThreadProc, nullptr, 0, nullptr);
    if (!g_workerThread) {
        OutputDebugStringW(L"[DimmerWH] CreateThread failed\r\n");
        Wh_Log(L"CreateThread failed: %lu", GetLastError());
        CloseHandle(g_stopEvent);
        g_stopEvent = nullptr;
        CloseHandle(g_instanceMutex);
        g_instanceMutex = nullptr;
        return FALSE;
    }

    OutputDebugStringW(L"[DimmerWH] Overlay worker thread created\r\n");
    Wh_Log(L"Overlay worker thread created successfully");
    return TRUE;
}

__attribute__((dllexport)) void Wh_ModSettingsChanged() {
    LoadSettings();
    // The worker polls the settings snapshot; the next poll applies any changes.
}

__attribute__((dllexport)) void Wh_ModUninit() {
    if (g_stopEvent) {
        SetEvent(g_stopEvent);
    }

    if (g_workerThread) {
        // The worker uses only a short timed wait and does not wait on this thread.
        WaitForSingleObject(g_workerThread, INFINITE);
        CloseHandle(g_workerThread);
        g_workerThread = nullptr;
    }


    if (g_stopEvent) {
        CloseHandle(g_stopEvent);
        g_stopEvent = nullptr;
    }
    if (g_instanceMutex) {
        CloseHandle(g_instanceMutex);
        g_instanceMutex = nullptr;
    }
    OutputDebugStringW(L"[DimmerWH] Wh_ModUninit complete\r\n");
}

