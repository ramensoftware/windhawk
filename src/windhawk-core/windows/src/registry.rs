//! The registry `SettingsBackend`: `REG_SZ` for strings, `REG_DWORD` for ints,
//! `REG_BINARY` for binary, the 64-bit view (`KEY_WOW64_64KEY`) throughout,
//! matching `RegistrySettings` in the main repository's
//! `shared/portable_settings.cpp` and the TypeScript `RegistryStorageBackend`
//! the fixtures were recorded from.

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_ENUMERATE_SUB_KEYS,
    KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY, KEY_WRITE, REG_BINARY, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW,
    RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW,
    RegRenameKey, RegSetValueExW,
};

use windhawk_core_ports::{SettingsBackend, SettingsError, SettingsTree, TreeLocation, TreeValue};

use crate::wide::{from_wide, from_wide_nul, to_wide};

/// The supported root hives (the prefixes `parseRegistryKey` accepts). An enum
/// (not a raw `HKEY`) so the backend stays `Send + Sync`. Sealed to
/// `pub(crate)`: a Win32 concept that should not leak from the crate - external
/// callers use `RegistryBackend::current_user`/`::local_machine`, and
/// `storage.rs`'s resolver keeps the raw `Hive`/`Hive::parse` internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hive {
    CurrentUser,
    LocalMachine,
    Users,
}

impl Hive {
    fn hkey(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::Users => HKEY_USERS,
        }
    }

    /// Parse the hive prefix of a `RegistryKey` value, accepting both the
    /// short and long spellings (`parseRegistryKey` in `storage/paths.ts`).
    pub(crate) fn parse(prefix: &str) -> Option<Hive> {
        match prefix {
            "HKEY_CURRENT_USER" | "HKCU" => Some(Hive::CurrentUser),
            "HKEY_USERS" | "HKU" => Some(Hive::Users),
            "HKEY_LOCAL_MACHINE" | "HKLM" => Some(Hive::LocalMachine),
            _ => None,
        }
    }
}

/// An owned open registry key that closes on drop.
struct OwnedKey(HKEY);

impl Drop for OwnedKey {
    fn drop(&mut self) {
        // SAFETY: self.0 is a key handle opened by RegOpenKeyExW /
        // RegCreateKeyExW in this module and closed exactly once (here).
        unsafe { RegCloseKey(self.0) };
    }
}

pub struct RegistryBackend {
    hive: Hive,
    /// The resolved `regSubKey` (e.g. `Software\Windhawk`); tree subkeys hang
    /// under it.
    root_sub_key: String,
}

impl RegistryBackend {
    pub(crate) fn new(hive: Hive, root_sub_key: String) -> Self {
        Self { hive, root_sub_key }
    }

    /// An `HKEY_CURRENT_USER`-rooted backend. The public surface that replaces
    /// a raw `Hive` at the external test/parity call sites, now that `Hive` is
    /// sealed.
    pub fn current_user(root_sub_key: String) -> Self {
        Self::new(Hive::CurrentUser, root_sub_key)
    }

    /// An `HKEY_LOCAL_MACHINE`-rooted backend (the manual WOW64 tests' hive).
    pub fn local_machine(root_sub_key: String) -> Self {
        Self::new(Hive::LocalMachine, root_sub_key)
    }

    fn full_sub_key(&self, sub_key: &str) -> String {
        match (self.root_sub_key.is_empty(), sub_key.is_empty()) {
            (true, _) => sub_key.to_owned(),
            (false, true) => self.root_sub_key.clone(),
            (false, false) => format!("{}\\{}", self.root_sub_key, sub_key),
        }
    }
}

fn require_registry(tree: &TreeLocation) -> Result<&str, SettingsError> {
    match tree {
        TreeLocation::Registry { sub_key } => Ok(sub_key),
        TreeLocation::Ini { .. } => Err(SettingsError::registry(
            "open",
            "ini location given to the registry backend",
            0,
            "registry backend received an INI tree location",
        )),
    }
}

const KEY_WOW64: REG_SAM_FLAGS = KEY_WOW64_64KEY;

