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
//!   and `WINDHAWK_COMPILER_PATH` (`BuildUIProcessEnvBlock`). The extension reads
//!   these to find clangd and the compiler; arm64 is not forwarded - the
//!   extension's own core detects the OS native machine;
//! - **spawn** the editor exe with the workspace directory as the folder argument,
//!   plus the `--locale=en --no-sandbox --disable-gpu-sandbox` locale and
//!   AppLocker/elevation workarounds `RunVSCodeUI` documents. The child inherits the
//!   native UI's integrity, which is what editor compiles/installs
//!   need, so no second elevation ladder is run.
//!
//! The shared color-theme keys are a separate concern from a launch: they track the
//! app's UI theme through [`Launcher::sync_theme`], which the `updateAppSettings` handler
//! calls when the theme setting changes. That rewrites only the color-theme keys (and only
//! when they are in a state Windhawk itself wrote), so an open editor re-themes live and the
//! next launch opens matching - without the launch path itself reading the theme.
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

use super::{parse_jsonc, to_pretty_json};
use crate::shell::ThemeSetting;

/// The VSCodium portable-data folder under `appData` (`getCoreInfo`
/// `fsPaths.appDataPath` joined with this), the same folder the C++
/// `StorageManager::GetUIDataPath` resolves. It holds the VSCodium user settings
/// (`PrepareUISettings`) and is passed to the child as `VSCODE_PORTABLE`. Distinct
/// from the native window's own WebView2 data folder, `UIMainData`
/// (`lifecycle::ui_data`).
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

/// The inherited-environment prefixes stripped before the child launches, so a
/// VSCodium/Electron parent's own variables do not leak into the spawned editor.
const STRIPPED_ENV_PREFIXES: [&str; 2] = ["ELECTRON_", "VSCODE_"];

/// A launch failure. Surfaced to the caller so the development handler can
/// return it to the front-end as the standard error payload (auto-surfaced in
/// the UI as a notification); a launch failure is recoverable, so it never
/// terminates the app.
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

    /// Sync the shared VSCodium user settings' color-theme keys to `theme` (or record the
    /// request), so an open editor re-themes live and the next launch opens matching. The
    /// `updateAppSettings` handler calls this when the theme setting changes; it touches
    /// only the color-theme keys, and only when they are in a state Windhawk itself wrote.
    fn sync_theme(&self, theme: ThemeSetting) -> io::Result<()>;
}

impl LaunchEditor for Launcher {
    fn open_workspace(&self, workspace: &Path) -> Result<(), LaunchError> {
        self.launch(workspace)
    }

    fn sync_theme(&self, theme: ThemeSetting) -> io::Result<()> {
        sync_theme_settings(&self.ui_data_path, theme)
    }
}

/// Whether a code editor is installed, from the `getCoreInfo` UI path.
///
/// The development tools are an optional install component; when they are absent
/// the UI path is empty, and the launch entry points reply "UI missing" instead of
/// attempting a launch. A UI path that is set but holds no editor exe still counts
/// as installed here - that is a launch failure, not a missing install.
///
/// A free function over the path rather than a method on the launch seam: both
/// processes read the same `getCoreInfo`, so the answer is available wherever the
/// question is asked and never costs a round trip to the elevated helper. It is
/// consulted on paths that have nothing to do with launching an editor - every
/// local compile checks it - which is what makes that matter.
pub fn dev_tools_installed(ui_path: &Path) -> bool {
    !ui_path.as_os_str().is_empty()
}

/// The VSCodium launcher, holding the resolved paths a launch needs
/// (`getCoreInfo` `fsPaths`), so a single instance can launch many per-mod
/// workspaces. Constructed once by the development handlers from the core info;
/// kept free of the protocol DTOs so it stays a testable OS-touchpoint leaf.
pub struct Launcher {
    /// `<appData>/UIData`, the VSCodium portable-data folder (`VSCODE_PORTABLE`).
    ui_data_path: PathBuf,
    /// `fsPaths.uiPath`, where the editor exe and clangd live (`WINDHAWK_UI_PATH`).
    ui_path: PathBuf,
    /// `fsPaths.compilerPath`, where the compiler and its clangd live
    /// (`WINDHAWK_COMPILER_PATH`).
    compiler_path: PathBuf,
}

