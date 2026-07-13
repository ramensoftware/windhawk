//! The multi-workspace manager.
//!
//! Each edited mod gets its own directory `EditorWorkspaces/N` - a numbered folder
//! inside the `EditorWorkspaces` container under `appData`, so several mods can be
//! edited in parallel (each in its own VSCodium window) and the numbered folders
//! cluster in one place instead of littering `appData`. This module owns the four
//! operations over that family plus the file-content builders that make a directory
//! VSCodium-ready:
//!
//! - **allocate** the lowest free `EditorWorkspaces/N` for a new/fork mod, by an
//!   atomic directory create so two rapid clicks never bind the same index;
//! - **locate** the workspace already editing a mod (edit-reuse), keyed on the
//!   identity marker `windhawk.editedModId` with an `@id`-parse fallback;
//! - **sweep** abandoned workspaces: keep a workspace whose mod still exists
//!   in storage, and reclaim the rest through a rename probe that spares an
//!   actively-open editor;
//! - **initialize** an allocated directory (port of the extension's
//!   `editorWorkspaceUtils`): `mod.wh.cpp`, `compile_flags.txt`, `.clang-format`,
//!   a git baseline, and the `.vscode/settings.json` editor-mode seed.
//!
//! Enumeration/allocation/locate/sweep operate inside the container and match a
//! workspace by a numeric name alone (`^\d+$`), so the container's own shared
//! `.clang-format.windhawk` and any `N.tmp` sweep leftover are excluded; the legacy
//! bare `EditorWorkspace` (the whole-UI VSCodium mode's single workspace), `UIData`,
//! and the UI's other data folders sit at the `appData` root, outside the container,
//! and are never even seen.
//!
//! The three core-backed inputs the manager needs are injected as seams, so the
//! policy and the content builders stay DLL-free and unit-testable: the compile
//! flags are passed in (from the core's `getCompileFlags`), and `doesModExist`
//! (sweep keep/reclaim) and the `@id` `parse_id` fallback (locate/sweep) are passed
//! as closures.

use std::fs;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::to_pretty_json;

/// The container directory under `appData` that holds every per-mod workspace, so
/// the numbered directories cluster in one folder instead of littering `appData`.
/// A workspace inside it is named by its running-counter index alone (`1`, `2`, ...),
/// the strict pattern being a non-empty run of ASCII digits.
const WORKSPACES_DIR: &str = "EditorWorkspaces";

/// The suffix a workspace is renamed to while the sweep probes it for an open
/// editor. Kept off the numeric `N` namespace so a transient `.tmp` neither
/// matches a locate nor blocks an allocation.
const TMP_SUFFIX: &str = ".tmp";

/// The mod source file at the root of a workspace.
const MOD_SOURCE_FILE: &str = "mod.wh.cpp";
/// The clangd flags file (`getCompileFlags`, one flag per line).
const COMPILE_FLAGS_FILE: &str = "compile_flags.txt";
/// The per-workspace clang-format config, copied from the shared parent file.
const CLANG_FORMAT_FILE: &str = ".clang-format";
/// The optional shared clang-format override: a file the user may place in the
/// `EditorWorkspaces` container (the parent of every workspace) to format all
/// workspaces at once. Copied into each workspace as `.clang-format` when
/// present; never created by the code.
const SHARED_CLANG_FORMAT_FILE: &str = ".clang-format.windhawk";
/// A `windhawk_api.h` from older workspaces; the header now lives in the compiler
/// include folder, so a stale copy is removed on init/re-seed for parity with the
/// extension.
const STALE_API_HEADER: &str = "windhawk_api.h";
/// The workspace settings folder and file that carry the editor-mode seed and the
/// identity marker.
const VSCODE_DIR: &str = ".vscode";
const VSCODE_SETTINGS_FILE: &str = "settings.json";

/// The identity marker: the mod a workspace edits, the exact key the extension
/// maintains through a compile-rename.
const KEY_EDITED_MOD_ID: &str = "windhawk.editedModId";
/// Whether the workspace holds unsaved edits; preserved across a re-seed so the
/// sidebar's "modified" indicator survives edit-reuse.
const KEY_EDITED_MOD_WAS_MODIFIED: &str = "windhawk.editedModWasModified";
/// The remaining editor-mode seed keys `enterEditorMode` persists.
const KEY_GIT_ENABLED: &str = "git.enabled";
const KEY_SHOW_TABS: &str = "workbench.editor.showTabs";
const KEY_STATUS_BAR: &str = "workbench.statusBar.visible";
const KEY_NO_WINDHAWK_EXIT_BUTTON: &str = "windhawk.noWindhawkExitButton";

/// A prepared per-mod workspace directory.
pub struct Workspace {
    path: PathBuf,
    index: u32,
}

impl Workspace {
    /// The workspace directory (the folder argument a VSCodium launch opens).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The running-counter index `N` of the workspace directory `EditorWorkspaces/N`.
    pub fn index(&self) -> u32 {
        self.index
    }
}