/// Which registry view (bitness) a write forces: the typed replacement for
/// `set_dword_value`'s former `wow64_32: bool`. Named for the absolute BITNESS
/// of the view, NOT `Native`/`Wow64`: on a 32-bit (WOW64) host the
/// process-native view IS the 32-bit one, so "Native" would invert there. The
/// axis is bitness, which `Bit64`/`Bit32` name unambiguously irrespective of
/// process bitness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryView {
    // The 64-bit view exists for the completeness of the bitness axis and its
    // mapping is pinned by a unit test, but no production caller selects it
    // today (the sole consumer, the installer-language write, always forces the
    // 32-bit view); kept rather than collapsing the enum back to a one-sided
    // flag, with the variant's absence from production construction allowed.
    #[allow(dead_code)]
    Bit64,
    Bit32,
}

impl RegistryView {
    /// The `KEY_WOW64_*` access flag for this view - the one home for the
    /// constant pairing.
    fn sam(self) -> REG_SAM_FLAGS {
        match self {
            RegistryView::Bit32 => windows_sys::Win32::System::Registry::KEY_WOW64_32KEY,
            RegistryView::Bit64 => KEY_WOW64_64KEY,
        }
    }
}

// The standard DELETE access right (winnt.h, 0x00010000), needed to open a key
// for the handle-based RegDeleteTree. windows-sys exposes DELETE from several
// modules, so name it locally to avoid an ambiguous import.
const DELETE_ACCESS: REG_SAM_FLAGS = 0x0001_0000;

impl SettingsBackend for RegistryBackend {
    fn open(
        &self,
        tree: &TreeLocation,
        write: bool,
    ) -> Result<Box<dyn SettingsTree>, SettingsError> {
        let sub_key = require_registry(tree)?;
        let full = self.full_sub_key(sub_key);
        let wide = to_wide(&full);
        let mut hkey: HKEY = std::ptr::null_mut();

        if write {
            let sam = KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64;
            // SAFETY: wide is NUL-terminated; phkResult is a valid out param.
            let rc = unsafe {
                RegCreateKeyExW(
                    self.hive.hkey(),
                    wide.as_ptr(),
                    0,
                    std::ptr::null_mut(),
                    REG_OPTION_NON_VOLATILE,
                    sam,
                    std::ptr::null(),
                    &mut hkey,
                    std::ptr::null_mut(),
                )
            };
            if rc != ERROR_SUCCESS {
                return Err(SettingsError::registry(
                    "create",
                    full,
                    rc,
                    "RegCreateKeyEx",
                ));
            }
            Ok(Box::new(RegistryTree {
                key: Some(OwnedKey(hkey)),
                location: full,
            }))
        } else {
            let sam = KEY_QUERY_VALUE | KEY_WOW64;
            // SAFETY: as above.
            let rc = unsafe { RegOpenKeyExW(self.hive.hkey(), wide.as_ptr(), 0, sam, &mut hkey) };
            match rc {
                ERROR_SUCCESS => Ok(Box::new(RegistryTree {
                    key: Some(OwnedKey(hkey)),
                    location: full,
                })),
                // An absent key reads as an empty tree (the TS `openKey`
                // returning null path: every value comes back `None`).
                ERROR_FILE_NOT_FOUND => Ok(Box::new(RegistryTree {
                    key: None,
                    location: full,
                })),
                _ => Err(SettingsError::registry("open", full, rc, "RegOpenKeyEx")),
            }
        }
    }