impl Launcher {
    /// A launcher rooted at the `appData` directory (from which `UIData` is derived,
    /// like the workspace manager derives its `EditorWorkspaces` container) plus the
    /// UI and compiler paths, all from `getCoreInfo`.
    pub fn new(
        app_data: impl Into<PathBuf>,
        ui_path: impl Into<PathBuf>,
        compiler_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ui_data_path: app_data.into().join(UI_DATA_DIR),
            ui_path: ui_path.into(),
            compiler_path: compiler_path.into(),
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
    /// a variable overrides any inherited value.
    fn apply_env(&self, command: &mut Command) {
        for name in std::env::vars_os().map(|(name, _)| name) {
            if should_strip_env(&name) {
                command.env_remove(&name);
            }
        }
        command.env(ENV_VSCODE_PORTABLE, &self.ui_data_path);
        command.env(ENV_WINDHAWK_UI_PATH, &self.ui_path);
        command.env(ENV_WINDHAWK_COMPILER_PATH, &self.compiler_path);
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
/// only if anything changed. Idempotent - a no-op once seeded. The color-theme keys are
/// a separate concern, synced on a theme-setting change by [`sync_theme_settings`].
fn prepare_ui_settings(ui_data: &Path) -> io::Result<()> {
    let user_dir = ui_data.join(USER_DATA_DIR).join(USER_DIR);
    fs::create_dir_all(&user_dir)?;
    let settings_path = user_dir.join(SETTINGS_FILE);

    let existing = read_settings_object(&settings_path)?;
    let (merged, updated) = merge_ui_settings(existing);
    if updated {
        fs::write(&settings_path, to_pretty_json(&Value::Object(merged)))?;
    }
    Ok(())
}

/// Read a JSON object from a VSCodium settings file, degrading a missing, unparseable, or
/// non-object file to an empty object (the `!settingsJson.is_object()` tolerance the C++
/// `PrepareUISettings` applies before merging). The parse is JSONC via [`parse_jsonc`], so a
/// settings file carrying comments or trailing commas keeps its real keys instead of
/// degrading to empty and being clobbered by the merge.
///
/// A file that exists but cannot be read is an error, not an empty object. The callers write
/// the result back, so degrading a read failure (a sharing violation while VSCodium holds the
/// file, a denied ACL) would replace every setting the user has with whatever the caller
/// merges in.
fn read_settings_object(path: &Path) -> io::Result<Map<String, Value>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(error),
    };
    Ok(parse_jsonc(&text)
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default())
}

/// Merge the base Windhawk editor block into an existing settings object, returning the
/// merged object and whether anything changed. A setting is written when it is absent, or
/// when the current value equals the migration value for that key (so an old migrated
/// `clangd.path` is upgraded, but a user-customized one is left alone). Every other key the
/// file carries is preserved, and existing keys keep their positions (`preserve_order`), so
/// this is a merge, not an overwrite - matching the C++'s per-key `contains` / migration
/// check. The color-theme keys are not merged here; [`apply_theme_settings`] forces them.
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
/// (workspace), so these are the browse-mode baseline. The color-theme keys are
/// not here - they depend on the app's theme setting and are appended by
/// [`theme_settings`].
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

/// Sync the shared VSCodium user settings' color-theme keys to the app's `theme`: read
/// `<uiData>/user-data/User/settings.json`, force the color-theme keys via
/// [`apply_theme_settings`], and write it back only if they changed. The `updateAppSettings`
/// handler calls this on a theme-setting change, so an open editor re-themes live (VSCodium
/// watches this file) and the next launch opens matching. A theme that already matches, and
/// one the user hand-picked inside VSCodium, is left untouched; and nothing is written - or
/// even created - when there is no change (so a `Dark` sync against a missing file is a
/// no-op). Unlike [`prepare_ui_settings`], this does not seed the base editor block; that
/// stays a launch-time concern.
fn sync_theme_settings(ui_data: &Path, theme: ThemeSetting) -> io::Result<()> {
    let user_dir = ui_data.join(USER_DATA_DIR).join(USER_DIR);
    let settings_path = user_dir.join(SETTINGS_FILE);

    let existing = read_settings_object(&settings_path)?;
    let (merged, changed) = apply_theme_settings(existing, theme);
    if changed {
        fs::create_dir_all(&user_dir)?;
        fs::write(&settings_path, to_pretty_json(&Value::Object(merged)))?;
    }
    Ok(())
}

/// The color-theme keys VSCodium should carry for the app's theme setting, so the editor
/// opens matching the app. `Dark` is VSCodium's own default and needs no keys (the
/// browse-mode baseline is dark). `Light` pins the light theme. `Auto` follows the OS
/// through `window.autoDetectColorScheme` (VSCodium then picks its preferred light/dark
/// theme per the system); `workbench.colorTheme` names the light theme, the value used when
/// auto-detect is off. Every returned key must be one of [`THEME_KEYS`], which
/// [`apply_theme_settings`] clears when a theme does not use it.
fn theme_settings(theme: ThemeSetting) -> Vec<(&'static str, Value)> {
    match theme {
        ThemeSetting::Dark => Vec::new(),
        ThemeSetting::Light => vec![("workbench.colorTheme", json!("Default Light+"))],
        ThemeSetting::Auto => vec![
            ("window.autoDetectColorScheme", json!(true)),
            ("workbench.colorTheme", json!("Default Light+")),
        ],
    }
}

/// Every VSCodium key [`theme_settings`] governs, across all themes. [`apply_theme_settings`]
/// removes any of these the current theme does not set, so switching the app theme fully
/// re-syncs the editor (e.g. leaving `Auto` clears `window.autoDetectColorScheme` rather
/// than stranding it `true`).
const THEME_KEYS: [&str; 2] = ["window.autoDetectColorScheme", "workbench.colorTheme"];

/// Force the color-theme keys to the app's `theme`, returning the updated object and whether
/// anything changed (so a sync whose theme already matches does not rewrite the file). The
/// app theme is authoritative for the editor theme: the keys the theme uses are set and the
/// [`THEME_KEYS`] it does not use are removed - but only when the file's current color-theme
/// keys are one of the combinations Windhawk itself writes (see
/// [`theme_keys_match_a_known_theme`]). A theme the user hand-picked inside VSCodium is an
/// unrecognized combination and is left untouched, so a deliberate editor-theme choice
/// survives the sync.
fn apply_theme_settings(
    mut settings: Map<String, Value>,
    theme: ThemeSetting,
) -> (Map<String, Value>, bool) {
    if !theme_keys_match_a_known_theme(&settings) {
        return (settings, false);
    }

    let desired = theme_settings(theme);
    let mut changed = false;

    for (key, value) in &desired {
        if settings.get(*key) != Some(value) {
            settings.insert((*key).to_owned(), value.clone());
            changed = true;
        }
    }

    for key in THEME_KEYS {
        let used = desired.iter().any(|(k, _)| *k == key);
        if !used && settings.shift_remove(key).is_some() {
            changed = true;
        }
    }

    (settings, changed)
}

/// Whether the settings' [`THEME_KEYS`] are exactly one of the combinations
/// [`theme_settings`] produces (for `Dark`, `Light`, or `Auto`) - a state Windhawk wrote,
/// safe to re-sync - rather than one the user hand-picked inside VSCodium (a custom
/// `workbench.colorTheme`, `window.autoDetectColorScheme: false`, and so on), which
/// [`apply_theme_settings`] leaves alone. The empty state (no theme keys) is the `Dark`
/// combination, so a freshly seeded env still syncs to the app theme.
fn theme_keys_match_a_known_theme(settings: &Map<String, Value>) -> bool {
    [ThemeSetting::Dark, ThemeSetting::Light, ThemeSetting::Auto]
        .into_iter()
        .any(|theme| theme_keys_equal(settings, theme))
}

/// Whether the settings' [`THEME_KEYS`] match exactly what [`theme_settings`] writes for
/// `theme`: each governed key present with the expected value, or absent when the theme
/// does not set it.
fn theme_keys_equal(settings: &Map<String, Value>, theme: ThemeSetting) -> bool {
    let desired = theme_settings(theme);
    THEME_KEYS.iter().all(|&key| {
        let want = desired
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, value)| value);
        settings.get(key) == want
    })
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
    fn fixture_launcher(app_data: &Path) -> Launcher {
        Launcher::new(app_data, r"C:\wh\ui", r"C:\wh\compiler")
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
        let launcher = fixture_launcher(temp.path());
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
        let launcher = fixture_launcher(temp.path());
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
        let launcher = Launcher::new(temp.path(), ui, r"C:\wh\compiler");

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
    fn dev_tools_track_a_non_empty_ui_path() {
        // A set UI path counts as installed even before any exe exists (a missing
        // exe is a launch failure, not a missing install).
        assert!(dev_tools_installed(Path::new(r"C:\wh\ui")));
        // An empty UI path is the development tools not being installed.
        assert!(!dev_tools_installed(Path::new("")));
    }

    #[test]
    fn launch_reports_editor_not_found_when_no_exe_exists() {
        let temp = TempDir::new().unwrap();
        // uiPath exists but holds no editor exe.
        let ui = temp.path().join("ui");
        fs::create_dir_all(&ui).unwrap();
        let launcher = Launcher::new(temp.path(), &ui, r"C:\wh\compiler");

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

        let settings = read_settings_object(&ui_settings_path(&ui_data)).unwrap();
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

    #[test]
    fn read_settings_object_parses_jsonc_comments_and_trailing_commas() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(SETTINGS_FILE);
        // A VSCodium-style settings file: a line comment, an inline block comment, and a
        // trailing comma - none of which strict JSON accepts.
        fs::write(
            &path,
            "{\n    // a user comment\n    \"editor.fontSize\": 15, /* inline */\n    \"telemetry.telemetryLevel\": \"all\",\n}\n",
        )
        .unwrap();

        let settings = read_settings_object(&path).unwrap();
        assert_eq!(settings["editor.fontSize"], json!(15));
        assert_eq!(settings["telemetry.telemetryLevel"], json!("all"));
    }

    #[test]
    fn read_settings_object_treats_a_missing_file_as_empty() {
        let temp = TempDir::new().unwrap();
        // The fresh-install case - no file, and not even a parent directory - is an empty
        // object to merge into, not an error.
        let path = temp.path().join(USER_DIR).join(SETTINGS_FILE);
        assert!(read_settings_object(&path).unwrap().is_empty());
    }

    #[test]
    fn read_settings_object_reports_a_read_failure() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(SETTINGS_FILE);
        // A settings.json that exists but cannot be read - here a directory standing in for
        // the locked or ACL-denied file - must not degrade to an empty object: the callers
        // write the result back, which would wipe every setting the user has.
        fs::create_dir(&path).unwrap();

        let error = read_settings_object(&path).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn prepare_ui_settings_preserves_a_jsonc_users_keys() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);
        let path = ui_settings_path(&ui_data);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A user-edited file with a comment, a trailing comma, a foreign key, and one of
        // our keys set to a custom value. A strict parse would fail and the merge would
        // clobber all of it; the JSONC parse keeps the user's keys, adding only the
        // missing base ones.
        fs::write(
            &path,
            "{\n    // keep my settings\n    \"editor.fontSize\": 15,\n    \"telemetry.telemetryLevel\": \"all\",\n}\n",
        )
        .unwrap();

        prepare_ui_settings(&ui_data).unwrap();

        let settings = read_settings_object(&path).unwrap();
        assert_eq!(settings["editor.fontSize"], json!(15));
        assert_eq!(settings["telemetry.telemetryLevel"], json!("all"));
        assert_eq!(settings["update.mode"], json!("none"));
    }

    // ---- theme seeding ---------------------------------------------------

    #[test]
    fn apply_theme_dark_seeds_no_theme_keys() {
        // Dark is VSCodium's own default: neither theme key is set.
        let (settings, changed) = apply_theme_settings(Map::new(), ThemeSetting::Dark);
        assert!(!changed);
        assert!(!settings.contains_key("workbench.colorTheme"));
        assert!(!settings.contains_key("window.autoDetectColorScheme"));
    }

    #[test]
    fn apply_theme_light_pins_the_light_theme() {
        let (settings, changed) = apply_theme_settings(Map::new(), ThemeSetting::Light);
        assert!(changed);
        assert_eq!(settings["workbench.colorTheme"], json!("Default Light+"));
        // Light is a fixed theme, not OS-following.
        assert!(!settings.contains_key("window.autoDetectColorScheme"));
    }

    #[test]
    fn apply_theme_auto_follows_the_os() {
        let (settings, changed) = apply_theme_settings(Map::new(), ThemeSetting::Auto);
        assert!(changed);
        assert_eq!(settings["window.autoDetectColorScheme"], json!(true));
        assert_eq!(settings["workbench.colorTheme"], json!("Default Light+"));
    }

    #[test]
    fn apply_theme_resyncs_a_known_combination() {
        // A file whose keys are a Windhawk-written combination (here Auto) re-syncs to the
        // app theme (here Light): the colorTheme stays, the stale auto-detect is dropped.
        let mut existing = Map::new();
        existing.insert("window.autoDetectColorScheme".to_owned(), json!(true));
        existing.insert("workbench.colorTheme".to_owned(), json!("Default Light+"));

        let (settings, changed) = apply_theme_settings(existing, ThemeSetting::Light);

        assert!(changed);
        assert_eq!(settings["workbench.colorTheme"], json!("Default Light+"));
        assert!(!settings.contains_key("window.autoDetectColorScheme"));
    }

    #[test]
    fn apply_theme_leaves_a_custom_editor_theme_untouched() {
        // A color theme the user picked inside VSCodium is not a known combination, so the
        // re-sync leaves it alone even though the app theme is Light.
        let mut existing = Map::new();
        existing.insert("workbench.colorTheme".to_owned(), json!("Monokai"));

        let (settings, changed) = apply_theme_settings(existing, ThemeSetting::Light);

        assert!(!changed);
        assert_eq!(settings["workbench.colorTheme"], json!("Monokai"));
    }

    #[test]
    fn apply_theme_leaves_an_unrecognized_auto_detect_combination_untouched() {
        // auto-detect on with a non-Windhawk light/dark theme pairing is a hand-tuned combo:
        // neither the value nor the extra key is a known combination, so it survives.
        let mut existing = Map::new();
        existing.insert("window.autoDetectColorScheme".to_owned(), json!(true));
        existing.insert("workbench.colorTheme".to_owned(), json!("Monokai"));

        let (settings, changed) = apply_theme_settings(existing, ThemeSetting::Dark);

        assert!(!changed);
        assert_eq!(settings["window.autoDetectColorScheme"], json!(true));
        assert_eq!(settings["workbench.colorTheme"], json!("Monokai"));
    }

    #[test]
    fn apply_theme_dark_clears_both_theme_keys() {
        // Switching to Dark strips every governed key back to the VSCodium default.
        let mut existing = Map::new();
        existing.insert("window.autoDetectColorScheme".to_owned(), json!(true));
        existing.insert("workbench.colorTheme".to_owned(), json!("Default Light+"));

        let (settings, changed) = apply_theme_settings(existing, ThemeSetting::Dark);

        assert!(changed);
        assert!(!settings.contains_key("window.autoDetectColorScheme"));
        assert!(!settings.contains_key("workbench.colorTheme"));
    }

    #[test]
    fn apply_theme_is_idempotent_when_already_synced() {
        // A sync whose theme already matches reports no change (so no rewrite).
        let (once, _) = apply_theme_settings(Map::new(), ThemeSetting::Auto);
        let (twice, changed) = apply_theme_settings(once.clone(), ThemeSetting::Auto);
        assert!(!changed);
        assert_eq!(once, twice);
    }

    // ---- sync_theme_settings ---------------------------------------------

    #[test]
    fn sync_theme_settings_creates_and_writes_the_light_theme() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        // No settings file yet; a Light sync creates it with just the color-theme key.
        sync_theme_settings(&ui_data, ThemeSetting::Light).unwrap();

        let settings = read_settings_object(&ui_settings_path(&ui_data)).unwrap();
        assert_eq!(settings["workbench.colorTheme"], json!("Default Light+"));
        assert!(!settings.contains_key("window.autoDetectColorScheme"));
    }

    #[test]
    fn sync_theme_settings_dark_on_a_missing_file_writes_nothing() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        // Dark needs no keys, so a sync against a missing file changes nothing and does
        // not create the file (or its parent dirs).
        sync_theme_settings(&ui_data, ThemeSetting::Dark).unwrap();

        assert!(!ui_settings_path(&ui_data).exists());
    }

    #[test]
    fn sync_theme_settings_leaves_the_base_block_intact() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        // Seed the base block (a Dark file has no theme keys), then sync to Light.
        prepare_ui_settings(&ui_data).unwrap();
        sync_theme_settings(&ui_data, ThemeSetting::Light).unwrap();

        let settings = read_settings_object(&ui_settings_path(&ui_data)).unwrap();
        // The theme key is added; every base setting survives.
        assert_eq!(settings["workbench.colorTheme"], json!("Default Light+"));
        assert_eq!(settings["telemetry.telemetryLevel"], json!("off"));
        assert_eq!(settings.len(), ui_settings().len() + 1);
    }

    #[test]
    fn sync_theme_settings_is_idempotent() {
        let temp = TempDir::new().unwrap();
        let ui_data = temp.path().join(UI_DATA_DIR);

        sync_theme_settings(&ui_data, ThemeSetting::Auto).unwrap();
        let after_first = fs::read_to_string(ui_settings_path(&ui_data)).unwrap();
        sync_theme_settings(&ui_data, ThemeSetting::Auto).unwrap();
        let after_second = fs::read_to_string(ui_settings_path(&ui_data)).unwrap();

        assert_eq!(after_first, after_second);
    }
}
