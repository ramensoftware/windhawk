#include "stdafx.h"

#include "ui_control.h"

#include "functions.h"
#include "logger.h"
#include "storage_manager.h"

using json = nlohmann::ordered_json;

namespace {

const json uiSettings = {
    {"telemetry.telemetryLevel", "off"},
    {"update.mode", "none"},
    {"update.showReleaseNotes", false},
    {"extensions.autoCheckUpdates", false},
    {"extensions.autoUpdate", false},
    {"files.autoSave", "afterDelay"},
    {"window.title", "${dirty}${activeEditorShort}${separator}${appName}"},
    {"workbench.enableExperiments", false},
    {"workbench.settings.enableNaturalLanguageSearch", false},
    {"workbench.editor.restoreViewState", false},
    {"workbench.tips.enabled", false},
    {"workbench.startupEditor", "none"},
    {"workbench.layoutControl.enabled", false},
    {"security.workspace.trust.enabled", false},
    {"editor.inlayHints.enabled", "off"},
    {"editor.tabSize", 4},
    {"editor.insertSpaces", true},
    {"editor.detectIndentation", false},
    {"clangd.path", "${env:WINDHAWK_COMPILER_PATH}\\bin\\clangd.exe"},
    {"clangd.arguments", {"-header-insertion=never"}},
    {"clangd.checkUpdates", false},
    {"window.menuBarVisibility", "compact"},
    {"workbench.activityBar.visible", false},
    {"workbench.editor.showTabs", false},
    {"workbench.statusBar.visible", false},
    {"git.enabled", false},
    {"git.showProgress", false},
    {"git.decorations.enabled", false},
    {"git.ignoreMissingGitWarning", true},
    {"git.ignoreLegacyWarning", true},
    {"git.ignoreWindowsGit27Warning", true},
};

const json uiSettingsToMigrate = {
    {"clangd.path",
     "${env:WINDHAWK_UI_PATH}"
     "\\resources\\app\\extensions\\clangd\\clangd\\bin\\clangd.exe"},
};

void MakeSureDirectoryExists(const std::filesystem::path& directory) {
    if (!std::filesystem::is_directory(directory)) {
        try {
            std::filesystem::create_directories(directory);
        } catch (const std::exception&) {
            if (!std::filesystem::is_directory(directory)) {
                throw;
            }

            // An exception was thrown, but the folder now exists. This
            // can happen when e.g. not all the path is accessible.
        }
    }
}

void PrepareUISettings(const std::filesystem::path& uiDataPath) {
    std::filesystem::path settingsPath = uiDataPath / L"user-data" / L"User";
    MakeSureDirectoryExists(settingsPath);

    settingsPath /= L"settings.json";

    json settingsJson;

    {
        std::ifstream settingsFile(settingsPath);
        if (settingsFile) {
            try {
                settingsFile >> settingsJson;
            } catch (const std::exception& e) {
                LOG(L"Parsing settings.json failed: %S", e.what());
            }
        }
    }

    if (!settingsJson.is_object()) {
        settingsJson = json::object();
    }

    bool updatedData = false;

    for (auto& [key, value] : uiSettings.items()) {
        bool updateValue = !settingsJson.contains(key);
        if (!updateValue) {
            auto it = uiSettingsToMigrate.find(key);
            if (it != uiSettingsToMigrate.end() && settingsJson[key] == *it) {
                updateValue = true;
            }
        }

        if (updateValue) {
            settingsJson[key] = value;
            updatedData = true;
        }
    }

    if (updatedData) {
        if (!Functions::WriteFileContentAtomically(settingsPath,
                                                   settingsJson.dump(4))) {
            LOG(L"Updating settings.json failed (%s)", settingsPath.c_str());
        }
    }
}

std::wstring BuildUIProcessEnvBlock(const std::filesystem::path& uiDataPath,
                                    const std::filesystem::path& uiPath,
                                    const std::filesystem::path& compilerPath) {
    std::wstring envBlock;

    wil::unique_environstrings_ptr currentEnv{GetEnvironmentStrings()};

    auto startsWithNoCase = [](PCWSTR str, std::wstring_view prefix) {
        return _wcsnicmp(str, prefix.data(), prefix.size()) == 0;
    };

    for (PCWSTR env = currentEnv.get(); *env; env += wcslen(env) + 1) {
        if (startsWithNoCase(env, L"ELECTRON_") ||
            startsWithNoCase(env, L"VSCODE_") ||
            startsWithNoCase(env, L"WINDHAWK_UI_PATH=") ||
            startsWithNoCase(env, L"WINDHAWK_COMPILER_PATH=")) {
            continue;
        }

        envBlock += env;
        envBlock += L'\0';
    }

    // Add the environment variables needed for VSCode.
    // VSCODE_PORTABLE: Makes VSCode use the specified folder for data storage.
    envBlock += L"VSCODE_PORTABLE=";
    envBlock += uiDataPath.native();
    envBlock += L'\0';

    // WINDHAWK_UI_PATH: Used to locate the clangd executable.
    envBlock += L"WINDHAWK_UI_PATH=";
    envBlock += uiPath.native();
    envBlock += L'\0';

    // WINDHAWK_COMPILER_PATH: Used to locate the compiler.
    envBlock += L"WINDHAWK_COMPILER_PATH=";
    envBlock += compilerPath.native();
    envBlock += L'\0';

    // Double null terminator to end the environment block.
    envBlock += L'\0';

    return envBlock;
}

void RunVSCodeUI() {
    auto uiDataPath = StorageManager::GetInstance().GetUIDataPath();
    PrepareUISettings(uiDataPath);

    auto uiPath = StorageManager::GetInstance().GetUIPath();

    // UIPath is optional in storage; without it there's no VSCode UI to launch.
    THROW_WIN32_IF(ERROR_FILE_NOT_FOUND, uiPath.empty());

    auto compilerPath = StorageManager::GetInstance().GetCompilerPath();

    std::wstring envBlock =
        BuildUIProcessEnvBlock(uiDataPath, uiPath, compilerPath);

    auto uiExePath = uiPath / L"VSCodium.exe";

    // If the VSCodium executable doesn't exist, try the VSCode executable.
    if (GetFileAttributes(uiExePath.c_str()) == INVALID_FILE_ATTRIBUTES &&
        GetLastError() == ERROR_FILE_NOT_FOUND) {
        uiExePath = uiPath / L"Code.exe";

        // If VSCode executable doesn't exist, give up.
        THROW_LAST_ERROR_IF(GetFileAttributes(uiExePath.c_str()) ==
                                INVALID_FILE_ATTRIBUTES &&
                            GetLastError() == ERROR_FILE_NOT_FOUND);
    }

    auto editorWorkspacePath =
        StorageManager::GetInstance().GetEditorWorkspacePath();
    MakeSureDirectoryExists(editorWorkspacePath);

    // The --locale command line switch is needed to avoid the "Install
    // language pack to change the display language" message if the OS
    // locale is not English.
    //
    // The --no-sandbox, --disable-gpu-sandbox command line switches seem to fix
    // a bug that sometimes causes VSCode to be stuck with an empty window when
    // launched:
    // https://github.com/ramensoftware/windhawk/issues/26
    // VSCode reference:
    // https://github.com/microsoft/vscode/issues/122951
    // Also, from the FAQ:
    // > Q: Unable to run as admin when AppLocker is enabled
    // > A: With the introduction of process sandboxing (discussed in this blog
    // post) running as administrator is currently unsupported when AppLocker is
    // configured due to a limitation of the runtime sandbox. You can refer to
    // Chromium issue #740132 for additional context. If your work requires that
    // you run VS Code from an elevated terminal, you can launch code with
    // --no-sandbox --disable-gpu-sandbox as a workaround.
    // https://github.com/microsoft/vscode-docs/blob/vnext/docs/setup/windows.md#unable-to-run-as-admin-when-applocker-is-enabled
    std::wstring commandLine =
        L"\"" + uiExePath.native() + L"\" \"" + editorWorkspacePath.native() +
        L"\" --locale=en --no-sandbox --disable-gpu-sandbox";

    STARTUPINFO si = {sizeof(STARTUPINFO)};
    wil::unique_process_information process;

    THROW_IF_WIN32_BOOL_FALSE(
        CreateProcess(uiExePath.c_str(), commandLine.data(), nullptr, nullptr,
                      FALSE, NORMAL_PRIORITY_CLASS | CREATE_UNICODE_ENVIRONMENT,
                      envBlock.data(), nullptr, &si, &process));
}

bool ShouldUseVSCodiumUI() {
    return wil::TryGetEnvironmentVariableW<std::wstring>(
               L"WINDHAWK_USE_VSCODIUM_UI") == L"1";
}

void RunWindhawkUI() {
    auto modulePath = wil::GetModuleFileName<std::wstring>();
    auto uiExePath =
        std::filesystem::path(modulePath).parent_path() / L"windhawk-ui.exe";

    // A bare launch means ensure-running-and-foreground; the UI enforces its
    // own single instance and forwards a second launch to the running one. The
    // app root is discovered relative to the executable, so no arguments are
    // passed.
    std::wstring commandLine = L"\"" + uiExePath.native() + L"\"";

    STARTUPINFO si = {sizeof(STARTUPINFO)};
    wil::unique_process_information process;

    THROW_IF_WIN32_BOOL_FALSE(CreateProcess(
        uiExePath.c_str(), commandLine.data(), nullptr, nullptr, FALSE,
        NORMAL_PRIORITY_CLASS, nullptr, nullptr, &si, &process));
}

}  // namespace

