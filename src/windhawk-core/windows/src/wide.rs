//! UTF-16 conversion helpers for the Win32 adapters. The ABI is UTF-8
//! everywhere; the adapters convert to UTF-16 at the Win32 edge and back.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// A NUL-terminated UTF-16 buffer for passing `&str` to a `PCWSTR` parameter.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A NUL-terminated UTF-16 buffer for passing a `&Path` to a `PCWSTR`
/// parameter, LOSSLESS across non-UTF-8 path components: it encodes the `OsStr`
/// directly instead of round-tripping through `to_string_lossy`, which would
/// mangle a component that is not UTF-8-representable before the FS call.
///
/// The trailing NUL is MANDATORY and load-bearing: unlike `str::encode_utf16`
/// (which `to_wide` above pairs with `once(0)`), `OsStrExt::encode_wide` does
/// NOT self-terminate, and every consumer passes the buffer as a NUL-terminated
/// `PCWSTR` (the `MoveFileExW`/`CreateFileW`/`GetPrivateProfileStringW` SAFETY
/// comments assert exactly that), so omitting it would hand Win32 an
/// unterminated pointer.
pub fn path_to_wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Decode a UTF-16 slice up to (not including) the first NUL, lossily.
pub fn from_wide_nul(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Decode an exact-length UTF-16 slice (no NUL handling), lossily.
pub fn from_wide(buf: &[u16]) -> String {
    String::from_utf16_lossy(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn path_to_wide_appends_the_mandatory_trailing_nul() {
        assert_eq!(path_to_wide(Path::new("x")).last(), Some(&0));
    }

    #[test]
    fn path_to_wide_round_trips_a_non_utf8_path() {
        // 0xD800 is an unpaired high surrogate - not representable in UTF-8, so
        // a to_string_lossy hop would mangle it to U+FFFD. encode_wide keeps it
        // byte-for-byte, which is the latent FS-boundary bug path_to_wide fixes.
        let units = [0x0078u16, 0xD800, 0x0079];
        let os = OsString::from_wide(&units);
        assert_eq!(
            path_to_wide(Path::new(&os)),
            vec![0x0078, 0xD800, 0x0079, 0]
        );
    }
}
