//! The precompiled-download arm of `installMod`: the install-side sibling of
//! `compiler::compile_mod`. Fetches each architecture's DLL and the
//! `versions.json` gate from the repository, enforces `minWindhawkVersion`,
//! then writes the DLLs into the pending set. Stays IN install (not `repo`): it
//! returns a `CompileOutput`, builds a `PendingHandle`, and writes via the
//! `Files` port - the download branch of `install_mod_body`'s
//! compile-or-download decision. Its subfolder set is the shared
//! `domain::subfolders_for_arch` (request order, deduped, unknown-arch
//! skipped); unlike cleanup, its fetch order is OBSERVABLE - the loop fetches
//! subfolders sequentially and the first failure names its subfolder - so it
//! relies on that shared order being request order.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use windhawk_core_domain::{
    compiled_dll_name, is_update_available, lcg_next_six, lcg_seed, subfolders_for_arch,
};
use windhawk_core_ports::{CancelToken, Files, Http, HttpRequest};

use crate::error::CoreError;
use crate::pending::PendingHandle;
use crate::runtime::OpContext;
use crate::services::compiler::CompileOutput;
use crate::services::net::{MAX_COLLECTED_BYTES, get_collected, is_success, repo_user_agent};
use crate::services::wire::file_err;
use crate::session::SessionInner;

