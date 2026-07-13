//! The VSCodium launcher: a port of the C++ `RunVSCodeUI` / `PrepareUISettings`
//! / `BuildUIProcessEnvBlock` (`windhawk/app/ui_control.cpp`), retargeted from
//! the single legacy `EditorWorkspace` to a per-mod workspace directory.
//!
//! A launch does four things, in order:
//!
//! - **prepare the shared VSCodium settings** - ensure the VSCodium user settings
//!   (`<appData>/UIData/user-data/User/settings.json`) carry the global editor
//!   settings block and the one clangd-path migration entry (`PrepareUISettings`).
//!   These are shared VSCodium *user* settings (not per-workspace), so this is
//!   idempotent across launches and a no-op after the first;
//! - **locate the editor exe** - `<uiPath>/VSCodium.exe`, falling back to
//!   `<uiPath>/Code.exe`; a missing editor is a launch-time error;
//! - **build the process environment** - strip inherited `ELECTRON_*` / `VSCODE_*`
//!   from the parent (so a VSCodium that spawned this UI does not leak its own
//!   environment into the child) and set `VSCODE_PORTABLE`, `WINDHAWK_UI_PATH`,
//!   `WINDHAWK_COMPILER_PATH`, and `WINDHAWK_ARM64_ENABLED=1` when arm64 is enabled
//!   (`BuildUIProcessEnvBlock`). The extension reads these to find clangd and the
//!   compiler;
//! - **spawn** the editor exe with the workspace directory as the folder argument,
//!   plus the `--locale=en --no-sandbox --disable-gpu-sandbox` locale and
//!   AppLocker/elevation workarounds `RunVSCodeUI` documents. The child inherits the
//!   native UI's integrity, which is what editor compiles/installs
//!   need, so no second elevation ladder is run.
//!
//! The launch is fire-and-forget: the spawned VSCodium is not tracked.
//! `std::process::Command` inherits the parent's environment by default, so the
//! port strips and sets over that inherited block rather than rebuilding it
//! from scratch. The pure pieces - the settings merge, the exe location, the
//! environment mutation - are free functions unit-tested without a spawn, and
//! the assembled `Command` is asserted via `get_program`/`get_args`/`get_envs`
//! without launching.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

use super::to_pretty_json;

/// The VSCodium portable-data folder under `appData` (`getCoreInfo`
/// `fsPaths.appDataPath` joined with this), the same folder the C++
/// `StorageManager::GetUIDataPath` resolves. It holds the VSCodium user settings
/// (`PrepareUISettings`) and is passed to the child as `VSCODE_PORTABLE`. Distinct
/// from the native window's own WebView2 data folder (`UIMainData` in `lib.rs`).
const UI_DATA_DIR: &str = "UIData";

/// The VSCodium user-settings path under the portable-data folder
/// (`<UIData>/user-data/User/settings.json`).
const USER_DATA_DIR: &str = "user-data";
const USER_DIR: &str = "User";
const SETTINGS_FILE: &str = "settings.json";

/// The editor executables tried in order under `uiPath`: VSCodium first, then a
/// plain VSCode as a fallback (`RunVSCodeUI`).
const VSCODIUM_EXE: &str = "VSCodium.exe";
const VSCODE_EXE: &str = "Code.exe";

/// The launch-time command-line switches `RunVSCodeUI` passes after the workspace
/// folder: `--locale=en` avoids the "install language pack" prompt on a non-English
/// OS, and `--no-sandbox --disable-gpu-sandbox` work around an empty-window bug and
/// the AppLocker "cannot run as admin" limitation (see the C++ for the linked issues).
const LAUNCH_ARGS: [&str; 3] = ["--locale=en", "--no-sandbox", "--disable-gpu-sandbox"];

/// The environment variables set for the child (`BuildUIProcessEnvBlock`).
const ENV_VSCODE_PORTABLE: &str = "VSCODE_PORTABLE";
const ENV_WINDHAWK_UI_PATH: &str = "WINDHAWK_UI_PATH";
const ENV_WINDHAWK_COMPILER_PATH: &str = "WINDHAWK_COMPILER_PATH";
const ENV_WINDHAWK_ARM64_ENABLED: &str = "WINDHAWK_ARM64_ENABLED";

/// The inherited-environment prefixes stripped before the child launches, so a
/// VSCodium/Electron parent's own variables do not leak into the spawned editor.
const STRIPPED_ENV_PREFIXES: [&str; 2] = ["ELECTRON_", "VSCODE_"];

