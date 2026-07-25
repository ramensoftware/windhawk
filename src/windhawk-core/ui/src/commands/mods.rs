//! Per-mod handlers: the reads `getInstalledMods`, `getModSourceData`,
//! `getModSettings`, `getModConfig`, and the synchronous writes
//! `setModSettings`, `updateModConfig`, `enableMod`, `deleteMod`,
//! `updateModRating`. Each parses the envelope `data` into a typed request DTO,
//! calls the host, and shapes the reply. A core failure is represented inline
//! (an empty map / `null` on a read, `succeeded: false` on a write), matching
//! the extension's `try/catch`; the only `Err` a handler propagates is a
//! malformed `data` it cannot decode, which the bridge default-shapes (the
//! one-reply invariant backstop).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    CompileInstalledModParams, InstallModParams, ListInstalledModsParams, ModConfigPatch,
    ModIdParams, ModMetadata, ParseModSourceParams, ParsedModSource, SetModEnabledParams,
    SetModRatingParams, SetModSettingsParams, UpdateModConfigParams,
};

use crate::commands::{app_language, app_settings, check_for_updates, language};
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::envelope::Envelope;
use crate::ipc::outcome::{AsyncKind, AsyncOp, Outcome, Terminal, TerminalShaper};
use crate::ipc::reply;
use crate::shape::installed::installed_mods_reply;
use crate::shape::source::mod_source_data_reply;
use crate::shape::webview_ipc::{
    EnableModReply, GetModSettingsReply, SetNewModConfig, UpdateModRatingReply, WriteReply, to_wire,
};

/// `getInstalledMods`: the installed-mods listing (metadata + config + update flag +
/// rating), with the profile sync the GUI performs. `language`/`checkForUpdates`
/// are re-read per call (see `commands::app_settings`).
pub fn get_installed_mods(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let reply = match app_settings(ctx) {
        Ok(settings) => {
            let params = ListInstalledModsParams {
                language: language(&settings),
                check_for_updates: check_for_updates(&settings),
                sync_profile: true,
            };
            match ctx.session.invoke("listInstalledMods", &params) {
                Ok(result) => {
                    let mut reply = installed_mods_reply(&result);
                    surface_load_errors(&result, &mut reply);
                    reply
                }
                Err(error) => empty_installed_mods(&error, "listInstalledMods"),
            }
        }
        Err(error) => empty_installed_mods(&error, "getInstalledMods app settings"),
    };
    Ok(Outcome::Reply(reply))
}

/// `getModConfig`: the full per-mod config, or `null` when the mod is not installed
/// (and on a core error). Forwarded untouched.
pub fn get_mod_config(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params = parse_mod_id(data)?;
    let reply = match ctx.session.invoke("getModConfig", &params) {
        Ok(config) => json!({ "modId": params.mod_id, "config": config }),
        Err(error) => {
            eprintln!(
                "windhawk-ui: getModConfig for '{}' failed: {error}",
                params.mod_id
            );
            let mut data = json!({ "modId": params.mod_id, "config": null });
            reply::attach_error(&mut data, &error);
            data
        }
    };
    Ok(Outcome::Reply(reply))
}

/// `getModSettings`: the per-mod runtime settings values, or an empty map on a core
/// error. Forwarded untouched.
pub fn get_mod_settings(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params = parse_mod_id(data)?;
    let reply = match ctx.session.invoke("getModSettings", &params) {
        Ok(settings) => to_wire(GetModSettingsReply {
            mod_id: params.mod_id,
            settings,
        }),
        Err(error) => {
            eprintln!(
                "windhawk-ui: getModSettings for '{}' failed: {error}",
                params.mod_id
            );
            let mut data = to_wire(GetModSettingsReply {
                mod_id: params.mod_id,
                settings: json!({}),
            });
            reply::attach_error(&mut data, &error);
            data
        }
    };
    Ok(Outcome::Reply(reply))
}

/// `getModSourceData`: the stored source plus the metadata/readme/initialSettings
/// extracted from it. A missing source (not installed) yields all-`null` data; the
/// parse runs session-free through the stateless `GatedCore` path.
pub fn get_mod_source_data(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params = parse_mod_id(data)?;
    let source = ctx.session.invoke_as::<String, _>("getModSource", &params);
    let parsed = source
        .as_ref()
        .ok()
        .and_then(|source| parse_mod_source(ctx, source, &app_language(ctx)));
    let mut reply = mod_source_data_reply(
        &params.mod_id,
        source.as_ref().ok().map(String::as_str),
        parsed.as_ref(),
    );
    if let Err(error) = &source {
        // A not-installed mod surfaces MOD_NOT_INSTALLED, which the front-end treats
        // as a benign absence; a real IO/registry failure surfaces to the user.
        reply::attach_error(&mut reply, error);
    }
    Ok(Outcome::Reply(reply))
}