namespace UIControl {

void RunUI() {
    if (ShouldUseVSCodiumUI()) {
        RunVSCodeUI();
    } else {
        RunWindhawkUI();
    }
}

std::vector<HWND> GetOpenUIWindows() {
    struct EnumWindowsParam {
        std::filesystem::path uiExePath1;
        std::filesystem::path uiExePath2;
        std::vector<HWND> windows;
    };

    auto uiPath = StorageManager::GetInstance().GetUIPath();

    // UIPath is optional in storage. When unset, leave the executable paths
    // empty so the callback skips VSCode/VSCodium matching; native UI windows
    // are matched by class name and don't depend on it.
    EnumWindowsParam enumWindowsParam;
    if (!uiPath.empty()) {
        enumWindowsParam.uiExePath1 = uiPath / L"VSCodium.exe";
        enumWindowsParam.uiExePath2 = uiPath / L"Code.exe";
    }

    EnumWindows(
        [](HWND hWnd, LPARAM lParam) {
            auto& enumWindowsParam =
                *reinterpret_cast<EnumWindowsParam*>(lParam);

            if (!IsWindowVisible(hWnd)) {
                return TRUE;
            }

            WCHAR szClassName[32];
            if (!GetClassName(hWnd, szClassName, _countof(szClassName))) {
                return TRUE;
            }

            // The native UI window sets a dedicated class name, so it can be
            // matched directly without inspecting the process image.
            if (_wcsicmp(szClassName, L"WindhawkTauriMainUI") == 0) {
                enumWindowsParam.windows.push_back(hWnd);
                return TRUE;
            }

            // No configured UI path means no VSCode/VSCodium windows to match.
            if (enumWindowsParam.uiExePath1.empty()) {
                return TRUE;
            }

            if (_wcsicmp(szClassName, L"Chrome_WidgetWin_1") != 0) {
                return TRUE;
            }

            DWORD dwProceccID;
            if (!GetWindowThreadProcessId(hWnd, &dwProceccID)) {
                return TRUE;
            }

            try {
                wil::unique_process_handle process(OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION, FALSE, dwProceccID));
                if (!process) {
                    return TRUE;
                }

                std::filesystem::path fullProcessImageName =
                    wil::QueryFullProcessImageName<std::wstring>(process.get());

                std::error_code ec;
                if (std::filesystem::equivalent(fullProcessImageName,
                                                enumWindowsParam.uiExePath1,
                                                ec) ||
                    std::filesystem::equivalent(fullProcessImageName,
                                                enumWindowsParam.uiExePath2,
                                                ec)) {
                    enumWindowsParam.windows.push_back(hWnd);
                }
            } catch (const std::exception& e) {
                LOG(L"EnumWindows callback failed for window %08X: %S",
                    static_cast<DWORD>(reinterpret_cast<ULONG_PTR>(hWnd)),
                    e.what());
            }

            return TRUE;
        },
        reinterpret_cast<LPARAM>(&enumWindowsParam));

