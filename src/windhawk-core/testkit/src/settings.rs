//! In-memory `SettingsBackend`. A behavioral fake: it stores typed values in
//! a map keyed by a normalized tree key, so command-level service tests run
//! without Windows. Byte-format fidelity is the real adapters' job; this fake
//! only models the keyed-value semantics.

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

    /// `value` as this store holds it: an INI file holds an int as its decimal
    /// text and bytes as hex, as `IniTree` writes them.
    fn held(self, value: TreeValue) -> TreeValue {
        match (self, value) {
            (TreeKind::Ini, TreeValue::Int(i)) => TreeValue::Str(i.to_string()),
            (TreeKind::Ini, TreeValue::Binary(b)) => TreeValue::Str(encode_hex(&b)),
            (_, value) => value,
        }
    }
}

/// The value stored under `name`. Both stores match value names ignoring case
/// (ASCII here, which every name the core writes is).
fn lookup<'t>(tree: &'t Tree, name: &str) -> Option<&'t TreeValue> {
    tree.iter()
        .find(|(stored, _)| stored.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

/// Store `value` under `name`, keeping the casing of a name already stored, as
/// both stores do on an overwrite.
fn store_value(tree: &mut Tree, name: &str, value: TreeValue) {
    let name = tree
        .keys()
        .find(|stored| stored.eq_ignore_ascii_case(name))
        .cloned()
        .unwrap_or_else(|| name.to_owned());
    tree.insert(name, value);
}

fn remove_value(tree: &mut Tree, name: &str) {
    tree.retain(|stored, _| !stored.eq_ignore_ascii_case(name));
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

/// The record of the mutating calls and the one-shot write fault they count
/// toward.
#[derive(Default)]
struct Writes {
    log: Vec<String>,
    /// Mutating calls made since the fault was armed.
    count: usize,
    /// The 1-based mutating call, counted from when it was armed, that fails.
    fail_at: Option<usize>,
}

impl Writes {
    /// Record one mutating call, failing it when it is the armed one.
    fn record(&mut self, entry: String) -> Result<(), SettingsError> {
        self.count += 1;
        if self.fail_at == Some(self.count) {
            self.fail_at = None;
            return Err(SettingsError::registry(
                "write",
                entry,
                5,
                "injected write failure",
            ));
        }
        self.log.push(entry);
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct FakeSettings {
    store: Arc<Mutex<Store>>,
    /// When set, every `open` and `remove_tree` returns this error, so
    /// command-level tests can exercise the error-mapping paths (the
    /// backend's first touch in every settings command is one of those two).
    fault: Arc<Mutex<Option<SettingsError>>>,
    writes: Arc<Mutex<Writes>>,
}

impl FakeSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make every subsequent `open`/`remove_tree` fail with `error` (fault
    /// injection).
    pub fn set_fault(&self, error: SettingsError) {
        *self.fault.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
    }

    fn check_fault(&self) -> Result<(), SettingsError> {
        match self.fault.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Every mutating call that succeeded, in order: `set <tree> <name>`,
    /// `remove <tree> <name>`, `replace <tree>`, `remove_tree <tree>` and
    /// `rename <from> <to>`, each tree as its [`tree_key`].
    pub fn write_log(&self) -> Vec<String> {
        self.writes_lock().log.clone()
    }

    /// Empty the write log.
    pub fn clear_write_log(&self) {
        self.writes_lock().log.clear();
    }

    /// Make the `n`th mutating call from now (1-based) fail, once. A failed call
    /// changes nothing and is not logged.
    pub fn fail_nth_write(&self, n: usize) {
        let mut writes = self.writes_lock();
        writes.count = 0;
        writes.fail_at = Some(n);
    }

    fn writes_lock(&self) -> std::sync::MutexGuard<'_, Writes> {
        self.writes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A clone of the whole store, for assertions.
    pub fn snapshot(&self) -> Store {
        self.store.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Seed a value into a tree (test setup) as a write stores it, creating the
    /// tree if absent.
    pub fn seed(&self, tree: &TreeLocation, name: &str, value: TreeValue) {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store_value(
            store.entry(tree_key(tree)).or_default(),
            name,
            tree_kind(tree).held(value),
        );
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
            writes: self.writes.clone(),
            key: tree_key(tree),
            kind: tree_kind(tree),
        }))
    }

    fn remove_tree(&self, tree: &TreeLocation) -> Result<(), SettingsError> {
        self.check_fault()?;
        self.writes_lock()
            .record(format!("remove_tree {}", tree_key(tree)))?;
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
        self.writes_lock()
            .record(format!("rename {} {}", tree_key(from), tree_key(to)))?;
        for key in matches {
            let suffix = key.strip_prefix(&from_prefix).unwrap_or_default();
            let tree = store.remove(&key).unwrap_or_default();
            store.insert(format!("{to_prefix}{suffix}"), tree);
        }
        Ok(())
    }

    fn list_subtrees(&self, parent: &TreeLocation) -> Result<Vec<String>, SettingsError> {
        self.check_fault()?;
        // The immediate child segment of every stored tree under `parent`: a
        // registry subkey, or an INI section of the same file named
        // `<section>\<child>`.
        let prefix = match parent {
            TreeLocation::Registry { sub_key } if sub_key.is_empty() => "reg:".to_owned(),
            TreeLocation::Registry { sub_key } => format!("reg:{sub_key}\\"),
            TreeLocation::Ini { .. } => format!("{}\\", tree_key(parent)),
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
    writes: Arc<Mutex<Writes>>,
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

    fn record(&self, op: &str, name: &str) -> Result<(), SettingsError> {
        self.writes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(format!("{op} {} {name}", self.key))
    }

    /// A refusal, of the kind the adapter this tree models reports.
    fn refusal(&self, op: &'static str, message: &str) -> SettingsError {
        match self.kind {
            TreeKind::Registry => SettingsError::registry(op, self.key.clone(), 0, message),
            TreeKind::Ini => SettingsError::ini(op, self.key.clone(), 0, message),
        }
    }

    /// Refuse a value name the INI adapter cannot write.
    fn check_name(&self, op: &'static str, name: &str) -> Result<(), SettingsError> {
        match unrepresentable_ini_name(name) {
            Some(why) if self.kind.is_ini() => {
                Err(self.refusal(op, &format!("value name {name:?} {why}")))
            }
            _ => Ok(()),
        }
    }

    fn set(&mut self, name: &str, value: TreeValue) -> Result<(), SettingsError> {
        self.check_name("set", name)?;
        if let TreeValue::Str(s) = &value
            && let Some(why) = self.unstorable_string(s)
        {
            return Err(self.refusal("set", &format!("value {why}")));
        }
        self.record("set", name)?;
        let value = self.kind.held(value);
        self.with_tree_mut(|t| store_value(t, name, value));
        Ok(())
    }
}

impl SettingsTree for FakeTree {
    fn get_string(&self, name: &str) -> Result<Option<String>, SettingsError> {
        Ok(self.with_tree(|t| match t.and_then(|t| lookup(t, name)) {
            Some(TreeValue::Str(s)) => Some(s.clone()),
            _ => None,
        }))
    }

    fn set_string(&mut self, name: &str, value: &str) -> Result<(), SettingsError> {
        self.set(name, TreeValue::Str(value.to_owned()))
    }

    fn unstorable_string(&self, value: &str) -> Option<&'static str> {
        // The adapters' refusals: a NUL ends the Win32 string either store
        // takes, and a line break ends an INI entry.
        if value.contains('\0') {
            Some("contains a NUL character")
        } else if self.kind.is_ini() && value.contains(['\r', '\n']) {
            Some("contains a line break")
        } else {
            None
        }
    }

    fn get_int(&self, name: &str) -> Result<Option<i32>, SettingsError> {
        Ok(self.with_tree(|t| match t.and_then(|t| lookup(t, name)) {
            Some(TreeValue::Int(i)) => Some(*i),
            // The INI parse yields a number for any stored text, so a present
            // value is never absent to an int read (non-numeric -> 0).
            Some(TreeValue::Str(s)) if self.kind.is_ini() => Some(parse_c_int(s)),
            _ => None,
        }))
    }

    fn set_int(&mut self, name: &str, value: i32) -> Result<(), SettingsError> {
        self.set(name, TreeValue::Int(value))
    }

    fn get_binary(&self, name: &str) -> Result<Option<Vec<u8>>, SettingsError> {
        self.with_tree(|t| match t.and_then(|t| lookup(t, name)) {
            Some(TreeValue::Binary(b)) => Ok(Some(b.clone())),
            Some(TreeValue::Str(s)) if self.kind.is_ini() => decode_hex(s)
                .map(Some)
                .ok_or_else(|| self.refusal("get_binary", "odd-length or non-hex value")),
            _ => Ok(None),
        })
    }

    fn set_binary(&mut self, name: &str, value: &[u8]) -> Result<(), SettingsError> {
        self.set(name, TreeValue::Binary(value.to_vec()))
    }

    fn remove(&mut self, name: &str) -> Result<(), SettingsError> {
        self.record("remove", name)?;
        self.with_tree_mut(|t| remove_value(t, name));
        Ok(())
    }

    fn enum_values(&self) -> Result<Vec<(String, TreeValue)>, SettingsError> {
        Ok(self.with_tree(|t| {
            t.map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default()
        }))
    }

    fn value_names(&self) -> Result<Vec<String>, SettingsError> {
        Ok(self.with_tree(|t| t.map(|t| t.keys().cloned().collect()).unwrap_or_default()))
    }

    /// The INI adapter rewrites the section in one call; the registry has no
    /// such write.
    fn replace_values(&mut self, values: &[(&str, TreeValue)]) -> Result<bool, SettingsError> {
        if !self.kind.is_ini() {
            return Ok(false);
        }
        for (name, value) in values {
            self.check_name("replace", name)?;
            if let TreeValue::Str(s) = value
                && let Some(why) = self.unstorable_string(s)
            {
                return Err(self.refusal("replace", &format!("value of {name:?} {why}")));
            }
        }
        self.writes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .record(format!("replace {}", self.key))?;
        self.with_tree_mut(|t| {
            t.clear();
            for (name, value) in values {
                store_value(t, name, self.kind.held(value.clone()));
            }
        });
        Ok(true)
    }
}

/// Why the INI adapter refuses `name` as a value name, which the profile API
/// writes verbatim ahead of the `=`: the rules of its `unrepresentable_name`.
fn unrepresentable_ini_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        Some("is empty")
    } else if name.contains(char::is_control) {
        Some("contains a control character")
    } else if name.contains('=') {
        Some("contains '='")
    } else if name.starts_with('[') {
        Some("starts with '['")
    } else if name.starts_with(';') {
        Some("starts with ';'")
    } else if name.trim() != name {
        Some("has leading or trailing whitespace")
    } else {
        None
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
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

    #[test]
    fn both_trees_match_value_names_ignoring_case() {
        for location in [registry_location(), ini_location()] {
            let backend = FakeSettings::new();
            backend.seed(&location, "Name", TreeValue::Str("old".to_owned()));
            let mut tree = backend.open(&location, true).unwrap();
            assert_eq!(tree.get_string("NAME").unwrap().as_deref(), Some("old"));

            // An overwrite keeps the stored casing.
            tree.set_string("name", "new").unwrap();
            assert_eq!(
                tree.enum_values().unwrap(),
                [("Name".to_owned(), TreeValue::Str("new".to_owned()))]
            );

            tree.remove("nAmE").unwrap();
            assert!(tree.value_names().unwrap().is_empty());
        }
    }

    #[test]
    fn an_ini_tree_holds_every_value_as_text() {
        let backend = FakeSettings::new();
        let mut tree = backend.open(&ini_location(), true).unwrap();
        tree.set_int("int", -5).unwrap();
        tree.set_binary("bytes", &[0xAB, 0x01]).unwrap();
        assert_eq!(tree.get_int("int").unwrap(), Some(-5));
        assert_eq!(tree.get_binary("bytes").unwrap(), Some(vec![0xAB, 0x01]));
        assert_eq!(
            tree.enum_values().unwrap(),
            [
                ("bytes".to_owned(), TreeValue::Str("AB01".to_owned())),
                ("int".to_owned(), TreeValue::Str("-5".to_owned())),
            ]
        );

        assert!(tree.replace_values(&[("int", TreeValue::Int(7))]).unwrap());
        assert_eq!(
            tree.enum_values().unwrap(),
            [("int".to_owned(), TreeValue::Str("7".to_owned()))]
        );
    }

    #[test]
    fn an_ini_tree_refuses_a_name_the_file_cannot_hold() {
        let backend = FakeSettings::new();
        let mut tree = backend.open(&ini_location(), true).unwrap();
        for name in ["", "a=b", "[Mod]", ";a", " a", "a\r\nb"] {
            assert!(tree.set_string(name, "x").is_err(), "{name:?}");
            assert!(tree.set_int(name, 1).is_err(), "{name:?}");
            let values = [("ok", TreeValue::Int(1)), (name, TreeValue::Int(1))];
            assert!(tree.replace_values(&values).is_err(), "{name:?}");
        }
        assert!(backend.write_log().is_empty());
    }
}
