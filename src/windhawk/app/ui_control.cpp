#include "stdafx.h"

#include "ui_control.h"

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
        std::ofstream userProfileFile(settingsPath);
        if (userProfileFile) {
            userProfileFile << std::setw(4) << settingsJson;
        }
    }
}

BOOL IsArm64NativeMachine() {
    using IsWow64Process2_t = BOOL(WINAPI*)(
        HANDLE hProcess, USHORT * pProcessMachine, USHORT * pNativeMachine);

    IsWow64Process2_t pIsWow64Process2 = nullptr;
    HMODULE kernel32Module = GetModuleHandle(L"kernel32.dll");
    if (kernel32Module) {
        pIsWow64Process2 = reinterpret_cast<IsWow64Process2_t>(
            GetProcAddress(kernel32Module, "IsWow64Process2"));
    }

    if (!pIsWow64Process2) {
        // ARM64 OSes should have IsWow64Process2.
        return FALSE;
    }

    USHORT processMachine = 0;
    USHORT nativeMachine = 0;
    return pIsWow64Process2(GetCurrentProcess(), &processMachine,
                            &nativeMachine) &&
           nativeMachine == IMAGE_FILE_MACHINE_ARM64;
}

}  // namespace

namespace UIControl {

void RunUI() {
    auto uiDataPath = StorageManager::GetInstance().GetUIDataPath();
    PrepareUISettings(uiDataPath);

    // Will be passed to VSCode to make it use the specified folder for data
    // storage.
    SetEnvironmentVariable(L"VSCODE_PORTABLE", uiDataPath.c_str());

    // Will be used to locate the clangd executable.
    auto uiPath = StorageManager::GetInstance().GetUIPath();
    SetEnvironmentVariable(L"WINDHAWK_UI_PATH", uiPath.c_str());

    auto compilerPath = StorageManager::GetInstance().GetCompilerPath();
    SetEnvironmentVariable(L"WINDHAWK_COMPILER_PATH", compilerPath.c_str());

    static bool arm64Enabled = IsArm64NativeMachine();
    if (arm64Enabled) {
        SetEnvironmentVariable(L"WINDHAWK_ARM64_ENABLED", L"1");
    }

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

    THROW_IF_WIN32_BOOL_FALSE(CreateProcess(
        uiExePath.c_str(), commandLine.data(), nullptr, nullptr, FALSE,
        NORMAL_PRIORITY_CLASS, nullptr, nullptr, &si, &process));
}

bool RunUIViaSchedTask() {
    // Access the Windows Task Service API by creating an instance of it and
    // attempt to connect to the Task Scheduler service on the local machine.
    wil::com_ptr<ITaskService> taskService =
        wil::CoCreateInstance<ITaskService>(CLSID_TaskScheduler);
    THROW_IF_FAILED(taskService->Connect(_variant_t(), _variant_t(),
                                         _variant_t(), _variant_t()));

    // Get a pointer to the root task folder, which is where the task resides.
    auto rootFolderPath = wil::make_bstr(L"\\");
    wil::com_ptr<ITaskFolder> rootFolder;
    THROW_IF_FAILED(taskService->GetFolder(rootFolderPath.get(), &rootFolder));

    auto taskName = wil::make_bstr(L"WindhawkRunUITask");
    wil::com_ptr<IRegisteredTask> task;
    THROW_IF_FAILED(rootFolder->GetTask(taskName.get(), &task));

    AllowSetForegroundWindow(ASFW_ANY);

    wil::com_ptr<IRunningTask> runTask;
    HRESULT hr =
        task->RunEx(_variant_t(), TASK_RUN_AS_SELF, 0, _bstr_t(), &runTask);
    if (hr == SCHED_E_TASK_DISABLED) {
        return false;
    }

    THROW_IF_FAILED(hr);
    return true;
}

std::vector<HWND> GetOpenUIWindows() {
    struct EnumWindowsParam {
        std::filesystem::path uiExePath1;
        std::filesystem::path uiExePath2;
        std::vector<HWND> windows;
    };

    auto uiPath = StorageManager::GetInstance().GetUIPath();

    EnumWindowsParam enumWindowsParam = {uiPath / L"VSCodium.exe",
                                         uiPath / L"Code.exe"};

    EnumWindows(
        [](HWND hWnd, LPARAM lParam) {
            auto& enumWindowsParam =
                *reinterpret_cast<EnumWindowsParam*>(lParam);

            if (!IsWindowVisible(hWnd)) {
                return TRUE;
            }

            WCHAR szClassName[32];
            if (!GetClassName(hWnd, szClassName, _countof(szClassName)) ||
                _wcsicmp(szClassName, L"Chrome_WidgetWin_1") != 0) {
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

void RunUIOrBringToFront(HWND hWnd, bool mustRunAsAdmin) {
    // If running, just bring to front.
    if (UIControl::BringUIToFront()) {
        return;
    }

    // If possible, just run the process.
    if (!mustRunAsAdmin) {
        UIControl::RunUI();
        return;
    }

    // Try to trigger the scheduled task to avoid elevation.
    try {
        if (UIControl::RunUIViaSchedTask()) {
            return;
        }
    } catch (const std::exception& e) {
        LOG(L"RunUIViaSchedTask error: %S", e.what());
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
