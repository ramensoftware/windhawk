//! `services::repo`: the repository HTTP client. `fetchCatalog` reads the
//! language catalog with a default fallback on 404; `fetchRepoModSource` reads
//! a mod's source at an optional version, CRLF-normalized; `fetchModVersions`
//! reads versions.json, normalized. A leaf service: it depends on no other
//! service and holds no stored state, so its commands take no command lock.
//! This is the single client behind the contract (the TS `repoClient.ts`,
//! itself the unification of the duplicated extension and CLI fetchers); the
//! front-ends no longer know repository URLs.
//!
//! Error mapping: a transport failure or a non-ok/non-404 response is
//! `REPO_UNREACHABLE`; a 404 for a mod resource is `MOD_NOT_IN_REPO` (the
//! catalog has its own 404 semantics - language fallback); a 200 with an
//! unparsable body is `REPO_UNREACHABLE`, not a generic failure (matching the
//! TS, which maps a JSON `SyntaxError` to the repository error rather than
//! letting it surface raw).

use std::sync::Arc;

use serde_json::Value;
use windhawk_core_domain as domain;
use windhawk_core_ports::{CancelToken, Http, HttpRequest};
use windhawk_core_protocol::{
    FetchCatalogParams, FetchModVersionsParams, FetchRepoModSourceParams, ModVersionInfo,
};

use crate::dispatch::decode_params;
use crate::error::CoreError;
use crate::runtime::PreparedOp;
use crate::services::net::{CollectSink, is_success, map_http_err, repo_user_agent};
use crate::services::wire::to_value_result;
use crate::session::SessionInner;

/// The default mod repository root (the TS `'https://mods.windhawk.net/'`),
/// overridden by `debugOverrides.modsUrlRoot` (`WINDHAWK_DEBUG_MODS_URL`).
const DEFAULT_MODS_URL_ROOT: &str = "https://mods.windhawk.net/";

/// The mod repository root (`<root>`, trailing slash), honoring the
/// `modsUrlRoot` debug override. The ONE home of the `DEFAULT_MODS_URL_ROOT`
/// fallback, shared by `mods_folder_url` and `RepoEndpoint::capture` - neither
/// re-derives the override-or-default itself.
fn mods_url_root(session: &SessionInner) -> String {
    session
        .config()
        .debug_overrides
        .mods_url_root
        .clone()
        .unwrap_or_else(|| DEFAULT_MODS_URL_ROOT.to_owned())
}

/// The repository mods folder URL (`<root>mods/`). Shared with
/// `services::install`'s precompiled-DLL download, which has no `RepoEndpoint`
/// (the front-ends no longer know repository URLs).
pub(crate) fn mods_folder_url(session: &SessionInner) -> String {
    format!("{}mods/", mods_url_root(session))
}

/// The resolved repository URLs and the request user agent, captured at command
/// start so the operation body holds no session reference.
struct RepoEndpoint {
    http: Arc<dyn Http>,
    user_agent: Option<String>,
    /// The root, e.g. `https://mods.windhawk.net/` (trailing slash).
    mods_url_root: String,
    /// Debug-only: skip TLS certificate validation (see
    /// `SessionConfig::ignore_cert_errors`); always `false` in release.
    ignore_cert_errors: bool,
}

impl RepoEndpoint {
    fn capture(session: &Arc<SessionInner>) -> Self {
        Self {
            http: session.deps().http.clone(),
            user_agent: repo_user_agent(session),
            mods_url_root: mods_url_root(session),
            ignore_cert_errors: session.config().ignore_cert_errors(),
        }
    }

    /// The mods folder, e.g. `https://mods.windhawk.net/mods/`.
    fn mods_folder_url(&self) -> String {
        format!("{}mods/", self.mods_url_root)
    }

    /// GET `url`, collecting the whole body. Returns the status and the bytes;
    /// maps transport failures to `REPO_UNREACHABLE` and cancellation through.
    fn get(&self, url: &str, cancel: &CancelToken) -> Result<(u16, Vec<u8>), CoreError> {
        let mut sink = CollectSink::default();
        let request = HttpRequest {
            url: url.to_owned(),
            user_agent: self.user_agent.clone(),
            ignore_cert_errors: self.ignore_cert_errors,
        };
        let status = self
            .http
            .get(&request, cancel, &mut sink)
            .map_err(|e| map_http_err(e, format!("Failed to reach {url}"), url))?;
        Ok((status, sink.into_bytes()))
    }
}

////////////////////////////////////////////////////////////////////////////
// fetchCatalog

