//! `services::update`: the installer runs - `startUpdate` (self-update) and
//! `startInstallDevTools` (install the optional development tools) - download
//! the Windhawk installer with progress events and cancellation, then launch it
//! detached (the "installing" transition). Both share this leaf service and
//! differ in the installer flags and which release they download (update pins
//! the highest cached latest version, falling back to `latest` only when none is
//! cached; dev-tools pins this build's own version, a same-version reinstall).
//! The single-flight `Update` lock is taken by the async dispatch
//! path, not here, and is shared across both (they run the same installer,
//! which restarts Windhawk); by the time the operation body runs, this session
//! owns the in-flight slot.
//!
//! The download is collected in memory and written once on success - the
//! observable behavior (progress events, the final installer file, cleanup on
//! cancel/error) matches the TS streaming writer without a streaming-file port.
//! Cleanup is best effort and runs on every exit path, like the TS `finally`:
//! on success the installer is already running and locks its file, so the
//! delete fails and is ignored; on cancel/error there is nothing on disk to
//! leak (the bytes never left memory). "Point of no return": once the detached
//! installer is launched it restarts Windhawk and closes this process.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use windhawk_core_domain::{Version, is_update_available};
use windhawk_core_ports::{DetachedRequest, Files, Http, HttpRequest, HttpSink, Processes};

use crate::error::CoreError;
use crate::runtime::{OpContext, PreparedOp};
use crate::services::net::{BoundedBody, is_success, map_http_err};
use crate::services::profile::resolved_latest_versions;
use crate::services::wire::WireResultExt;
use crate::session::SessionInner;

/// The self-update fallback URL: the `windhawk_setup.exe` asset of the GitHub
/// `latest` release, used only when no cached latest version is available to pin
/// an explicit `releases/download/<version>/` URL (a fresh install that has not
/// synced the catalog, or an unreadable profile). Overridden by
/// `debugOverrides.updateUrl` (`WINDHAWK_DEBUG_UPDATE_URL`).
const UPDATE_INSTALLER_URL: &str =
    "https://github.com/ramensoftware/windhawk/releases/latest/download/windhawk_setup.exe";

/// A version-pinned GitHub release download URL is
/// `{PREFIX}{version}{SUFFIX}` - the same shape as [`DEVTOOLS_INSTALLER_URL`].
const INSTALLER_URL_PREFIX: &str = "https://github.com/ramensoftware/windhawk/releases/download/";
const INSTALLER_URL_SUFFIX: &str = "/windhawk_setup.exe";

/// The `windhawk_setup.exe` download URL pinned to an explicit release tag. The
/// tag lands in the URL path verbatim, so the caller must have accepted it with
/// [`Version::str_is_valid`].
fn installer_url_for_version(version: &str) -> String {
    format!("{INSTALLER_URL_PREFIX}{version}{INSTALLER_URL_SUFFIX}")
}

/// The dev-tools installer URL: pinned to this build's own release
/// (`CARGO_PKG_VERSION`, which the GitHub release tag matches exactly), NOT
/// `latest`. Adding the dev tools is a same-version reinstall
/// (`InstallerMode::DevTools` -> `/AUTO_REINSTALL`); fetching `latest` could pull
/// a newer installer and turn the reinstall into an upgrade, defeating the
/// auto-selected reinstall. Overridden by `debugOverrides.updateUrl` like the
/// update URL.
const DEVTOOLS_INSTALLER_URL: &str = concat!(
    "https://github.com/ramensoftware/windhawk/releases/download/",
    env!("CARGO_PKG_VERSION"),
    "/windhawk_setup.exe"
);

/// The installer file name inside the private temp folder.
const INSTALLER_FILE_NAME: &str = "windhawk_setup.exe";

/// Which installer run a download/launch performs. Both share the single-flight
/// lock and download machinery; they differ in the command-line flags and the
/// default download URL.
#[derive(Clone, Copy)]
enum InstallerMode {
    /// `startUpdate`: a self-update.
    Update,
    /// `startInstallDevTools`: install the optional development tools (the
    /// compiler + VSCodium UI) into the current install.
    DevTools,
}

impl InstallerMode {
    /// The mode-specific NSIS flags, minus the portable `/PORTABLE ... /LANG /D=`
    /// wrapper the caller adds.
    fn flags(self) -> &'static str {
        match self {
            InstallerMode::Update => "/AUTO_UPDATE",
            // A same-version reinstall that adds the dev-tools component:
            // `/AUTO_REINSTALL` auto-selects the reinstall option on the installer's
            // reinstall page (the version is unchanged), and `/DEVTOOLS` forces the
            // dev-tools component on for the silent run (windhawk_setup.nsi).
            InstallerMode::DevTools => "/AUTO_REINSTALL /DEVTOOLS",
        }
    }
}

/// A resolved installer download: the URL to fetch, plus the release version it
/// pins when known. `version` is `None` for the `latest` fallback and the debug
/// override (an arbitrary URL of unknown version); it is reported in the
/// operation's completion result so a caller can cite exactly what was pulled.
struct ResolvedInstaller {
    url: String,
    version: Option<String>,
}

