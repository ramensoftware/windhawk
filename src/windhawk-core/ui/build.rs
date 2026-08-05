//! Tauri build hook. It also guarantees a `frontendDist` exists before
//! `tauri::generate_context!` embeds it: the real front-end `dist/` is pulled
//! into the git-ignored `assets/` by the front-end asset sync, but the crate
//! must still compile in a checkout that has not run that sync (CI, a fresh
//! clone). So if `assets/` has no `index.html`, write a minimal placeholder
//! page - enough for the context macro to embed and for the window to load a
//! "not synced" notice. A real sync overwrites it.
//!
//! The log output pane is part of the React front-end bundle (a Monaco viewer
//! in `windhawk-frontend`), staged by the same `assets/` sync as the rest of
//! the app, so there is nothing extra to stage here.
//!
//! The third piece is the manifest this crate's test binaries need
//! ([`embed_test_manifest`]); the app executable's is tauri-build's own.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    ensure_placeholder_assets();
    embed_test_manifest();
    tauri_build::build();
}

/// Write a placeholder `assets/index.html` when the synced front-end is absent,
/// so `generate_context!` always has a `frontendDist` to embed.
fn ensure_placeholder_assets() {
    let assets = Path::new("assets");
    let index = assets.join("index.html");
    if index.exists() {
        return;
    }
    // Best-effort: a failure here surfaces as the context macro's own missing-dir
    // error, which is the clearer message.
    let _ = fs::create_dir_all(assets);
    let _ = fs::write(
        &index,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Windhawk</title></head>\
         <body><p>The Windhawk UI front-end assets have not been synced into this \
         build.</p></body></html>\n",
    );
}

/// The manifest the test binaries are built with: the common-controls dependency, and
/// nothing else.
const TEST_MANIFEST: &str = "tests-app-manifest.xml";

/// Embed [`TEST_MANIFEST`] in this crate's test binaries.
///
/// A binary's manifest is what decides which comctl32 it binds to, and this crate calls
/// entry points only version 6 has: `LoadIconWithScaleDown` for the window icons
/// (shell.rs), `TaskDialogIndirect` for the fatal- and stuck-startup dialogs
/// (lifecycle/window.rs). The app executable is bound to version 6 by the manifest
/// tauri-build embeds, but that one rides the resource file, which cargo links into
/// BINARIES - so a test binary, which links the same library code, gets no manifest and
/// binds to System32's comctl32 (5.82). That one exports neither entry point, and a
/// binary importing what its libraries do not export does not fail at the call: it fails
/// to LOAD, as `STATUS_ENTRYPOINT_NOT_FOUND`, before the first test runs and with
/// nothing said about which entry point.
fn embed_test_manifest() {
    // Only the MSVC linker takes these; nothing else builds this Windows-only crate.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    let manifest =
        Path::new(&env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join(TEST_MANIFEST);
    println!("cargo:rerun-if-changed={}", manifest.display());

    // Asked for on EVERY target rather than through `rustc-link-arg-tests`, which does
    // not reach the one target that needs it most: cargo's per-kind link-arg forms name
    // target kinds, and a library's own unit tests are not a kind of their own - they are
    // the lib built with `--test`, which only the general form covers (`-tests` is the
    // `tests/` binaries alone).
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Embed it as written, rather than with the `asInvoker` trustInfo section the linker
    // adds of its own accord - which is not in the manifest the app runs with either.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    // And off again for the app executable, the one target that must not take it:
    // tauri-build's manifest is already in the resource file compiled for binaries, and
    // a binary carrying two is a link error. This switch comes last on that target's
    // command line, and the linker goes by the last it is given; were that ever to stop
    // holding, the build breaks loudly rather than shipping the wrong manifest.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
}