pub fn prepare_fetch_catalog(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: FetchCatalogParams = decode_params("fetchCatalog", params)?;
    let endpoint = RepoEndpoint::capture(session);
    Ok(PreparedOp(Box::new(move |ctx| {
        fetch_catalog(&endpoint, &params.language, ctx.cancel_token())
    })))
}

fn fetch_catalog(
    endpoint: &RepoEndpoint,
    language: &str,
    cancel: &CancelToken,
) -> Result<Value, CoreError> {
    let language_url = format!("{}catalogs/{language}.json", endpoint.mods_url_root);
    let (status, body) = endpoint.get(&language_url, cancel)?;

    // 404 on the language catalog -> fall back to the default catalog.
    let (status, body, url) = if status == 404 {
        let default_url = format!("{}catalog.json", endpoint.mods_url_root);
        let (status, body) = endpoint.get(&default_url, cancel)?;
        (status, body, default_url)
    } else {
        (status, body, language_url)
    };

    if !is_success(status) {
        return Err(CoreError::repo_unreachable(
            format!("Repository catalog fetch failed: {status}"),
            url,
        ));
    }

    // Pass the catalog JSON through verbatim (the TS `response.json() as
    // Catalog` does no reshaping); a non-JSON 200 is a repository problem.
    serde_json::from_slice::<Value>(&body).map_err(|e| {
        CoreError::repo_unreachable(format!("Repository returned non-JSON catalog: {e}"), url)
    })
}

////////////////////////////////////////////////////////////////////////////
// fetchRepoModSource

pub fn prepare_fetch_repo_mod_source(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: FetchRepoModSourceParams = decode_params("fetchRepoModSource", params)?;
    let endpoint = RepoEndpoint::capture(session);
    Ok(PreparedOp(Box::new(move |ctx| {
        let text = fetch_mod_resource(
            &endpoint,
            &mod_source_url(&endpoint, &params.mod_id, params.version.as_deref()),
            &params.mod_id,
            params.version.as_deref(),
            ctx.cancel_token(),
        )?;
        // CRLF-normalize, matching what the install flow persists to disk (the
        // TS `text.replace(/\r\n|\r|\n/g, '\r\n')`).
        Ok(Value::String(domain::normalize_crlf(&text)))
    })))
}

/// Fetch a repository mod's source at an optional version, CRLF-normalized like
/// the async `fetchRepoModSource` command persists it. Shared with
/// `services::user_data`'s import, which resolves a reference-only mod's source
/// inline on its own operation thread (import runs many installs under one
/// operation) rather than issuing a nested async `fetchRepoModSource`. A 404 is
/// `MOD_NOT_IN_REPO`; any other transport/HTTP failure is `REPO_UNREACHABLE`.
pub(crate) fn fetch_mod_source(
    session: &Arc<SessionInner>,
    mod_id: &str,
    version: Option<&str>,
    cancel: &CancelToken,
) -> Result<String, CoreError> {
    let endpoint = RepoEndpoint::capture(session);
    let text = fetch_mod_resource(
        &endpoint,
        &mod_source_url(&endpoint, mod_id, version),
        mod_id,
        version,
        cancel,
    )?;
    Ok(domain::normalize_crlf(&text))
}

fn mod_source_url(endpoint: &RepoEndpoint, mod_id: &str, version: Option<&str>) -> String {
    let folder = endpoint.mods_folder_url();
    match version {
        Some(version) => format!("{folder}{mod_id}/{version}.wh.cpp"),
        None => format!("{folder}{mod_id}.wh.cpp"),
    }
}

////////////////////////////////////////////////////////////////////////////
// fetchModVersions

pub fn prepare_fetch_mod_versions(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: FetchModVersionsParams = decode_params("fetchModVersions", params)?;
    let endpoint = RepoEndpoint::capture(session);
    Ok(PreparedOp(Box::new(move |ctx| {
        let url = format!(
            "{}{}/versions.json",
            endpoint.mods_folder_url(),
            params.mod_id
        );
        let text = fetch_mod_resource(&endpoint, &url, &params.mod_id, None, ctx.cancel_token())?;
        let versions = parse_versions(&text, &url)?;
        to_value_result("fetchModVersions", &versions)
    })))
}