    return enumWindowsParam.windows;
}

bool BringUIToFront() {
    // The native UI sets a dedicated window class, so locate it directly and
    // bring it to the foreground the same way as the VSCodium windows below. A
    // missing or hidden window counts as not running, so the caller launches it
    // (which also brings it to front via the single-instance handoff).
    if (!ShouldUseVSCodiumUI()) {
        HWND hWnd = FindWindow(L"WindhawkTauriMainUI", nullptr);
        if (!hWnd || !IsWindowVisible(hWnd)) {
            return false;
        }

        if (IsIconic(hWnd)) {
            PostMessage(hWnd, WM_SYSCOMMAND, SC_RESTORE, 0);
        }

        SetForegroundWindow(hWnd);
        return true;
    }

    auto windows = GetOpenUIWindows();
    if (windows.size() == 0) {
        return false;
    }

    for (HWND hWnd : windows) {
        if (IsIconic(hWnd)) {
            PostMessage(hWnd, WM_SYSCOMMAND, SC_RESTORE, 0);
        }

        SetForegroundWindow(hWnd);
    }

    return true;
}

void RunUIOrBringToFront(HWND hWnd) {
    // If running, just bring to front.
    if (UIControl::BringUIToFront()) {
        return;
    }

    // Only the whole-UI VSCodium mode wants an elevated UI: it performs its
    // privileged writes directly, where the native UI brokers them through an
    // elevated second instance of itself. A portable install gains nothing from
    // elevation either.
    bool mustRunAsAdmin = ShouldUseVSCodiumUI() &&
                          !StorageManager::GetInstance().IsPortable() &&
                          !Functions::IsRunAsAdmin();

    // If possible, just run the process.
    if (!mustRunAsAdmin) {
        UIControl::RunUI();
        return;
    }

    // Elevate and run a process that will start the UI.
    auto modulePath = wil::GetModuleFileName<std::wstring>();
    PCWSTR commandLine = L"-run-ui";

    int nResult =
        (int)(UINT_PTR)ShellExecute(hWnd, L"runas", modulePath.c_str(),
                                    commandLine, nullptr, SW_SHOWNORMAL);

    THROW_LAST_ERROR_IF(nResult <= 32 && GetLastError() != ERROR_CANCELLED);
}

bool CloseUI() {
    auto windows = GetOpenUIWindows();
    bool succeeded = false;

    for (HWND hWnd : windows) {
        succeeded |= !!PostMessage(hWnd, WM_SYSCOMMAND, SC_CLOSE, 0);
    }

    return succeeded;
}

}  // namespace UIControl