/// The inputs that seed a freshly allocated workspace. The compile flags are
/// the core's `getCompileFlags` result, passed in so the manager sources them
/// from the core rather than hard-coding a second copy.
pub struct WorkspaceInit<'a> {
    /// The `mod.wh.cpp` contents (template / installed source / fork transform).
    pub mod_source: &'a str,
    /// The bare mod id (no `local@` scope) written as the `editedModId` marker.
    pub mod_id: &'a str,
    /// The clangd flag set for `compile_flags.txt`.
    pub compile_flags: &'a [String],
}

/// What a sweep did with each `EditorWorkspaces/N` it considered. Indices are the
/// running counters, not paths, so a test can assert the decision without string
/// surgery.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Workspaces whose mod still exists in storage; left untouched.
    pub kept: Vec<u32>,
    /// Unused workspaces with no open editor; renamed and deleted.
    pub reclaimed: Vec<u32>,
    /// Unused workspaces the rename probe found an editor still holding; spared.
    pub in_use: Vec<u32>,
}

/// The manager over the `EditorWorkspaces/N` family, whose container is derived from
/// the `app_data` root passed to [`WorkspaceManager::new`].
///
/// It serializes its mutating operations behind one process-local lock:
/// allocate+initialize runs as a unit, and each sweep runs as a unit, so an
/// init completes atomically with respect to any sweep (the native UI holds no
/// directory handle during init, so an unserialized sweep's rename probe could
/// otherwise reclaim a directory being seeded) and two sweeps never overlap.
/// The lock is process-local, a separate concern from the cross-process
/// disjointness of storage.
pub struct WorkspaceManager {
    /// The `EditorWorkspaces` container (`<appData>/EditorWorkspaces`) that holds the
    /// numbered workspaces and the shared `.clang-format.windhawk`.
    dir: PathBuf,
    mutating: Mutex<()>,
}

impl WorkspaceManager {
    /// A manager rooted at the `appData` directory (`getCoreInfo`
    /// `fsPaths.appDataPath`), the same parent that holds `UIData` and the legacy
    /// `EditorWorkspace`. The per-mod workspaces live one level down, under the
    /// `EditorWorkspaces` container, so nothing new lands at the `appData` root.
    pub fn new(app_data: impl Into<PathBuf>) -> Self {
        Self {
            dir: app_data.into().join(WORKSPACES_DIR),
            mutating: Mutex::new(()),
        }
    }