    fn remove_tree(&self, tree: &TreeLocation) -> Result<(), SettingsError> {
        let sub_key = require_registry(tree)?;
        let full = self.full_sub_key(sub_key);
        let wide = to_wide(&full);

        // The store lives in the 64-bit view: every open/create/enumerate in
        // this backend forces KEY_WOW64_64KEY, matching the C++ engine and
        // RegistrySettings in shared/portable_settings.cpp. The predefined-hive
        // + path forms of RegDeleteTreeW / RegRenameKey open the target in the
        // PROCESS DEFAULT view, which on a 32-bit (WOW64) host is the wrong
        // (Wow6432Node) view - there the keys do not exist, so the delete
        // silently no-ops (ERROR_FILE_NOT_FOUND) and the real entries survive.
        // So open the key IN the 64-bit view and operate on the handle:
        // RegDeleteTree(handle, null) clears its descendants and values, then
        // RegDeleteKeyEx removes the now-empty key itself (also 64-bit view,
        // matching RegistrySettings::RemoveSection's RegDeleteKeyEx(..,
        // KEY_WOW64_64KEY, ..)).
        let sam =
            DELETE_ACCESS | KEY_ENUMERATE_SUB_KEYS | KEY_QUERY_VALUE | KEY_SET_VALUE | KEY_WOW64;
        let mut hkey: HKEY = std::ptr::null_mut();
        // SAFETY: wide is NUL-terminated; phkResult is a valid out param.
        let rc = unsafe { RegOpenKeyExW(self.hive.hkey(), wide.as_ptr(), 0, sam, &mut hkey) };
        match rc {
            // An absent key is nothing to remove (the TS openKey-null path).
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(()),
            ERROR_SUCCESS => {}
            _ => {
                return Err(SettingsError::registry(
                    "remove_tree",
                    full,
                    rc,
                    "RegOpenKeyEx",
                ));
            }
        }
        {
            let key = OwnedKey(hkey);
            // SAFETY: key.0 is a valid open key; a null subkey deletes the key's
            // descendants and values, but not the key itself.
            let rc = unsafe { RegDeleteTreeW(key.0, std::ptr::null()) };
            if rc != ERROR_SUCCESS {
                return Err(SettingsError::registry(
                    "remove_tree",
                    full,
                    rc,
                    "RegDeleteTree",
                ));
            }
            // `key` is dropped here, closing the handle before the final delete.
        }
        // Delete the emptied key itself, in the same 64-bit view.
        // SAFETY: wide is NUL-terminated; the hive handle is a predefined key.
        let rc = unsafe { RegDeleteKeyExW(self.hive.hkey(), wide.as_ptr(), KEY_WOW64, 0) };
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
            Ok(())
        } else {
            Err(SettingsError::registry(
                "remove_tree",
                full,
                rc,
                "RegDeleteKeyEx",
            ))
        }
    }

    fn rename_tree(&self, from: &TreeLocation, to: &TreeLocation) -> Result<(), SettingsError> {
        let from_sub = require_registry(from)?;
        let to_sub = require_registry(to)?;
        let full_from = self.full_sub_key(from_sub);
        // RegRenameKey takes the new leaf name, not a full path; the rename
        // stays under the same parent (`Engine\Mods\<from>` -> `<to>`).
        let new_leaf = to_sub.rsplit('\\').next().unwrap_or(to_sub);
        let from_w = to_wide(&full_from);
        let new_w = to_wide(new_leaf);

        // Open the source key in the 64-bit view and rename via the handle (a
        // null subkey renames the key the handle names), matching the TS
        // `renameKey(key, null, toId)` over a WOW64_64KEY handle. The
        // predefined-hive + path form of RegRenameKey would resolve in the
        // process default view (Wow6432Node on a 32-bit host), where the key
        // does not exist - the same view trap as remove_tree.
        let sam = KEY_WRITE | KEY_WOW64;
        let mut hkey: HKEY = std::ptr::null_mut();
        // SAFETY: from_w is NUL-terminated; phkResult is a valid out param.
        let rc = unsafe { RegOpenKeyExW(self.hive.hkey(), from_w.as_ptr(), 0, sam, &mut hkey) };
        match rc {
            // An absent source key is a no-op (the TS openKey-null path).
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(()),
            ERROR_SUCCESS => {}
            _ => {
                return Err(SettingsError::registry(
                    "rename_tree",
                    full_from,
                    rc,
                    "RegOpenKeyEx",
                ));
            }
        }
        let key = OwnedKey(hkey);
        // SAFETY: key.0 is a valid open key; new_w is NUL-terminated; a null
        // subkey renames the key the handle names.
        let rc = unsafe { RegRenameKey(key.0, std::ptr::null(), new_w.as_ptr()) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(SettingsError::registry(
                "rename_tree",
                full_from,
                rc,
                "RegRenameKey",
            ))
        }
    }

    fn list_subtrees(&self, parent: &TreeLocation) -> Result<Vec<String>, SettingsError> {
        let sub_key = require_registry(parent)?;
        let full = self.full_sub_key(sub_key);
        let wide = to_wide(&full);
        let mut hkey: HKEY = std::ptr::null_mut();
        let sam = KEY_ENUMERATE_SUB_KEYS | KEY_WOW64;
        // SAFETY: wide is NUL-terminated; phkResult is a valid out param.
        let rc = unsafe { RegOpenKeyExW(self.hive.hkey(), wide.as_ptr(), 0, sam, &mut hkey) };
        match rc {
            // An absent parent enumerates as empty (the TS openKey-null path).
            ERROR_FILE_NOT_FOUND => return Ok(Vec::new()),
            ERROR_SUCCESS => {}
            _ => {
                return Err(SettingsError::registry(
                    "list_subtrees",
                    full,
                    rc,
                    "RegOpenKeyEx",
                ));
            }
        }
        let key = OwnedKey(hkey);

        let mut out = Vec::new();
        let mut index: u32 = 0;
        loop {
            // Registry key names are bounded at 255 chars; +1 for the NUL.
            let mut name_buf = vec![0u16; 256];
            let mut name_len = name_buf.len() as u32;
            // SAFETY: name_buf has name_len units; the optional out params are
            // null (only the subkey name is needed).
            let rc = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                return Err(SettingsError::registry(
                    "list_subtrees",
                    full,
                    rc,
                    "RegEnumKeyEx",
                ));
            }
            name_buf.truncate(name_len as usize);
            out.push(from_wide(&name_buf));
            index += 1;
        }
        Ok(out)
    }
}