/// `updateModConfig`: apply a `Partial<ModConfig>` patch to a mod's config. On
/// success the `setNewModConfig` event echoes the patch (so the front-end's caches
/// update without a re-read), mirroring the extension; the reply carries only the
/// `succeeded` flag. The request side is the typed `UpdateModConfigParams` (a core
/// param rename is a build error), while the event echoes the raw `config` Value
/// verbatim, matching the extension's `data.config` pass-through.
pub fn update_mod_config(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let req: UpdateModConfigRequest = serde_json::from_value(data.clone())?;
    let patch: ModConfigPatch = serde_json::from_value(req.config.clone())?;
    let params = UpdateModConfigParams {
        mod_id: req.mod_id.clone(),
        patch,
    };
    let result = match ctx.session.invoke("updateModConfig", &params) {
        Ok(_) => {
            let event = to_wire(SetNewModConfig {
                mod_id: req.mod_id.clone(),
                config: req.config.clone(),
            });
            ctx.emit.emit(Envelope::event("setNewModConfig", event));
            Ok(())
        }
        Err(error) => {
            eprintln!(
                "windhawk-ui: updateModConfig for '{}' failed: {error}",
                req.mod_id
            );
            Err(error)
        }
    };
    let reply = WriteReply {
        mod_id: req.mod_id,
        ..Default::default()
    };
    Ok(Outcome::Reply(finish_write(reply, result)))
}

/// `setModSettings`: write a mod's runtime settings map verbatim (the core clears
/// the section first - patch-by-replace, not key merge). The front-end sends the
/// whole map, so the handler forwards it untouched (no read-merge-write, unlike the
/// CLI's single-key edit). The reply carries the `succeeded` flag.
pub fn set_mod_settings(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: SetModSettingsParams = serde_json::from_value(data.clone())?;
    let result = invoke_write(ctx, "setModSettings", &params);
    let reply = WriteReply {
        mod_id: params.mod_id,
        ..Default::default()
    };
    Ok(Outcome::Reply(finish_write(reply, result)))
}

/// `enableMod`: toggle a mod enabled/disabled (`setModEnabled`, which also mirrors
/// the state into the user profile for non-local mods). Unlike the CLI there is no
/// already-in-state no-op short-circuit - the extension's handler writes
/// unconditionally and lets the engine pick up the change. The reply echoes the
/// requested `enabled` state regardless of `succeeded`, matching the extension.
pub fn enable_mod(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: SetModEnabledParams = serde_json::from_value(data.clone())?;
    let result = invoke_write(ctx, "setModEnabled", &params);
    let reply = EnableModReply {
        mod_id: params.mod_id,
        enabled: params.enable,
        ..Default::default()
    };
    Ok(Outcome::Reply(finish_write(reply, result)))
}

/// `deleteMod`: uninstall a mod (`removeMod`: config, source, DLLs, profile
/// entry). Not a development action. After the removal, sweep abandoned editor
/// workspaces: the deleted mod's workspace is now unused, so a closed one is
/// reclaimed while an open editor is spared - a no-op when no editor is wired.
/// The reply carries the `succeeded` flag.
pub fn delete_mod(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: ModIdParams = serde_json::from_value(data.clone())?;
    let result = invoke_write(ctx, "removeMod", &params);
    crate::commands::dev::sweep_abandoned_workspaces(ctx);
    let reply = WriteReply {
        mod_id: params.mod_id,
        ..Default::default()
    };
    Ok(Outcome::Reply(finish_write(reply, result)))
}

/// `updateModRating`: store the user's rating in the profile (`setModRating`; a
/// nonzero rating is stored, 0 clears it). The reply echoes the requested `rating`
/// regardless of `succeeded`, matching the extension.
pub fn update_mod_rating(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: SetModRatingParams = serde_json::from_value(data.clone())?;
    let result = invoke_write(ctx, "setModRating", &params);
    let reply = UpdateModRatingReply {
        mod_id: params.mod_id,
        rating: params.rating,
        ..Default::default()
    };
    Ok(Outcome::Reply(finish_write(reply, result)))
}

