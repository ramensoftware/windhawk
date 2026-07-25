//! Repository handlers: the async network reads `getFeaturedMods`,
//! `getRepositoryMods`, `getRepositoryModSourceData`, and `getModVersions`.
//! Each starts a core async op (`fetchCatalog` / `fetchRepoModSource` /
//! `fetchModVersions`) and registers the per-command [`AsyncKind`] so the pump
//! turns its terminal into the reply; the catalog browse + repo source-data are
//! [`Terminal::Composite`] (a fetch then ONE follow-up), the featured subset +
//! versions are [`Terminal::Shaped`].
//!
//! The reply shaping (success AND failure) is the same per-command function on
//! the synchronous start-failure path here and the async terminal path in the
//! pump, so a command's reply representation cannot drift between them. The
//! per-message `syncCatalogToProfile`/tray notification the extension folds
//! into the catalog fetches is NOT done here - it moves to the startup refresh
//! and the profile watcher, keeping these composites a single follow-up.

use serde::Serialize;
use serde_json::{Map, Value, json};
use windhawk_core_host::HostError;
use windhawk_core_protocol::{
    FetchCatalogParams, FetchModVersionsParams, FetchRepoModSourceParams, ListInstalledModsParams,
    ParseModSourceParams, ParsedModSource,
};

use crate::commands::{app_settings, check_for_updates, language};
use crate::ipc::bridge::BridgeCtx;
use crate::ipc::outcome::{AsyncKind, AsyncOp, Completion, FollowUp, Outcome, Terminal};
use crate::ipc::reply;
use crate::shape;
use crate::shape::webview_ipc::{
    GetFeaturedModsReply, GetModVersionsReply, GetRepositoryModSourceDataReply, SourceData, to_wire,
};

/// `getFeaturedMods`: fetch the catalog, reply with the featured subset.
/// `Shaped` - the reply is a pure projection of the terminal catalog.
pub fn get_featured_mods(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let (lang, _) = settings_summary(ctx);
    start_async(
        ctx,
        "fetchCatalog",
        &FetchCatalogParams { language: lang },
        AsyncKind {
            terminal: Terminal::Shaped(featured_terminal),
            progress: None,
            effect: None,
        },
        Value::Null,
        |error, ctx_value| featured_terminal(Err(error), ctx_value),
    )
}

/// `getRepositoryMods`: fetch the catalog, then `listInstalledMods` to overlay
/// installed state. `Composite` - the reply cannot answer from the catalog
/// alone.
pub fn get_repository_mods(ctx: &BridgeCtx, _data: &Value) -> Result<Outcome, HostError> {
    let (lang, check) = settings_summary(ctx);
    start_async(
        ctx,
        "fetchCatalog",
        &FetchCatalogParams {
            language: lang.clone(),
        },
        AsyncKind {
            terminal: Terminal::Composite(Completion {
                follow_up: repo_mods_follow_up,
                merge: repo_mods_merge,
                on_failure: repo_mods_failure,
            }),
            progress: None,
            effect: None,
        },
        json!({ "language": lang, "checkForUpdates": check }),
        |_error, ctx_value| repo_mods_failure(ctx_value),
    )
}

/// `getRepositoryModSourceData`: fetch the repo source for a (optional)
/// version, then `parseModSource` on the FETCHED source. `Composite` - the
/// follow-up input is not known until the fetch completes (it IS the source).
pub fn get_repository_mod_source_data(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: FetchRepoModSourceParams = serde_json::from_value(data.clone())?;
    let (lang, _) = settings_summary(ctx);

    let mut context = Map::new();
    context.insert("modId".to_owned(), json!(params.mod_id));
    context.insert("language".to_owned(), json!(lang));
    if let Some(version) = &params.version {
        context.insert("version".to_owned(), json!(version));
    }
    let context = Value::Object(context);

    start_async(
        ctx,
        "fetchRepoModSource",
        &params,
        AsyncKind {
            terminal: Terminal::Composite(Completion {
                follow_up: repo_source_follow_up,
                merge: repo_source_merge,
                on_failure: repo_source_failure,
            }),
            progress: None,
            effect: None,
        },
        context,
        |_error, ctx_value| repo_source_failure(ctx_value),
    )
}