struct RegistryTree {
    /// `None` for a read-open of an absent key (all reads return `None`).
    key: Option<OwnedKey>,
    location: String,
}

impl RegistryTree {
    fn hkey(&self) -> Option<HKEY> {
        self.key.as_ref().map(|k| k.0)
    }

    fn err(&self, op: &'static str, rc: u32, what: &str) -> SettingsError {
        SettingsError::registry(op, self.location.clone(), rc, what.to_owned())
    }

    /// Read a value's raw type + bytes: a size-query call (null data buffer)
    /// for the type and length, then one sized read. There is no
    /// `ERROR_MORE_DATA` grow loop - the size query returns the exact length,
    /// so the single follow-up read always fits.
    fn query_raw(&self, name: &str) -> Result<Option<(u32, Vec<u8>)>, SettingsError> {
        let Some(hkey) = self.hkey() else {
            return Ok(None);
        };
        let name_w = to_wide(name);
        let mut size: u32 = 0;
        let mut value_type: u32 = 0;
        // First call: type + size only.
        // SAFETY: name_w is NUL-terminated; out params are valid; lpData null
        // with *lpcbData = 0 just queries the size.
        let rc = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA {
            return Err(self.err("get", rc, "RegQueryValueEx (size)"));
        }
        let mut buf = vec![0u8; size as usize];
        let mut data_size = size;
        // SAFETY: buf has data_size bytes; out params valid.
        let rc = unsafe {
            RegQueryValueExW(
                hkey,
                name_w.as_ptr(),
                std::ptr::null(),
                &mut value_type,
                buf.as_mut_ptr(),
                &mut data_size,
            )
        };
        if rc == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if rc != ERROR_SUCCESS {
            return Err(self.err("get", rc, "RegQueryValueEx (data)"));
        }
        buf.truncate(data_size as usize);
        Ok(Some((value_type, buf)))
    }

    fn set_raw(&self, name: &str, value_type: u32, data: &[u8]) -> Result<(), SettingsError> {
        let Some(hkey) = self.hkey() else {
            return Err(self.err("set", 0, "set on a read-only/absent key"));
        };
        let name_w = to_wide(name);
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        // SAFETY: name_w is NUL-terminated; data/len describe a valid buffer.
        let rc =
            unsafe { RegSetValueExW(hkey, name_w.as_ptr(), 0, value_type, data.as_ptr(), len) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(self.err("set", rc, "RegSetValueEx"))
        }
    }
}

/// Decode a `REG_SZ` byte buffer (UTF-16LE, possibly NUL-terminated).
fn decode_sz(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    from_wide_nul(&units)
}

fn decode_dword(bytes: &[u8]) -> Option<i32> {
    if bytes.len() == 4 {
        Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    } else {
        None
    }
}

