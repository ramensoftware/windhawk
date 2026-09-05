//! `services::repo`: the repository HTTP client. `fetchCatalog` reads the
//! language catalog with a default fallback on 404; `fetchRepoModSource` reads
//! a mod's source at an optional version, CRLF-normalized; `fetchModVersions`
//! reads versions.json, normalized. A leaf service: it depends on no other
//! service and holds no stored state, so its commands take no command lock.
//! This is the single client behind the contract (the TS `repoClient.ts`,
//! itself the unification of the duplicated extension and CLI fetchers); the
//! front-ends no longer know repository URLs.
//!
//! Every request here asks for a compressed body (`accept_compression`), which
//! the transport decodes before this module sees it - the published catalog is
//! several times smaller on the wire and identical on arrival. The catalog also
//! revalidates: [`CatalogCache`] holds the last one fetched with its `ETag`, so
//! a repeat fetch sends `If-None-Match` and a `304` costs no body at all.
//!
//! The `modId` and `version` a caller names are concatenated into the URL path,
//! which sanitizes nothing, so each is held to its charset before the request is
//! built (`check_storage_id` / `check_mod_version`) - the same gate the
//! mod-keyed storage commands take.
//!
//! Error mapping: a transport failure or a non-ok/non-404 response is
//! `REPO_UNREACHABLE`; a 404 for a mod resource is `MOD_NOT_IN_REPO` (the
//! catalog has its own 404 semantics - language fallback); a 200 with an
//! unparsable body is `REPO_UNREACHABLE`, not a generic failure (matching the
//! TS, which maps a JSON `SyntaxError` to the repository error rather than
//! letting it surface raw).

use std::sync::{Arc, Mutex};

use serde_json::Value;
use windhawk_core_domain as domain;
use windhawk_core_ports::{CancelToken, Http, HttpRequest, HttpResponse};
use windhawk_core_protocol::{
    FetchCatalogParams, FetchModVersionsParams, FetchRepoModSourceParams, ModVersionInfo,
};

use crate::dispatch::{check_mod_version, check_storage_id, decode_params};
use crate::error::CoreError;
use crate::runtime::PreparedOp;
use crate::services::net::{MAX_COLLECTED_BYTES, get_collected, is_success, repo_user_agent};
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

/// The catalog HTTP cache: the URL last served, the `ETag` it came with, and
/// the bytes, so the next fetch of that URL revalidates instead of downloading
/// the whole catalog again. Session-scoped state owned by this service, the
/// shape core-internals.md section 2.2 sanctions for a cache.
///
/// The invalidation story is the repository's, not ours: an entry is handed
/// back only on a `304`, which is the server stating that these exact bytes are
/// what a `200` would carry right now. So the cache cannot serve a catalog the
/// repository has moved past, and it holds no durable state - the catalog is a
/// remote document, not one of the persistent stores that section governs.
///
/// One slot rather than a map: a session fetches one language, and the language
/// fallback settles on whichever URL actually served the catalog, so the slot
/// stays put. Switching languages evicts it, at the cost of one full fetch.
#[derive(Default)]
pub struct CatalogCache {
    entry: Mutex<Option<Arc<CatalogEntry>>>,
}

/// One cached catalog: the URL it came from, its validator, and the raw body,
/// re-parsed on a hit so a `304` yields exactly the value a `200` would have.
struct CatalogEntry {
    url: String,
    etag: String,
    body: Vec<u8>,
}

impl CatalogCache {
    /// The entry for `url`, or `None` when the slot holds another URL or
    /// nothing. Handed out as an `Arc` so a concurrent fetch that replaces the
    /// slot cannot pull the body out from under this one.
    fn get(&self, url: &str) -> Option<Arc<CatalogEntry>> {
        let entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        entry.clone().filter(|entry| entry.url == url)
    }

    fn store(&self, url: String, etag: String, body: Vec<u8>) {
        let mut entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
        *entry = Some(Arc::new(CatalogEntry { url, etag, body }));
    }
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
    /// The session's catalog validator cache, shared rather than copied so a
    /// fetch started from this capture stores into the session's slot.
    catalog_cache: Arc<CatalogCache>,
}