/// `getModVersions`: a repository mod's version history. `Shaped`.
pub fn get_mod_versions(ctx: &BridgeCtx, data: &Value) -> Result<Outcome, HostError> {
    let params: FetchModVersionsParams = serde_json::from_value(data.clone())?;
    let context = json!({ "modId": params.mod_id });
    start_async(
        ctx,
        "fetchModVersions",
        &params,
        AsyncKind {
            terminal: Terminal::Shaped(mod_versions_terminal),
            progress: None,
            effect: None,
        },
        context,
        |error, ctx_value| mod_versions_terminal(Err(error), ctx_value),
    )
}

// ---------------------------------------------------------------------------
// Shapers (shared by the sync start-failure path and the async terminal path).
// ---------------------------------------------------------------------------

/// `getFeaturedMods` reply: `{ featuredMods: <subset> | null }`. A failure yields
/// `null` (the extension's catch).
fn featured_terminal(outcome: Result<Value, HostError>, _ctx: &Value) -> Value {
    let featured_mods = match outcome {
        Ok(catalog) => shape::catalog::featured_subset(&catalog),
        Err(_) => Value::Null,
    };
    to_wire(GetFeaturedModsReply { featured_mods })
}

/// `getModVersions` reply: `{ modId, versions: [...] }`. A failure yields an empty
/// list (the extension's catch).
fn mod_versions_terminal(outcome: Result<Value, HostError>, ctx: &Value) -> Value {
    // modId always rides in the request context as a string; the empty-string fallback
    // is reached only for a malformed/absent context (never in practice) and keeps the
    // `string` contract rather than emitting null.
    let mod_id = ctx
        .get("modId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let versions = match outcome {
        Ok(versions) => versions,
        Err(_) => json!([]),
    };
    to_wire(GetModVersionsReply { mod_id, versions })
}

fn repo_mods_follow_up(_completed: &Value, context: &Value) -> FollowUp {
    let params = ListInstalledModsParams {
        language: context
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("en")
            .to_owned(),
        check_for_updates: context
            .get("checkForUpdates")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        sync_profile: false,
    };
    FollowUp {
        command: "listInstalledMods",
        params: serde_json::to_value(params).unwrap_or(Value::Null),
        stateless: false,
    }
}

fn repo_mods_merge(completed: &Value, follow_up: &Value, _ctx: &Value) -> Value {
    shape::catalog::repository_mods_overlay(completed, follow_up)
}

fn repo_mods_failure(_ctx: &Value) -> Value {
    json!({ "mods": null })
}

fn repo_source_follow_up(completed: &Value, context: &Value) -> FollowUp {
    let params = ParseModSourceParams {
        source: completed.as_str().unwrap_or_default().to_owned(),
        language: context
            .get("language")
            .and_then(Value::as_str)
            .unwrap_or("en")
            .to_owned(),
    };
    FollowUp {
        command: "parseModSource",
        params: serde_json::to_value(params).unwrap_or(Value::Null),
        stateless: true,
    }
}

fn repo_source_merge(completed: &Value, follow_up: &Value, context: &Value) -> Value {
    let parsed: Option<ParsedModSource> = serde_json::from_value(follow_up.clone()).ok();
    repo_source_reply(
        context,
        shape::source::source_data(completed.as_str(), parsed.as_ref()),
    )
}

fn repo_source_failure(context: &Value) -> Value {
    repo_source_reply(context, shape::source::source_data(None, None))
}

