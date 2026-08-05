//! The portable-mode `SettingsBackend`: an INI file accessed through the Win32
//! profile APIs (`GetPrivateProfileStringW` / `WritePrivateProfileStringW`),
//! exactly as `IniFileSettings` in `shared/portable_settings.cpp`. Calling the
//! same APIs (rather than reimplementing an INI parser) makes encoding,
//! quoting, and the internal byte-range locking match the C++ side by
//! construction. Files are created with a UTF-16LE BOM so the APIs operate in
//! Unicode.

use std::borrow::Cow;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND,
    ERROR_SUCCESS, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, WriteFile,
};
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileStringW, WritePrivateProfileStringW,
};

use windhawk_core_ports::{SettingsBackend, SettingsError, SettingsTree, TreeLocation, TreeValue};

use crate::os;
use crate::wide::{path_to_wide, to_wide};

fn require_ini(tree: &TreeLocation) -> Result<(&Path, &str), SettingsError> {
    match tree {
        TreeLocation::Ini { file, section } => Ok((file.as_path(), section.as_str())),
        TreeLocation::Registry { .. } => Err(SettingsError::ini(
            "open",
            "registry location given to the INI backend",
            0,
            "INI backend received a registry tree location",
        )),
    }
}

#[derive(Default)]
pub struct IniBackend;

impl IniBackend {
    pub fn new() -> Self {
        Self
    }
}

impl SettingsBackend for IniBackend {
    fn open(
        &self,
        tree: &TreeLocation,
        write: bool,
    ) -> Result<Box<dyn SettingsTree>, SettingsError> {
        let (file, section) = require_ini(tree)?;
        if write {
            ensure_file_with_bom(file)?;
        }
        Ok(Box::new(IniTree {
            file: file.to_path_buf(),
            section: section.to_owned(),
        }))
    }

    fn remove_tree(&self, tree: &TreeLocation) -> Result<(), SettingsError> {
        let (file, section) = require_ini(tree)?;
        // WritePrivateProfileString(section, NULL, NULL, file) removes the
        // whole section. An absent file (or its parent directory) is a no-op,
        // per the port contract - the section is already gone.
        match write_profile(file, section, None, None) {
            Err(e)
                if e.os.os_error == NonZeroU32::new(ERROR_FILE_NOT_FOUND)
                    || e.os.os_error == NonZeroU32::new(ERROR_PATH_NOT_FOUND) =>
            {
                Ok(())
            }
            other => other,
        }
    }

    fn rename_tree(&self, from: &TreeLocation, to: &TreeLocation) -> Result<(), SettingsError> {
        let (from_file, _) = require_ini(from)?;
        let (to_file, _) = require_ini(to)?;
        // The `[Mod]` and `[Settings]` sections share one `<modId>.ini` file, so
        // renaming the file moves both (the TS `renameConfig` renames the whole
        // file). MoveFileExW(MOVEFILE_REPLACE_EXISTING), like the atomic write
        // path; an absent source is a no-op (the TS ignores ENOENT).
        match std::fs::rename(from_file, to_file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(SettingsError::ini(
                "rename_tree",
                from_file.display().to_string(),
                e.raw_os_error().unwrap_or(0) as u32,
                e.to_string(),
            )),
        }
    }

    fn list_subtrees(&self, _parent: &TreeLocation) -> Result<Vec<String>, SettingsError> {
        // INI mode has no nested trees - each mod is a separate file - so the
        // portable mod listing enumerates `.ini` files through the `Files`
        // port instead. This is never called in INI mode.
        Ok(Vec::new())
    }
}

struct IniTree {
    file: PathBuf,
    section: String,
}

impl IniTree {
    fn err(&self, op: &'static str, os: u32, what: &str) -> SettingsError {
        SettingsError::ini(op, self.file.display().to_string(), os, what.to_owned())
    }
}

