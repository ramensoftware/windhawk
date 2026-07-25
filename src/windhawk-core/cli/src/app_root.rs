//! App-root DISCOVERY. Pure precedence logic, unit-tested in isolation: the
//! inputs (explicit `--app-root`, `WINDHAWK_UI_PATH`, the CLI exe's directory)
//! are passed in rather than read from the process, so the rules are testable
//! without touching the environment. Discovery is host policy; the core only
//! VALIDATES the resolved root (`APP_ROOT_INVALID`).

use std::path::Path;

use windhawk_core_host::windhawk_ini::has_windhawk_ini;

use crate::error::CliError;

/// Resolve the app root from the discovery inputs, in precedence order:
///   1. explicit `--app-root` (must contain `windhawk.ini`)
///   2. `WINDHAWK_UI_PATH`: try `dirname(dirname(x))`, then `x` itself
///   3. the directory holding the CLI exe if it contains `windhawk.ini`
///
/// Returns `ENV_INVALID` (exit 3) when the root cannot be located, or when an
/// explicit `--app-root` does not contain `windhawk.ini`.
pub fn resolve_app_root(
    explicit: Option<&str>,
    ui_path: Option<&str>,
    exe_dir: Option<&Path>,
) -> Result<String, CliError> {
    if let Some(explicit) = explicit {
        if !has_windhawk_ini(Path::new(explicit)) {
            return Err(CliError::env_invalid(format!(
                "--app-root path does not contain windhawk.ini: {explicit}"
            )));
        }
        return Ok(explicit.to_owned());
    }

    if let Some(ui_path) = ui_path {
        // If WINDHAWK_UI_PATH looks like a UI subdirectory (parent-of-parent is
        // the app root), use that.
        if let Some(derived) = Path::new(ui_path).parent().and_then(Path::parent)
            && has_windhawk_ini(derived)
        {
            return Ok(derived.to_string_lossy().into_owned());
        }
        // Otherwise treat WINDHAWK_UI_PATH itself as the app root.
        if has_windhawk_ini(Path::new(ui_path)) {
            return Ok(ui_path.to_owned());
        }
    }

    // The CLI exe ships in the installation directory next to windhawk.ini.
    if let Some(exe_dir) = exe_dir
        && has_windhawk_ini(exe_dir)
    {
        return Ok(exe_dir.to_string_lossy().into_owned());
    }

    Err(CliError::env_invalid(
        "Could not locate Windhawk app root. Pass --app-root <path>, set \
         WINDHAWK_UI_PATH, or run the windhawk-cli.exe located in the Windhawk \
         installation directory.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn make_ini(dir: &Path) {
        fs::write(dir.join("windhawk.ini"), "[Storage]\r\n").unwrap();
    }

    #[test]
    fn explicit_app_root_must_contain_ini() {
        let dir = tempfile::tempdir().unwrap();
        make_ini(dir.path());
        let p = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            resolve_app_root(Some(&p), None, Some(Path::new("C:\\nope"))).unwrap(),
            p
        );
    }

    #[test]
    fn explicit_app_root_without_ini_is_env_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_string_lossy().into_owned();
        let err = resolve_app_root(Some(&p), None, Some(dir.path())).unwrap_err();
        assert_eq!(err.code(), "ENV_INVALID");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn ui_path_parent_of_parent_wins() {
        // <root>/ui-sub/leaf -> dirname(dirname(leaf)) == <root>.
        let root = tempfile::tempdir().unwrap();
        make_ini(root.path());
        let leaf: PathBuf = root.path().join("ui-sub").join("leaf");
        fs::create_dir_all(&leaf).unwrap();
        let resolved = resolve_app_root(
            None,
            Some(&leaf.to_string_lossy()),
            Some(Path::new("C:\\nope")),
        )
        .unwrap();
        assert_eq!(Path::new(&resolved), root.path());
    }

    #[test]
    fn ui_path_itself_used_when_parent_has_no_ini() {
        let ui = tempfile::tempdir().unwrap();
        make_ini(ui.path());
        let p = ui.path().to_string_lossy().into_owned();
        let resolved = resolve_app_root(None, Some(&p), Some(Path::new("C:\\nope"))).unwrap();
        assert_eq!(resolved, p);
    }

    #[test]
    fn falls_back_to_exe_dir_with_ini() {
        let exe_dir = tempfile::tempdir().unwrap();
        make_ini(exe_dir.path());
        let resolved = resolve_app_root(None, None, Some(exe_dir.path())).unwrap();
        assert_eq!(Path::new(&resolved), exe_dir.path());
    }

    #[test]
    fn no_root_anywhere_is_env_invalid() {
        let empty = tempfile::tempdir().unwrap();
        // Neither the exe dir (empty, no ini) nor a missing exe dir resolves.
        assert_eq!(
            resolve_app_root(None, None, Some(empty.path()))
                .unwrap_err()
                .exit_code(),
            3
        );
        assert_eq!(
            resolve_app_root(None, None, None).unwrap_err().exit_code(),
            3
        );
    }
}
