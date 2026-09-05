//! In-memory `SettingsBackend` (core-internals.md section 3, testkit). A
//! behavioral fake: it stores typed values in a map keyed by a normalized
//! tree key, so command-level service tests run without Windows. Byte-format
//! fidelity is the real adapters' job (verified by the fixture-replay and
//! referee suites); this fake only models the keyed-value semantics.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use windhawk_core_ports::{SettingsBackend, SettingsError, SettingsTree, TreeLocation, TreeValue};

type Tree = BTreeMap<String, TreeValue>;
type Store = BTreeMap<String, Tree>;

/// A stable string key per tree location, so registry and INI locations map
/// to distinct in-memory trees.
pub fn tree_key(tree: &TreeLocation) -> String {
    match tree {
        TreeLocation::Registry { sub_key } => format!("reg:{sub_key}"),
        TreeLocation::Ini { file, section } => {
            format!("ini:{}|{}", file.display(), section)
        }
    }
}

/// Which adapter an open tree models. The single fake backend serves BOTH
/// storage modes, and the two disagree on a read that asks for a type other
/// than the one stored: `RegistryTree` reads it as absent, while an INI file
/// holds every value as text, so `IniTree` coerces between string and int.
#[derive(Clone, Copy)]
enum TreeKind {
    Registry,
    Ini,
}

impl TreeKind {
    fn is_ini(self) -> bool {
        matches!(self, TreeKind::Ini)
    }
}

fn tree_kind(tree: &TreeLocation) -> TreeKind {
    match tree {
        TreeLocation::Registry { .. } => TreeKind::Registry,
        TreeLocation::Ini { .. } => TreeKind::Ini,
    }
}

/// Whether a store key names the tree `prefix` or something a rename of that
/// tree carries with it: a registry descendant subkey (`<prefix>\...`) or, for
/// an INI file prefix (which ends in `|`), any section of that file.
fn under_prefix(key: &str, prefix: &str) -> bool {
    key == prefix
        || key.starts_with(&format!("{prefix}\\"))
        || (prefix.ends_with('|') && key.starts_with(prefix))
}

/// A `rename_tree` failure. The single fake backend serves BOTH modes, so the
/// kind is derived from the location (the real adapters each bake in their own).
fn rename_err(from: &TreeLocation, location: &str, message: &str) -> SettingsError {
    match from {
        TreeLocation::Registry { .. } => {
            SettingsError::registry("rename_tree", location, 0, message)
        }
        TreeLocation::Ini { .. } => SettingsError::ini("rename_tree", location, 0, message),
    }
}

#[derive(Default, Clone)]
pub struct FakeSettings {
    store: Arc<Mutex<Store>>,
    /// When set, every `open` and `remove_tree` returns this error, so
    /// command-level tests can exercise the error-mapping paths (the
    /// backend's first touch in every settings command is one of those two).
    fault: Arc<Mutex<Option<SettingsError>>>,
}