    /// Allocate the lowest free `EditorWorkspaces/N` and initialize it from `init`,
    /// as one unit under the mutating lock.
    ///
    /// Allocation is the directory create: the lowest `N >= 1` whose directory does
    /// not exist under the container is created atomically, advancing on a collision,
    /// so concurrent allocations never bind the same index. A leftover
    /// `EditorWorkspaces/N.tmp` neither counts as taken nor collides, since the create
    /// targets the numeric name. The container itself is created first so the first
    /// allocation on a fresh install has a parent to create into.
    pub fn allocate_and_initialize(&self, init: &WorkspaceInit) -> io::Result<Workspace> {
        let _guard = self.lock();

        fs::create_dir_all(&self.dir)?;

        let mut index: u32 = 1;
        let path = loop {
            let candidate = self.dir.join(index.to_string());
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    index = index
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("ran out of EditorWorkspace indices"))?;
                }
                Err(error) => return Err(error),
            }
        };

        self.initialize(&path, init)?;
        Ok(Workspace { path, index })
    }

    /// Locate the workspace already editing `target_mod_id` (a bare id), for
    /// edit-reuse. Enumerates the `EditorWorkspaces/N` directories in index order,
    /// reads each one's identity (the `editedModId` marker, then a parsed `@id`
    /// fallback), and returns the first match. A read-only probe: it takes no lock
    /// and mutates nothing, so it is safe to run alongside an allocate or a sweep.
    pub fn locate(
        &self,
        target_mod_id: &str,
        parse_id: impl Fn(&str) -> Option<String>,
    ) -> io::Result<Option<Workspace>> {
        for (index, path) in self.enumerate_workspaces()? {
            if read_workspace_mod_id(&path, &parse_id).as_deref() == Some(target_mod_id) {
                return Ok(Some(Workspace { path, index }));
            }
        }
        Ok(None)
    }

    /// Re-seed an existing workspace's editor-mode settings before an
    /// edit-reuse launch: rewrite `.vscode/settings.json` (a merge, not an
    /// overwrite, so `editedModWasModified` and any extension-added keys
    /// survive) and carry over the stale-header cleanup. Required, not
    /// optional: a workspace found via the `@id` fallback may have had
    /// `editedModId` cleared by a prior `exitEditorMode`, and without rewriting
    /// it `restoreEditorMode` would enter browse mode. Leaves `mod.wh.cpp`
    /// as-is, since it may hold unsaved edits.
    pub fn reseed_editor_mode(&self, workspace: &Path, mod_id: &str) -> io::Result<()> {
        remove_stale_api_header(workspace);
        seed_editor_mode_settings(workspace, mod_id)
    }

    /// Garbage-collect abandoned workspaces, as one unit under the mutating
    /// lock. Run at native-UI startup and after a mod deletion.
    ///
    /// A leftover `EditorWorkspaces/N.tmp` from an interrupted sweep is deleted
    /// up front. Then, for each `EditorWorkspaces/N`: read its mod id, and if
    /// the mod still exists in storage (`does_mod_exist("local@"+id)`) keep it -
    /// that is a real mod's workspace, holding its unsaved edits and pch.
    /// Otherwise the workspace is unused (never compiled, its mod deleted, or
    /// unidentifiable), and it is reclaimed by the rename probe: rename the
    /// folder to `.tmp`, which fails while an editor holds it open, so a failed
    /// rename means "leave it" and a successful rename means "no editor open",
    /// after which the renamed folder is deleted. The probe is a robust
    /// heuristic, not a formal guarantee, so the sweep is best-effort.
    pub fn sweep(
        &self,
        does_mod_exist: impl Fn(&str) -> bool,
        parse_id: impl Fn(&str) -> Option<String>,
    ) -> io::Result<SweepReport> {
        let _guard = self.lock();

        self.delete_leftover_tmp_dirs()?;

        let mut report = SweepReport::default();
        for (index, path) in self.enumerate_workspaces()? {
            let keep = match read_workspace_mod_id(&path, &parse_id) {
                Some(id) => does_mod_exist(&format!("local@{id}")),
                // No marker and no parseable @id: a crashed-mid-init or empty
                // directory with no mod to check, reclaimed when its editor is
                // closed (the rename probe still protects an open one).
                None => false,
            };

            if keep {
                report.kept.push(index);
            } else if self.reclaim(index, &path) {
                report.reclaimed.push(index);
            } else {
                report.in_use.push(index);
            }
        }
        Ok(report)
    }

    /// Acquire the mutating lock, recovering from a poisoned mutex (a panic in a
    /// prior holder) the same way the event pump does: the guarded state is only the
    /// serialization, so a recovered lock stays sound.
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.mutating
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The existing `EditorWorkspaces/N` directories, as `(index, path)` sorted by
    /// index. Keys on the numeric name, so the shared `.clang-format.windhawk` and any
    /// `N.tmp` inside the container are excluded; a missing container (a fresh install
    /// with no workspace ever allocated) yields no workspaces rather than an error.
    /// Enumerating the container, not the `appData` root, means the legacy bare
    /// `EditorWorkspace`, `UIData`, and the UI's own data folders are never even seen.
    fn enumerate_workspaces(&self) -> io::Result<Vec<(u32, PathBuf)>> {
        let mut workspaces = Vec::new();
        for entry in self.read_container()? {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(index) = workspace_index(name) {
                workspaces.push((index, entry.path()));
            }
        }
        workspaces.sort_by_key(|(index, _)| *index);
        Ok(workspaces)
    }

    /// Delete any `EditorWorkspaces/N.tmp` left by an interrupted sweep. Best-effort:
    /// a failure leaves the folder for the next sweep to retry.
    fn delete_leftover_tmp_dirs(&self) -> io::Result<()> {
        for entry in self.read_container()? {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if tmp_workspace_index(name).is_some() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
        Ok(())
    }

    /// The container's directory entries, treating a missing container as empty so
    /// locate/sweep on a fresh install (before any allocation created it) is a no-op
    /// rather than a `NotFound` error.
    fn read_container(&self) -> io::Result<Vec<fs::DirEntry>> {
        match fs::read_dir(&self.dir) {
            Ok(entries) => entries.collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error),
        }
    }

    /// Probe `EditorWorkspaces/N` for an open editor and reclaim it if none. Returns
    /// `true` when the folder was renamed and deleted (no editor), `false` when the
    /// rename failed and the folder was left in place (still in use). Rename-first is
    /// atomic and non-destructive: it detects the open editor without partially
    /// deleting a live workspace.
    fn reclaim(&self, index: u32, path: &Path) -> bool {
        let tmp = self.dir.join(format!("{index}{TMP_SUFFIX}"));
        // A rename that fails means an editor still holds the directory (clangd's
        // working directory, the file watcher, an open document) - or, after a
        // racing tmp cleanup, that the target reappeared; either way, leave the
        // workspace. A rename that succeeds means no editor is open.
        if fs::rename(path, &tmp).is_err() {
            return false;
        }
        let _ = fs::remove_dir_all(&tmp);
        true
    }

    /// Seed a freshly allocated (empty) workspace into a VSCodium-ready state,
    /// mirroring the extension's `initializeFromModSource` + `initializeEditorSettings`
    /// plus the `.vscode/settings.json` editor-mode seed.
    fn initialize(&self, workspace: &Path, init: &WorkspaceInit) -> io::Result<()> {
        fs::write(workspace.join(MOD_SOURCE_FILE), init.mod_source)?;
        remove_stale_api_header(workspace);
        fs::write(
            workspace.join(COMPILE_FLAGS_FILE),
            compile_flags_contents(init.compile_flags),
        )?;
        self.write_clang_format(workspace)?;
        git_init_and_stage(workspace);
        seed_editor_mode_settings(workspace, init.mod_id)?;
        Ok(())
    }

    /// Provide the workspace's `.clang-format`: copy the shared
    /// `EditorWorkspaces/.clang-format.windhawk` override when the user has
    /// authored one in the container (the parent of every workspace), otherwise
    /// write the default (Chromium-based) content directly. Like the extension,
    /// the shared override is never created here - it exists only when the user
    /// makes it, so its presence means "formatting was customized". This is the
    /// shared-parent analogue of the extension's per-workspace copy-if-present
    /// / else-write-default.
    fn write_clang_format(&self, workspace: &Path) -> io::Result<()> {
        let shared = self.dir.join(SHARED_CLANG_FORMAT_FILE);
        let dest = workspace.join(CLANG_FORMAT_FILE);
        if shared.exists() {
            fs::copy(&shared, dest)?;
        } else {
            fs::write(dest, default_clang_format())?;
        }
        Ok(())
    }
}