/// The installer download for this run. `debugOverrides.updateUrl` wins for both
/// modes (version unknown); otherwise a dev-tools reinstall keeps its build-pinned
/// [`DEVTOOLS_INSTALLER_URL`] and a self-update pins the highest cached latest
/// version via [`update_installer_url`].
fn resolve_installer_url(session: &SessionInner, mode: InstallerMode) -> ResolvedInstaller {
    if let Some(url) = session.config().debug_overrides.update_url.clone() {
        return ResolvedInstaller { url, version: None };
    }
    match mode {
        InstallerMode::Update => update_installer_url(session),
        InstallerMode::DevTools => ResolvedInstaller {
            url: DEVTOOLS_INSTALLER_URL.to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        },
    }
}

/// The self-update installer pinned to the highest known cached latest version -
/// the greater of the (pre-release-folded) stable and bleeding-edge channels, so
/// it never offers less than any channel advertised and mirrors the newest-release
/// target the `latest` pointer resolved to. Falls back to [`UPDATE_INSTALLER_URL`]
/// (version unknown) when nothing well-formed is cached; best effort, so a profile
/// read failure degrades to the fallback rather than failing the update.
fn update_installer_url(session: &SessionInner) -> ResolvedInstaller {
    let (latest, latest_be) = resolved_latest_versions(session).unwrap_or((None, None));
    let target = [latest, latest_be]
        .into_iter()
        .flatten()
        // The cached versions come from the catalog, and the pinned one is spliced
        // into the URL path, so a channel carrying anything but a well-formed
        // version cannot aim the download - it drops out and the remaining channel
        // (or the `latest` fallback) decides the target.
        .filter(|v| Version::str_is_valid(v))
        .reduce(|acc, v| {
            if is_update_available(Some(&acc), Some(&v)) {
                v
            } else {
                acc
            }
        });
    match target {
        Some(version) => ResolvedInstaller {
            url: installer_url_for_version(&version),
            version: Some(version),
        },
        None => ResolvedInstaller {
            url: UPDATE_INSTALLER_URL.to_owned(),
            version: None,
        },
    }
}

/// `startUpdate`: download and launch the installer for a self-update.
pub fn prepare_start_update(
    session: &Arc<SessionInner>,
    _params: Value,
) -> Result<PreparedOp, CoreError> {
    prepare_installer_op(session, InstallerMode::Update)
}

/// `startInstallDevTools`: download and launch the installer to add the optional
/// development tools to the current install. Shares the update download/launch
/// machinery and the single-flight `Update` lock; it downloads this build's
/// pinned release (not `latest`) and runs it with the reinstall flags.
pub fn prepare_start_install_devtools(
    session: &Arc<SessionInner>,
    _params: Value,
) -> Result<PreparedOp, CoreError> {
    prepare_installer_op(session, InstallerMode::DevTools)
}

fn prepare_installer_op(
    session: &Arc<SessionInner>,
    mode: InstallerMode,
) -> Result<PreparedOp, CoreError> {
    let http = session.deps().http.clone();
    let files = session.deps().files.clone();
    let processes = session.deps().processes.clone();
    let ResolvedInstaller {
        url: installer_url,
        version: installer_version,
    } = resolve_installer_url(session, mode);
    let ignore_cert_errors = session.config().ignore_cert_errors();
    let portable = session.storage().portable();
    let app_root_path = session.storage().info().app_root_path.clone();

    Ok(PreparedOp(Box::new(move |ctx| {
        let folder = files.create_temp_dir("windhawk_update_").wire()?;
        let installer_path = folder.join(INSTALLER_FILE_NAME);
        let plan = UpdatePlan {
            installer_url: &installer_url,
            installer_version: installer_version.as_deref(),
            installer_path: &installer_path,
            app_root_path: &app_root_path,
            portable,
            ignore_cert_errors,
            mode,
        };
        let result = download_and_launch(
            http.as_ref(),
            files.as_ref(),
            processes.as_ref(),
            &plan,
            ctx,
        );
        // Best-effort cleanup on every exit path (the TS `finally`). On the path
        // that launched it the installer is running out of the folder, holding
        // the one file in it open, so this routinely removes nothing - which is
        // why the folder is given up rather than merely removed.
        let _ = files.release_temp_dir(&folder);
        result
    })))
}

/// The resolved inputs for one installer download+launch: the installer URL and
/// the temp path to write it to, the release version the URL pins (reported in
/// the completion result, `None` when unknown), the app root for the portable
/// `/D=` target, and the two flags. Named fields make the two `&str` paths and
/// the two bools non-swappable; the ports and `OpContext` stay separate
/// arguments.
struct UpdatePlan<'a> {
    installer_url: &'a str,
    installer_version: Option<&'a str>,
    installer_path: &'a Path,
    app_root_path: &'a str,
    portable: bool,
    ignore_cert_errors: bool,
    mode: InstallerMode,
}