impl RepoEndpoint {
    fn capture(session: &Arc<SessionInner>) -> Self {
        Self {
            http: session.deps().http.clone(),
            user_agent: repo_user_agent(session),
            mods_url_root: mods_url_root(session),
            ignore_cert_errors: session.config().ignore_cert_errors(),
            catalog_cache: session.catalog_cache(),
        }
    }

    /// The mods folder, e.g. `https://mods.windhawk.net/mods/`.
    fn mods_folder_url(&self) -> String {
        format!("{}mods/", self.mods_url_root)
    }

    /// GET `url`, collecting the whole body. Returns the status and the bytes;
    /// maps transport failures to `REPO_UNREACHABLE` and cancellation through.
    fn get(&self, url: &str, cancel: &CancelToken) -> Result<(u16, Vec<u8>), CoreError> {
        let (response, body) = self.get_conditional(url, None, cancel)?;
        Ok((response.status, body))
    }

    /// GET `url` with an optional `If-None-Match` validator, collecting the
    /// whole body. Returns the response head - the caller judges the status and
    /// keeps the `ETag` - and the bytes, which a `304` leaves empty.
    fn get_conditional(
        &self,
        url: &str,
        if_none_match: Option<&str>,
        cancel: &CancelToken,
    ) -> Result<(HttpResponse, Vec<u8>), CoreError> {
        let request = HttpRequest {
            url: url.to_owned(),
            user_agent: self.user_agent.clone(),
            accept_compression: true,
            if_none_match: if_none_match.map(str::to_owned),
            ignore_cert_errors: self.ignore_cert_errors,
            max_bytes: MAX_COLLECTED_BYTES as u64,
        };
        get_collected(self.http.as_ref(), &request, cancel)
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
    let mut fetched = fetch_catalog_url(endpoint, language_url, cancel)?;

    // 404 on the language catalog -> fall back to the default catalog.
    if fetched.status == 404 {
        let default_url = format!("{}catalog.json", endpoint.mods_url_root);
        fetched = fetch_catalog_url(endpoint, default_url, cancel)?;
    }

    let CatalogFetch { url, status, body } = fetched;
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

/// One catalog GET and the bytes to read it from.
struct CatalogFetch {
    url: String,
    /// The status the fetch is judged by. A revalidated `304` reports the `200`
    /// it stands in for, since the cached bytes are exactly what a `200` would
    /// have carried - the caller has one success case, not two.
    status: u16,
    body: Vec<u8>,
}

/// Fetch one catalog URL through the validator cache: send `If-None-Match` when
/// the cache holds this URL, answer a `304` from the cached bytes, and record a
/// fresh body against the `ETag` the response carried. A response without an
/// `ETag` is served as-is and leaves the cache alone, so the next fetch is
/// unconditional - which is the pre-cache behavior, not a failure.
fn fetch_catalog_url(
    endpoint: &RepoEndpoint,
    url: String,
    cancel: &CancelToken,
) -> Result<CatalogFetch, CoreError> {
    let cached = endpoint.catalog_cache.get(&url);
    let validator = cached.as_ref().map(|entry| entry.etag.as_str());
    let (response, body) = endpoint.get_conditional(&url, validator, cancel)?;

    if response.status == 304
        && let Some(entry) = cached
    {
        return Ok(CatalogFetch {
            url,
            status: 200,
            body: entry.body.clone(),
        });
    }

    if is_success(response.status)
        && let Some(etag) = response.etag
    {
        endpoint
            .catalog_cache
            .store(url.clone(), etag, body.clone());
    }

    Ok(CatalogFetch {
        url,
        status: response.status,
        body,
    })
}

////////////////////////////////////////////////////////////////////////////
// fetchRepoModSource

pub fn prepare_fetch_repo_mod_source(
    session: &Arc<SessionInner>,
    params: Value,
) -> Result<PreparedOp, CoreError> {
    let params: FetchRepoModSourceParams = decode_params("fetchRepoModSource", params)?;
    check_storage_id("fetchRepoModSource", "modId", &params.mod_id)?;
    check_mod_version(
        "fetchRepoModSource",
        "version",
        params.version.as_deref().unwrap_or_default(),
    )?;
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
    check_storage_id("fetchModVersions", "modId", &params.mod_id)?;
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