/// A launch failure. Surfaced to the caller so the development handler can
/// present it in a native message box and log it; a launch failure is
/// recoverable, so it never terminates the app.
#[derive(Debug)]
pub enum LaunchError {
    /// Neither `VSCodium.exe` nor `Code.exe` exists under the resolved UI path.
    EditorNotFound(PathBuf),
    /// An I/O failure preparing the shared VSCodium settings or spawning the editor.
    Io(io::Error),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::EditorNotFound(ui_path) => write!(
                f,
                "Could not find the code editor ({VSCODIUM_EXE} or {VSCODE_EXE}) in {}.",
                ui_path.display()
            ),
            LaunchError::Io(error) => write!(f, "Could not open the code editor: {error}"),
        }
    }
}

impl Error for LaunchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LaunchError::EditorNotFound(_) => None,
            LaunchError::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for LaunchError {
    fn from(error: io::Error) -> Self {
        LaunchError::Io(error)
    }
}

/// The spawn step behind a seam: the development handlers open a prepared
/// workspace through this rather than a concrete [`Launcher`], so a handler
/// test can assert the launch reached the workspace (its path, that it was
/// reached at all) with a recording fake instead of spawning a real VSCodium.
/// `Launcher` is the production implementor.
pub trait LaunchEditor: Send + Sync {
    /// Open VSCodium on a prepared workspace directory (or record the request).
    fn open_workspace(&self, workspace: &Path) -> Result<(), LaunchError>;

    /// Whether a code editor is installed (the resolved UI path is non-empty).
    /// The development tools are an optional install component; when they are
    /// absent the UI path is empty, and the development handlers reply "UI
    /// missing" instead of attempting a launch. A UI path that is set but holds
    /// no editor exe is still "available" here - that is a launch failure, not
    /// a missing install. Defaults to `true` so a recording test seam is
    /// available.
    fn is_available(&self) -> bool {
        true
    }
}

impl LaunchEditor for Launcher {
    fn open_workspace(&self, workspace: &Path) -> Result<(), LaunchError> {
        self.launch(workspace)
    }

    fn is_available(&self) -> bool {
        !self.ui_path.as_os_str().is_empty()
    }
}

/// The VSCodium launcher, holding the resolved paths and flag a launch needs
/// (`getCoreInfo` `fsPaths` + `arm64Enabled`), so a single instance can launch
/// many per-mod workspaces. Constructed once by the development handlers from
/// the core info; kept free of the protocol DTOs so it stays a testable
/// OS-touchpoint leaf.
pub struct Launcher {
    /// `<appData>/UIData`, the VSCodium portable-data folder (`VSCODE_PORTABLE`).
    ui_data_path: PathBuf,
    /// `fsPaths.uiPath`, where the editor exe and clangd live (`WINDHAWK_UI_PATH`).
    ui_path: PathBuf,
    /// `fsPaths.compilerPath`, where the compiler and its clangd live
    /// (`WINDHAWK_COMPILER_PATH`).
    compiler_path: PathBuf,
    /// `getCoreInfo` `arm64Enabled`: gates `WINDHAWK_ARM64_ENABLED=1`.
    arm64_enabled: bool,
}

impl Launcher {
    /// A launcher rooted at the `appData` directory (from which `UIData` is derived,
    /// like the workspace manager derives its `EditorWorkspaces` container) plus the
    /// UI and compiler paths and the arm64 flag, all from `getCoreInfo`.
    pub fn new(
        app_data: impl Into<PathBuf>,
        ui_path: impl Into<PathBuf>,
        compiler_path: impl Into<PathBuf>,
        arm64_enabled: bool,
    ) -> Self {
        Self {
            ui_data_path: app_data.into().join(UI_DATA_DIR),
            ui_path: ui_path.into(),
            compiler_path: compiler_path.into(),
            arm64_enabled,
        }
    }

    /// Launch VSCodium on a prepared workspace directory: prepare the shared
    /// user settings, locate the editor exe, and spawn it with the workspace as
    /// the folder argument. Fire-and-forget - the returned `Child` is dropped,
    /// which leaves the editor running. VSCodium's single-instance mechanism,
    /// keyed on the folder path under the shared `VSCODE_PORTABLE`, focuses an
    /// already-open window for the same workspace instead of opening a second
    /// (edit-reuse).
    pub fn launch(&self, workspace: &Path) -> Result<(), LaunchError> {
        prepare_ui_settings(&self.ui_data_path)?;
        let exe = self
            .locate_editor_exe()
            .ok_or_else(|| LaunchError::EditorNotFound(self.ui_path.clone()))?;
        let _child = self.build_command(&exe, workspace).spawn()?;
        Ok(())
    }