/// The index for a container entry named `N` (a workspace), or `None` when the name
/// is not a non-empty run of ASCII digits. Excludes the shared `.clang-format.windhawk`
/// and any `N.tmp` sweep leftover inside the container.
fn workspace_index(name: &str) -> Option<u32> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    name.parse().ok()
}

/// The index for a container entry named `N.tmp` (a sweep-probe leftover), or `None`.
fn tmp_workspace_index(name: &str) -> Option<u32> {
    let inner = name.strip_suffix(TMP_SUFFIX)?;
    if inner.is_empty() || !inner.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    inner.parse().ok()
}

/// Read a workspace's mod id: the `editedModId` marker from
/// `.vscode/settings.json` (a pure JSON parse, the primary and cheapest
/// identity), falling back to parsing the `mod.wh.cpp` `@id` through the
/// injected seam when the marker is absent (the one case the marker does not
/// cover, the extension clearing it on exit). Returns `None` when neither
/// yields an id.
fn read_workspace_mod_id(
    workspace: &Path,
    parse_id: &impl Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Some(id) = read_edited_mod_id(workspace) {
        return Some(id);
    }
    let source = fs::read_to_string(workspace.join(MOD_SOURCE_FILE)).ok()?;
    parse_id(&source)
}

/// The `editedModId` marker from a workspace's `.vscode/settings.json`, or `None`
/// if the file is missing/unparseable or the key is absent or not a string.
fn read_edited_mod_id(workspace: &Path) -> Option<String> {
    let text = fs::read_to_string(settings_path(workspace)).ok()?;
    let settings: Value = serde_json::from_str(&text).ok()?;
    settings.get(KEY_EDITED_MOD_ID)?.as_str().map(str::to_owned)
}

/// Remove a stale `windhawk_api.h` if present. Best-effort: a missing file (the
/// fresh-directory case) or a removal failure is ignored.
fn remove_stale_api_header(workspace: &Path) {
    let _ = fs::remove_file(workspace.join(STALE_API_HEADER));
}

/// Set the git baseline the extension's `initializeEditorSettings` does: `git init`
/// when there is no `.git`, then `git add mod.wh.cpp` to stage the clean baseline so
/// later unsaved edits show as a diff and the sidebar's "modified" indicator works.
///
/// Best-effort, matching the extension's `spawnSync(..., { stdio: 'ignore' })` with
/// no error check: a missing or failing `git` is swallowed (the workspace still
/// opens, just without a baseline), never surfaced as a launch failure. In Rust a
/// spawn errors when `git` is absent, so the error is explicitly ignored rather than
/// propagated. stdio is nulled so a git prompt can never block the launch.
///
/// `CREATE_NO_WINDOW` is required, not cosmetic: `git.exe` is a console program, and
/// the native UI is a GUI (windows-subsystem) process, so without this flag Windows
/// allocates a fresh console for each git child and a terminal window flashes on
/// every workspace init (the extension got this for free from Node's default
/// `windowsHide: true`). Nulling stdio does not suppress it - the window comes from
/// console allocation, which only the creation flag controls.
fn git_init_and_stage(workspace: &Path) {
    let run = |args: &[&str]| {
        let _ = Command::new("git")
            .args(args)
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    };

    if !workspace.join(".git").exists() {
        run(&["init"]);
    }
    if workspace.join(".git").exists() {
        run(&["add", MOD_SOURCE_FILE]);
    }
}

