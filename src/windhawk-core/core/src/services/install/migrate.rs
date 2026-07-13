//! The mod-settings migration of `installMod` (the TS `setModConfig` settings
//! half): merge the new source's engine settings into the existing store, plus
//! the helpers that read the previous source's settings and convert the domain
//! engine-settings list into the JSON map the migration works on. `migrate`
//! borrows `services::mods::{read,write}_mod_settings` (they stay in `mods`);
//! its home boundary is the migration LOGIC, not the tree access.

use std::collections::HashSet;

use serde_json::{Map, Number, Value};
use windhawk_core_domain::{EngineSettingValue, ModId, extract_initial_settings_for_engine};

use crate::callbacks::LogLevel;
use crate::error::CoreError;
use crate::services::mods::{read_mod_settings, write_mod_settings};
use crate::session::SessionInner;

/// The mod-settings migration (the TS `setModConfig` settings half): on a fresh
/// install (no previous settings and no pre-existing config) write the new
/// settings verbatim; otherwise merge the new settings into the existing ones
/// (the union of the previous initial settings and the current stored settings,
/// the latter winning) and rewrite only if the merge added anything.
pub(super) fn migrate_mod_settings(
    session: &SessionInner,
    storage_id: &str,
    initial_settings: &Map<String, Value>,
    previous_initial_settings: Option<&Map<String, Value>>,
    config_existed: bool,
) -> Result<(), CoreError> {
    if previous_initial_settings.is_none() && !config_existed {
        write_mod_settings(session, storage_id, initial_settings)?;
        return Ok(());
    }

    // existing = {...previous, ...current}: previous first, current overrides a
    // shared key's value (keeping its position) and appends new ones - the JS
    // spread, reproduced by `serde_json`'s insertion-ordered `insert`.
    let mut existing: Map<String, Value> = Map::new();
    if let Some(prev) = previous_initial_settings {
        for (k, v) in prev {
            existing.insert(k.clone(), v.clone());
        }
    }
    for (k, v) in read_mod_settings(session, storage_id)? {
        existing.insert(k, v);
    }

    let (merged, changed) = merge_mod_settings(&existing, initial_settings);
    if changed {
        write_mod_settings(session, storage_id, &merged)?;
    }
    Ok(())
}

/// `mergeModSettings`: add each new setting whose array-prefix is not already
/// present in `existing`, returning the merged map and whether anything was
/// added.
fn merge_mod_settings(
    existing: &Map<String, Value>,
    new: &Map<String, Value>,
) -> (Map<String, Value>, bool) {
    let existing_prefixes: HashSet<String> = existing.keys().map(|k| name_prefix(k)).collect();
    let mut merged = existing.clone();
    let mut changed = false;
    for (name, value) in new {
        if !existing_prefixes.contains(&name_prefix(name)) {
            merged.insert(name.clone(), value.clone());
            changed = true;
        }
    }
    (merged, changed)
}

/// `getNamePrefix`: an array setting's identity is its name up to the first
/// `[index]` (collapsed to `[0]`), so all elements of one array share a prefix;
/// a scalar's prefix is its whole name (the TS `name.replace(/\[\d+\].*$/, '[0]')`).
fn name_prefix(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b']' {
                return format!("{}[0]", &name[..i]);
            }
        }
        i += 1;
    }
    name.to_owned()
}

/// Convert the domain engine-settings list into an insertion-ordered JSON map
/// (string/number values), the representation the migration and
/// `write_mod_settings` share.
pub(super) fn engine_items_to_map(items: Vec<(String, EngineSettingValue)>) -> Map<String, Value> {
    let mut map = Map::new();
    for (key, value) in items {
        let value = match value {
            EngineSettingValue::Int(i) => Value::Number(Number::from(i)),
            EngineSettingValue::Str(s) => Value::String(s),
        };
        map.insert(key, value);
    }
    map
}

/// The OLD stored source's engine settings, best effort (the TS try/catch
/// around `extractInitialSettingsForEngine(getSource(storageId))`): a missing
/// source is silently `None`, a read or parse failure is logged and `None`, and
/// a source with no settings block is `None`.
pub(super) fn read_previous_engine_settings(
    session: &SessionInner,
    storage_id: &str,
) -> Option<Map<String, Value>> {
    let path = session.storage().mod_source_file(storage_id);
    let text = match session.deps().files.read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.is_not_found() => return None,
        Err(e) => {
            session.log(
                LogLevel::Error,
                format!(
                    "Failed to extract previous initial settings for engine: {}",
                    e.message()
                ),
            );
            return None;
        }
    };
    // Same store-vs-local gate as the new source (a `local@` mod's old source is
    // parsed as written); the prior settings drive the reinstall migration.
    let apply_workarounds = !ModId::str_is_local(storage_id);
    match extract_initial_settings_for_engine(&text, apply_workarounds) {
        Ok(Some(items)) => Some(engine_items_to_map(items)),
        Ok(None) => None,
        Err(e) => {
            session.log(
                LogLevel::Error,
                format!("Failed to extract previous initial settings for engine: {e}"),
            );
            None
        }
    }
}