impl SettingsTree for IniTree {
    fn get_string(&self, name: &str) -> Result<Option<String>, SettingsError> {
        get_profile_string(&self.file, &self.section, name)
            .map_err(|os| self.err("get", os, "GetPrivateProfileString"))
    }

    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        if let Some(why) = unrepresentable_name(name) {
            return Err(self.err("set", 0, &format!("value name {name:?} {why}")));
        }
        // `WritePrivateProfileStringW` takes the value as a NUL-terminated
        // string, so an embedded NUL ends it: everything after it is dropped and
        // the call still reports success. `escape_ini_value` cannot rescue that
        // (the INI line format has no encoding for a NUL), so refuse the write
        // rather than store a silently truncated value.
        if value.contains('\0') {
            return Err(self.err("set", 0, "value contains a NUL character"));
        }
        // An INI entry ends at the line break, so a value carrying one cannot
        // be stored either: it would read back cut at the break, with the rest
        // of it parsed as further lines of a file that also holds the mod's
        // `[Mod]` config. Refused for the same reason as a NUL - the registry
        // backend stores both halves of such a value faithfully, and a write
        // that cannot keep the value is better refused than reported as done.
        if value.contains(['\r', '\n']) {
            return Err(self.err("set", 0, "value contains a line break"));
        }
        let escaped = escape_ini_value(value);
        write_profile(&self.file, &self.section, Some(name), Some(&escaped))
    }

    fn get_int(&self, name: &str) -> Result<Option<i32>, SettingsError> {
        Ok(self.get_string(name)?.map(|s| parse_c_int(&s)))
    }

    fn set_int(&mut self, name: &str, value: i32) -> Result<(), SettingsError> {
        // SetInt -> SetString(to_wstring(value)); a decimal triggers no
        // escaping, but route through set_string for exactness.
        self.set_string(name, &value.to_string())
    }

    fn get_binary(&self, name: &str) -> Result<Option<Vec<u8>>, SettingsError> {
        match self.get_string(name)? {
            None => Ok(None),
            Some(s) => decode_hex(&s)
                .map(Some)
                .ok_or_else(|| self.err("get_binary", 0, "odd-length or non-hex value")),
        }
    }

    fn set_binary(&mut self, name: &str, value: &[u8]) -> Result<(), SettingsError> {
        self.set_string(name, &encode_hex(value))
    }

    fn remove(&mut self, name: &str) -> Result<(), SettingsError> {
        // WritePrivateProfileString(section, name, NULL, file) removes the
        // value.
        write_profile(&self.file, &self.section, Some(name), None)
    }

    fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError> {
        let names = enum_profile_names(&self.file, &self.section)
            .map_err(|os| self.err("enum", os, "GetPrivateProfileString"))?;
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            // INI values carry no type; every value is a string (the TS
            // portable getModSettings returns raw strings).
            if let Some(value) = self.get_string(&name)? {
                out.push((name, TreeValue::Str(value)));
            }
        }
        Ok(out)
    }
}

/// Create the file (and its parent directory) with a UTF-16LE BOM if it does
/// not already exist, mirroring the C++ `CREATE_NEW` + BOM write.
fn ensure_file_with_bom(file: &Path) -> Result<(), SettingsError> {
    if let Some(parent) = file.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(SettingsError::ini(
            "create_dir",
            parent.display().to_string(),
            e.raw_os_error().unwrap_or(0) as u32,
            e.to_string(),
        ));
    }
    let wide = path_to_wide(file);
    // SAFETY: wide is NUL-terminated; the out-of-band args are documented
    // null/zero per CreateFileW's contract.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ,
            std::ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let os = os::last_error();
        // An existing file is the benign outcome and a no-op: it keeps the BOM
        // it was created with. CREATE_NEW reports the name collision ahead of
        // any sharing or access check, so a file that is held open, read-only,
        // or write-denied still lands here. Any other code means the file is
        // absent and could not be created, so the BOM was not written and
        // nothing can be stored.
        if os != ERROR_FILE_EXISTS {
            return Err(SettingsError::ini(
                "create",
                file.display().to_string(),
                os,
                "CreateFile",
            ));
        }
        return Ok(());
    }
    let bom: [u8; 2] = [0xFF, 0xFE];
    let mut written: u32 = 0;
    // SAFETY: handle is valid; bom is a 2-byte buffer; written is a valid out
    // param.
    unsafe {
        WriteFile(handle, bom.as_ptr(), 2, &mut written, std::ptr::null_mut());
        CloseHandle(handle);
    }
    Ok(())
}

/// `WritePrivateProfileString(section, name?, value?, file)` plus a cache
/// flush so the bytes are on disk for external readers and the fixture tests.
fn write_profile(
    file: &Path,
    section: &str,
    name: Option<&str>,
    value: Option<&str>,
) -> Result<(), SettingsError> {
    let file_w = path_to_wide(file);
    let section_w = to_wide(section);
    let name_w = name.map(to_wide);
    let value_w = value.map(to_wide);
    let name_ptr = name_w.as_ref().map_or(std::ptr::null(), |w| w.as_ptr());
    let value_ptr = value_w.as_ref().map_or(std::ptr::null(), |w| w.as_ptr());

    // This is the THIRD SetLastError(0) reset site, but it wraps a DIFFERENT
    // API (`WritePrivateProfileStringW`, reading GetLastError only on failure),
    // so it uses the os:: reset/read helpers directly rather than the
    // GetPrivateProfileStringW read bracket.
    os::clear_last_error();
    // SAFETY: all wide buffers are NUL-terminated; null name/value request
    // value/section removal per the API contract.
    let ok = unsafe {
        WritePrivateProfileStringW(section_w.as_ptr(), name_ptr, value_ptr, file_w.as_ptr())
    };
    if ok == 0 {
        let os = os::last_error();
        return Err(SettingsError::ini(
            "set",
            file.display().to_string(),
            os,
            "WritePrivateProfileString",
        ));
    }
    // Flush the profile cache to disk (WritePrivateProfileString(NULL, NULL,
    // NULL, file)).
    // SAFETY: file_w is NUL-terminated; null section/name/value flush.
    unsafe {
        WritePrivateProfileStringW(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            file_w.as_ptr(),
        );
    }
    Ok(())
}