/// Write the `.vscode/settings.json` editor-mode seed by a read-merge-write:
/// set the seed keys and preserve every other key the file already carries.
///
/// `editedModId`, `git.enabled`, and the layout keys are set unconditionally, while
/// `editedModWasModified` is set to `false` only when absent - a re-seed preserves
/// an existing value so edit-reuse keeps the "modified" indicator rather than
/// resetting it. On a fresh directory this merges into an empty object (a plain
/// write). One merge serves both the initial seed and the edit-reuse re-seed.
fn seed_editor_mode_settings(workspace: &Path, mod_id: &str) -> io::Result<()> {
    let path = settings_path(workspace);

    let settings = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();

    let merged = editor_mode_settings(settings, mod_id);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, to_pretty_json(&merged))
}

/// The `.vscode/settings.json` path for a workspace.
fn settings_path(workspace: &Path) -> PathBuf {
    workspace.join(VSCODE_DIR).join(VSCODE_SETTINGS_FILE)
}

/// Apply the editor-mode seed keys to an existing settings object in place (the
/// pure merge policy, extracted so it is testable without touching the
/// filesystem). The `workbench.*` layout keys are intentionally redundant with
/// what `restoreEditorMode -> toggleMinimalLayout(false)` re-applies on
/// activation; they exist only so the pre-activation frame opens in editor-mode
/// chrome instead of flashing browse chrome, so a future reader should not
/// "simplify" the seed by dropping them.
fn editor_mode_settings(mut settings: Map<String, Value>, mod_id: &str) -> Value {
    settings.insert(
        KEY_EDITED_MOD_ID.to_owned(),
        Value::String(mod_id.to_owned()),
    );
    settings
        .entry(KEY_EDITED_MOD_WAS_MODIFIED)
        .or_insert(Value::Bool(false));
    settings.insert(KEY_GIT_ENABLED.to_owned(), Value::Bool(true));
    settings.insert(KEY_SHOW_TABS.to_owned(), Value::Bool(true));
    settings.insert(KEY_STATUS_BAR.to_owned(), Value::Bool(true));
    settings.insert(KEY_NO_WINDHAWK_EXIT_BUTTON.to_owned(), Value::Bool(true));
    Value::Object(settings)
}

/// The `compile_flags.txt` body: one flag per line, newline-joined with a trailing
/// newline (the extension's `compileFlags.join('\n') + '\n'`).
fn compile_flags_contents(flags: &[String]) -> String {
    let mut body = flags.join("\n");
    body.push('\n');
    body
}