/// `installMod`: download-or-compile and install a repository mod. The
/// synchronous pre-phase parses the supplied source for metadata and reconciles
/// its id against `modId`; a parse/id failure replies inline with
/// `installedModDetails: null` (the extension's catch), no async op started. On
/// a successful start the reply comes from the op's terminal: `{ modId,
/// installedModDetails: { metadata, config } }` (the pre-parsed metadata + the
/// installed config), or `null` on failure. `compileLocally` is the app's
/// `alwaysCompileModsLocally`; the install is always tracked in the profile. When
/// `compileLocally` is set but the development tools (the compiler) are not installed,
/// the pre-phase replies `uiMissing` (like the launch entry points) and starts no op.
pub fn install_mod(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let req: InstallModRequest = serde_json::from_value(data.clone())?;
    // Read the app settings ONCE for the two values derived from them (the parse
    // language and the compile-vs-download flag) rather than invoking getAppSettings
    // for each. A read failure degrades to `en` / download, as before.
    let settings = app_settings(ctx).ok();
    let language = settings
        .as_ref()
        .map(language)
        .unwrap_or_else(|| "en".to_owned());

    let Some(metadata) = reconciled_metadata(ctx, &req.mod_source, &req.mod_id, &language) else {
        return Ok(Outcome::Reply(null_mod_details(
            &req.mod_id,
            "installedModDetails",
        )));
    };

    let compile_locally = settings
        .as_ref()
        .map(|settings| settings.always_compile_mods_locally)
        .unwrap_or(false);
    // A local compile needs the development tools (the compiler); when they are not
    // installed, reply `uiMissing` so the front-end raises the install-dev-tools modal
    // instead of starting a compile that would fail. The download-precompiled path
    // (compile_locally == false) needs no tools, so it is not gated. Mirrors the launch
    // entry points' availability gate (commands/dev/mod.rs).
    if compile_locally && !ctx.editor.launcher().is_available() {
        return Ok(Outcome::Reply(ui_missing_details(
            &req.mod_id,
            "installedModDetails",
        )));
    }
    let context = json!({ "modId": req.mod_id, "metadata": metadata });
    let params = InstallModParams {
        storage_id: req.mod_id,
        source: req.mod_source,
        metadata,
        disabled: req.disabled,
        logging_enabled: req.logging_enabled,
        compile_locally,
        track_in_profile: true,
        pch_folder: None,
        rename_from_storage_id: None,
    };
    start_mod_op(ctx, "installMod", &params, context, install_terminal)
}

/// `compileMod`: recompile an installed mod from its stored source. The
/// synchronous pre-phase reads the stored source, parses it for metadata, and
/// reconciles the id against the storage id (with the `local@` prefix stripped,
/// as the extension does); any failure replies inline with `compiledModDetails:
/// null`. On a successful start the terminal replies `{ modId,
/// compiledModDetails: { metadata, config } }` or `null`. A recompile always compiles
/// locally, so when the development tools (the compiler) are not installed the
/// pre-phase replies `uiMissing` (like the launch entry points) and starts no op.
pub fn compile_mod(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params_in: ModIdParams = serde_json::from_value(data.clone())?;
    const KEY: &str = "compiledModDetails";

    // compileInstalledMod always compiles locally, so it needs the development tools
    // (the compiler). When they are not installed, reply `uiMissing` so the front-end
    // raises the install-dev-tools modal instead of starting a compile that would fail.
    // Mirrors the launch entry points' availability gate (commands/dev/mod.rs).
    if !ctx.editor.launcher().is_available() {
        return Ok(Outcome::Reply(ui_missing_details(&params_in.mod_id, KEY)));
    }

    let source = match ctx
        .session
        .invoke_as::<String, _>("getModSource", &params_in)
    {
        Ok(source) => source,
        Err(_) => return Ok(Outcome::Reply(null_mod_details(&params_in.mod_id, KEY))),
    };
    let language = app_language(ctx);
    let Some(metadata) =
        parse_mod_source(ctx, &source, &language).and_then(|parsed| parsed.metadata)
    else {
        return Ok(Outcome::Reply(null_mod_details(&params_in.mod_id, KEY)));
    };
    // The stored id (possibly `local@<id>`) must match the source's bare `@id`.
    let expected = params_in
        .mod_id
        .strip_prefix("local@")
        .unwrap_or(&params_in.mod_id);
    if metadata.id.as_deref() != Some(expected) {
        return Ok(Outcome::Reply(null_mod_details(&params_in.mod_id, KEY)));
    }

    let context = json!({ "modId": params_in.mod_id, "metadata": metadata });
    let params = CompileInstalledModParams {
        storage_id: params_in.mod_id,
        source,
        metadata,
    };
    start_mod_op(
        ctx,
        "compileInstalledMod",
        &params,
        context,
        compile_terminal,
    )
}