/// One raw versions.json entry (the typed decode target). `version` is required -
/// a missing or non-string version fails the decode and maps to the existing
/// `REPO_UNREACHABLE` "unexpected shape". `timestamp` is decoded LENIENTLY as a
/// raw `Value` (folded to `0` below unless it is a JSON number): a present
/// non-number must NOT error, which a plain `#[serde(default)]` does not
/// guarantee (it fires only on ABSENCE), so the field is taken untyped and
/// normalized after deserialize. `is_pre_release` is DERIVED, not wire data, so
/// it is not a field here.
#[derive(serde::Deserialize)]
struct VersionEntry {
    version: String,
    #[serde(default)]
    timestamp: Value,
}

/// Parse and normalize versions.json into the contract's `ModVersionInfo`
/// list. A non-JSON or non-array body is `REPO_UNREACHABLE` (the TS maps the
/// `SyntaxError`/shape check to the repository error).
fn parse_versions(text: &str, url: &str) -> Result<Vec<ModVersionInfo>, CoreError> {
    // Parse to a Value first so a non-JSON body keeps its distinct message; the
    // typed decode then catches a non-array body or any entry
    // missing/non-string `version` as the "unexpected shape" error (both
    // verbatim).
    let parsed: Value = serde_json::from_str(text).map_err(|e| {
        CoreError::repo_unreachable(
            format!("Repository returned non-JSON for {url}: {e}"),
            url.to_owned(),
        )
    })?;
    let unexpected = || {
        CoreError::repo_unreachable(
            format!("Repository returned unexpected shape for {url}"),
            url.to_owned(),
        )
    };
    let entries: Vec<VersionEntry> = serde_json::from_value(parsed).map_err(|_| unexpected())?;

    let out = entries
        .into_iter()
        .map(|entry| {
            let timestamp = match entry.timestamp {
                Value::Number(n) => n,
                _ => serde_json::Number::from(0),
            };
            ModVersionInfo {
                is_pre_release: entry.version.contains('-'),
                version: entry.version,
                timestamp,
            }
        })
        .collect();
    Ok(out)
}

////////////////////////////////////////////////////////////////////////////

/// GET a mod resource (source or versions.json), mapping the 404 case to
/// `MOD_NOT_IN_REPO` and any other non-ok status to `REPO_UNREACHABLE` (the TS
/// `fetchModResource`). Returns the body as UTF-8 text; a body that is not
/// valid UTF-8 is a `REPO_UNREACHABLE` too - it is a truncated or corrupt
/// response, in the same class as the statuses above, and decoding it lossily
/// would hand the caller a mod source that differs from the published one.
fn fetch_mod_resource(
    endpoint: &RepoEndpoint,
    url: &str,
    mod_id: &str,
    version: Option<&str>,
    cancel: &CancelToken,
) -> Result<String, CoreError> {
    let (status, body) = endpoint.get(url, cancel)?;
    if status == 404 {
        return Err(CoreError::mod_not_in_repo(
            mod_id.to_owned(),
            version.map(str::to_owned),
        ));
    }
    if !is_success(status) {
        return Err(CoreError::repo_unreachable(
            format!("Fetch failed ({url}): {status}"),
            url.to_owned(),
        ));
    }
    String::from_utf8(body).map_err(|_| {
        CoreError::repo_unreachable(
            format!("Fetch failed ({url}): the response is not valid UTF-8"),
            url.to_owned(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // parse_versions tolerates a timestamp that is absent OR
    // present-but-not-a-number, folding both to 0. The existing
    // fetchModVersions coverage feeds only numeric timestamps, so the leniency
    // is otherwise unpinned - a typed `#[serde(default)]` swap fires only on
    // ABSENCE, so a present non-number would newly error without an explicit
    // lenient decode. This characterizes the behavior the swap keeps.
    #[test]
    fn parse_versions_folds_absent_or_non_number_timestamp_to_zero() {
        let zero = serde_json::Number::from(0);
        let url = "https://example/versions.json";

        // Absent timestamp -> 0.
        let v = parse_versions(r#"[{"version": "1.0"}]"#, url).unwrap();
        assert_eq!(v[0].timestamp, zero);

        // Present but non-number timestamps (string, null, bool) -> 0.
        let v = parse_versions(
            r#"[{"version":"1.0","timestamp":"oops"},
                {"version":"2.0","timestamp":null},
                {"version":"3.0","timestamp":true}]"#,
            url,
        )
        .unwrap();
        assert!(v.iter().all(|e| e.timestamp == zero));

        // A real numeric timestamp passes through unchanged.
        let v = parse_versions(r#"[{"version":"1.0","timestamp":1700000000}]"#, url).unwrap();
        assert_eq!(v[0].timestamp, serde_json::Number::from(1_700_000_000));
    }
}
