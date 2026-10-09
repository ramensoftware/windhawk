//! `services::user_scope`: the commands that switch the session user's own
//! configuration of a mod on and off (`setModUserScope`), creating it from the
//! global one the first time, and that delete it (`deleteModUserConfig`). While
//! it is on, the engine uses it in the user's processes and the core reads and
//! writes it; while it is off, both use the global one. Both commands answer
//! with the mod's `ModConfig` as it reads afterwards, since every
//! user-scopeable value can read differently. Portable storage has no user
//! configurations, so there both are refused.
//!
//! A user configuration exists once its tree holds `Disabled`
//! (`services::mods::resolve_scope`), and the engine may read it at any moment,
//! without the mod lock these commands hold. So the copy writes that value LAST
//! and the delete removes it FIRST: a reader sees no user configuration or a
//! whole one, never a half-written one.

use serde_json::Value;
use windhawk_core_ports::{SettingsTree, TreeValue};
use windhawk_core_protocol::{ModConfigPatch, ModIdParams, SetModUserScopeParams};

use crate::callbacks::LogLevel;
use crate::dispatch::{check_storage_id, decode_params};
use crate::error::CoreError;
use crate::services::mods::{
    ConfigTarget, ModConfigScope, SETTINGS_CHANGE_TIME, USER_SCOPED, does_config_exist,
    read_global_mod_config, read_mod_config, resolve_scope,
};
use crate::services::settings_io::{DISABLED, open_tree, write_bool, write_mod_config_patch};
use crate::services::wire::{WireResultExt, to_value_result};
use crate::session::SessionInner;

/// `setModUserScope`: with `active`, switch the user's own configuration on,
/// copying the global one into it first when there is none; without, switch it
/// off, so the global one is in use again and the user's is kept.
pub fn set_mod_user_scope(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: SetModUserScopeParams = decode_params("setModUserScope", params)?;
    check_storage_id("setModUserScope", "modId", &params.mod_id)?;
    check_not_portable(session, "setModUserScope")?;
    let mod_id = &params.mod_id;
    if !does_config_exist(session, mod_id)? {
        return Err(CoreError::mod_not_installed(mod_id));
    }

    match resolve_scope(session, mod_id)? {
        ModConfigScope::Global if params.active => {
            copy_global_config(session, mod_id, &session.user().sid)?
        }
        ModConfigScope::Global => {}
        ModConfigScope::User { sid, .. } => {
            let storage = session.storage();
            let location = ConfigTarget::User(&sid).config_tree(storage, mod_id);
            let mut tree = open_tree(storage, &location, true)?;
            write_bool(tree.as_mut(), USER_SCOPED, params.active)?;
        }
    }
    config_result(session, "setModUserScope", mod_id)
}

/// `deleteModUserConfig`: remove the user's own configuration, leaving the
/// global one in use. A user with none is a no-op.
pub fn delete_mod_user_config(session: &SessionInner, params: Value) -> Result<Value, CoreError> {
    let params: ModIdParams = decode_params("deleteModUserConfig", params)?;
    check_storage_id("deleteModUserConfig", "modId", &params.mod_id)?;
    check_not_portable(session, "deleteModUserConfig")?;
    let mod_id = &params.mod_id;
    if !does_config_exist(session, mod_id)? {
        return Err(CoreError::mod_not_installed(mod_id));
    }

    if let ModConfigScope::User { sid, .. } = resolve_scope(session, mod_id)? {
        remove_user_config(session, mod_id, &sid)?;
    }
    config_result(session, "deleteModUserConfig", mod_id)
}

fn check_not_portable(session: &SessionInner, command: &str) -> Result<(), CoreError> {
    if session.storage().portable() {
        return Err(CoreError::invalid_request(format!(
            "{command}: user configurations are not available in portable mode"
        )));
    }
    Ok(())
}

/// The mod's `ModConfig` after a command, as `getModConfig` reads it.
fn config_result(session: &SessionInner, command: &str, mod_id: &str) -> Result<Value, CoreError> {
    match read_mod_config(session, mod_id)? {
        Some(config) => to_value_result(command, &config),
        None => Err(CoreError::mod_not_installed(mod_id)),
    }
}