/// `getRepositoryModSourceData` reply: `{ modId, version?, data }`. The optional
/// `version` is echoed only when the request carried one (the extension's
/// `version: data.version`, omitted by JSON.stringify when undefined).
fn repo_source_reply(context: &Value, data: SourceData) -> Value {
    let reply = GetRepositoryModSourceDataReply {
        // modId always rides in the context as a string (see mod_versions_terminal); the
        // empty-string fallback only guards a malformed context and holds the `string`
        // contract. `version` is echoed only when the request carried a string one.
        mod_id: context
            .get("modId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        version: context
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        data,
    };
    to_wire(reply)
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// Start an async op: on a successful start return [`Outcome::Async`] (the pump
/// answers later); on a synchronous start failure produce the command's failure
/// reply inline through the SAME shaping the async terminal uses.
fn start_async<P, F>(
    ctx: &BridgeCtx,
    command: &'static str,
    params: &P,
    kind: AsyncKind,
    context: Value,
    failure: F,
) -> Result<Outcome, HostError>
where
    P: Serialize,
    F: FnOnce(HostError, &Value) -> Value,
{
    match ctx.session.invoke_async(command, params) {
        Ok(op_id) => Ok(Outcome::Async(AsyncOp {
            op_id,
            kind,
            context,
        })),
        Err(error) => {
            eprintln!("windhawk-ui: {command} could not start: {error}");
            // Shape the failure reply through the command's own failure shaper, then
            // attach the error object so a synchronous start failure surfaces like an
            // async terminal failure.
            let object = reply::error_object(&error);
            let mut data = failure(error, &context);
            reply::attach_error_object(&mut data, object);
            Ok(Outcome::Reply(data))
        }
    }
}

/// The app language and update-check flag, re-read per call (the extension caches
/// them from `getInitialAppSettings`; the cached-vs-fresh distinction is not part
/// of the protocol contract). A read failure degrades to `en` / no-check.
fn settings_summary(ctx: &BridgeCtx) -> (String, bool) {
    let settings = app_settings(ctx).ok();
    let lang = settings
        .as_ref()
        .map(language)
        .unwrap_or_else(|| "en".to_owned());
    let check = settings.as_ref().map(check_for_updates).unwrap_or(false);
    (lang, check)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn featured_terminal_shapes_success_and_failure() {
        let catalog =
            json!({ "mods": { "a": { "metadata": {}, "details": {}, "featured": true } } });
        assert_eq!(
            featured_terminal(Ok(catalog), &Value::Null),
            json!({ "featuredMods": { "a": { "metadata": {}, "details": {}, "featured": true } } })
        );
        assert_eq!(
            featured_terminal(Err(HostError::decode("x".into())), &Value::Null),
            json!({ "featuredMods": null })
        );
    }

    #[test]
    fn mod_versions_terminal_echoes_mod_id_on_both_branches() {
        let ctx = json!({ "modId": "m" });
        let versions = json!([{ "version": "1.0", "timestamp": 1, "isPreRelease": false }]);
        assert_eq!(
            mod_versions_terminal(Ok(versions.clone()), &ctx),
            json!({ "modId": "m", "versions": versions })
        );
        assert_eq!(
            mod_versions_terminal(Err(HostError::decode("x".into())), &ctx),
            json!({ "modId": "m", "versions": [] })
        );
    }

    #[test]
    fn repo_mods_composite_pieces() {
        // follow_up builds the listInstalledMods request from the context.
        let context = json!({ "language": "fr", "checkForUpdates": true });
        let fu = repo_mods_follow_up(&Value::Null, &context);
        assert_eq!(fu.command, "listInstalledMods");
        assert!(!fu.stateless);
        assert_eq!(fu.params["language"], json!("fr"));
        assert_eq!(fu.params["checkForUpdates"], json!(true));
        assert_eq!(fu.params["syncProfile"], json!(false));

        // merge overlays installed onto the catalog. The installed entry is
        // metadata-only (config null) - is_installed() is true on metadata alone, so
        // the overlay is grafted without needing a full ModConfig.
        let catalog = json!({ "mods": { "a": { "metadata": { "name": "A" }, "details": {} } } });
        let installed = json!({
            "mods": { "a": { "metadata": { "name": "A" }, "config": null, "updateAvailable": false, "userRating": 2 } },
            "loadErrors": []
        });
        let merged = repo_mods_merge(&catalog, &installed, &context);
        assert_eq!(
            merged["mods"]["a"]["repository"]["metadata"]["name"],
            json!("A")
        );
        assert_eq!(merged["mods"]["a"]["installed"]["userRating"], json!(2));

        assert_eq!(repo_mods_failure(&context), json!({ "mods": null }));
    }

    #[test]
    fn repo_source_composite_pieces() {
        let context = json!({ "modId": "m", "version": "1.2", "language": "en" });
        let fu = repo_source_follow_up(&json!("// the source"), &context);
        assert_eq!(fu.command, "parseModSource");
        assert!(fu.stateless);
        assert_eq!(fu.params["source"], json!("// the source"));
        assert_eq!(fu.params["language"], json!("en"));

        let parsed = json!({
            "metadata": { "id": "m", "name": "M" },
            "readme": "# M",
            "initialSettings": null,
            "errors": {}
        });
        let merged = repo_source_merge(&json!("// src"), &parsed, &context);
        assert_eq!(merged["modId"], json!("m"));
        // The version is echoed because the request carried one.
        assert_eq!(merged["version"], json!("1.2"));
        assert_eq!(merged["data"]["source"], json!("// src"));
        assert_eq!(merged["data"]["metadata"]["id"], json!("m"));

        // A failure yields all-null data; an absent version is omitted.
        let no_version = json!({ "modId": "m", "language": "en" });
        let failed = repo_source_failure(&no_version);
        assert_eq!(failed["modId"], json!("m"));
        assert_eq!(failed.get("version"), None);
        assert_eq!(failed["data"]["source"], Value::Null);
    }

    /// End-to-end through the REAL DLL: register the `getRepositoryModSourceData`
    /// composite exactly as the handler builds it, feed a canned fetched source as
    /// the terminal `completed` event, and let the dispatcher run the follow-up
    /// `parseModSource` FOR REAL through the stateless `GatedCore` path (no network,
    /// no session - `parseModSource` needs no app root). This covers the one seam the
    /// canned-follow-up pump tests cannot: the production stateless routing plus the
    /// real parse feeding the composite merge.
    ///
    /// The cdylib (`windhawk_core.dll`) is emitted by `cargo build`/`cargo test
    /// --workspace`, NOT by a bare `cargo test -p windhawk-ui`; build the workspace
    /// first, matching the CLI's `gated_core_load` test.
    #[test]
    fn repository_mod_source_data_composite_parses_the_fetched_source_for_real() {
        use crate::logwindow::NoopLogController;
        use crate::pump::events::dispatch_event;
        use crate::pump::ops::{OpEntry, OpRegistry};
        use crate::pump::test_support::Recorder;
        use windhawk_core_host::{GatedCore, HostError};

        let core = GatedCore::load(&built_cdylib().to_string_lossy()).expect("load the cdylib");
        let ops = OpRegistry::new();
        let rec = Recorder::default();

        ops.register(
            1,
            OpEntry {
                command: "getRepositoryModSourceData".to_owned(),
                message_id: 7,
                kind: AsyncKind {
                    terminal: Terminal::Composite(Completion {
                        follow_up: repo_source_follow_up,
                        merge: repo_source_merge,
                        on_failure: repo_source_failure,
                    }),
                    progress: None,
                    effect: None,
                },
                context: json!({ "modId": "happy-mod", "version": "1.2.3", "language": "en" }),
                cancel: None,
            },
        );

        // The production stateless follow-up seam: the REAL GatedCore.invoke_stateless.
        let follow_up = |request: &FollowUp| -> Result<Value, HostError> {
            assert!(
                request.stateless,
                "the repo source-data follow-up is stateless parseModSource"
            );
            core.invoke_stateless(request.command, &request.params)
        };

        let source = "// ==WindhawkMod==\n// @id happy-mod\n// @name Happy Mod\n\
                      // @version 1.2.3\n// @author Tester\n// @description A test mod.\n\
                      // ==/WindhawkMod==\n";
        let completed = json!({ "type": "completed", "result": source }).to_string();
        dispatch_event(
            &ops,
            &rec,
            &NoopLogController,
            &follow_up,
            &|_| unreachable!("this op names no host effect"),
            1,
            &completed,
        );

        let emitted = rec.take();
        assert_eq!(emitted.len(), 1, "expected one reply, got {emitted:?}");
        let reply = &emitted[0];
        assert_eq!(reply.command, "getRepositoryModSourceData");
        assert_eq!(reply.message_id, Some(7));
        assert_eq!(reply.data["modId"], json!("happy-mod"));
        assert_eq!(reply.data["version"], json!("1.2.3"));
        assert_eq!(reply.data["data"]["source"], json!(source));
        // The REAL parseModSource extracted the metadata from the fetched source.
        assert_eq!(reply.data["data"]["metadata"]["id"], json!("happy-mod"));
        assert_eq!(reply.data["data"]["metadata"]["name"], json!("Happy Mod"));
    }

    /// Locate the freshly built cdylib next to the test deps dir (two levels up from
    /// the unit-test binary under `target/<profile>/deps/`), as `cli/client.rs` does.
    fn built_cdylib() -> std::path::PathBuf {
        let exe = std::env::current_exe().expect("test exe path");
        let target_dir = exe
            .parent()
            .and_then(std::path::Path::parent)
            .expect("target profile dir");
        let dll = target_dir.join("windhawk_core.dll");
        assert!(
            dll.exists(),
            "expected the cdylib at {dll:?}; is the workspace built? (cargo build --workspace)"
        );
        dll
    }
}