    /// The editor exe under `uiPath`: `VSCodium.exe`, else `Code.exe`, else `None`
    /// (the launch-time error case). Mirrors `RunVSCodeUI`'s VSCodium-then-Code probe.
    fn locate_editor_exe(&self) -> Option<PathBuf> {
        let vscodium = self.ui_path.join(VSCODIUM_EXE);
        if vscodium.is_file() {
            return Some(vscodium);
        }
        let code = self.ui_path.join(VSCODE_EXE);
        if code.is_file() {
            return Some(code);
        }
        None
    }

    /// Assemble the spawn: the editor exe, the workspace folder argument, the
    /// launch switches, and the child environment. Split out from
    /// [`Launcher::launch`] so a test can assert the program, args, and
    /// environment via `get_program`/`get_args`/`get_envs` without spawning.
    fn build_command(&self, exe: &Path, workspace: &Path) -> Command {
        let mut command = Command::new(exe);
        command.arg(workspace).args(LAUNCH_ARGS);
        self.apply_env(&mut command);
        command
    }

    /// Apply the child environment (`BuildUIProcessEnvBlock`) over the inherited
    /// block: strip the inherited `ELECTRON_*` / `VSCODE_*` variables, then set the
    /// Windhawk ones. `Command` inherits the parent environment by default, so setting
    /// a variable overrides any inherited value; `WINDHAWK_ARM64_ENABLED` is left as
    /// inherited when arm64 is disabled, matching the C++ (it only strips/sets it when
    /// enabled).
    fn apply_env(&self, command: &mut Command) {
        for name in std::env::vars_os().map(|(name, _)| name) {
            if should_strip_env(&name) {
                command.env_remove(&name);
            }
        }
        command.env(ENV_VSCODE_PORTABLE, &self.ui_data_path);
        command.env(ENV_WINDHAWK_UI_PATH, &self.ui_path);
        command.env(ENV_WINDHAWK_COMPILER_PATH, &self.compiler_path);
        if self.arm64_enabled {
            command.env(ENV_WINDHAWK_ARM64_ENABLED, "1");
        }
    }
}