impl FakeSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make every subsequent `open`/`remove_tree` fail with `error`
    /// (fault injection, core-internals.md section 3).
    pub fn set_fault(&self, error: SettingsError) {
        *self.fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    fn check_fault(&self) -> Result<(), SettingsError> {
        match self.fault.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// A clone of the whole store, for assertions.
    pub fn snapshot(&self) -> Store {
        self.store.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Seed a value into a tree (test setup), creating the tree if absent.
    pub fn seed(&self, tree: &TreeLocation, name: &str, value: TreeValue) {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .entry(tree_key(tree))
            .or_default()
            .insert(name.to_owned(), value);
    }

    /// Whether a tree currently exists in the store.
    pub fn tree_exists(&self, tree: &TreeLocation) -> bool {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&tree_key(tree))
    }
}

impl SettingsBackend for FakeSettings {
    fn open(
        &self,
        tree: &TreeLocation,
        write: bool,
    ) -> Result<Box<dyn SettingsTree>, SettingsError> {
        self.check_fault()?;
        if write {
            // Opening for write creates the tree, mirroring RegCreateKeyEx /
            // the BOM-stamped INI file.
            self.store
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(tree_key(tree))
                .or_default();
        }
        Ok(Box::new(FakeTree {
            store: self.store.clone(),
            key: tree_key(tree),
            kind: tree_kind(tree),
        }))
    }

    fn remove_tree(&self, tree: &TreeLocation) -> Result<(), SettingsError> {
        self.check_fault()?;
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&tree_key(tree));
        Ok(())
    }

    fn rename_tree(&self, from: &TreeLocation, to: &TreeLocation) -> Result<(), SettingsError> {
        self.check_fault()?;
        // Mirror the adapters' whole-subtree move: registry renames the subkey
        // and its descendants (`reg:<from>` plus `reg:<from>\...`); INI renames
        // the backing file, moving every section keyed under that file
        // (`ini:<file>|...`). Keys are remapped by replacing the source prefix.
        let (from_prefix, to_prefix): (String, String) = match (from, to) {
            (TreeLocation::Registry { sub_key: f }, TreeLocation::Registry { sub_key: t }) => {
                (format!("reg:{f}"), format!("reg:{t}"))
            }
            (TreeLocation::Ini { file: f, .. }, TreeLocation::Ini { file: t, .. }) => (
                format!("ini:{}|", f.display()),
                format!("ini:{}|", t.display()),
            ),
            _ => {
                return Err(rename_err(
                    from,
                    "mismatched locations",
                    "rename_tree given mismatched location kinds",
                ));
            }
        };

        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let matches: Vec<String> = store
            .keys()
            .filter(|k| under_prefix(k, &from_prefix))
            .cloned()
            .collect();
        // An absent source is a no-op whatever the destination holds; past that,
        // a live destination is refused rather than replaced, as RegRenameKey
        // refuses it and the INI backend refuses to move a file over another
        // mod's.
        if matches.is_empty() {
            return Ok(());
        }
        if store.keys().any(|k| under_prefix(k, &to_prefix)) {
            return Err(rename_err(
                from,
                &tree_key(to),
                "rename_tree destination already exists",
            ));
        }
        for key in matches {
            let suffix = key.strip_prefix(&from_prefix).unwrap_or_default();
            let tree = store.remove(&key).unwrap_or_default();
            store.insert(format!("{to_prefix}{suffix}"), tree);
        }
        Ok(())
    }

    fn list_subtrees(&self, parent: &TreeLocation) -> Result<Vec<String>, SettingsError> {
        self.check_fault()?;
        // Registry only (INI mode lists files through the Files port). Find the
        // immediate child segment of every stored tree under `parent`.
        let prefix = match parent {
            TreeLocation::Registry { sub_key } if sub_key.is_empty() => "reg:".to_owned(),
            TreeLocation::Registry { sub_key } => format!("reg:{sub_key}\\"),
            TreeLocation::Ini { .. } => return Ok(Vec::new()),
        };
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for key in store.keys() {
            if let Some(rest) = key.strip_prefix(&prefix)
                && !rest.is_empty()
                && !rest.contains('\\')
            {
                out.push(rest.to_owned());
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

struct FakeTree {
    store: Arc<Mutex<Store>>,
    key: String,
    kind: TreeKind,
}

impl FakeTree {
    fn with_tree<R>(&self, f: impl FnOnce(Option<&Tree>) -> R) -> R {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        f(store.get(&self.key))
    }

    fn with_tree_mut<R>(&self, f: impl FnOnce(&mut Tree) -> R) -> R {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        f(store.entry(self.key.clone()).or_default())
    }
}

impl SettingsTree for FakeTree {
    fn get_string(&self, name: &str) -> Result<Option<String>, SettingsError> {
        Ok(self.with_tree(|t| match t.and_then(|t| t.get(name)) {
            Some(TreeValue::Str(s)) => Some(s.clone()),
            // An INI file holds an int as its decimal text, which is what a
            // string read of it hands back.
            Some(TreeValue::Int(i)) if self.kind.is_ini() => Some(i.to_string()),
            _ => None,
        }))
    }

    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        self.with_tree_mut(|t| t.insert(name.to_owned(), TreeValue::Str(value.to_owned())));
        Ok(())
    }

    fn get_int(&self, name: &str) -> Result<Option<i32>, SettingsError> {
        Ok(self.with_tree(|t| match t.and_then(|t| t.get(name)) {
            Some(TreeValue::Int(i)) => Some(*i),
            // The INI parse yields a number for any stored text, so a present
            // value is never absent to an int read (non-numeric -> 0).
            Some(TreeValue::Str(s)) if self.kind.is_ini() => Some(parse_c_int(s)),
            _ => None,
        }))
    }

    fn set_int(&mut self, name: &str, value: i32) -> Result<(), SettingsError> {
        self.with_tree_mut(|t| t.insert(name.to_owned(), TreeValue::Int(value)));
        Ok(())
    }

    fn get_binary(&self, name: &str) -> Result<Option<Vec<u8>>, SettingsError> {
        Ok(self.with_tree(|t| match t.and_then(|t| t.get(name)) {
            Some(TreeValue::Binary(b)) => Some(b.clone()),
            _ => None,
        }))
    }

    fn set_binary(&mut self, name: &str, value: &[u8]) -> Result<(), SettingsError> {
        self.with_tree_mut(|t| t.insert(name.to_owned(), TreeValue::Binary(value.to_vec())));
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), SettingsError> {
        self.with_tree_mut(|t| t.remove(name));
        Ok(())
    }

    fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError> {
        Ok(self.with_tree(|t| {
            t.map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default()
        }))
    }
}

/// `std::stol(s, nullptr, 0)` clamped to `i32`: base 0 (`0x` hex, a leading `0`
/// octal, else decimal), leading numeric prefix only, NaN/empty -> 0, value for
/// value the INI adapter's parse (`IniTree::get_int`).
fn parse_c_int(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (radix, digits) =
        if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
            (16, hex)
        } else if rest.starts_with('0') {
            (8, rest)
        } else {
            (10, rest)
        };
    let take: String = digits.chars().take_while(|c| c.is_digit(radix)).collect();
    let value = i64::from_str_radix(&take, radix).unwrap_or(0);
    let value = if neg { -value } else { value };
    value.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn registry_location() -> TreeLocation {
        TreeLocation::Registry {
            sub_key: "Settings".to_owned(),
        }
    }

    fn ini_location() -> TreeLocation {
        TreeLocation::Ini {
            file: PathBuf::from("settings.ini"),
            section: "Settings".to_owned(),
        }
    }

    #[test]
    fn a_registry_tree_reads_another_type_as_absent() {
        let backend = FakeSettings::new();
        let location = registry_location();
        backend.seed(&location, "AsString", TreeValue::Str("1".to_owned()));
        backend.seed(&location, "AsInt", TreeValue::Int(1));
        let tree = backend.open(&location, false).unwrap();
        assert_eq!(tree.get_int("AsString").unwrap(), None);
        assert_eq!(tree.get_string("AsInt").unwrap(), None);
    }

    #[test]
    fn an_ini_tree_coerces_between_string_and_int() {
        let backend = FakeSettings::new();
        let location = ini_location();
        backend.seed(&location, "AsString", TreeValue::Str("1".to_owned()));
        backend.seed(&location, "NotANumber", TreeValue::Str("abc".to_owned()));
        backend.seed(&location, "ZeroPadded", TreeValue::Str("010".to_owned()));
        backend.seed(&location, "AsInt", TreeValue::Int(1));
        let tree = backend.open(&location, false).unwrap();
        assert_eq!(tree.get_int("AsString").unwrap(), Some(1));
        // A present value that is not a number is 0, not absent, so the
        // service's default does NOT apply to it.
        assert_eq!(tree.get_int("NotANumber").unwrap(), Some(0));
        // Base 0, as in the adapter: a leading zero is octal.
        assert_eq!(tree.get_int("ZeroPadded").unwrap(), Some(8));
        assert_eq!(tree.get_string("AsInt").unwrap(), Some("1".to_owned()));
    }
}
