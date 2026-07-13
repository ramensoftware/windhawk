//! The single `windhawk.ini` access point both consumers reuse: the existence
//! check app-root discovery validates a candidate directory with, and the
//! `[Storage] Portable` read the user-agent suffix needs. Both touch
//! `windhawk.ini`, so one module owns the BOM/UTF-8 decode and neither consumer
//! re-implements it. App-root DISCOVERY stays in each consumer (the CLI's
//! explicit-path/`WINDHAWK_UI_PATH`/cwd order, the UI's exe-relative walk-up);
//! the host only VALIDATES a candidate directory here, it does not discover
//! one. The core remains the authority on storage resolution - this reads the
//! one flag for the header.

use std::path::Path;

/// True if `dir` contains a `windhawk.ini` - the existence check app-root
/// discovery validates a candidate directory with.
pub fn has_windhawk_ini(dir: &Path) -> bool {
    dir.join("windhawk.ini").exists()
}

/// Whether the install at `app_root` is portable, from `[Storage] Portable` in
/// windhawk.ini. Matches the core's `!!parseInt(Portable, 10)` (a nonzero
/// integer is portable); a read or parse miss is a benign non-portable (the
/// user-agent suffix is server-visible only).
pub fn is_portable(app_root: &str) -> bool {
    let Ok(bytes) = std::fs::read(Path::new(app_root).join("windhawk.ini")) else {
        return false;
    };
    portable_flag(&decode_ini(&bytes))
}

/// Decode windhawk.ini bytes: UTF-16LE when it carries the BOM (the on-disk form
/// a real install and the test fixtures use), otherwise UTF-8 (lossy).
fn decode_ini(bytes: &[u8]) -> String {
    if let [0xFF, 0xFE, rest @ ..] = bytes {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Find `[Storage] Portable` and return whether its value is a nonzero integer.
/// A simple section/key scan suffices for the one flag.
fn portable_flag(ini: &str) -> bool {
    let mut in_storage = false;
    for line in ini.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_storage = section.eq_ignore_ascii_case("Storage");
        } else if in_storage
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("Portable")
        {
            // parseInt(value, 10): the leading integer, nonzero is portable.
            return parse_leading_int(value.trim()) != 0;
        }
    }
    false
}

/// `parseInt(s, 10)` clamped to "0 on no leading integer": an optional sign
/// then ASCII digits, trailing junk ignored.
fn parse_leading_int(s: &str) -> i64 {
    let bytes = s.as_bytes();
    let start = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let end = bytes[start..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map_or(bytes.len(), |offset| start + offset);
    s[..end].parse::<i64>().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_flag_reads_the_storage_section() {
        assert!(portable_flag(
            "[Storage]\r\nPortable=1\r\nAppDataPath=x\r\n"
        ));
        assert!(!portable_flag("[Storage]\r\nPortable=0\r\n"));
        // Absent key, or the key outside [Storage], is non-portable.
        assert!(!portable_flag("[Storage]\r\nAppDataPath=x\r\n"));
        assert!(!portable_flag("[Other]\r\nPortable=1\r\n"));
        // Case-insensitive section and key; trailing junk after the integer.
        assert!(portable_flag("[storage]\r\nportable = 1 ; portable\r\n"));
    }

    #[test]
    fn decode_ini_handles_utf16le_bom_and_utf8() {
        let mut utf16 = vec![0xFFu8, 0xFE];
        for unit in "[Storage]\r\nPortable=1\r\n".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(portable_flag(&decode_ini(&utf16)));
        assert!(portable_flag(&decode_ini(b"[Storage]\nPortable=1\n")));
    }

    #[test]
    fn is_portable_reads_the_windhawk_ini() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy();
        // No windhawk.ini yet -> non-portable.
        assert!(!is_portable(&root));
        std::fs::write(
            dir.path().join("windhawk.ini"),
            "[Storage]\r\nPortable=1\r\n",
        )
        .unwrap();
        assert!(is_portable(&root));
    }

    #[test]
    fn has_windhawk_ini_checks_for_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_windhawk_ini(dir.path()));
        std::fs::write(dir.path().join("windhawk.ini"), "[Storage]\r\n").unwrap();
        assert!(has_windhawk_ini(dir.path()));
    }
}
