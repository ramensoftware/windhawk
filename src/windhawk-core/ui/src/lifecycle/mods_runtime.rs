//! Seed the engine's mod runtime libraries at startup.
//!
//! The install tree ships `ModsRuntime\{32,64,arm64}\` with the libraries a compiled
//! mod loads (libc++, libunwind, and the mod shim). The engine loads them from
//! `Engine\Mods\<arch>\` under appData, so they are mirrored there. The installer
//! copies them at install time; this backstops that copy so an install missing the
//! files (or one whose engine folder was cleared) still gets them, letting the engine
//! apply shims to existing mods without waiting for the next compile.
//!
//! Only files absent from the destination are copied, so a library the engine has
//! already mapped is never disturbed. Best-effort: it runs off the window thread at
//! startup (like the editor workspace sweep) and any failure is logged, not fatal.

use std::fs;
use std::io;
use std::path::Path;

/// The install-tree folder holding the per-architecture runtime libraries
/// (`<appRoot>\ModsRuntime`), the copy source.
const MODS_RUNTIME_DIR: &str = "ModsRuntime";

/// Copy the mod runtime libraries from the install tree's `ModsRuntime` into the
/// engine's mods folder under appData (`Engine\Mods`), for every file not already
/// present at the destination. Best-effort; a failure is logged and swallowed.
pub(crate) fn copy_mods_runtime_libs(app_root: &Path, app_data: &Path) {
    let src = app_root.join(MODS_RUNTIME_DIR);
    let dest = app_data.join("Engine").join("Mods");
    if let Err(error) = copy_missing_recursive(&src, &dest) {
        eprintln!("windhawk-ui: seeding the mod runtime libraries failed: {error}");
    }
}

/// Recursively copy every file under `src` to the mirrored path under `dest`, creating
/// parent directories as needed and skipping any file that already exists at the
/// destination. A missing `src` (an install without a `ModsRuntime` folder) is a no-op
/// rather than an error.
fn copy_missing_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(src) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_missing_recursive(&src_path, &dest_path)?;
        } else if !dest_path.exists() {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// The engine mods folder (`Engine\Mods`) under an appData root.
    fn engine_mods(app_data: &Path) -> std::path::PathBuf {
        app_data.join("Engine").join("Mods")
    }

    #[test]
    fn copies_missing_files_preserving_the_arch_subfolders() {
        let temp = TempDir::new().unwrap();
        let app_root = temp.path().join("install");
        let app_data = temp.path().join("data");
        let runtime = app_root.join(MODS_RUNTIME_DIR);
        write(&runtime.join("64").join("libc++.whl"), "cpp");
        write(&runtime.join("32").join("windhawk-mod-shim.dll"), "shim");

        copy_mods_runtime_libs(&app_root, &app_data);

        let mods = engine_mods(&app_data);
        assert_eq!(
            fs::read_to_string(mods.join("64").join("libc++.whl")).unwrap(),
            "cpp"
        );
        assert_eq!(
            fs::read_to_string(mods.join("32").join("windhawk-mod-shim.dll")).unwrap(),
            "shim"
        );
    }

    #[test]
    fn leaves_an_existing_destination_file_untouched() {
        let temp = TempDir::new().unwrap();
        let app_root = temp.path().join("install");
        let app_data = temp.path().join("data");
        write(
            &app_root
                .join(MODS_RUNTIME_DIR)
                .join("64")
                .join("libc++.whl"),
            "new",
        );
        // A library the engine may already have mapped: it must survive untouched, so a
        // still-loaded copy is never churned.
        let dest = engine_mods(&app_data).join("64").join("libc++.whl");
        write(&dest, "existing");

        copy_mods_runtime_libs(&app_root, &app_data);

        assert_eq!(fs::read_to_string(&dest).unwrap(), "existing");
    }

    #[test]
    fn a_missing_mods_runtime_folder_is_a_noop() {
        let temp = TempDir::new().unwrap();
        let app_root = temp.path().join("install");
        let app_data = temp.path().join("data");
        fs::create_dir_all(&app_root).unwrap();

        // No ModsRuntime under app_root: nothing is copied and no error surfaces.
        copy_mods_runtime_libs(&app_root, &app_data);
        assert!(!engine_mods(&app_data).exists());
    }
}