/// The shared clear-last-error -> `GetPrivateProfileStringW` -> read-last-error
/// bracket of the two profile-string readers. The reset is needed because the
/// API can succeed with an empty result, so "no error" is indistinguishable
/// from a stale prior error without clearing first. Returns the chars written
/// (excluding the NUL) and the post-call last-error; the buffer
/// sizing/termination policy stays with each caller (they genuinely differ).
/// `key` is the value name, or `None` to request the section's name list.
fn get_private_profile_string(
    section: &str,
    key: Option<&str>,
    buf: &mut [u16],
    file: &Path,
) -> (u32, u32) {
    let section_w = to_wide(section);
    let key_w = key.map(to_wide);
    let file_w = path_to_wide(file);
    let key_ptr = key_w.as_ref().map_or(std::ptr::null(), |k| k.as_ptr());
    os::clear_last_error();
    // SAFETY: to_wide / path_to_wide produce NUL-terminated buffers; key_ptr is
    // null or a NUL-terminated name living in key_w; buf has buf.len() writable
    // units; null default per the C++.
    let returned = unsafe {
        GetPrivateProfileStringW(
            section_w.as_ptr(),
            key_ptr,
            std::ptr::null(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            file_w.as_ptr(),
        )
    };
    (returned, os::last_error())
}

/// `IniFileSettings::GetString`: returns `None` for an absent value (the API
/// signals ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND) and `Some` for a
/// present value, including the empty string.
fn get_profile_string(file: &Path, section: &str, name: &str) -> Result<Option<String>, u32> {
    let mut size: usize = 256;
    loop {
        let mut buf = vec![0u16; size];
        let (returned, err) = get_private_profile_string(section, Some(name), &mut buf, file);
        if err == ERROR_MORE_DATA {
            // Double rather than step: each retry re-reads and re-parses the
            // whole file, so a linear step makes reading one large value cost
            // a pass per step.
            size *= 2;
            continue;
        }
        if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        if err != ERROR_SUCCESS {
            return Err(err);
        }
        buf.truncate(returned as usize);
        return Ok(Some(String::from_utf16_lossy(&buf)));
    }
}

/// Enumerate the value names of a section, via
/// `GetPrivateProfileString(section, NULL, NULL, ...)` (a double-NUL-terminated
/// list), in file order.
fn enum_profile_names(file: &Path, section: &str) -> Result<Vec<String>, u32> {
    let mut size: usize = 1024;
    loop {
        let mut buf = vec![0u16; size];
        let (returned, err) = get_private_profile_string(section, None, &mut buf, file);
        // A too-small section-enumeration buffer is signaled by the API
        // returning nSize-2 (the room left by the double-NUL terminator), NOT
        // always by ERROR_MORE_DATA - so grow on either and retry. This is the
        // extra termination policy `get_profile_string` does not have.
        if err == ERROR_MORE_DATA || returned as usize == size.saturating_sub(2) {
            size *= 2;
            continue;
        }
        if err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND {
            return Ok(Vec::new());
        }
        if err != ERROR_SUCCESS {
            return Err(err);
        }
        buf.truncate(returned as usize);
        let mut names = Vec::new();
        for chunk in buf.split(|&u| u == 0) {
            if chunk.is_empty() {
                continue;
            }
            names.push(String::from_utf16_lossy(chunk));
        }
        return Ok(names);
    }
}

/// Why `name` cannot be written as a value name, or `None` when it can.
///
/// Values get an escaping pass ([`escape_ini_value`]) but names do not:
/// `WritePrivateProfileStringW` emits the name verbatim ahead of the `=`, and
/// the INI line format has no quoting for it. So a name carrying a line break or
/// a leading `[` writes extra lines - a section header among them - rather than
/// one entry, and a name carrying `=`, surrounding whitespace, or a leading `;`
/// reads back as something other than what was written. A mod's `<modId>.ini`
/// holds `[Settings]` and the `[Mod]` config side by side, so an injected header
/// there lands in a file whose other section decides what the engine loads.
///
/// This rejects nothing the callers legitimately write: the flat settings
/// notation (`Scalar`, `Group.child`, `List[0]`) and the fixed config names are
/// all representable. It is the last line of defense - a name reaching here from
/// untrusted input is expected to have been refused earlier, by
/// `domain::is_valid_flat_key`.
fn unrepresentable_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("is empty");
    }
    if name.contains(char::is_control) {
        return Some("contains a control character");
    }
    if name.contains('=') {
        return Some("contains '='");
    }
    if name.starts_with('[') {
        return Some("starts with '['");
    }
    if name.starts_with(';') {
        return Some("starts with ';'");
    }
    if name.trim() != name {
        return Some("has leading or trailing whitespace");
    }
    None
}