/// Create the user's configuration as a copy of the global one, switched on: the
/// settings, then the user-scopeable values with the global settings change
/// time, then `UserScoped`, then `Disabled`. On a failure what was written is
/// removed again, the marker first, and the failure is reported.
fn copy_global_config(session: &SessionInner, mod_id: &str, sid: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    let global = read_global_mod_config(session, mod_id)?
        .ok_or_else(|| CoreError::mod_not_installed(mod_id))?;
    let change_time = open_tree(storage, &storage.mod_config_tree(mod_id), false)?
        .get_int(SETTINGS_CHANGE_TIME)
        .wire()?;
    let settings = open_tree(storage, &storage.mod_settings_tree(mod_id), false)?
        .enum_values()
        .wire()?;
    let (_, mut scopeable) = ModConfigPatch::from(&global).split_by_scope();
    // The marker, written on its own at the end.
    scopeable.disabled = None;

    let target = ConfigTarget::User(sid);
    let config_location = target.config_tree(storage, mod_id);
    let settings_location = target.settings_tree(storage, mod_id);
    let copy = || -> Result<(), CoreError> {
        // A tree left without its marker (a copy whose cleanup failed) is no
        // configuration to anyone, so the copy starts from nothing.
        storage.backend().remove_tree(&settings_location).wire()?;
        storage.backend().remove_tree(&config_location).wire()?;

        let mut settings_tree = open_tree(storage, &settings_location, true)?;
        for (name, value) in &settings {
            set_value(settings_tree.as_mut(), name, value)?;
        }
        drop(settings_tree);

        let mut tree = open_tree(storage, &config_location, true)?;
        write_mod_config_patch(tree.as_mut(), &scopeable)?;
        if let Some(change_time) = change_time {
            tree.set_int(SETTINGS_CHANGE_TIME, change_time).wire()?;
        }
        write_bool(tree.as_mut(), USER_SCOPED, true)?;
        write_bool(tree.as_mut(), DISABLED, global.disabled)
    };

    copy().inspect_err(|error| {
        if let Err(cleanup) = remove_user_config(session, mod_id, sid) {
            session.log(
                LogLevel::Warn,
                format!(
                    "Creating the user configuration of {mod_id} failed ({error}), and removing \
                     what it wrote failed too: {cleanup}"
                ),
            );
        }
    })
}

/// Write one stored value with the type it was read with.
fn set_value(tree: &mut dyn SettingsTree, name: &str, value: &TreeValue) -> Result<(), CoreError> {
    match value {
        TreeValue::Str(s) => tree.set_string(name, s),
        TreeValue::Int(i) => tree.set_int(name, *i),
        TreeValue::Binary(b) => tree.set_binary(name, b),
    }
    .wire()
}

/// Remove a user configuration: its `Disabled` marker, then its settings and its
/// values. In registry mode the emptied `Users` key stays, since another user's
/// configuration can be being created under it at the same moment by a process
/// that holds no lock of this one's.
fn remove_user_config(session: &SessionInner, mod_id: &str, sid: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    let target = ConfigTarget::User(sid);
    let config_location = target.config_tree(storage, mod_id);
    {
        let mut tree = open_tree(storage, &config_location, true)?;
        tree.remove(DISABLED).wire()?;
    }
    storage
        .backend()
        .remove_tree(&target.settings_tree(storage, mod_id))
        .wire()?;
    storage.backend().remove_tree(&config_location).wire()
}

/// Remove every user configuration of a mod, whoever it belongs to, a
/// half-written one included.
pub(crate) fn remove_user_configs(session: &SessionInner, mod_id: &str) -> Result<(), CoreError> {
    let storage = session.storage();
    if storage.portable() {
        return Ok(());
    }
    for sid in storage
        .backend()
        .list_subtrees(&storage.mod_users_tree(mod_id))
        .wire()?
    {
        remove_user_config(session, mod_id, &sid)?;
    }
    Ok(())
}