fn download_and_launch(
    http: &dyn Http,
    files: &dyn Files,
    processes: &dyn Processes,
    plan: &UpdatePlan,
    ctx: &OpContext,
) -> Result<Value, CoreError> {
    let mut sink = DownloadSink::new(ctx);
    let request = HttpRequest {
        url: plan.installer_url.to_owned(),
        user_agent: None,
        // The progress percentage and the truncation check below both read the
        // announced Content-Length, which a decoded transfer does not report -
        // so this download stays uncompressed. It costs nothing: the installer
        // is an already-compressed binary that no server usefully encodes.
        accept_compression: false,
        if_none_match: None,
        ignore_cert_errors: plan.ignore_cert_errors,
        max_bytes: MAX_INSTALLER_BYTES as u64,
    };
    let status = http
        .get(&request, ctx.cancel_token(), &mut sink)
        .map_err(|e| {
            map_http_err(
                e,
                "Failed to download update".to_owned(),
                plan.installer_url,
            )
        })?
        .status;
    if !is_success(status) {
        return Err(CoreError::repo_unreachable(
            format!("Failed to download update: {status}"),
            plan.installer_url.to_owned(),
        ));
    }
    if sink.over_limit() {
        return Err(CoreError::repo_unreachable(
            format!("Failed to download update: response exceeds {MAX_INSTALLER_BYTES} bytes"),
            plan.installer_url.to_owned(),
        ));
    }
    // A body that ends early without the transport reporting a failure would be
    // written and launched as a truncated installer, so refuse one that does not
    // match the announced length. Only a server that announced a length can be
    // checked this way - a chunked response carries none, and the transfer is
    // taken as complete.
    if let Some(total) = sink.total
        && sink.downloaded != total
    {
        return Err(CoreError::repo_unreachable(
            format!(
                "Failed to download update: got {} of {total} bytes",
                sink.downloaded
            ),
            plan.installer_url.to_owned(),
        ));
    }
    // The TS reports 100% on download finish, before the installing event.
    ctx.emit_progress(json!({ "progress": 100 }));

    files
        .write_atomic(plan.installer_path, &sink.into_body())
        .wire()?;

    // The transfer polls the cancel token, but nothing after it does, and the
    // launch below restarts Windhawk - so a cancel that lands between the last
    // chunk and the launch has to be caught here or not at all. This is the
    // last point that can happen: `installing` announces the point of no
    // return, and the bytes written above are the temp copy the cleanup drops.
    ctx.check_canceled()?;

    ctx.emit_installing();

    // NSIS requires /D to be last and unquoted even with spaces, so the whole
    // tail is one verbatim argument (the TS windowsVerbatimArguments).
    let raw_args = if plan.portable {
        format!(
            "/PORTABLE {} /LANG=1033 /D={}",
            plan.mode.flags(),
            plan.app_root_path
        )
    } else {
        plan.mode.flags().to_owned()
    };
    processes
        .spawn_detached(&DetachedRequest {
            program: plan.installer_path.to_string_lossy().into_owned(),
            raw_args,
        })
        .map_err(|e| CoreError::internal(format!("Failed to start installer: {e}")))?;

    // The completion result cites the pinned version so a caller (the CLI's
    // `update run`) reports exactly what was pulled, without re-deriving it;
    // `null` when the version is unknown (fallback or debug override).
    Ok(json!({ "version": plan.installer_version }))
}

/// The byte budget for the installer download, both as the request's
/// `max_bytes` (which stops the transfer) and as what the sink keeps. The
/// published `windhawk_setup.exe` is around 12 MB, so the budget sits an order
/// of magnitude above any real installer and only stops a server that keeps
/// sending.
const MAX_INSTALLER_BYTES: usize = 256 * 1024 * 1024;

/// Accumulates the installer in memory and reports download progress as whole
/// percentages, emitting an event only when the percentage changes (the TS
/// `lastReportedProgress` throttle). The error-page body of a non-2xx response
/// is dropped (the service checks the status afterward).
struct DownloadSink<'a> {
    ctx: &'a OpContext,
    success: bool,
    total: Option<u64>,
    downloaded: u64,
    last_reported: i64,
    body: BoundedBody,
}

impl<'a> DownloadSink<'a> {
    fn new(ctx: &'a OpContext) -> Self {
        Self {
            ctx,
            success: false,
            total: None,
            downloaded: 0,
            last_reported: -1,
            body: BoundedBody::new(MAX_INSTALLER_BYTES),
        }
    }

    fn over_limit(&self) -> bool {
        self.body.over_limit()
    }

    fn into_body(self) -> Vec<u8> {
        self.body.into_bytes()
    }
}

impl HttpSink for DownloadSink<'_> {
    fn on_response(&mut self, status: u16, content_length: Option<u64>) {
        self.success = is_success(status);
        self.total = content_length;
        self.body.reserve(content_length);
    }

    fn on_chunk(&mut self, data: &[u8]) {
        if !self.success || !self.body.push(data) {
            return;
        }
        self.downloaded += data.len() as u64;
        let percent = match self.total {
            Some(total) if total > 0 => (self.downloaded.saturating_mul(100) / total) as i64,
            _ => 0,
        };
        if percent != self.last_reported {
            self.last_reported = percent;
            self.ctx.emit_progress(json!({ "progress": percent }));
        }
    }
}