impl SettingsTree for RegistryTree {
    fn get_string(&self, name: &str) -> Result<Option<String>, SettingsError> {
        Ok(match self.query_raw(name)? {
            Some((t, bytes)) if t == REG_SZ => Some(decode_sz(&bytes)),
            _ => None,
        })
    }

    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        // REG_SZ includes the terminating NUL, matching the C++
        // (wcslen+1)*sizeof(WCHAR) write.
        let mut wide = to_wide(value);
        let bytes: Vec<u8> = std::mem::take(&mut wide)
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        self.set_raw(name, REG_SZ, &bytes)
    }

    fn get_int(&self, name: &str) -> Result<Option<i32>, SettingsError> {
        Ok(match self.query_raw(name)? {
            Some((t, bytes)) if t == REG_DWORD => decode_dword(&bytes),
            _ => None,
        })
    }

    fn set_int(&mut self, name: &str, value: i32) -> Result<(), SettingsError> {
        self.set_raw(name, REG_DWORD, &value.to_le_bytes())
    }

    fn get_binary(&self, name: &str) -> Result<Option<Vec<u8>>, SettingsError> {
        Ok(match self.query_raw(name)? {
            Some((t, bytes)) if t == REG_BINARY => Some(bytes),
            _ => None,
        })
    }

    fn set_binary(&mut self, name: &str, value: &[u8]) -> Result<(), SettingsError> {
        self.set_raw(name, REG_BINARY, value)
    }

    fn remove(&mut self, name: &str) -> Result<(), SettingsError> {
        let Some(hkey) = self.hkey() else {
            return Ok(());
        };
        let name_w = to_wide(name);
        // SAFETY: name_w is NUL-terminated; hkey is a valid open key.
        let rc = unsafe { RegDeleteValueW(hkey, name_w.as_ptr()) };
        if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(self.err("remove", rc, "RegDeleteValue"))
        }
    }

    fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError> {
        let Some(hkey) = self.hkey() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut index: u32 = 0;
        loop {
            // Value names are bounded at 16383 chars; +1 for the NUL.
            let mut name_buf = vec![0u16; 16384];
            let mut name_len = name_buf.len() as u32;
            // SAFETY: name_buf has name_len units; the data out params are
            // null (we re-read each value by name to type it uniformly).
            let rc = unsafe {
                RegEnumValueW(
                    hkey,
                    index,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                return Err(self.err("enum", rc, "RegEnumValue"));
            }
            name_buf.truncate(name_len as usize);
            let name = from_wide(&name_buf);
            if let Some((t, bytes)) = self.query_raw(&name)? {
                let value = if t == REG_SZ {
                    TreeValue::Str(decode_sz(&bytes))
                } else if t == REG_DWORD {
                    match decode_dword(&bytes) {
                        Some(i) => TreeValue::Int(i),
                        None => {
                            index += 1;
                            continue;
                        }
                    }
                } else if t == REG_BINARY {
                    TreeValue::Binary(bytes)
                } else {
                    index += 1;
                    continue;
                };
                out.push((name, value));
            }
            index += 1;
        }
        Ok(out)
    }
}

/// Write a single DWORD to an arbitrary hive\subkey value in a given WOW64
/// view, creating the key. Used for the installer-language write
/// (`applyAppSettings`, non-portable), which does not go through the resolved
/// settings backend. Returns `Ok(())` on success or `Err(rc)` with the failing
/// Win32 code (the caller builds an `OsError` from the rc so the best-effort
/// warning can state WHY the write failed, rather than swallowing it into a
/// bool).
pub(crate) fn set_dword_value(
    hive: Hive,
    sub_key: &str,
    value_name: &str,
    value: u32,
    view: RegistryView,
) -> Result<(), u32> {
    let wide = to_wide(sub_key);
    let mut hkey: HKEY = std::ptr::null_mut();
    // SAFETY: wide is NUL-terminated; phkResult is a valid out param.
    let rc = unsafe {
        RegCreateKeyExW(
            hive.hkey(),
            wide.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | view.sam(),
            std::ptr::null(),
            &mut hkey,
            std::ptr::null_mut(),
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(rc);
    }
    let key = OwnedKey(hkey);
    let name_w = to_wide(value_name);
    let bytes = value.to_le_bytes();
    // SAFETY: name_w is NUL-terminated; bytes is a 4-byte DWORD buffer.
    let rc = unsafe { RegSetValueExW(key.0, name_w.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4) };
    if rc == ERROR_SUCCESS { Ok(()) } else { Err(rc) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::{KEY_WOW64_32KEY, KEY_WOW64_64KEY};

    #[test]
    fn registry_view_maps_to_the_wow64_flag() {
        // The bool->variant correspondence `set_installer_language` relies on
        // (its only caller passes `Bit32`). The real helper has no higher-level
        // coverage (its tests use the testkit fake, which records the lcid and
        // never passes a view), so pin the mapping directly here.
        assert_eq!(RegistryView::Bit32.sam(), KEY_WOW64_32KEY);
        assert_eq!(RegistryView::Bit64.sam(), KEY_WOW64_64KEY);
    }
}
