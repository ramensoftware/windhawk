//! `services::nuke`: the `Nuked` value of a mod's global configuration, the
//! reason the mod catalog took the mod off every machine, and the one module
//! that reads or writes it. The engine loads no mod that has it, in any process,
//! whatever the mod's own enabled switch says, so the switch is never written
//! and removing the value restores the mod exactly as it was.
//!
//! The value is kept by one rule, applied to an installed mod the online data
//! names: an entry that flags the installed version writes the reason. An entry
//! that does not removes it when its flag is keyed by version, or when it
//! offers the installed version, and anything else leaves it. A flag that is a
//! reason flags every version. One keyed by version flags the versions it names
//! and clears every other, and keeps naming a bad version after a fixed one is
//! published. The catalog sync applies it here ([`apply_catalog`]); the app's
//! daily update check applies it to `versions.json` the same way. An install of
//! the version the last of them saw unflagged removes the value too
//! ([`restore_on_install`]), and enabling a mod that has it is refused
//! ([`refuse_enable`]).
//!
//! Only the global configuration holds the value. No patch carries it, so a
//! user configuration, which is created by a patch, never does.

use std::collections::BTreeMap;

use windhawk_core_domain::ModId;
use windhawk_core_ports::SettingsTree;
use windhawk_core_protocol::{CatalogModForSync, NukeFlag};

use crate::callbacks::LogLevel;
use crate::error::CoreError;
use crate::services::settings_io::{NUKED, open_tree, read_string, write_string};
use crate::services::storage::mod_config_ids;
use crate::services::wire::WireResultExt;
use crate::session::SessionInner;

/// The reason a configuration tree holds, `""` when it holds none.
pub(crate) fn stored_reason(tree: &dyn SettingsTree) -> Result<String, CoreError> {
    Ok(read_string(tree, NUKED)?.unwrap_or_default())
}

/// Refuse to enable a mod whose global configuration holds a reason, with
/// `MOD_NUKED` carrying it. The global configuration is the one read whichever
/// configuration the write targets, as the engine reads it, so a user
/// configuration is no way around the refusal.
pub(crate) fn refuse_enable(session: &SessionInner, mod_id: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    let tree = open_tree(storage, &storage.mod_config_tree(mod_id), false)?;
    let reason = stored_reason(&*tree)?;
    if reason.is_empty() {
        return Ok(());
    }
    Err(CoreError::mod_nuked(mod_id, reason))
}

/// Apply the catalog's flags to every installed mod it names: a global
/// configuration with a non-empty `LibraryFileName`, never a `local@` one. The
/// ids come from the configurations on the machine, so no id taken from the
/// catalog becomes a storage path.
///
/// Each mod is read and written under its exclusive `Mod` lock, taken and
/// released one mod at a time, and only a change is written, so a run with
/// nothing new writes nothing. A failure is logged and the next mod is tried:
/// the rule is idempotent, so the next sync, or the update check, converges.
///
/// For a mod whose offered version is flagged, `forget_offer` runs under the
/// same lock, right after the record. An install commits under that lock, so it
/// never finds the value beside a cached offer of the flagged version, which
/// [`restore_on_install`] would take for the version seen unflagged.
pub(crate) fn apply_catalog(
    session: &SessionInner,
    catalog: &BTreeMap<String, CatalogModForSync>,
    mut forget_offer: impl FnMut(&str),
) {
    let ids = match mod_config_ids(session) {
        Ok(ids) => ids,
        Err(e) => {
            session.log(
                LogLevel::Warn,
                format!(
                    "failed to list the installed mods to apply the catalog's nuked flags: {e}"
                ),
            );
            return;
        }
    };
    for mod_id in ids {
        if ModId::str_is_local(&mod_id) {
            continue;
        }
        let Some(entry) = catalog.get(&mod_id) else {
            continue;
        };
        let offer_flagged = matches!(
            entry.nuke_flag(entry.metadata.version.as_deref()),
            NukeFlag::Flagged(_)
        );
        let mod_lock = session.mod_lock(&mod_id);
        let _guard = mod_lock.write().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = apply_entry(session, &mod_id, entry) {
            session.log(
                LogLevel::Warn,
                format!("failed to apply the catalog's nuked flag to mod '{mod_id}': {e}"),
            );
        }
        if offer_flagged {
            forget_offer(&mod_id);
        }
    }
}

/// The rule for one mod and its catalog entry. The configuration is opened for
/// write only for a change, so it is never created for a mod that has none.
fn apply_entry(
    session: &SessionInner,
    mod_id: &str,
    entry: &CatalogModForSync,
) -> Result<(), CoreError> {
    let storage = session.storage();
    let location = storage.mod_config_tree(mod_id);
    let tree = open_tree(storage, &location, false)?;
    if read_string(&*tree, "LibraryFileName")?.is_none_or(|name| name.is_empty()) {
        return Ok(());
    }
    let stored = read_string(&*tree, NUKED)?;
    let installed = read_string(&*tree, "Version")?;

    let flag = entry.nuke_flag(installed.as_deref());
    match &flag {
        NukeFlag::Flagged(reason) => {
            if stored.as_deref() != Some(reason.as_str()) {
                let mut tree = open_tree(storage, &location, true)?;
                write_string(tree.as_mut(), NUKED, reason)?;
                session.log(
                    LogLevel::Info,
                    format!("mod '{mod_id}' was disabled by the Windhawk mod catalog: {reason}"),
                );
            }
        }
        NukeFlag::Clear | NukeFlag::Unlisted => {
            // A version the keys leave out is clean, whatever the entry offers.
            // Otherwise only the entry for the installed version clears the
            // value: a fix published for a bad version says nothing about the
            // copy still installed.
            let online = entry
                .metadata
                .version
                .as_deref()
                .filter(|version| !version.is_empty());
            let clean =
                flag == NukeFlag::Unlisted || (online.is_some() && installed.as_deref() == online);
            if stored.is_some() && clean {
                let mut tree = open_tree(storage, &location, true)?;
                tree.remove(NUKED).wire()?;
                session.log(
                    LogLevel::Info,
                    format!("mod '{mod_id}' was restored by the Windhawk mod catalog"),
                );
            }
        }
        NukeFlag::Malformed => session.log(
            LogLevel::Warn,
            format!("the catalog's nuked value for mod '{mod_id}' is malformed; skipping the mod"),
        ),
    }
    Ok(())
}

/// Remove the value from a mod an install commit is committing `version` of,
/// when that is `latest_seen`: the profile's cached `latestVersion`, which the
/// catalog sync and the update check record only for a version they saw
/// unflagged. So this is the rule's removal, applied when the installed version
/// changes. It never adds the value, there being no flag to go on, and another
/// version leaves it. The caller holds the mod's exclusive `Mod` lock.
pub(crate) fn restore_on_install(
    session: &SessionInner,
    mod_id: &str,
    version: &str,
    latest_seen: Option<&str>,
) -> Result<(), CoreError> {
    if version.is_empty() || latest_seen != Some(version) {
        return Ok(());
    }
    let storage = session.storage();
    let location = storage.mod_config_tree(mod_id);
    if read_string(&*open_tree(storage, &location, false)?, NUKED)?.is_none() {
        return Ok(());
    }
    open_tree(storage, &location, true)?.remove(NUKED).wire()?;
    session.log(
        LogLevel::Info,
        format!("mod '{mod_id}' was restored by installing version {version}"),
    );
    Ok(())
}