/// Parse the supplied source for metadata and reconcile its `@id` against `mod_id`
/// (it must exist and match), returning the metadata on success. `None` is the
/// extension's "throw -> null reply" for a missing/mismatched id or an unparsable
/// source.
fn reconciled_metadata(
    ctx: &BridgeCtx,
    source: &str,
    mod_id: &str,
    language: &str,
) -> Option<ModMetadata> {
    let metadata = parse_mod_source(ctx, source, language).and_then(|parsed| parsed.metadata)?;
    if metadata.id.as_deref() == Some(mod_id) {
        Some(metadata)
    } else {
        None
    }
}

/// Start `installMod`/`compileInstalledMod` and register its terminal shaper. A
/// synchronous start failure replies inline through the SAME shaper (so the null
/// reply is single-sourced with the async path).
fn start_mod_op<P: Serialize>(
    ctx: &BridgeCtx,
    command: &'static str,
    params: &P,
    context: Value,
    terminal: TerminalShaper,
) -> Result<Outcome, HostError> {
    match ctx.session.invoke_async(command, params) {
        Ok(op_id) => Ok(Outcome::Async(AsyncOp {
            op_id,
            kind: AsyncKind {
                terminal: Terminal::Shaped(terminal),
                progress: None,
                effect: None,
            },
            context,
        })),
        Err(error) => {
            eprintln!("windhawk-ui: {command} could not start: {error}");
            // Shape the failure reply through the SAME terminal, then attach the
            // error object so a synchronous start failure surfaces like an async one.
            let object = reply::error_object(&error);
            let mut data = terminal(Err(error), &context);
            reply::attach_error_object(&mut data, object);
            Ok(Outcome::Reply(data))
        }
    }
}

/// `installMod` terminal reply: `{ modId, installedModDetails: { metadata, config }
/// | null }`.
fn install_terminal(outcome: Result<Value, HostError>, ctx: &Value) -> Value {
    mod_details_terminal(outcome, ctx, "installedModDetails")
}

/// `compileMod` terminal reply: `{ modId, compiledModDetails: { metadata, config }
/// | null }`.
fn compile_terminal(outcome: Result<Value, HostError>, ctx: &Value) -> Value {
    mod_details_terminal(outcome, ctx, "compiledModDetails")
}

/// The shared `{ modId, <key>: { metadata, config } | null }` terminal shape: on
/// success the pre-parsed `metadata` from the context plus the operation's `config`;
/// on failure `null` under `<key>` (the same null shape [`null_mod_details`] gives
/// the synchronous pre-op failure, so the two cannot drift).
fn mod_details_terminal(outcome: Result<Value, HostError>, ctx: &Value, key: &str) -> Value {
    let mod_id = ctx.get("modId").cloned().unwrap_or(Value::Null);
    let details = match outcome {
        Ok(result) => json!({
            "metadata": ctx.get("metadata").cloned().unwrap_or(Value::Null),
            "config": result.get("config").cloned().unwrap_or(Value::Null),
        }),
        Err(_) => Value::Null,
    };
    details_reply(mod_id, key, details)
}

/// The `{ modId, <key>: null }` reply for a synchronous pre-op failure (an
/// unparsable source or an id mismatch), where the parsed metadata is not available.
fn null_mod_details(mod_id: &str, key: &str) -> Value {
    details_reply(Value::String(mod_id.to_owned()), key, Value::Null)
}

/// The `{ modId, <key>: null, uiMissing: true }` reply for a local compile that
/// cannot run because the development tools (the compiler) are not installed. The
/// front-end turns `uiMissing` into the "install development tools" modal, exactly as
/// the launch entry points do, and starts no op. `<key>: null` keeps the shape a
/// superset of [`null_mod_details`], so the front-end's details guard still
/// short-circuits before the `uiMissing` branch runs.
fn ui_missing_details(mod_id: &str, key: &str) -> Value {
    let mut reply = null_mod_details(mod_id, key);
    if let Value::Object(map) = &mut reply {
        map.insert("uiMissing".to_owned(), Value::Bool(true));
    }
    reply
}

/// Build `{ modId, <key>: <details> }`.
fn details_reply(mod_id: Value, key: &str, details: Value) -> Value {
    let mut obj = Map::new();
    obj.insert("modId".to_owned(), mod_id);
    obj.insert(key.to_owned(), details);
    Value::Object(obj)
}

/// The `installMod` envelope `data` (`{ modId, modSource, disabled?,
/// loggingEnabled? }`, the front-end's `InstallModData`). `modSource` differs from
/// the core param `source` and the storage id is the bare `modId`, so the request
/// is decoded here and the typed `InstallModParams` is built from it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallModRequest {
    mod_id: String,
    mod_source: String,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    logging_enabled: Option<bool>,
}