/// Whether an inherited environment variable is stripped before launch: any
/// `ELECTRON_*` or `VSCODE_*` name (`BuildUIProcessEnvBlock`). The prefixes are ASCII,
/// so a lossy conversion is safe for the check while the original `OsStr` is used for
/// the removal.
fn should_strip_env(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    STRIPPED_ENV_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Ensure the shared VSCodium user settings carry the Windhawk editor block
/// (`PrepareUISettings`): read `<uiData>/user-data/User/settings.json`, merge in the
/// settings that are absent (plus the one clangd-path migration), and write it back
/// only if anything changed. Idempotent - a no-op once seeded.
fn prepare_ui_settings(ui_data: &Path) -> io::Result<()> {
    let user_dir = ui_data.join(USER_DATA_DIR).join(USER_DIR);
    fs::create_dir_all(&user_dir)?;
    let settings_path = user_dir.join(SETTINGS_FILE);

    let existing = read_settings_object(&settings_path);
    let (merged, updated) = merge_ui_settings(existing);
    if updated {
        fs::write(&settings_path, to_pretty_json(&Value::Object(merged)))?;
    }
    Ok(())
}

/// Read a JSON object from a settings file, degrading a missing, unreadable,
/// unparseable, or non-object file to an empty object - the same tolerance the C++
/// `PrepareUISettings` applies before merging (`!settingsJson.is_object()`).
fn read_settings_object(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// Merge the Windhawk editor settings into an existing settings object, returning the
/// merged object and whether anything changed. A setting is written when it is absent,
/// or when the current value equals the migration value for that key (so an old
/// migrated `clangd.path` is upgraded, but a user-customized one is left alone). Every
/// other key the file carries is preserved, and existing keys keep their positions
/// (`preserve_order`), so this is a merge, not an overwrite - matching the C++'s
/// per-key `contains` / migration check.
fn merge_ui_settings(mut settings: Map<String, Value>) -> (Map<String, Value>, bool) {
    let mut updated = false;
    for (key, value) in ui_settings() {
        let should_set = match settings.get(key) {
            None => true,
            Some(current) => migration_value(key).is_some_and(|migrated| *current == migrated),
        };
        if should_set {
            settings.insert(key.to_owned(), value);
            updated = true;
        }
    }
    (settings, updated)
}

/// The Windhawk editor settings block (`uiSettings` in the C++), in the order
/// the C++ writes them so a freshly seeded file matches. `clangd.path` points
/// at the compiler via `WINDHAWK_COMPILER_PATH`. The chrome keys here
/// (`workbench.editor.showTabs`, `workbench.statusBar.visible`, `git.enabled`
/// are all `false`) are the shared *user* defaults; a per-mod workspace's
/// `.vscode/settings.json` overrides them to the editor-mode values
/// (workspace), so these are the browse-mode baseline.
fn ui_settings() -> [(&'static str, Value); 31] {
    [
        ("telemetry.telemetryLevel", json!("off")),
        ("update.mode", json!("none")),
        ("update.showReleaseNotes", json!(false)),
        ("extensions.autoCheckUpdates", json!(false)),
        ("extensions.autoUpdate", json!(false)),
        ("files.autoSave", json!("afterDelay")),
        (
            "window.title",
            json!("${dirty}${activeEditorShort}${separator}${appName}"),
        ),
        ("workbench.enableExperiments", json!(false)),
        (
            "workbench.settings.enableNaturalLanguageSearch",
            json!(false),
        ),
        ("workbench.editor.restoreViewState", json!(false)),
        ("workbench.tips.enabled", json!(false)),
        ("workbench.startupEditor", json!("none")),
        ("workbench.layoutControl.enabled", json!(false)),
        ("security.workspace.trust.enabled", json!(false)),
        ("editor.inlayHints.enabled", json!("off")),
        ("editor.tabSize", json!(4)),
        ("editor.insertSpaces", json!(true)),
        ("editor.detectIndentation", json!(false)),
        (
            "clangd.path",
            json!("${env:WINDHAWK_COMPILER_PATH}\\bin\\clangd.exe"),
        ),
        ("clangd.arguments", json!(["-header-insertion=never"])),
        ("clangd.checkUpdates", json!(false)),
        ("window.menuBarVisibility", json!("compact")),
        ("workbench.activityBar.visible", json!(false)),
        ("workbench.editor.showTabs", json!(false)),
        ("workbench.statusBar.visible", json!(false)),
        ("git.enabled", json!(false)),
        ("git.showProgress", json!(false)),
        ("git.decorations.enabled", json!(false)),
        ("git.ignoreMissingGitWarning", json!(true)),
        ("git.ignoreLegacyWarning", json!(true)),
        ("git.ignoreWindowsGit27Warning", json!(true)),
    ]
}

/// The migration value for a settings key (`uiSettingsToMigrate` in the C++): only
/// `clangd.path` migrates, from the old UI-path-relative clangd to the compiler-path
/// one in [`ui_settings`]. A file still holding this old value is upgraded; any other
/// key, or a user-customized `clangd.path`, is left untouched.
fn migration_value(key: &str) -> Option<Value> {
    (key == "clangd.path").then(|| {
        json!(
            "${env:WINDHAWK_UI_PATH}\\resources\\app\\extensions\\clangd\\clangd\\bin\\clangd.exe"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::ffi::OsString;

    use tempfile::TempDir;

    /// A launcher over fixed fixture paths, so tests exercise the command/env assembly
    /// without a real install.
    fn fixture_launcher(app_data: &Path, arm64_enabled: bool) -> Launcher {
        Launcher::new(app_data, r"C:\wh\ui", r"C:\wh\compiler", arm64_enabled)
    }

    /// The explicitly set/removed environment of a `Command`, keyed by name, as owned
    /// strings so a test can look up a variable regardless of the ambient environment.
    fn command_envs(command: &Command) -> HashMap<String, Option<String>> {
        command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    // ---- env stripping ---------------------------------------------------

    #[test]
    fn strips_only_electron_and_vscode_prefixed_vars() {
        assert!(should_strip_env(OsStr::new("VSCODE_PID")));
        assert!(should_strip_env(OsStr::new("VSCODE_PORTABLE")));
        assert!(should_strip_env(OsStr::new("ELECTRON_RUN_AS_NODE")));
        // A Windhawk var is set explicitly, not stripped by prefix; unrelated vars stay.
        assert!(!should_strip_env(OsStr::new("WINDHAWK_UI_PATH")));
        assert!(!should_strip_env(OsStr::new("PATH")));
        assert!(!should_strip_env(OsStr::new("HOMEVSCODE_")));
    }

    // ---- command assembly ------------------------------------------------

    #[test]
    fn build_command_uses_the_workspace_folder_and_launch_switches() {
        let temp = TempDir::new().unwrap();
        let launcher = fixture_launcher(temp.path(), false);
        let exe = Path::new(r"C:\wh\ui\VSCodium.exe");
        let workspace = Path::new(r"C:\wh\appdata\EditorWorkspaces\1");

        let command = launcher.build_command(exe, workspace);

        assert_eq!(command.get_program(), OsStr::new(r"C:\wh\ui\VSCodium.exe"));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new(r"C:\wh\appdata\EditorWorkspaces\1"),
                OsStr::new("--locale=en"),
                OsStr::new("--no-sandbox"),
                OsStr::new("--disable-gpu-sandbox"),
            ]
        );
    }

    #[test]
    fn build_command_sets_the_windhawk_env_over_the_inherited_block() {
        let temp = TempDir::new().unwrap();
        let launcher = fixture_launcher(temp.path(), false);
        let command = launcher.build_command(Path::new("code.exe"), Path::new("ws"));
        let envs = command_envs(&command);

        // VSCODE_PORTABLE is the UIData folder under the launcher's appData root.
        assert_eq!(
            envs.get(ENV_VSCODE_PORTABLE),
            Some(&Some(
                temp.path().join(UI_DATA_DIR).to_string_lossy().into_owned()
            ))
        );
        assert_eq!(
            envs.get(ENV_WINDHAWK_UI_PATH),
            Some(&Some(r"C:\wh\ui".to_owned()))
        );
        assert_eq!(
            envs.get(ENV_WINDHAWK_COMPILER_PATH),
            Some(&Some(r"C:\wh\compiler".to_owned()))
        );
        // arm64 disabled: the flag is left as inherited, never set explicitly.
        assert!(!envs.contains_key(ENV_WINDHAWK_ARM64_ENABLED));
    }

    #[test]
    fn build_command_sets_arm64_flag_when_enabled() {
        let temp = TempDir::new().unwrap();
        let launcher = fixture_launcher(temp.path(), true);
        let command = launcher.build_command(Path::new("code.exe"), Path::new("ws"));
        let envs = command_envs(&command);
        assert_eq!(
            envs.get(ENV_WINDHAWK_ARM64_ENABLED),
            Some(&Some("1".to_owned()))
        );
    }

    #[test]
    fn build_command_strips_an_inherited_vscode_var() {
        // SAFETY-free stand-in: rather than mutate the process environment (unsafe and
        // racy across tests), assert the strip decision the applier is built on. The
        // applier itself iterates std::env and calls env_remove for each such name.
        let inherited: Vec<OsString> = vec![
            OsString::from("VSCODE_PID"),
            OsString::from("WINDHAWK_UI_PATH"),
        ];
        let stripped: Vec<_> = inherited
            .iter()
            .filter(|name| should_strip_env(name))
            .collect();
        assert_eq!(stripped, vec![&OsString::from("VSCODE_PID")]);
    }

    // ---- exe location ----------------------------------------------------

    #[test]
    fn locate_prefers_vscodium_then_falls_back_to_code() {
        let temp = TempDir::new().unwrap();
        let ui = temp.path();
        let launcher = Launcher::new(temp.path(), ui, r"C:\wh\compiler", false);

        // Neither present: no editor found.
        assert!(launcher.locate_editor_exe().is_none());

        // Only Code.exe: fall back to it.
        fs::write(ui.join(VSCODE_EXE), b"").unwrap();
        assert_eq!(launcher.locate_editor_exe(), Some(ui.join(VSCODE_EXE)));

        // VSCodium.exe present too: it wins.
        fs::write(ui.join(VSCODIUM_EXE), b"").unwrap();
        assert_eq!(launcher.locate_editor_exe(), Some(ui.join(VSCODIUM_EXE)));
    }

    #[test]
    fn is_available_tracks_a_non_empty_ui_path() {
        let temp = TempDir::new().unwrap();
        // A set UI path is "available" even before any exe exists (a missing exe is
        // a launch failure, not a missing install).
        let installed = Launcher::new(temp.path(), r"C:\wh\ui", r"C:\wh\compiler", false);
        assert!(installed.is_available());
        // An empty UI path (development tools not installed) is unavailable.
        let missing = Launcher::new(temp.path(), "", "", false);
        assert!(!missing.is_available());
    }

    #[test]
    fn launch_reports_editor_not_found_when_no_exe_exists() {
        let temp = TempDir::new().unwrap();
        // uiPath exists but holds no editor exe.
        let ui = temp.path().join("ui");
        fs::create_dir_all(&ui).unwrap();
        let launcher = Launcher::new(temp.path(), &ui, r"C:\wh\compiler", false);

        let error = launcher.launch(temp.path()).unwrap_err();
        match error {
            LaunchError::EditorNotFound(path) => assert_eq!(path, ui),
            other => panic!("expected EditorNotFound, got {other:?}"),
        }
    }

    // ---- prepare_ui_settings ---------------------------------------------

    fn ui_settings_path(ui_data: &Path) -> PathBuf {
        ui_data
            .join(USER_DATA_DIR)
            .join(USER_DIR)
            .join(SETTINGS_FILE)
    }

    #[test]
    fn prepare_ui_settings_seeds_a_fresh_settings_file() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        prepare_ui_settings(&ui_data).unwrap();

        let settings = read_settings_object(&ui_settings_path(&ui_data));
        // Every editor setting is present, in the C++ order.
        assert_eq!(settings.len(), ui_settings().len());
        assert_eq!(settings["telemetry.telemetryLevel"], json!("off"));
        assert_eq!(
            settings["clangd.path"],
            json!("${env:WINDHAWK_COMPILER_PATH}\\bin\\clangd.exe")
        );
        assert_eq!(settings["editor.tabSize"], json!(4));
        assert_eq!(
            settings["clangd.arguments"],
            json!(["-header-insertion=never"])
        );
        let keys: Vec<_> = settings.keys().collect();
        assert_eq!(keys[0], "telemetry.telemetryLevel");
        assert_eq!(
            keys.last().unwrap().as_str(),
            "git.ignoreWindowsGit27Warning"
        );
    }

    #[test]
    fn prepare_ui_settings_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        prepare_ui_settings(&ui_data).unwrap();
        let after_first = fs::read_to_string(ui_settings_path(&ui_data)).unwrap();
        prepare_ui_settings(&ui_data).unwrap();
        let after_second = fs::read_to_string(ui_settings_path(&ui_data)).unwrap();

        assert_eq!(after_first, after_second);
    }

    #[test]
    fn merge_preserves_foreign_keys_and_user_values() {
        // A file with a user-customized clangd.path, an unrelated key, and one of our
        // keys already set to a non-default value: the merge adds only the missing keys
        // and leaves all three of these alone.
        let mut existing = Map::new();
        existing.insert("clangd.path".to_owned(), json!(r"C:\my\clangd.exe"));
        existing.insert("editor.fontSize".to_owned(), json!(15));
        existing.insert("editor.tabSize".to_owned(), json!(2));

        let (merged, updated) = merge_ui_settings(existing);

        assert!(updated);
        // Custom / foreign / already-present values survive untouched.
        assert_eq!(merged["clangd.path"], json!(r"C:\my\clangd.exe"));
        assert_eq!(merged["editor.fontSize"], json!(15));
        assert_eq!(merged["editor.tabSize"], json!(2));
        // A missing key was added.
        assert_eq!(merged["telemetry.telemetryLevel"], json!("off"));
    }

    #[test]
    fn merge_upgrades_the_migrated_clangd_path() {
        let migrated = migration_value("clangd.path").unwrap();
        let mut existing = Map::new();
        existing.insert("clangd.path".to_owned(), migrated);

        let (merged, updated) = merge_ui_settings(existing);

        assert!(updated);
        // The old migrated value is replaced with the compiler-path clangd.
        assert_eq!(
            merged["clangd.path"],
            json!("${env:WINDHAWK_COMPILER_PATH}\\bin\\clangd.exe")
        );
    }

    #[test]
    fn merge_of_an_already_seeded_object_reports_no_change() {
        let (seeded, _) = merge_ui_settings(Map::new());
        let (again, updated) = merge_ui_settings(seeded.clone());
        assert!(!updated);
        assert_eq!(seeded, again);
    }
}
