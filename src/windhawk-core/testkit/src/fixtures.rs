//! Helpers for the shared contract fixture corpus (core-internals.md
//! section 9.3).

use std::path::PathBuf;

/// Absolute path of the fixture corpus root.
pub fn fixtures_dir() -> PathBuf {
    // testkit/ sits directly under the workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("contract")
        .join("fixtures")
}

/// The command names of the corpus: one directory per command.
pub fn fixture_commands() -> std::io::Result<Vec<String>> {
    let mut commands = Vec::new();
    for entry in std::fs::read_dir(fixtures_dir())? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            commands.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    commands.sort();
    Ok(commands)
}

/// The fixture files (scenario captures) of one command, as
/// (file name, parsed JSON) pairs.
pub fn fixture_files(command: &str) -> std::io::Result<Vec<(String, serde_json::Value)>> {
    let mut files = Vec::new();
    let mut entries: Vec<_> =
        std::fs::read_dir(fixtures_dir().join(command))?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path())?;
        let value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        files.push((name, value));
    }
    Ok(files)
}