/// Download a mod's precompiled per-architecture DLLs into the pending set (the
/// TS `modFiles.downloadPrecompiledMod`): collect each architecture's DLL and
/// `versions.json` from the repository, enforce the `minWindhawkVersion` gate,
/// then write the DLLs. The repository folder URL is core-internal (derived
/// from `debugOverrides.modsUrlRoot` / the default root, like `services::repo`).
/// A non-2xx DLL/versions.json response is `REPO_UNREACHABLE`; a write failure
/// unlinks the partials and is `IO_FAILED`; cancellation propagates.
pub(super) fn download_precompiled_mod(
    session: &Arc<SessionInner>,
    mod_id: &str,
    version: &str,
    architectures: &[String],
    ctx: &OpContext,
) -> Result<CompileOutput, CoreError> {
    let arm64_enabled = session.arm64_enabled();
    let subfolders = subfolders_for_arch(architectures, arm64_enabled);
    if subfolders.is_empty() {
        return Err(CoreError::internal(
            "The current architecture is not supported",
        ));
    }

    let mods_folder = crate::services::repo::mods_folder_url(session);
    let user_agent = repo_user_agent(session);
    let ignore_cert_errors = session.config().ignore_cert_errors();
    let http = session.deps().http.clone();

    // Collect each architecture's DLL into memory, checking the status (the TS
    // fetches all in parallel; sequential here is observably the same).
    let mut bodies: Vec<(&'static str, Vec<u8>)> = Vec::with_capacity(subfolders.len());
    for &subfolder in &subfolders {
        let url = format!("{mods_folder}{mod_id}/{version}_{subfolder}.dll");
        let (status, body) = http_get(
            http.as_ref(),
            &url,
            user_agent.as_deref(),
            ignore_cert_errors,
            ctx.cancel_token(),
        )?;
        if !is_success(status) {
            return Err(CoreError::repo_unreachable(
                format!("Failed to download {subfolder} DLL: {status}"),
                url,
            ));
        }
        bodies.push((subfolder, body));
    }

    // versions.json -> the minWindhawkVersion gate.
    let versions_url = format!("{mods_folder}{mod_id}/versions.json");
    let (vstatus, vbody) = http_get(
        http.as_ref(),
        &versions_url,
        user_agent.as_deref(),
        ignore_cert_errors,
        ctx.cancel_token(),
    )?;
    if !is_success(vstatus) {
        return Err(CoreError::repo_unreachable(
            format!("Failed to download versions.json: {vstatus}"),
            versions_url,
        ));
    }
    if let Some(min) = find_min_windhawk_version(&vbody, version) {
        let current = session.config().windhawk_version.as_deref();
        // The gate fails when the installed version precedes the required one by
        // SemVer precedence (the TS `semver.lt(currentVersion, requiredVersion)`),
        // so a pre-release does not satisfy a requirement for its own release.
        if is_update_available(current, Some(&min)) {
            let current_str = current.unwrap_or_default();
            return Err(CoreError::internal(format!(
                "Mod version {version} requires Windhawk {min} or later, but current version is {current_str}"
            )));
        }
    }

    // Write the DLLs (registered in the pending set). On a write failure unlink
    // the partials (the TS cleanup) and surface IO_FAILED.
    let files = session.deps().files.clone();
    let engine_mods_dir = session.storage().engine_mods_dir();

    let mut pending = PendingHandle::new(session.pending());
    let target_dll_name = reserve_dll_name(
        files.as_ref(),
        &mut pending,
        &engine_mods_dir,
        mod_id,
        version,
        &subfolders,
        session.deps().clock.now_ms(),
    );
    for (subfolder, body) in &bodies {
        let path = engine_mods_dir.join(subfolder).join(&target_dll_name);
        if let Err(e) = files.write_atomic(&path, body) {
            pending.unlink_all(files.as_ref());
            return Err(file_err(e));
        }
    }

    Ok(CompileOutput {
        target_dll_name,
        pending,
        // A precompiled download runs no compiler, so it has no warnings.
        warnings: String::new(),
    })
}

/// GET a small repository resource fully into memory (the precompiled-download
/// path), mapping transport failures to `REPO_UNREACHABLE` and cancellation
/// through. The caller checks the status; a 404 is the caller's to interpret.
fn http_get(
    http: &dyn Http,
    url: &str,
    user_agent: Option<&str>,
    ignore_cert_errors: bool,
    cancel: &CancelToken,
) -> Result<(u16, Vec<u8>), CoreError> {
    let request = HttpRequest {
        url: url.to_owned(),
        user_agent: user_agent.map(str::to_owned),
        accept_compression: true,
        if_none_match: None,
        ignore_cert_errors,
        max_bytes: MAX_COLLECTED_BYTES as u64,
    };
    let (response, body) = get_collected(http, &request, cancel)?;
    Ok((response.status, body))
}

/// The `minWindhawkVersion` of the matching `versions.json` entry, if present
/// and truthy (the TS `versionsJsonText.find(v => v.version === version)
/// ?.minWindhawkVersion`). An unparseable or non-array body skips the gate.
fn find_min_windhawk_version(versions_json: &[u8], version: &str) -> Option<String> {
    let parsed: Value = serde_json::from_slice(versions_json).ok()?;
    for entry in parsed.as_array()? {
        if entry.get("version").and_then(Value::as_str) == Some(version) {
            return entry
                .get("minWindhawkVersion")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        }
    }
    None
}

/// A name for the downloaded DLLs colliding with neither a currently-present
/// compiled DLL nor another in-flight operation's reservation, taken for this
/// operation in `pending` (the TS `randomIntFromInterval(100000, 999999)`
/// suffix). The same two checks the compiler's name generator makes.
///
/// The suffix is derived from the `Clock` port like the compiler's - no new
/// randomness dependency, and deterministic under the test clock - so two
/// operations on one mod that read the same millisecond derive the same one.
/// Both then run unlocked with nothing written yet, so the reservation is what
/// separates them: the loop steps the domain LCG until it holds a name no other
/// operation is already writing.
///
/// The on-disk check answers the other direction, and matters here because the
/// reservation doubles as the unlink list: reinstalling a version whose DLL
/// already carries the drawn suffix would put an artifact this operation never
/// wrote under its control, to be overwritten while it may be loaded and then
/// deleted by `unlink_all` on a write failure or a cancel.
fn reserve_dll_name(
    files: &dyn Files,
    pending: &mut PendingHandle,
    engine_mods_dir: &Path,
    mod_id: &str,
    version: &str,
    subfolders: &[&str],
    seed_ms: i64,
) -> String {
    let mut state = lcg_seed(seed_ms);
    loop {
        let name = compiled_dll_name(mod_id, version, lcg_next_six(&mut state));
        let paths: Vec<PathBuf> = subfolders
            .iter()
            .map(|s| engine_mods_dir.join(s).join(&name))
            .collect();
        if !paths.iter().any(|p| files.exists(p)) && pending.claim_all(paths) {
            return name;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending::PendingArtifacts;
    use windhawk_core_testkit::FakeFiles;

    const SEED_MS: i64 = 1_700_000_000_000;
    const FIXTURE_MODS_DIR: &str = "C:\\fixture\\AppData\\Engine\\Mods";

    #[test]
    fn a_name_another_operation_reserved_is_not_handed_out_again() {
        // The suffix comes from the clock, so two downloads of one mod at one
        // version started in the same millisecond derive the same name while
        // both run unlocked with nothing written yet.
        let set = Arc::new(PendingArtifacts::new());
        let files = FakeFiles::new();
        let dir = Path::new(FIXTURE_MODS_DIR);
        let subfolders = ["64"];

        let mut first = PendingHandle::new(set.clone());
        let a = reserve_dll_name(
            &files,
            &mut first,
            dir,
            "test-mod",
            "1.0",
            &subfolders,
            SEED_MS,
        );
        let mut second = PendingHandle::new(set.clone());
        let b = reserve_dll_name(
            &files,
            &mut second,
            dir,
            "test-mod",
            "1.0",
            &subfolders,
            SEED_MS,
        );

        assert_ne!(a, b, "one clock millisecond must not yield one DLL name");
        assert!(set.contains(&dir.join("64").join(&a)));
        assert!(set.contains(&dir.join("64").join(&b)));
    }

    #[test]
    fn a_name_a_present_dll_occupies_is_not_handed_out() {
        // Reinstalling a version whose DLL already carries the drawn suffix:
        // the name must move on rather than reserve, and so line up for
        // deletion, a file this operation never wrote. A hit in ANY subfolder
        // rejects the name, since the reservation covers all of them.
        let set = Arc::new(PendingArtifacts::new());
        let files = FakeFiles::new();
        let dir = Path::new(FIXTURE_MODS_DIR);
        let subfolders = ["32", "64"];

        // The name this clock reading would otherwise hand out.
        let mut probe = PendingHandle::new(set.clone());
        let taken = reserve_dll_name(
            &files,
            &mut probe,
            dir,
            "test-mod",
            "1.0",
            &subfolders,
            SEED_MS,
        );
        drop(probe);
        files.seed(dir.join("32").join(&taken), b"installed".to_vec());

        let mut pending = PendingHandle::new(set.clone());
        let name = reserve_dll_name(
            &files,
            &mut pending,
            dir,
            "test-mod",
            "1.0",
            &subfolders,
            SEED_MS,
        );

        assert_ne!(name, taken, "a DLL already on disk must not be reserved");
        assert!(!set.contains(&dir.join("32").join(&taken)));
        assert!(set.contains(&dir.join("32").join(&name)));
        assert!(set.contains(&dir.join("64").join(&name)));
    }

    // Download's subfolder set and request order are `domain::subfolders_for_arch`
    // (tested in domain::compile_targets), shared with cleanup. Download's
    // SEQUENTIAL fetch and first-failure subfolder error are pinned end-to-end.

    // find_min_windhawk_version SKIPS a malformed entry and keeps scanning,
    // unlike parse_versions which ERRORS the whole fetch on a
    // missing/non-string `version`. A single strict Vec<Dto> decode shared by
    // both would fail on a malformed SIBLING and skip the gate entirely
    // (`.ok()?`), flipping an installer rejection into a silent pass. These pin
    // the skip-vs-error divergence and the minWindhawkVersion string tolerance
    // (the end-to-end gate test feeds one clean entry, so both are otherwise
    // unpinned).
    #[test]
    fn min_windhawk_version_skips_a_malformed_sibling_and_still_gates() {
        // A malformed sibling (no `version`) precedes the well-formed matching
        // entry carrying a blocking minWindhawkVersion: the gate still fires.
        let body = br#"[{"timestamp": 123},
                        {"version": "1.0", "minWindhawkVersion": "2.0.0"}]"#;
        assert_eq!(
            find_min_windhawk_version(body, "1.0"),
            Some("2.0.0".to_owned())
        );

        // A sibling whose `version` is a non-string is likewise skipped, not an
        // error, and scanning continues to the match.
        let body = br#"[{"version": 5, "minWindhawkVersion": "9.9.9"},
                        {"version": "1.0", "minWindhawkVersion": "2.0.0"}]"#;
        assert_eq!(
            find_min_windhawk_version(body, "1.0"),
            Some("2.0.0".to_owned())
        );
    }

    #[test]
    fn min_windhawk_version_tolerates_a_non_string_or_empty_gate_field() {
        // Present-but-non-string or empty minWindhawkVersion on the MATCHING
        // entry -> no gate (None); preserved across the typed-decode swap.
        assert_eq!(
            find_min_windhawk_version(br#"[{"version": "1.0"}]"#, "1.0"),
            None
        );
        assert_eq!(
            find_min_windhawk_version(br#"[{"version": "1.0", "minWindhawkVersion": ""}]"#, "1.0"),
            None
        );
        assert_eq!(
            find_min_windhawk_version(br#"[{"version": "1.0", "minWindhawkVersion": 3}]"#, "1.0"),
            None
        );
        // An unparseable body skips the gate entirely.
        assert_eq!(find_min_windhawk_version(b"not json", "1.0"), None);
    }
}
