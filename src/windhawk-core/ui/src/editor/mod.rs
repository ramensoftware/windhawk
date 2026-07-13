//! The launch-into-VSCode subsystem. The native UI's development responsibility
//! is workspace preparation plus a VSCodium spawn; this subsystem holds the OS
//! touchpoints that job needs (file/dir I/O and, in later leaves, the git and
//! VSCodium process spawns), confined here rather than spread through the
//! command handlers.
//!
//! [`workspace`] is the multi-workspace manager: it allocates a per-mod
//! `EditorWorkspaceN` directory, locates the one already editing a mod,
//! garbage-collects abandoned ones, and initializes a directory into a
//! VSCodium-ready state. [`launch`] is the VSCodium launcher: it prepares the
//! shared VSCodium settings, builds the process environment, locates the editor
//! exe, and spawns it on a prepared workspace. [`template`] is the vendored
//! new-mod source. [`Editor`] bundles the manager and the launch seam into the
//! one long-lived value the `commands/dev/` handlers reach through the bridge
//! context.

pub mod launch;
pub mod template;
pub mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use launch::{LaunchEditor, Launcher};
use workspace::WorkspaceManager;

/// The launch-into-VSCode environment the development handlers share: the
/// multi-workspace [`WorkspaceManager`] and the VSCodium launch seam,
/// constructed once from `getCoreInfo` and held behind the bridge context. It
/// is a single process-wide value, not a per-call one, because the manager's
/// process-local mutating lock only serializes allocate/sweep when every
/// handler and the startup sweep share the same manager instance.
pub struct Editor {
    workspaces: WorkspaceManager,
    launcher: Arc<dyn LaunchEditor>,
}

impl Editor {
    /// The production editor: a real [`Launcher`] over the `getCoreInfo` paths, and a
    /// [`WorkspaceManager`] rooted at the same `appData` (whose `EditorWorkspaces`
    /// container sits beside the launcher's `UIData`).
    pub fn new(
        app_data: impl Into<PathBuf>,
        ui_path: impl Into<PathBuf>,
        compiler_path: impl Into<PathBuf>,
        arm64_enabled: bool,
    ) -> Editor {
        let app_data = app_data.into();
        let launcher = Launcher::new(app_data.clone(), ui_path, compiler_path, arm64_enabled);
        Editor {
            workspaces: WorkspaceManager::new(app_data),
            launcher: Arc::new(launcher),
        }
    }

    /// An editor over an injected launch seam, for the handler orchestration
    /// tests: a real workspace manager against a temp `appData`, with the
    /// VSCodium spawn replaced by a recording fake so a test asserts the launch
    /// inputs without launching.
    pub fn with_launcher(app_data: impl Into<PathBuf>, launcher: Arc<dyn LaunchEditor>) -> Editor {
        Editor {
            workspaces: WorkspaceManager::new(app_data),
            launcher,
        }
    }

    /// The multi-workspace manager (allocate / locate / sweep / initialize).
    pub(crate) fn workspaces(&self) -> &WorkspaceManager {
        &self.workspaces
    }

    /// The VSCodium launch seam.
    pub(crate) fn launcher(&self) -> &dyn LaunchEditor {
        self.launcher.as_ref()
    }
}

/// Serialize a JSON value with 4-space indentation and a trailing newline, the JSON
/// house style shared across the editor subsystem's written files (the workspace
/// `.vscode/settings.json` seed and the shared VSCodium `settings.json`), matching the
/// C++ `PrepareUISettings` `dump(4)`.
pub(crate) fn to_pretty_json(value: &Value) -> String {
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    value
        .serialize(&mut serializer)
        .expect("serializing a serde_json::Value never fails");
    buffer.push(b'\n');
    String::from_utf8(buffer).expect("serde_json emits valid UTF-8")
}