/// Wrap a value in double quotes when the profile API would otherwise not read
/// back what was written: one with leading/trailing whitespace (the reader trims
/// it) or one already wrapped in matching quotes (the reader strips them). The
/// quotes are the only escape the INI line format has, which is why a value
/// carrying a line break is refused by the caller rather than escaped here.
fn escape_ini_value(value: &str) -> Cow<'_, str> {
    let chars: Vec<char> = value.chars().collect();
    let can_be_trimmed = !chars.is_empty() && (chars[0] <= ' ' || chars[chars.len() - 1] <= ' ');
    let is_quoted = chars.len() >= 2
        && chars[0] == chars[chars.len() - 1]
        && (chars[0] == '"' || chars[0] == '\'');

    if !can_be_trimmed && !is_quoted {
        return Cow::Borrowed(value);
    }

    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    out.push_str(value);
    out.push('"');
    Cow::Owned(out)
}

/// `std::stol(s, nullptr, 0)` clamped to `i32`: base-0 (0x = hex), leading
/// numeric prefix only, NaN/empty -> 0 (the engine/TS coercion of a string
/// where an int is expected).
fn parse_c_int(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (radix, digits) = match rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        Some(hex) => (16u32, hex),
        None => (10, rest),
    };
    let taken: String = digits.chars().take_while(|c| c.is_digit(radix)).collect();
    let value = i64::from_str_radix(&taken, radix).unwrap_or(0);
    let value = if neg { -value } else { value };
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_quoted_only_where_the_reader_would_not_return_them() {
        assert_eq!(escape_ini_value("plain"), "plain");
        assert_eq!(escape_ini_value(""), "");
        assert_eq!(escape_ini_value(" lead"), "\" lead\"");
        assert_eq!(escape_ini_value("trail "), "\"trail \"");
        assert_eq!(escape_ini_value("\"quoted\""), "\"\"quoted\"\"");
        assert_eq!(escape_ini_value("'quoted'"), "\"'quoted'\"");
        // Internal whitespace alone does not trigger quoting.
        assert_eq!(escape_ini_value("a b"), "a b");
        // A tab counts as trimmable whitespace at either end.
        assert_eq!(escape_ini_value("\ttabbed"), "\"\ttabbed\"");
    }

    #[test]
    fn value_names_the_line_format_cannot_express_are_refused() {
        // The names the callers actually write.
        for name in ["Scalar", "group.inner", "matrix[2].cell", "LibraryFileName"] {
            assert_eq!(unrepresentable_name(name), None, "{name} must be writable");
        }
        // A name is emitted verbatim ahead of the `=`, with no quoting, so these
        // would write something other than one entry - a section header among
        // them, in the file that also holds the mod's `[Mod]` config.
        for name in [
            "",
            "a\r\n[Mod]\r\nLibraryFileName=evil.dll\r\nb",
            "a\nb",
            "a\0b",
            "a=b",
            "[Mod]",
            ";comment",
            " padded",
            "padded ",
        ] {
            assert!(
                unrepresentable_name(name).is_some(),
                "{name:?} must be refused"
            );
        }
    }

    #[test]
    fn hex_round_trips_uppercase() {
        assert_eq!(encode_hex(&[0xDE, 0xAD, 0xBE, 0xEF]), "DEADBEEF");
        assert_eq!(decode_hex("DEADBEEF"), Some(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        assert_eq!(decode_hex("ABC"), None);
    }

    #[test]
    fn c_int_parsing() {
        assert_eq!(parse_c_int("42"), 42);
        assert_eq!(parse_c_int("-7"), -7);
        assert_eq!(parse_c_int("0x10"), 16);
        assert_eq!(parse_c_int("nope"), 0);
        assert_eq!(parse_c_int(""), 0);
    }
}
