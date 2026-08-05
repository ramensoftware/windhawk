//! Where the main window's own data goes: the WebView2 browser profile and the
//! window-state file.
//!
//! It is the one folder the window cannot be built without, and what decides
//! where it sits is that a browser profile belongs to a PERSON - their cookies,
//! their local storage, their cache - and is not something two processes can
//! hold open at once. A system install's app data is one tree for the whole
//! machine, so the folder goes under this user's own `%LOCALAPPDATA%` instead.
//! Nothing has to be arranged for it there: the folder is theirs, so the
//! unelevated window creates it for itself, and no other account can reach in.
//!
//! A portable copy keeps its data inside the install directory, which is what
//! makes it portable - it travels with the copy, where a folder in one machine's
//! user profile would be left behind - and writes it as whoever can run the copy
//! at all.

use std::path::{Path, PathBuf};

/// The environment variable naming this user's local application data directory.
const LOCAL_APP_DATA_VAR: &str = "LOCALAPPDATA";

/// Windhawk's folder under `%LOCALAPPDATA%`. A system install's shared app data
/// is `%ProgramData%\Windhawk`, and this is its per-user counterpart, so it
/// carries the same name.
const WINDHAWK_SUBDIR: &str = "Windhawk";

/// The folder inside it, and inside a portable copy's own app data. It is named
/// apart from `UIData`, the VSCodium portable-data folder the editor launcher
/// owns beside it in a portable install's tree.
const UI_DATA_SUBDIR: &str = "UIMainData";

/// The folder this install writes the window's data in. `app_data` is the
/// resolved Windhawk app-data directory (`getCoreInfo` `fsPaths.appDataPath`).
///
/// `None` when a system install has no `%LOCALAPPDATA%` to resolve, which is the
/// one input this cannot make up: falling back to the install tree would put the
/// profile in a folder the whole machine shares, which is the arrangement this
/// exists to avoid.
pub fn ui_data_dir(app_data: &Path, portable: bool) -> Option<PathBuf> {
    if portable {
        return Some(app_data.join(UI_DATA_SUBDIR));
    }
    let local_app_data = std::env::var_os(LOCAL_APP_DATA_VAR)?;
    Some(
        Path::new(&local_app_data)
            .join(WINDHAWK_SUBDIR)
            .join(UI_DATA_SUBDIR),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // A portable copy writes beside its own install directory, and its data is
    // meant to travel with it: nothing in the path may be true of one machine
    // only.
    #[test]
    fn a_portable_copy_writes_inside_the_install() {
        let app_data = Path::new(r"D:\Windhawk\AppData");

        assert_eq!(
            ui_data_dir(app_data, true),
            Some(app_data.join("UIMainData"))
        );
    }

    // A system install shares its app data with every user on the machine, and a
    // browser profile is one person's, so the window's data goes in this user's
    // own profile rather than in there.
    #[test]
    fn a_system_install_writes_under_the_user_profile() {
        let local_app_data =
            std::env::var_os(LOCAL_APP_DATA_VAR).expect("a user session has %LOCALAPPDATA%");

        let dir =
            ui_data_dir(Path::new(r"C:\ProgramData\Windhawk"), false).expect("the folder resolves");

        assert_eq!(
            dir,
            Path::new(&local_app_data)
                .join("Windhawk")
                .join("UIMainData")
        );
    }
}