/// The default `.clang-format` content (Chromium-based) written into a
/// workspace when no shared override exists. Its leading comment tells the user
/// how to override formatting for every workspace at once. This mirrors the
/// extension, which writes the same default to `.clang-format` with a "create a
/// .clang-format.windhawk" hint; the wording is adapted to the shared
/// parent-folder override the many-workspace model uses. The comment lives here
/// in the per-workspace `.clang-format`, not in the override file itself.
fn default_clang_format() -> String {
    [
        "# To change formatting for all Windhawk editor workspaces, create a",
        "# .clang-format.windhawk file in the parent folder with the desired settings.",
        "BasedOnStyle: Chromium",
        "IndentWidth: 4",
        r"CommentPragmas: ^[ \t]+@[a-zA-Z]+",
    ]
    .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use serde_json::json;
    use tempfile::TempDir;

    /// A `parse_id` fake standing in for the core's `parseModSource`: pulls the id
    /// out of a `// @id <id>` line. Real enough to exercise the fallback without a
    /// DLL.
    fn fake_parse_id(source: &str) -> Option<String> {
        source.lines().find_map(|line| {
            line.trim_start()
                .strip_prefix("// @id")
                .map(|rest| rest.trim().to_owned())
                .filter(|id| !id.is_empty())
        })
    }

    fn make_dir(app_data: &Path, name: &str) -> PathBuf {
        let path = app_data.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// The `EditorWorkspaces` container under an appData root.
    fn container(app_data: &Path) -> PathBuf {
        app_data.join(WORKSPACES_DIR)
    }

    /// Create the workspace `EditorWorkspaces/<index>` under an appData root.
    fn make_workspace(app_data: &Path, index: u32) -> PathBuf {
        make_dir(&container(app_data), &index.to_string())
    }

    fn write_marker(workspace: &Path, mod_id: &str) {
        let settings = json!({ KEY_EDITED_MOD_ID: mod_id });
        fs::create_dir_all(workspace.join(VSCODE_DIR)).unwrap();
        fs::write(settings_path(workspace), settings.to_string()).unwrap();
    }

    fn read_settings(workspace: &Path) -> Value {
        let text = fs::read_to_string(settings_path(workspace)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    // ---- content builders ------------------------------------------------

    #[test]
    fn compile_flags_join_one_per_line_with_trailing_newline() {
        let flags = vec!["-x".to_owned(), "c++".to_owned(), "-std=c++23".to_owned()];
        assert_eq!(compile_flags_contents(&flags), "-x\nc++\n-std=c++23\n");
        // An empty flag list is still a single trailing newline, not an empty file.
        assert_eq!(compile_flags_contents(&[]), "\n");
    }

    #[test]
    fn clang_format_default_points_at_the_shared_parent_file() {
        let content = default_clang_format();
        assert!(content.contains(".clang-format.windhawk file in the parent folder"));
        assert!(content.contains("all Windhawk editor workspaces"));
        assert!(content.contains("BasedOnStyle: Chromium"));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn editor_mode_seed_sets_all_the_required_keys() {
        let merged = editor_mode_settings(Map::new(), "my-mod");
        assert_eq!(merged[KEY_EDITED_MOD_ID], json!("my-mod"));
        assert_eq!(merged[KEY_EDITED_MOD_WAS_MODIFIED], json!(false));
        assert_eq!(merged[KEY_GIT_ENABLED], json!(true));
        assert_eq!(merged[KEY_SHOW_TABS], json!(true));
        assert_eq!(merged[KEY_STATUS_BAR], json!(true));
        assert_eq!(merged[KEY_NO_WINDHAWK_EXIT_BUTTON], json!(true));
    }

    #[test]
    fn editor_mode_reseed_preserves_was_modified_and_foreign_keys() {
        // A re-seed of a workspace that already tracked unsaved edits and carried an
        // extension-added key must keep both, updating only the seed keys.
        let settings = match json!({
            KEY_EDITED_MOD_ID: "old-id",
            KEY_EDITED_MOD_WAS_MODIFIED: true,
            "editor.fontSize": 15,
        }) {
            Value::Object(map) => map,
            _ => unreachable!(),
        };
        let merged = editor_mode_settings(settings, "new-id");
        assert_eq!(merged[KEY_EDITED_MOD_ID], json!("new-id"));
        // Preserved, not reset to false.
        assert_eq!(merged[KEY_EDITED_MOD_WAS_MODIFIED], json!(true));
        // A foreign key survives the merge.
        assert_eq!(merged["editor.fontSize"], json!(15));
    }

    // ---- naming ----------------------------------------------------------

    #[test]
    fn workspace_index_matches_only_a_numeric_name() {
        assert_eq!(workspace_index("1"), Some(1));
        assert_eq!(workspace_index("42"), Some(42));
        // The sweep-probe leftover, the shared override, and any non-numeric name in
        // the container are excluded.
        assert_eq!(workspace_index("1.tmp"), None);
        assert_eq!(workspace_index(".clang-format.windhawk"), None);
        assert_eq!(workspace_index("EditorWorkspace"), None);
        assert_eq!(workspace_index(""), None);
        // The tmp matcher is the mirror image.
        assert_eq!(tmp_workspace_index("3.tmp"), Some(3));
        assert_eq!(tmp_workspace_index("3"), None);
        assert_eq!(tmp_workspace_index(".tmp"), None);
    }

    // ---- allocate --------------------------------------------------------

    fn init_for(id: &str) -> (String, Vec<String>) {
        (
            format!("// @id {id}\n// body\n"),
            vec!["-x".to_owned(), "c++".to_owned()],
        )
    }

    #[test]
    fn allocate_takes_the_lowest_free_index() {
        let temp = TempDir::new().unwrap();
        let manager = WorkspaceManager::new(temp.path());
        let (source, flags) = init_for("a");
        let init = WorkspaceInit {
            mod_source: &source,
            mod_id: "a",
            compile_flags: &flags,
        };

        let first = manager.allocate_and_initialize(&init).unwrap();
        assert_eq!(first.index(), 1);
        assert_eq!(first.path(), container(temp.path()).join("1"));

        let second = manager.allocate_and_initialize(&init).unwrap();
        assert_eq!(second.index(), 2);

        // Freeing index 1 (as a user deletion would) lets the next allocation reuse it.
        fs::remove_dir_all(container(temp.path()).join("1")).unwrap();
        let third = manager.allocate_and_initialize(&init).unwrap();
        assert_eq!(third.index(), 1);
    }

    #[test]
    fn allocate_ignores_a_leftover_tmp_and_the_legacy_workspace() {
        let temp = TempDir::new().unwrap();
        // The legacy whole-UI workspace sits at the appData root, a different parent
        // than the container, so it can never be mistaken for a per-mod index.
        make_dir(temp.path(), "EditorWorkspace");
        // A leftover tmp inside the container must not block allocating index 1.
        make_dir(&container(temp.path()), "1.tmp");
        let manager = WorkspaceManager::new(temp.path());
        let (source, flags) = init_for("a");
        let init = WorkspaceInit {
            mod_source: &source,
            mod_id: "a",
            compile_flags: &flags,
        };

        let workspace = manager.allocate_and_initialize(&init).unwrap();
        assert_eq!(workspace.index(), 1);
        assert_eq!(workspace.path(), container(temp.path()).join("1"));
    }

    // ---- initialize ------------------------------------------------------

    #[test]
    fn initialize_produces_a_vscodium_ready_directory() {
        let temp = TempDir::new().unwrap();
        let manager = WorkspaceManager::new(temp.path());
        let source = "// @id demo\n// contents\n";
        let flags = vec!["-x".to_owned(), "c++".to_owned(), "-std=c++23".to_owned()];
        let init = WorkspaceInit {
            mod_source: source,
            mod_id: "demo",
            compile_flags: &flags,
        };

        let workspace = manager.allocate_and_initialize(&init).unwrap();
        let dir = workspace.path();

        assert_eq!(
            fs::read_to_string(dir.join(MOD_SOURCE_FILE)).unwrap(),
            source
        );
        assert_eq!(
            fs::read_to_string(dir.join(COMPILE_FLAGS_FILE)).unwrap(),
            "-x\nc++\n-std=c++23\n"
        );

        // With no user override in the container, the workspace gets the default
        // .clang-format, and the shared override file is NOT created (like the
        // extension, so its presence still signals a user customization).
        let shared = container(temp.path()).join(SHARED_CLANG_FORMAT_FILE);
        assert!(!shared.exists());
        assert_eq!(
            fs::read_to_string(dir.join(CLANG_FORMAT_FILE)).unwrap(),
            default_clang_format()
        );

        // The editor-mode seed carries the identity marker for the mod.
        let settings = read_settings(dir);
        assert_eq!(settings[KEY_EDITED_MOD_ID], json!("demo"));
        assert_eq!(settings[KEY_GIT_ENABLED], json!(true));
    }

    #[test]
    fn initialize_reuses_a_preexisting_shared_clang_format() {
        let temp = TempDir::new().unwrap();
        let custom = "# custom shared override\nBasedOnStyle: LLVM\n";
        let shared_dir = container(temp.path());
        fs::create_dir_all(&shared_dir).unwrap();
        fs::write(shared_dir.join(SHARED_CLANG_FORMAT_FILE), custom).unwrap();
        let manager = WorkspaceManager::new(temp.path());
        let (source, flags) = init_for("a");
        let init = WorkspaceInit {
            mod_source: &source,
            mod_id: "a",
            compile_flags: &flags,
        };

        let workspace = manager.allocate_and_initialize(&init).unwrap();
        // The user's shared file is copied verbatim, not overwritten with the default.
        assert_eq!(
            fs::read_to_string(workspace.path().join(CLANG_FORMAT_FILE)).unwrap(),
            custom
        );
    }

    #[test]
    fn seed_written_to_disk_preserves_was_modified_on_reseed() {
        let temp = TempDir::new().unwrap();
        let workspace = make_workspace(temp.path(), 1);
        // Simulate the extension having marked the workspace modified.
        fs::create_dir_all(workspace.join(VSCODE_DIR)).unwrap();
        fs::write(
            settings_path(&workspace),
            json!({ KEY_EDITED_MOD_ID: "demo", KEY_EDITED_MOD_WAS_MODIFIED: true }).to_string(),
        )
        .unwrap();

        let manager = WorkspaceManager::new(temp.path());
        manager.reseed_editor_mode(&workspace, "demo").unwrap();

        let settings = read_settings(&workspace);
        assert_eq!(settings[KEY_EDITED_MOD_WAS_MODIFIED], json!(true));
    }

    // ---- locate ----------------------------------------------------------

    #[test]
    fn locate_finds_the_workspace_by_edited_mod_id_marker() {
        let temp = TempDir::new().unwrap();
        let ws1 = make_workspace(temp.path(), 1);
        write_marker(&ws1, "alpha");
        let ws2 = make_workspace(temp.path(), 2);
        write_marker(&ws2, "beta");

        let manager = WorkspaceManager::new(temp.path());
        let found = manager.locate("beta", fake_parse_id).unwrap().unwrap();
        assert_eq!(found.path(), ws2);
        assert_eq!(found.index(), 2);

        assert!(manager.locate("missing", fake_parse_id).unwrap().is_none());
    }

    #[test]
    fn locate_falls_back_to_parsed_id_when_marker_absent() {
        let temp = TempDir::new().unwrap();
        let workspace = make_workspace(temp.path(), 1);
        // No .vscode/settings.json marker; only the source @id identifies it (the
        // exit-cleared-key case the fallback covers).
        fs::write(workspace.join(MOD_SOURCE_FILE), "// @id gamma\n// body\n").unwrap();

        let manager = WorkspaceManager::new(temp.path());
        let found = manager.locate("gamma", fake_parse_id).unwrap().unwrap();
        assert_eq!(found.path(), workspace);
    }

    #[test]
    fn locate_never_matches_the_legacy_bare_workspace() {
        let temp = TempDir::new().unwrap();
        // The legacy workspace at the appData root holds a mod.wh.cpp with a matching
        // @id, but it sits outside the container, so the manager never enumerates it
        // (and here the container does not even exist yet).
        let legacy = make_dir(temp.path(), "EditorWorkspace");
        fs::write(legacy.join(MOD_SOURCE_FILE), "// @id legacy\n").unwrap();
        write_marker(&legacy, "legacy");

        let manager = WorkspaceManager::new(temp.path());
        assert!(manager.locate("legacy", fake_parse_id).unwrap().is_none());
    }

    // ---- sweep -----------------------------------------------------------

    #[test]
    fn sweep_keeps_workspaces_whose_mod_still_exists() {
        let temp = TempDir::new().unwrap();
        let kept = make_workspace(temp.path(), 1);
        write_marker(&kept, "live");
        let reclaimed = make_workspace(temp.path(), 2);
        write_marker(&reclaimed, "deleted");

        let manager = WorkspaceManager::new(temp.path());
        // Only "local@live" exists in storage; "local@deleted" was removed.
        let report = manager
            .sweep(|mod_id| mod_id == "local@live", fake_parse_id)
            .unwrap();

        assert_eq!(report.kept, vec![1]);
        assert_eq!(report.reclaimed, vec![2]);
        assert!(report.in_use.is_empty());
        assert!(kept.exists());
        assert!(!reclaimed.exists());
    }

    #[test]
    fn sweep_reclaims_an_unidentified_closed_workspace() {
        let temp = TempDir::new().unwrap();
        // No marker and no parseable @id: a crashed-mid-init directory.
        let orphan = make_workspace(temp.path(), 1);
        fs::write(orphan.join("stray.txt"), "junk").unwrap();

        let manager = WorkspaceManager::new(temp.path());
        let report = manager.sweep(|_| true, fake_parse_id).unwrap();

        assert_eq!(report.reclaimed, vec![1]);
        assert!(!orphan.exists());
    }

    #[test]
    fn sweep_spares_an_open_workspace_then_reclaims_it_once_closed() {
        let temp = TempDir::new().unwrap();
        let workspace = make_workspace(temp.path(), 1);
        write_marker(&workspace, "deleted");
        // An open file handle inside the directory stands in for an editor holding
        // it (clangd/file watcher/open document): on Windows the rename probe fails
        // while any inner file is open.
        fs::write(workspace.join(MOD_SOURCE_FILE), "// @id deleted\n").unwrap();
        let held = File::open(workspace.join(MOD_SOURCE_FILE)).unwrap();

        let manager = WorkspaceManager::new(temp.path());
        // The mod does not exist, so the sweep tries to reclaim - but the held handle
        // blocks the rename, so the workspace is spared.
        let report = manager.sweep(|_| false, fake_parse_id).unwrap();
        assert_eq!(report.in_use, vec![1]);
        assert!(report.reclaimed.is_empty());
        assert!(workspace.exists());
        // No half-renamed leftover.
        assert!(!container(temp.path()).join("1.tmp").exists());

        // Once the editor closes, a later sweep reclaims it and frees the index.
        drop(held);
        let report = manager.sweep(|_| false, fake_parse_id).unwrap();
        assert_eq!(report.reclaimed, vec![1]);
        assert!(!workspace.exists());
    }

    #[test]
    fn sweep_deletes_a_leftover_tmp_up_front() {
        let temp = TempDir::new().unwrap();
        // A tmp folder left by an interrupted sweep, inside the container.
        make_dir(&container(temp.path()), "5.tmp");
        let manager = WorkspaceManager::new(temp.path());

        manager.sweep(|_| true, fake_parse_id).unwrap();
        assert!(!container(temp.path()).join("5.tmp").exists());
    }

    #[test]
    fn sweep_spares_siblings_and_the_shared_override() {
        let temp = TempDir::new().unwrap();
        // At the appData root: the legacy whole-UI workspace and UIData, neither of
        // which the manager (rooted at the container) ever enumerates.
        let legacy = make_dir(temp.path(), "EditorWorkspace");
        write_marker(&legacy, "legacy");
        make_dir(temp.path(), "UIData");
        // Inside the container: a reclaimable workspace and the shared override, which
        // is a file with a non-numeric name so the sweep must skip it.
        let workspace = make_workspace(temp.path(), 1);
        write_marker(&workspace, "deleted");
        let shared = container(temp.path()).join(SHARED_CLANG_FORMAT_FILE);
        fs::write(&shared, "shared").unwrap();

        let manager = WorkspaceManager::new(temp.path());
        let report = manager.sweep(|_| false, fake_parse_id).unwrap();

        // Only the workspace was reclaimed.
        assert_eq!(report.reclaimed, vec![1]);
        assert!(!workspace.exists());
        // The legacy workspace, UIData, and the shared override all survive.
        assert!(legacy.exists());
        assert!(temp.path().join("UIData").exists());
        assert!(shared.exists());
    }
}