/// Invoke a core write command, logging and returning a core error so the reply can
/// carry BOTH the `succeeded: false` flag and the error object (the extension's
/// `try/catch` over `succeeded`, plus the unified error surface). Only a malformed
/// request `data` (a failed DTO decode) propagates instead - to the bridge's default
/// error reply.
fn invoke_write<P: Serialize>(ctx: &BridgeCtx, command: &str, params: &P) -> Result<(), HostError> {
    ctx.session
        .invoke(command, params)
        .map(|_| ())
        .inspect_err(|error| {
            eprintln!("windhawk-ui: {command} failed: {error}");
        })
}

/// Finish a write reply: serialize the typed `base` (its echo fields - `modId`, and
/// `enabled`/`rating` where the contract echoes the requested value), stamp the
/// `succeeded` flag from the outcome, and on failure attach the error object the
/// front-end surfaces. `succeeded` is derived HERE, not by the caller, so it cannot
/// disagree with the attached error - `finish_write` is its single writer. The error
/// stays OUT of the struct so `reply::error_object` remains its single owner: the DTO
/// guards the echo shape, `succeeded` + the attached object guard the outcome.
fn finish_write<B: Serialize>(base: B, result: Result<(), HostError>) -> Value {
    let mut value = to_wire(base);
    if let Value::Object(map) = &mut value {
        map.insert("succeeded".to_owned(), Value::Bool(result.is_ok()));
    }
    if let Err(error) = &result {
        reply::attach_error(&mut value, error);
    }
    value
}

/// The empty `getInstalledMods` reply for a core failure (`{ installedMods: {} }`),
/// logged and carrying the error object the front-end surfaces.
fn empty_installed_mods(error: &HostError, what: &str) -> Value {
    eprintln!("windhawk-ui: {what} failed: {error}");
    let mut data = json!({ "installedMods": {} });
    reply::attach_error(&mut data, error);
    data
}

/// The `updateModConfig` envelope `data` (`{ modId, config }`). The front-end names
/// the patch `config`, while the core param is `patch` (`UpdateModConfigParams`), so
/// the data cannot deserialize straight into the request DTO; this keeps the raw
/// `config` Value for the `setNewModConfig` echo and the typed patch is decoded from
/// it for the invoke.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateModConfigRequest {
    mod_id: String,
    config: Value,
}

/// Parse a mod source session-free (`parseModSource` via the stateless transport)
/// at the given app language. A parse failure degrades to `None` (the shaper then
/// carries source-only data) rather than failing the reply. The language is passed
/// in so a handler that also needs other settings reads `getAppSettings` once.
fn parse_mod_source(ctx: &BridgeCtx, source: &str, language: &str) -> Option<ParsedModSource> {
    ctx.core
        .invoke_stateless_as::<ParsedModSource, _>(
            "parseModSource",
            &ParseModSourceParams {
                source: source.to_owned(),
                language: language.to_owned(),
            },
        )
        .ok()
}

/// Decode the envelope `data` into `{ modId }`. A malformed `data` is the one
/// failure a read handler propagates (the bridge default-shapes it).
fn parse_mod_id(data: &Value) -> Result<ModIdParams, HostError> {
    Ok(serde_json::from_value(data.clone())?)
}

/// Surface the per-mod metadata load errors a `listInstalledMods` result reports:
/// log each (the dev console) AND attach a summary error object to the reply so they
/// reach the user as a notification (the extension showed a message box). This is a
/// PARTIAL success - the mods that loaded are still returned - so it carries a
/// UI-side `MODS_LOAD_FAILED` code rather than a wire [`ErrorCode`].
fn surface_load_errors(list_result: &Value, reply: &mut Value) {
    let Some(errors) = list_result.get("loadErrors").and_then(Value::as_array) else {
        return;
    };
    if errors.is_empty() {
        return;
    }
    let mut details = Vec::with_capacity(errors.len());
    for error in errors {
        let mod_id = error.get("modId").and_then(Value::as_str).unwrap_or("?");
        let message = error.get("error").and_then(Value::as_str).unwrap_or("");
        eprintln!("windhawk-ui: failed to load metadata for mod '{mod_id}': {message}");
        details.push(if message.is_empty() {
            mod_id.to_owned()
        } else {
            format!("{mod_id}: {message}")
        });
    }
    let summary = format!(
        "{} mod(s) could not be loaded. {}",
        errors.len(),
        details.join("; ")
    );
    reply::attach_error_object(
        reply,
        json!({ "code": "MODS_LOAD_FAILED", "message": summary }),
    );
}
